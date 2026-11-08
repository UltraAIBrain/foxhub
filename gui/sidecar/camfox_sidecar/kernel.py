"""Kernel (browser build) management.

Info is read through the package's own multiversion store. Mutations
(fetch / set / remove) run the documented `camoufox` CLI, streaming its
output as events so the GUI can show download progress. Frozen (PyInstaller)
builds call the CLI in-process; source builds spawn it with the interpreter.
"""

import subprocess
import sys
from typing import Any, Dict

from .protocol import write_event

FROZEN = getattr(sys, "frozen", False)


def _cli(*args: str) -> int:
    """Run the camoufox CLI, forwarding its output as log events."""
    if FROZEN:
        return _cli_inprocess(*args)
    return _cli_subprocess(*args)


def _cli_inprocess(*args: str) -> int:
    """Run the CLI inside this frozen process.

    `python -m camoufox` is unavailable (sys.executable is this exe), and a
    second frozen copy just for the CLI would double the size. Rich output
    goes straight to stdout, where the Rust side treats non-JSON lines as
    logs; our JSON events interleave safely.
    """
    from camoufox.__main__ import cli

    previous_argv = sys.argv
    sys.argv = ["camoufox", *args]
    try:
        cli()
    except SystemExit as exc:
        code = exc.code if isinstance(exc.code, int) else 0
        if code:
            raise RuntimeError(f"camoufox {' '.join(args)} exited with {code}") from exc
        return 0
    finally:
        sys.argv = previous_argv
    return 0


def _cli_subprocess(*args: str) -> int:
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

    # camelCase on the wire, matching the frontend's KernelInfo contract.
    installed = []
    for iv in multiversion.list_installed():
        installed.append(
            {
                "label": multiversion.installed_label(iv),
                "channel": iv.channel_path,
                "relativePath": iv.relative_path,
                "path": str(iv.path),
                "isActive": iv.is_active,
                "isPrerelease": iv.is_prerelease,
                "assetSize": iv.asset_size,
                "createdAt": iv.created_at,
            }
        )
    active = multiversion.get_active_path()
    version = None
    if active is not None:
        try:
            version = pkgman.installed_verstr()
        except Exception:  # noqa: BLE001 - an unpacked dir without version.json
            version = None
    install_dir = getattr(multiversion, "INSTALL_DIR", None) or pkgman.INSTALL_DIR
    return {
        "installDir": str(install_dir),
        "activePath": str(active) if active else None,
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
