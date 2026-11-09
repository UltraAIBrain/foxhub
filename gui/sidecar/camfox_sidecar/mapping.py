"""Pure mapping helpers, kept free of the camoufox import so they unit-test
without the package installed."""

import hashlib
import json
from typing import Any, Dict, List, Mapping, Optional


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


def canvas_covered(prefs: Mapping[str, Any]) -> bool:
    """Whether the launcher turned on RFP's canvas placeholder.

    Upstream covers the canvas when the identity cannot match what the host
    really renders (another OS, another GPU, or a GPU fpgen never recorded);
    the readback is then random per draw, unlike the uncovered host-parity
    path whose pixels are fixed.
    """
    overrides = str(prefs.get("privacy.baselineFingerprintingProtection.overrides") or "")
    return "CanvasImageExtractionPrompt" in overrides


def _pair(config: Mapping[str, Any], *keys: str) -> Optional[List[Any]]:
    """A width/height pair, or None when any member is missing.

    A JSON null inside the list would fail the Rust side's `Vec<u32>` decode,
    which then drops the entire summary and leaves the profile showing "no
    fingerprint" although the identity was stored.
    """
    values = [config.get(k) for k in keys]
    numeric = all(isinstance(v, (int, float)) and not isinstance(v, bool) for v in values)
    return values if numeric else None


def summary(config: Dict[str, Any]) -> Dict[str, Any]:
    """The human-readable slice of the identity shown in the GUI.

    Keys are camelCase to match the Rust side's serde rename.
    """
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
        "screen": _pair(config, "screen.width", "screen.height"),
        "availScreen": _pair(config, "screen.availWidth", "screen.availHeight"),
        "window": _pair(config, "window.outerWidth", "window.outerHeight"),
        "innerWindow": _pair(config, "window.innerWidth", "window.innerHeight"),
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
