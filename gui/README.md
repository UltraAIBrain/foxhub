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
  profile, or redraw on every launch. The edit panel shows the resolved
  identity (UA, platform, screen, GPU, locale, timezone, fonts/voices, media
  devices) plus a stable **identity ID** — the hash of the frozen config, so
  the user can prove every launch of a profile is the same machine.
- **WebRTC** — per environment: follow the identity (with GeoIP it advertises
  the proxy exit IP), block WebRTC entirely, or pin a specific IPv4 (IPv6
  candidates disabled). GeoIP works with or without a proxy.
- **Multi-open isolation** — one process + one profile directory + one
  identity per environment; launch as many as the machine allows. The OS
  window is sized to the identity's claimed outer window (written into the
  profile's `xulstore.json` before each launch), so the real window matches
  what the machine claims. On Windows the spawned executable is a launcher
  that hands off to the browser process, so liveness is tracked by the
  process carrying the profile path, not by the spawned pid. Closing posts
  WM_CLOSE first and only forces after a 20-second grace, which lets Firefox
  flush cookies and session state. After a GUI restart, still-running
  browsers are re-attached automatically so Close works on them again (a
  relaunched authenticated-proxy profile is needed to restore its proxy
  forwarder).
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

## Packaging (self-contained installer)

The release app bundles the sidecar as a frozen PyInstaller build
(`camfox-sidecar.exe` + `_internal`), so end users need **no Python
installed**. The browser kernel itself is not bundled (2+ GB); it is
downloaded in-app on first run (Kernel Management page or `camoufox fetch`).

Build steps, from `gui/`, with the interpreter that has `camoufox[geoip]`,
playwright and pyinstaller installed:

```bash
cd sidecar
python -m PyInstaller camfox_sidecar.spec --noconfirm \
    --distpath ../src-tauri/resources --workpath build
cd ..
pnpm tauri build
```

The installer lands in `src-tauri/target/release/bundle/nsis/`. The frozen
sidecar is rebuilt from the spec whenever the launcher package or the
sidecar changes; a stale build silently runs old fingerprint logic.

`src-tauri/resources/camfox-sidecar/.gitkeep` keeps the resource path valid
for dev builds before the first frozen build exists.

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

- **Canvas fingerprint stability.** Canvas *pixels* are fixed by the identity
  and are bit-identical on every launch of a profile (verified: two launches
  of the same frozen identity, no differing pixels). `toDataURL()` /
  `toBlob()` bytes still differ per session because stock Firefox appends a
  per-session random `deBG` chunk to the PNG — measured identically on a
  stock Mozilla Firefox build. Sites that hash the decoded pixels see a
  stable canvas; sites that hash the whole PNG string see the same per-session
  change every Firefox user has. Removing that chunk would deviate from stock
  Firefox and become its own fingerprint, so it is deliberately kept.
- Generating a fingerprint probes the host GPU by really launching the kernel
  headed once (two windows flash for a few seconds). The sidecar caches the
  probe result on disk (invalidated by a kernel change, 30-day TTL), so the
  flash happens once per kernel build; later saves and random-mode launches
  read the cache. The probe must run in the same display mode as the real
  launch — a headless probe would report a different renderer and break
  canvas coherence — so it cannot be hidden.
- Dev builds need the Python interpreter and the `camoufox` package
  installed on the machine (fingerprint generation needs its data files);
  packaged releases carry a frozen sidecar instead.
- A fixed identity is frozen at generation time on that host: moving a profile
  to another machine leaves WebGL/canvas on the host-parity path (random
  canvas readback), exactly as the Python launcher behaves.
- Automation mode serves a non-persistent browser (Playwright launchServer
  cannot serve a persistent context over WebSocket), so cookies do not persist
  in that mode; use normal open + cookie export for that.
- Firefox cannot use authenticated proxies natively on a bare launch; the
  built-in forwarder covers it, and the browser itself only sees localhost.
- The Microsoft Store Python is unsupported for dev builds: its AppData
  virtualization hides the fetched kernel from every other process (see
  Prerequisites).
