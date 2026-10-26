"""Module for finding Vagrant CLI flags that are missing in Migratory.

This script traverses the Vagrant command tree and compares the `--flag` options
present in Vagrant's `--help` output against those in Migratory's `--help` output.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from typing import Any

VAGRANT_CLI_JSON: str = os.path.join(os.path.dirname(__file__), "vagrant_cli.json")


def get_flags(cmd_args: list[str], is_vagrant: bool) -> set[str]:
    """Extracts long-form CLI flags from the help output of a command.

    Args:
        cmd_args (list[str]): The command arguments to run with `--help`.
        is_vagrant (bool): True if invoking Vagrant directly, False for Migratory.

    Returns:
        set[str]: The set of long flags (excluding `--help` and `--version`).
    """
    try:
        if is_vagrant:
            out = subprocess.check_output(
                cmd_args + ["--help"], text=True, stderr=subprocess.STDOUT
            )
        else:
            out = subprocess.check_output(
                ["cargo", "run", "-q", "--"] + cmd_args + ["--help"],
                text=True,
                stderr=subprocess.STDOUT,
            )
    except subprocess.CalledProcessError as e:
        out = e.output
    except FileNotFoundError:
        return set()

    flags: set[str] = set()
    for line in out.splitlines():
        matches = re.findall(r"--[a-zA-Z0-9-]+", line)
        for m in matches:
            if m not in ["--help", "--version"]:
                flags.add(m)
    return flags


def walk_tree(tree: dict[str, Any], path: list[str]) -> dict[str, set[str]]:
    """Recursively walks the command tree to find flags missing in Migratory.

    Args:
        tree (dict[str, Any]): The command tree to traverse.
        path (list[str]): The current command path tokens.

    Returns:
        dict[str, set[str]]: A mapping from command path strings to sets of missing flags.
    """
    missing_flags: dict[str, set[str]] = {}

    if path:
        v_flags = get_flags(["vagrant"] + path, True)
        m_flags = get_flags(path, False)
        missing = v_flags - m_flags
        if missing:
            missing_flags[" ".join(path)] = missing

    for cmd, sub in tree.items():
        missing_flags.update(walk_tree(sub, path + [cmd]))

    return missing_flags


def main() -> None:
    """Main entry point for finding and printing missing CLI flags."""
    with open(VAGRANT_CLI_JSON, encoding="utf-8") as f:
        vagrant_tree: dict[str, Any] = json.load(f)

    mf = walk_tree(vagrant_tree, [])
    for p, flags in mf.items():
        print(f"Command '{p}' is missing flags: {', '.join(sorted(flags))}")


if __name__ == "__main__":  # pragma: no cover
    main()
