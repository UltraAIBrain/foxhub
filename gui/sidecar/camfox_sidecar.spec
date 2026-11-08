# -*- mode: python ; coding: utf-8 -*-
"""PyInstaller spec for the CamFox sidecar.

Build (from gui/sidecar/, with the interpreter that has
`camoufox[geoip]` + playwright installed):

    python -m PyInstaller camfox_sidecar.spec --noconfirm --distpath ../src-tauri/resources

Output: ../src-tauri/resources/camfox-sidecar/camfox-sidecar.exe (+ _internal),
bundled into the Tauri app as the `sidecar` resource so releases need no
system Python.
"""

from PyInstaller.utils.hooks import collect_data_files, collect_submodules

datas = []
hiddenimports = [
    # Function-level imports the scanner misses across package borders.
    "camoufox.server",
    "camoufox.host_rendering",
    "camoufox.webgl",
    "camoufox.geolocation",
    "camoufox.fpgen_model",
    "camoufox.cpu_affinity",
    "camoufox.virtdisplay",
    "camoufox.gpudisplay",
    "camoufox.async_api",
    "camoufox.sync_api",
    "fpgen",
    "numpy",
    "orjson",
    "ua_parser",
    "maxminddb",
    "click",
    "rich",
    "requests",
    "platformdirs",
]

# camoufox ships its font/voice/locale catalogues as package data; fpgen and
# playwright carry data too (playwright's driver/node.exe is what automation
# mode runs through camoufox.server).
for pkg in ("camoufox", "fpgen", "playwright", "language_tags", "ua_parser"):
    datas += collect_data_files(pkg)
    hiddenimports += collect_submodules(pkg)

a = Analysis(
    ["run.py"],
    pathex=["."],
    binaries=[],
    datas=datas,
    hiddenimports=hiddenimports,
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=["tkinter", "matplotlib", "PyQt5"],
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name="camfox-sidecar",
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=False,
    console=True,  # stdout carries the JSON protocol
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.datas,
    strip=False,
    upx=False,
    upx_exclude=[],
    name="camfox-sidecar",
)
