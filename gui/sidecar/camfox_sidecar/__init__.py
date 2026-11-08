"""CamFox GUI sidecar.

A one-shot CLI the Tauri backend drives over stdin/stdout JSON. It wraps the
`camoufox` Python package so the fingerprint pipeline (fpgen, coherence,
fonts, voices, WebGL, GeoIP) is never reimplemented in another language: the
launcher in `python/src/camoufox/` stays the only copy of that logic.
"""

__version__ = "0.1.0"
