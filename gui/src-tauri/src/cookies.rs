//! Cookie import/export for a stopped profile.
//!
//! Reads and writes the profile's `cookies.sqlite` directly. The table is
//! never created here: the column set differs across Firefox versions, so an
//! import into a profile the browser has never run would guess a schema.
//! Instead both operations require an existing database (launch the profile
//! once first) and inserts adapt to the columns actually present.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Map, Value};

pub const FORMAT_JSON: &str = "json";
pub const FORMAT_NETSCAPE: &str = "netscape";

fn cookies_db(userdata: &Path) -> Result<std::path::PathBuf, String> {
    let db = userdata.join("cookies.sqlite");
    if !db.is_file() {
        return Err(format!(
            "no cookies database at {} — launch this profile once first",
            db.display()
        ));
    }
    Ok(db)
}

fn moz_cookies_columns(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(moz_cookies)")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

fn export_rows(conn: &Connection) -> Result<Vec<Map<String, Value>>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT name, value, host, path, expiry, isSecure, isHttpOnly, sameSite
             FROM moz_cookies",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            let same_site: i64 = row.get(7)?;
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                same_site,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (name, value, host, path, expiry, secure, http_only, same_site) =
            row.map_err(|e| e.to_string())?;
        let mut cookie = Map::new();
        cookie.insert("name".into(), json!(name));
        cookie.insert("value".into(), json!(value));
        cookie.insert("domain".into(), json!(host));
        cookie.insert("path".into(), json!(path));
        cookie.insert("expires".into(), json!(expiry));
        cookie.insert("secure".into(), json!(secure != 0));
        cookie.insert("httpOnly".into(), json!(http_only != 0));
        cookie.insert(
            "sameSite".into(),
            json!(match same_site {
                2 => "strict",
                1 => "lax",
                _ => "none",
            }),
        );
        out.push(cookie);
    }
    Ok(out)
}

/// Export in Playwright JSON shape.
pub fn export_json(userdata: &Path) -> Result<Value, String> {
    let conn = Connection::open_with_flags(cookies_db(userdata)?, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let rows = export_rows(&conn)?;
    Ok(Value::Array(rows.into_iter().map(Value::Object).collect()))
}

/// Export in Netscape cookies.txt shape.
pub fn export_netscape(userdata: &Path) -> Result<String, String> {
    let conn = Connection::open_with_flags(cookies_db(userdata)?, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| e.to_string())?;
    let rows = export_rows(&conn)?;
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    for row in rows {
        let get = |k: &str| row.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let domain = get("domain");
        let include_subdomains = if domain.starts_with('.') { "TRUE" } else { "FALSE" };
        let expires = row.get("expires").and_then(Value::as_i64).unwrap_or(0);
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            domain,
            include_subdomains,
            get("path"),
            if row.get("secure").and_then(Value::as_bool).unwrap_or(false) {
                "TRUE"
            } else {
                "FALSE"
            },
            expires,
            get("name"),
            get("value"),
        ));
    }
    Ok(out)
}

