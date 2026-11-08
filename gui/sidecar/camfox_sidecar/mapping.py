"""Pure mapping helpers, kept free of the camoufox import so they unit-test
without the package installed."""

import json
from typing import Any, Dict, List, Optional


def decode_chunks(env: Dict[str, str], prefix: str) -> Optional[str]:
    """Join the chunked env vars the browser reads (CAMOU_CONFIG_1..N)."""
    parts: List[str] = []
    i = 1
    while f"{prefix}{i}" in env:
        parts.append(env[f"{prefix}{i}"])
        i += 1
    return "".join(parts) if parts else None


def proxy_kwarg(proxy: Dict[str, Any]) -> Optional[Dict[str, str]]:
    """GUI proxy form -> Playwright proxy dict used by launch_options."""
    kind = proxy.get("kind")
    host, port = proxy.get("host"), proxy.get("port")
    if kind not in ("http", "socks5") or not host or not port:
        return None
    out = {"server": f"{kind}://{host}:{int(port)}"}
    if proxy.get("username"):
        out["username"] = proxy["username"]
    if proxy.get("password"):
        out["password"] = proxy["password"]
    return out


def parse_config(config_str: str) -> Dict[str, Any]:
    return json.loads(config_str)


def summary(config: Dict[str, Any]) -> Dict[str, Any]:
    """The small human-readable slice of the identity shown in the GUI.

    Keys are camelCase to match the Rust side's serde rename.
    """
    return {
        "ua": config.get("navigator.userAgent"),
        "platform": config.get("navigator.platform"),
        "screen": [config.get("screen.width"), config.get("screen.height")],
        "window": [
            config.get("window.outerWidth"),
            config.get("window.outerHeight"),
        ],
        "timezone": config.get("timezone"),
        "locale": config.get("locale:all") or config.get("navigator.language"),
        "webrtcIpv4": config.get("webrtc:ipv4"),
        "ip": config.get("webrtc:ipv4") if config.get("timezone") else None,
        "hardwareConcurrency": config.get("navigator.hardwareConcurrency"),
        "devicePixelRatio": config.get("window.devicePixelRatio"),
    }
