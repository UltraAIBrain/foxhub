"""Kernel (browser build) management.

Info is read through the package's own multiversion store. Mutations
(fetch / set / remove) delegate to the documented `python -m camoufox` CLI
run with the same interpreter, streaming its output as events so the GUI can
show download progress.
"""

import subprocess
import sys
from typing import Any, Dict

from .protocol import write_event


def _cli(*args: str) -> int:
    """Run the camoufox CLI, forwarding stdout/stderr as events."""
    process = subprocess.Popen(  # noqa: S603 - fixed argv, no shell
        [sys.executable, "-m", "camoufox", *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    assert process.stdout is not None
    for line in process.stdout:
        write_event("log", line=line.rstrip("\n"))
    return process.wait()


def kernel_info(req: Dict[str, Any]) -> Dict[str, Any]:
    from camoufox import multiversion, pkgman

    installed = []
    for iv in multiversion.list_installed():
        installed.append(
            {
                "label": multiversion.installed_label(iv),
                "channel": iv.channel_path,
                "relative_path": iv.relative_path,
                "path": str(iv.path),
                "is_active": iv.is_active,
                "is_prerelease": iv.is_prerelease,
                "asset_size": iv.asset_size,
                "created_at": iv.created_at,
            }
        )
    active = multiversion.get_active_path()
    version = None
    if active is not None:
        try:
            version = pkgman.installed_verstr()
        except Exception:  # noqa: BLE001 - an unpacked dir without version.json
            version = None
    return {
        "install_dir": str(multiversion.INSTALL_DIR if hasattr(multiversion, "INSTALL_DIR") else pkgman.INSTALL_DIR),
        "active_path": str(active) if active else None,
        "version": version,
        "installed": installed,
    }


def kernel_fetch(req: Dict[str, Any]) -> Dict[str, Any]:
    specifier = req.get("specifier")
    args = ["fetch"] + ([specifier] if specifier else [])
    code = _cli(*args)
    if code != 0:
        raise RuntimeError(f"camoufox fetch exited with {code}")
    return {"specifier": specifier}


def kernel_set(req: Dict[str, Any]) -> Dict[str, Any]:
    specifier = req.get("specifier")
    if not specifier:
        raise ValueError("specifier is required")
    code = _cli("set", specifier)
    if code != 0:
        raise RuntimeError(f"camoufox set exited with {code}")
    return kernel_info({})


def kernel_remove(req: Dict[str, Any]) -> Dict[str, Any]:
    specifier = req.get("specifier")
    if not specifier:
        raise ValueError("specifier is required")
    code = _cli("remove", specifier)
    if code != 0:
        raise RuntimeError(f"camoufox remove exited with {code}")
    return kernel_info({})