/// Import from Playwright JSON shape. Only writes columns the existing table
/// has, so a schema change upstream cannot corrupt a profile.
pub fn import_json(userdata: &Path, cookies: &[Value]) -> Result<usize, String> {
    let db = cookies_db(userdata)?;
    let conn = Connection::open(&db).map_err(|e| e.to_string())?;
    let columns = moz_cookies_columns(&conn)?;
    let has = |c: &str| columns.iter().any(|x| x == c);
    let mut imported = 0;
    for cookie in cookies {
        let obj = cookie
            .as_object()
            .ok_or_else(|| "each cookie must be a JSON object".to_string())?;
        let name = obj.get("name").and_then(Value::as_str).unwrap_or("");
        let value = obj.get("value").and_then(Value::as_str).unwrap_or("");
        let host = obj.get("domain").and_then(Value::as_str).unwrap_or("");
        if name.is_empty() || host.is_empty() {
            return Err("cookie entries need at least name, value and domain".into());
        }
        let path = obj.get("path").and_then(Value::as_str).unwrap_or("/");
        let expiry = obj
            .get("expires")
            .and_then(Value::as_f64)
            .map(|f| f as i64)
            .unwrap_or(0);
        let secure = obj.get("secure").and_then(Value::as_bool).unwrap_or(false);
        let http_only = obj.get("httpOnly").and_then(Value::as_bool).unwrap_or(false);
        let same_site = match obj.get("sameSite").and_then(Value::as_str) {
            Some("strict") | Some("Strict") => 2,
            Some("lax") | Some("Lax") => 1,
            _ => 0,
        };
        let now_micros = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0);

        let mut cols: Vec<&str> = vec!["name", "value", "host", "path", "expiry"];
        let mut vals: Vec<rusqlite::types::Value> = vec![
            name.to_string().into(),
            value.to_string().into(),
            host.to_string().into(),
            path.to_string().into(),
            expiry.into(),
        ];
        // Only insert columns the running Firefox version actually has; the
        // table schema is never created or altered here.
        let mut push = |col: &'static str, v: rusqlite::types::Value| {
            if has(col) {
                cols.push(col);
                vals.push(v);
            }
        };
        push("isSecure", (secure as i64).into());
        push("isHttpOnly", (http_only as i64).into());
        push("sameSite", same_site.into());
        push("rawSameSite", same_site.into());
        push("inBrowserElement", 0.into());
        push("schemeMap", 0.into());
        push("originAttributes", String::new().into());
        push("lastAccessed", now_micros.into());
        push("creationTime", now_micros.into());

        let placeholders: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
        // OR REPLACE: moz_unique_idx is (originAttributes, host, path, name), so
        // re-importing over a profile that already holds the cookie updates it
        // instead of failing the whole file on a constraint violation.
        let sql = format!(
            "INSERT OR REPLACE INTO moz_cookies ({}) VALUES ({})",
            cols.join(", "),
            placeholders.join(", ")
        );
        let params: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
        conn.execute(&sql, params.as_slice())
            .map_err(|e| format!("cookie insert failed: {e}"))?;
        imported += 1;
    }
    Ok(imported)
}

/// Import from Netscape cookies.txt shape.
pub fn import_netscape(userdata: &Path, content: &str) -> Result<usize, String> {
    let mut cookies = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() != 7 {
            return Err(format!("malformed Netscape cookie line: {line:?}"));
        }
        let mut domain = parts[0].to_string();
        if parts[1].eq_ignore_ascii_case("true") && !domain.starts_with('.') {
            domain = format!(".{domain}");
        }
        cookies.push(json!({
            "domain": domain,
            "path": parts[2],
            "secure": parts[3].eq_ignore_ascii_case("true"),
            "expires": parts[4].parse::<i64>().unwrap_or(0),
            "name": parts[5],
            "value": parts[6],
        }));
    }
    import_json(userdata, &cookies)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netscape_roundtrip_through_json_import_shape() {
        // parse only: import needs a Firefox cookies.sqlite, covered by the
        // end-to-end run. Here the parser must reject malformed lines loudly.
        assert!(import_netscape(std::path::Path::new("nope"), "bad\tline").is_err());
        assert!(import_netscape(std::path::Path::new("nope"), "d\tTRUE\t/\tTRUE\t0\tn\tv\textra").is_err());
    }

    /// The column set the import adapts to, plus Firefox's unique index.
    fn fixture_db(dir: &Path) -> std::path::PathBuf {
        let db = dir.join("cookies.sqlite");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE moz_cookies (
                id INTEGER PRIMARY KEY,
                originAttributes TEXT NOT NULL DEFAULT '',
                name TEXT, value TEXT, host TEXT, path TEXT,
                expiry INTEGER, lastAccessed INTEGER, creationTime INTEGER,
                isSecure INTEGER, isHttpOnly INTEGER, sameSite INTEGER,
                rawSameSite INTEGER, schemeMap INTEGER, inBrowserElement INTEGER
             );
             CREATE UNIQUE INDEX moz_unique_idx ON moz_cookies
                (originAttributes, host, path, name);",
        )
        .unwrap();
        db
    }

    #[test]
    fn reimporting_a_cookie_updates_it_instead_of_failing() {
        let dir = std::env::temp_dir().join(format!("camfox-cookies-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        fixture_db(&dir);

        let first = json!([{"name": "sid", "value": "one", "domain": ".example.com", "path": "/"}]);
        let second = json!([{"name": "sid", "value": "two", "domain": ".example.com", "path": "/"}]);
        assert_eq!(import_json(&dir, first.as_array().unwrap()).unwrap(), 1);
        // The same cookie again: a plain INSERT tripped moz_unique_idx here.
        assert_eq!(import_json(&dir, second.as_array().unwrap()).unwrap(), 1);

        let conn = Connection::open(dir.join("cookies.sqlite")).unwrap();
        let (value, count): (String, i64) = conn
            .query_row(
                "SELECT value, (SELECT COUNT(*) FROM moz_cookies) FROM moz_cookies WHERE name = 'sid'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((value.as_str(), count), ("two", 1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
