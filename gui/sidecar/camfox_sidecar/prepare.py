"""Build one launch bundle: fingerprint, prefs, executable.

Everything here routes through `camoufox.utils.launch_options`, the reference
launcher pipeline. The sidecar never assembles identity values itself.
"""

import warnings
from typing import Any, Dict, List

from camoufox._warnings import FallbackWarning, LeakWarning
from camoufox.exceptions import CamoufoxNotInstalled
from camoufox.fingerprints import Screen
from camoufox.pkgman import camoufox_path
from camoufox.utils import launch_options

from .mapping import canvas_covered, decode_chunks, parse_config, proxy_kwarg, summary


def _collect_warnings(caught: List[warnings.WarningMessage]) -> List[str]:
    out: List[str] = []
    for w in caught:
        if isinstance(w.message, (LeakWarning, FallbackWarning)):
            line = f"{type(w.message).__name__}: {w.message}"
            if line not in out:
                out.append(line)
    return out


def prepare(req: Dict[str, Any]) -> Dict[str, Any]:
    try:
        camoufox_path(download_if_missing=False)
    except CamoufoxNotInstalled as exc:
        raise RuntimeError(f"kernel-not-installed: {exc}") from exc

    # Serve the headed host-GPU probe from disk after the first run, so the
    # kernel window does not flash on every profile save or random launch.
    from .host_gpu_cache import install

    install()

    kwargs: Dict[str, Any] = {"headless": False, "i_know_what_im_doing": True}
    if req.get("os"):
        kwargs["os"] = req["os"]
    if req.get("screen"):
        w, h = int(req["screen"]["width"]), int(req["screen"]["height"])
        kwargs["screen"] = Screen(max_width=w, max_height=h, min_width=w, min_height=h)
    if req.get("window"):
        kwargs["window"] = (int(req["window"]["width"]), int(req["window"]["height"]))
    if req.get("locale"):
        kwargs["locale"] = req["locale"]
    if req.get("timezone") and not req.get("geoip"):
        # GeoIP owns the timezone; a caller value alongside it contradicts it.
        kwargs.setdefault("config", {})["timezone"] = req["timezone"]
    if req.get("geoip"):
        kwargs["geoip"] = True
    proxy = proxy_kwarg(req.get("proxy") or {})
    if proxy:
        kwargs["proxy"] = proxy
    _apply_webrtc(req.get("webrtc") or {}, kwargs)

    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        result = launch_options(**kwargs)

    env: Dict[str, str] = result.get("env") or {}
    config_str = decode_chunks(env, "CAMOU_CONFIG_")
    if not config_str:
        raise RuntimeError("launch_options produced no CAMOU_CONFIG payload")
    config = parse_config(config_str)
    prefs = result.get("firefox_user_prefs") or {}
    extra_env = {k: v for k, v in env.items() if k == "FONTCONFIG_FILE"}

    return {
        "executable_path": result.get("executable_path"),
        "args": result.get("args") or [],
        "config": config,
        "prefs": prefs,
        "proxy": result.get("proxy"),
        "extra_env": extra_env,
        "summary": {**summary(config), "canvasCovered": canvas_covered(prefs)},
        "warnings": _collect_warnings(list(caught)),
    }


def _apply_webrtc(spec: Dict[str, Any], kwargs: Dict[str, Any]) -> None:
    """Map the GUI's WebRTC choice onto launch_options.

    auto: leave WebRTC to the identity (Camoufox's hardening prefs stay, and
    geoip pins the advertised address to the exit IP). block: no peer
    connections at all. custom: present this IPv4 in ICE candidates.
    """
    mode = spec.get("mode") or "auto"
    if mode == "block":
        kwargs["block_webrtc"] = True
    elif mode == "custom":
        ip = (spec.get("ipv4") or "").strip()
        if not ip:
            raise ValueError("WebRTC mode 'custom' needs an IPv4 address")
        from camoufox.ip import valid_ipv4

        if not valid_ipv4(ip):
            raise ValueError(f"invalid WebRTC IPv4 address: {ip!r}")
        kwargs.setdefault("config", {})["webrtc:ipv4"] = ip
        # A single-address identity must not advertise IPv6 candidates.
        kwargs["config"]["network:ipv4Only"] = True
    elif mode != "auto":
        raise ValueError(f"unknown WebRTC mode: {mode!r}")


def kwargs_for_automation(req: Dict[str, Any]) -> Dict[str, Any]:
    """launch_server kwargs that reuse a stored identity.

    `launch_options` merges generated values with `merge_into`, which only
    fills keys the caller did not set, so a stored config pins the identity
    while the usual coherence pass still runs over it.
    """
    kwargs: Dict[str, Any] = {
        "headless": bool(req.get("headless")),
        "i_know_what_im_doing": True,
    }
    if req.get("config"):
        kwargs["config"] = req["config"]
    if req.get("os"):
        kwargs["os"] = req["os"]
    if req.get("locale"):
        kwargs["locale"] = req["locale"]
    if req.get("prefs"):
        kwargs["firefox_user_prefs"] = req["prefs"]
    proxy = proxy_kwarg(req.get("proxy") or {})
    if proxy:
        kwargs["proxy"] = proxy
    return kwargs


def check_proxy(req: Dict[str, Any]) -> Dict[str, Any]:
    """Resolve the exit IP (through the proxy when given) and its GeoIP data."""
    from camoufox.geolocation import geoip_allowed, get_geolocation
    from camoufox.ip import Proxy, public_ip

    proxy = proxy_kwarg(req.get("proxy") or {})
    server = None
    if proxy:
        server = Proxy(server=proxy["server"], username=proxy.get("username"), password=proxy.get("password")).as_string()
    ip = public_ip(server)
    out: Dict[str, Any] = {"ip": ip}
    try:
        geoip_allowed()
        geo_cfg = get_geolocation(ip).as_config()
        out["timezone"] = geo_cfg.get("timezone")
        out["latitude"] = geo_cfg.get("geolocation:latitude")
        out["longitude"] = geo_cfg.get("geolocation:longitude")
        out["locale"] = geo_cfg.get("locale:all")
    except Exception as exc:  # noqa: BLE001 - reported as a partial result
        out["geoip_error"] = f"{type(exc).__name__}: {exc}"
    return out
