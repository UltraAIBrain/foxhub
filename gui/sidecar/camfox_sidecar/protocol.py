"""stdin/stdout JSON protocol shared by every sidecar command.

The request is one JSON document on stdin. The response is one JSON document
written as the last line of stdout. The `camoufox` package prints its own
progress (rich) and warnings to the same streams, so consumers must take the
last line that parses as a JSON object with an "ok" key and treat everything
else as log output. Streaming commands (kernel fetch, automate) emit
{"event": ...} lines before the final response for the same reason.
"""

import json
import sys
from typing import Any, Callable, Dict

Request = Dict[str, Any]
Response = Dict[str, Any]


def read_request() -> Request:
    raw = sys.stdin.read()
    req = json.loads(raw) if raw.strip() else {}
    if not isinstance(req, dict):
        raise ValueError("request must be a JSON object")
    return req


def write_event(event: str, **fields: Any) -> None:
    """Emit an intermediate line for streaming consumers."""
    sys.stdout.write(json.dumps({"event": event, **fields}, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def write_response(resp: Response) -> None:
    sys.stdout.write(json.dumps(resp, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def ok(**fields: Any) -> Response:
    return {"ok": True, **fields}


def fail(error: str, **fields: Any) -> Response:
    return {"ok": False, "error": error, **fields}


def finalize(result: Response) -> Response:
    """Success payloads from handlers may omit the ok flag; wrap them."""
    return result if "ok" in result else ok(**result)


def run_command(commands: Dict[str, Callable[[Request], Response]]) -> int:
    """Dispatch one request; returns a process exit code.

    Long-running commands (automation) consume the request the same way but
    never return; the caller kills the process when the session ends.
    """
    name = sys.argv[1] if len(sys.argv) > 1 else ""
    handler = commands.get(name)
    if handler is None:
        write_response(fail(f"unknown command: {name!r}", commands=sorted(commands)))
        return 2
    try:
        write_response(finalize(handler(read_request())))
        return 0
    except BrokenPipeError:
        return 0
    except Exception as exc:  # noqa: BLE001 - the boundary reports everything
        write_response(fail(f"{type(exc).__name__}: {exc}"))
        return 1
