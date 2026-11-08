from typing import Any, Dict

from .automate import automate
from .kernel import kernel_fetch, kernel_info, kernel_remove, kernel_set
from .prepare import check_proxy, prepare
from .protocol import run_command


def main() -> int:
    commands: Dict[str, Any] = {
        "prepare": prepare,
        "check-proxy": check_proxy,
        "kernel-info": kernel_info,
        "kernel-fetch": kernel_fetch,
        "kernel-set": kernel_set,
        "kernel-remove": kernel_remove,
        "automate": automate,
    }
    return run_command(commands)


if __name__ == "__main__":
    raise SystemExit(main())
