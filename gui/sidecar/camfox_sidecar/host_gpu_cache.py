"""Disk cache for the host-GPU probe.

`camoufox.utils.launch_options` probes the host GPU by really launching the
kernel headed once (a page flashes on screen). Upstream memoizes it with a
process-local lru_cache, but the sidecar is a fresh process per call, so the
GUI paid the flash on every profile save and every random-mode launch.

The probe result only depends on (kernel build, rendering prefs) on this
machine, so we wrap the upstream lru_cache with a disk cache. Upstream files
are not touched: the wrapper is installed from the sidecar and forwards to
the original function on a miss. Cached probes stop matching after a kernel
change because the kernel path embeds the version directory; a stale entry
also expires after 30 days, in case a driver update changed the reported
renderer.
"""

import json
import time
from pathlib import Path
from typing import Any, Dict, Optional, Tuple

from .protocol import write_event

CACHE_TTL_SECONDS = 30 * 24 * 3600

_WRAPPED: Optional[Tuple[Any, Any]] = None  # (original, cache-file path)


def cache_path() -> Path:
    from camoufox.pkgman import INSTALL_DIR

    return Path(INSTALL_DIR) / "gui-host-gpu-cache.json"


def _load(path: Path) -> Dict[str, Any]:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        return data if isinstance(data, dict) else {}
    except (OSError, ValueError):
        return {}


def _key(executable_path: str, headless: bool, prefs: Tuple[Tuple[str, Any], ...]) -> str:
    # The env part of the upstream key is machine noise (full environ copy);
    # the renderer depends on the build and the rendering prefs only.
    return json.dumps([executable_path, headless, list(prefs)], sort_keys=True, default=str)


def install() -> None:
    """Wrap camoufox's cached host-GPU probe with the disk cache.

    Idempotent; a no-op when the probe attribute is not what we expect.
    """
    global _WRAPPED
    if _WRAPPED is not None:
        return
    import camoufox.host_rendering as host_rendering

    original = host_rendering._host_gpu
    wrapper_target = getattr(original, "__wrapped__", None)
    if wrapper_target is None or not hasattr(original, "cache_clear"):
        return  # upstream changed shape; run without the disk cache
    path = cache_path()

    def cached(executable_path: str, headless: bool, env: Tuple[Tuple[str, str], ...], prefs):
        key = _key(executable_path, headless, prefs)
        data = _load(path)
        entry = data.get(key)
        if entry is not None and time.time() - entry.get("t", 0) < CACHE_TTL_SECONDS:
            gpu = entry.get("gpu")
            return tuple(gpu) if gpu else None
        write_event("log", line="host GPU probe: launching the kernel once (first run only)")
        gpu = wrapper_target(executable_path, headless, env, prefs)
        data[key] = {"t": time.time(), "gpu": list(gpu) if gpu else None}
        try:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(data), encoding="utf-8")
        except OSError:
            pass  # cache write failure must not fail identity generation
        return gpu

    host_rendering._host_gpu = cached
    _WRAPPED = (wrapper_target, path)
