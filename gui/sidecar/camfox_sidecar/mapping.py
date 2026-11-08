"""Pure mapping helpers, kept free of the camoufox import so they unit-test
without the package installed."""

import hashlib
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


def identity_id(config: Dict[str, Any]) -> str:
    """Stable id of the frozen identity, so a user can compare launches.

    The config is the whole presented machine; a fixed profile keeps it
    byte-identical across launches, and this hash proves it.
    """
    blob = json.dumps(config, sort_keys=True, ensure_ascii=False)
    return hashlib.sha256(blob.encode("utf-8")).hexdigest()[:16]


def summary(config: Dict[str, Any]) -> Dict[str, Any]:
    """The human-readable slice of the identity shown in the GUI.

    Keys are camelCase to match the Rust side's serde rename.
    """
    screen = [config.get("screen.width"), config.get("screen.height")]
    avail = [config.get("screen.availWidth"), config.get("screen.availHeight")]
    media = [
        config.get("mediaDevices:micros", 0),
        config.get("mediaDevices:webcams", 0),
        config.get("mediaDevices:speakers", 0),
    ]
    return {
        "identityId": identity_id(config),
        "ua": config.get("navigator.userAgent"),
        "platform": config.get("navigator.platform"),
        "oscpu": config.get("navigator.oscpu"),
        "screen": screen,
        "availScreen": avail,
        "window": [
            config.get("window.outerWidth"),
            config.get("window.outerHeight"),
        ],
        "innerWindow": [
            config.get("window.innerWidth"),
            config.get("window.innerHeight"),
        ],
        "colorDepth": config.get("screen.colorDepth"),
        "timezone": config.get("timezone"),
        "locale": config.get("locale:all") or config.get("navigator.language"),
        "webrtcIpv4": config.get("webrtc:ipv4"),
        "ip": config.get("webrtc:ipv4") if config.get("timezone") else None,
        "hardwareConcurrency": config.get("navigator.hardwareConcurrency"),
        "devicePixelRatio": config.get("window.devicePixelRatio"),
        "gpuVendor": config.get("webGl:vendor"),
        "gpuRenderer": config.get("webGl:renderer"),
        "fontsCount": len(config.get("fonts") or []),
        "voicesCount": len(config.get("voices") or []),
        "mediaDevices": media,
    }
