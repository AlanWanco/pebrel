#!/usr/bin/env python3
"""Run the complete native suite with one Rust workspace feature graph."""

from __future__ import annotations

import subprocess
import sys


def native_commands() -> list[list[str]]:
    return [
        [sys.executable, "-m", "unittest", "discover", "-s", "scripts/tests", "-v"],
        [sys.executable, "-m", "unittest", "discover", "-s", "scripts/conformance/tests", "-v"],
        [
            "cargo", "test", "--locked", "--workspace",
            "--features", "nebula/gpui-test-support", "--timings",
        ],
    ]


def main() -> int:
    for command in native_commands():
        print("Running:", " ".join(command), flush=True)
        subprocess.run(command, check=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
