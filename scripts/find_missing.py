"""Module for finding Vagrant CLI commands that are missing in Migratory.

This script loads the reference Vagrant command tree from `vagrant_cli.json`,
builds the current Migratory command tree, and prints any commands present in
Vagrant that are not yet implemented in Migratory.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
from typing import Any

VAGRANT_CLI_JSON: str = os.path.join(os.path.dirname(__file__), "vagrant_cli.json")


def get_migratory_subcommands(base_cmd: list[str]) -> list[str]:
    """Retrieves the subcommands for a given Migratory command path.

    Args:
        base_cmd (list[str]): The base command tokens used to invoke Migratory.

    Returns:
        list[str]: A list of subcommand names parsed from Migratory's --help output.
    """
    try:
        out = subprocess.check_output(
            base_cmd + ["--help"], text=True, stderr=subprocess.STDOUT
        )
    except subprocess.CalledProcessError as e:
        out = e.output

    commands: list[str] = []
    lines = out.splitlines()
    in_commands_section = False
    for line in lines:
        if line.strip() == "Commands:":
            in_commands_section = True
            continue
        if in_commands_section:
            if line.startswith("Options:"):
                break
            if line.strip() == "" or line.startswith("  -"):
                continue
            match = re.match(r"^\s+([a-z0-9-]+)\s+", line)
            if match:
                cmd = match.group(1)
                commands.append(cmd)
    return commands


def build_migratory_tree(base_cmd: list[str]) -> dict[str, Any]:
    """Recursively builds the nested command dictionary for Migratory.

    Args:
        base_cmd (list[str]): The command prefix to inspect.

    Returns:
        dict[str, Any]: A nested dictionary mapping command names to subtrees.
    """
    cmds = get_migratory_subcommands(base_cmd)
    tree: dict[str, Any] = {}
    for cmd in cmds:
        if cmd == "help":
            continue
        sub = build_migratory_tree(base_cmd + [cmd])
        tree[cmd] = sub
    return tree


def find_missing(
    v_tree: dict[str, Any], m_tree: dict[str, Any], path: str = ""
) -> list[str]:
    """Finds commands in the Vagrant tree that are missing from the Migratory tree.

    Args:
        v_tree (dict[str, Any]): The reference Vagrant command tree.
        m_tree (dict[str, Any]): The Migratory command tree.
        path (str): The prefix path accumulated during traversal.

    Returns:
        list[str]: A list of missing command paths.
    """
    missing: list[str] = []
    for cmd, v_sub in v_tree.items():
        if cmd not in m_tree:
            missing.append(path + cmd)
        else:
            m_sub = m_tree[cmd]
            missing.extend(find_missing(v_sub, m_sub, path + cmd + " "))
    return missing


def main() -> None:
    """Main entry point for finding and printing missing Vagrant commands."""
    with open(VAGRANT_CLI_JSON, encoding="utf-8") as f:
        vagrant_tree: dict[str, Any] = json.load(f)

    migratory_tree = build_migratory_tree(["cargo", "run", "-q", "--"])
    missing_cmds = find_missing(vagrant_tree, migratory_tree)
    for m in missing_cmds:
        print(m)


if __name__ == "__main__":  # pragma: no cover
    main()
