# CamFox GUI

An AdsPower-style control panel for [Camoufox](../README.md): manage browser
environments (profiles), draw one fingerprint identity per environment, and
open several isolated browser windows at once.

Every environment is a separate `camoufox` process with its own Firefox
profile directory (cookies, storage, extensions) and its own identity, passed
to the browser through the same `CAMOU_CONFIG_*` / `CAMOU_PREFS_*` channels the
Python launcher uses. Fingerprint generation is **not** reimplemented here: the
Tauri backend shells out to the `camoufox` Python package through a small
sidecar CLI (`sidecar/camfox_sidecar`), so `python/src/camoufox/` stays the
only copy of that logic.

## Features

- **Environment management** — create / edit / duplicate / delete profiles
  with groups, notes and search; batch import/export as JSON.
- **Fingerprints** — OS, screen, window size, locale, timezone and proxy are
  drawn as one coherent identity via `camoufox.utils.launch_options`
  (fpgen, coherence, fonts, voices, WebGL, media devices). Fixed identity per
  profile, or redraw on every launch.
- **Multi-open isolation** — one process + one profile directory + one
  identity per environment; launch as many as the machine allows.
- **Proxies** — HTTP / SOCKS5 per environment, with or without credentials.
  Authenticated proxies are served through a local SOCKS5 forwarder (Rust,
  in-process) that injects the credentials upstream; the browser only ever
  sees `127.0.0.1`. "Check proxy" resolves the exit IP and its GeoIP data.
- **Cookie import/export** — Playwright JSON or Netscape `cookies.txt`,
  while the environment is stopped.
- **Automation** — expose a running environment on a Playwright WebSocket
  endpoint (via `camoufox.server.launch_server`) for
  `firefox.connect(endpoint)` scripts.
- **Kernel management** — download / switch / remove browser builds through
  the documented `camoufox` CLI.

## Layout

| Path | What it is |
|---|---|
| `src/` | React 18 + Ant Design 5 frontend (Vite, TypeScript). |
| `src-tauri/` | Tauri 2 Rust backend: profile store, launcher, proxy forwarder, cookie IO. |
| `sidecar/camfox_sidecar/` | One-shot Python CLI (`prepare`, `check-proxy`, `kernel-*`, `automate`) wrapping the `camoufox` package. |
| `sidecar/tests/` | Sidecar unit tests (pure logic, no network). |

## Prerequisites

- Node.js ≥ 18 and pnpm
- Rust (msvc toolchain on Windows) and WebView2
- **A python.org Python ≥ 3.11** — not the Microsoft Store build. The Store
  Python writes under `AppData` through per-app virtualization: files its
  installer puts in `AppData\Local` (the fetched kernel among them) are
  invisible to every other process, so Playwright's driver and the GUI
  launcher both fail with "executable doesn't exist". Settings → Python
  interpreter must point at the real interpreter.
- The repo's launcher package and the GeoIP extra, installed with that
  interpreter:
  ```bash
  pip install -e "./python[geoip]"
  ```
- A browser kernel (`camoufox fetch` once, or use the Kernel Management page).

## Development

```bash
cd gui
pnpm install
pnpm tauri dev
```

The sidecar is found automatically at `gui/sidecar` in dev builds. Point
Settings → Sidecar directory elsewhere only if you move it.

## Tests

```bash
cd gui/src-tauri && cargo test        # store, env chunking, forwarder handshake
cd gui/sidecar && pytest tests -q     # sidecar protocol/mapping
```

The end-to-end checks (launch two environments, verify distinct identities and
cookie isolation, proxy forwarder exit IP, cookie roundtrip, automation
endpoint) are manual; see the checklist below.

## Manual verification checklist

1. Create two environments with different OS settings and launch both:
   two independent windows; `bot.sannysoft.com` / CreepJS show different
   UA, platform and screen.
2. Log into a site in environment A; environment B sees no session.
3. Set an authenticated SOCKS5 proxy on an environment, launch, and confirm
   the exit IP on the "Check proxy" readout matches the proxy.
4. Export cookies from environment A (JSON), import into a fresh
   environment, and confirm the session carries over.
5. Enable automation on an environment, open it via the automation action,
   and connect:
   ```python
   from playwright.sync_api import sync_playwright

   with sync_playwright() as p:
       browser = p.firefox.connect("ws://127.0.0.1:<port>")
       print(browser.contexts[0].pages[0].title())
   ```

## Known limitations

- The Python interpreter and the `camoufox` package must be installed on the
  machine (fingerprint generation needs its data files). Bundling the sidecar
  as a frozen executable is future work.
- A fixed identity is frozen at generation time on that host: moving a profile
  to another machine leaves WebGL/canvas on the host-parity path (random
  canvas readback), exactly as the Python launcher behaves.
- Automation mode serves a non-persistent browser (Playwright launchServer
  cannot serve a persistent context over WebSocket), so cookies do not persist
  in that mode; use normal open + cookie export for that.
- Firefox cannot use authenticated proxies natively on a bare launch; the
  built-in forwarder covers it, and the browser itself only sees localhost.
