"""Frozen-entry launcher.

PyInstaller runs this as a top-level script, where the package's own
`__main__.py` relative imports do not resolve; absolute import instead.
"""

import sys

from camfox_sidecar.cli import main

if __name__ == "__main__":
    sys.exit(main())
