"""Automation mode: serve one profile over a Playwright websocket endpoint.

Runs `camoufox.server.launch_server` with the profile's stored identity and
blocks until the backend kills this process. The `ws://` endpoint is printed
by the Playwright driver on inherited stdout; the Rust side picks it out of
the output stream.
"""

import sys
from typing import Any, Dict

from .prepare import kwargs_for_automation
from .protocol import write_event


def automate(req: Dict[str, Any]) -> None:
    from camoufox.server import launch_server

    write_event("starting")
    launch_server(**kwargs_for_automation(req))
    # launch_server blocks forever; if it ever returns, exit non-zero so the
    # backend reports the endpoint as dead.
    sys.exit(1)
