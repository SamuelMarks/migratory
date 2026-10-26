#!/usr/bin/env python3
"""Module for validating the Migratory CLI interface against the Vagrant CLI.

This script builds or loads a command tree of Vagrant's CLI and compares it
against Migratory's CLI tree to detect any hallucinated commands or subcommands.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from typing import Any

VAGRANT_CLI_JSON: str = os.path.join(os.path.dirname(__file__), "vagrant_cli.json")


def get_vagrant_top_level_commands() -> list[str]:
    """Retrieves the top-level commands supported by Vagrant.

    Returns:
        list[str]: A list of top-level Vagrant command names.
    """
    try:
        out = subprocess.check_output(["vagrant", "list-commands"], text=True)
    except subprocess.CalledProcessError as e:
        out = e.output
    except FileNotFoundError:
        print("Vagrant not installed. Cannot build vagrant_cli.json.")
        sys.exit(1)

    commands: list[str] = []
    for line in out.splitlines():
        if (
            line.strip() == ""
            or line.startswith("Below is")
            or line.startswith("description")
            or line.startswith("To see all subcommands")
        ):
            continue
        cmd = line.strip().split()[0]
        if not cmd.startswith("-"):
            commands.append(cmd)
    return commands


def get_vagrant_subcommands(base_cmd: list[str]) -> list[str]:
    """Retrieves the subcommands for a given Vagrant command path.

    Args:
        base_cmd (list[str]): The base command tokens (e.g. ['vagrant', 'box']).

    Returns:
        list[str]: A list of subcommand names under the base command.
    """
    try:
        out = subprocess.check_output(
            base_cmd + ["--help"], text=True, stderr=subprocess.STDOUT
        )
    except subprocess.CalledProcessError as e:
        out = e.output
    except FileNotFoundError:
        return []

    commands: list[str] = []
    lines = out.splitlines()
    in_commands_section = False
    for line in lines:
        if line.strip() == "Available subcommands:":
            in_commands_section = True
            continue
        if in_commands_section:
            if line.startswith("For help on") or "Options:" in line:
                break
            if line.strip() == "" or line.startswith("-"):
                continue
            cmd = line.strip().split()[0]
            if not cmd.startswith("-"):
                commands.append(cmd)
    return commands


def build_vagrant_tree(base_cmd: list[str]) -> dict[str, Any]:
    """Recursively builds the nested command dictionary for Vagrant.

    Args:
        base_cmd (list[str]): The command prefix to inspect.

    Returns:
        dict[str, Any]: A nested dictionary mapping command names to subtrees.
    """
    if len(base_cmd) == 1 and base_cmd[0] == "vagrant":
        cmds = get_vagrant_top_level_commands()
    else:
        cmds = get_vagrant_subcommands(base_cmd)

    tree: dict[str, Any] = {}
    for cmd in cmds:
        sub = build_vagrant_tree(base_cmd + [cmd])
        tree[cmd] = sub
    return tree


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


def compare_trees(
    migratory_tree: dict[str, Any], vagrant_tree: dict[str, Any], path: str = ""
) -> list[str]:
    """Compares the Migratory command tree against the Vagrant command tree.

    Args:
        migratory_tree (dict[str, Any]): The nested command tree for Migratory.
        vagrant_tree (dict[str, Any]): The reference command tree for Vagrant.
        path (str): The space-separated command path accumulated during recursion.

    Returns:
        list[str]: A list of validation error messages for hallucinated commands.
    """
    errors: list[str] = []
    for cmd, mig_sub in migratory_tree.items():
        cmd_path = f"{path} {cmd}".strip()
        if cmd not in vagrant_tree:
            errors.append(
                f"Hallucination detected: Migratory has command '{cmd_path}' which does not exist in Vagrant."
            )
        else:
            vag_sub = vagrant_tree[cmd]
            errors.extend(compare_trees(mig_sub, vag_sub, cmd_path))
    return errors


def main() -> None:
    """Main entry point for validating the Migratory CLI against Vagrant."""
    if not os.path.exists(VAGRANT_CLI_JSON):
        print("vagrant_cli.json not found. Building it from system vagrant...")
        vagrant_tree = build_vagrant_tree(["vagrant"])
        with open(VAGRANT_CLI_JSON, "w", encoding="utf-8") as f:
            json.dump(vagrant_tree, f, indent=2)
        print("vagrant_cli.json created.")
    else:
        with open(VAGRANT_CLI_JSON, "r", encoding="utf-8") as f:
            vagrant_tree = json.load(f)

    print("Building Migratory CLI tree...")
    migratory_tree = build_migratory_tree(["cargo", "run", "-q", "--"])

    print("Comparing Migratory CLI against Vagrant CLI...")
    errors = compare_trees(migratory_tree, vagrant_tree)

    if errors:
        print()
        print("CLI Validation Failed:")
        for err in errors:
            print(f" - {err}")
        sys.exit(1)

    print("CLI Validation Passed! Migratory interface aligns with Vagrant.")


if __name__ == "__main__":  # pragma: no cover
    main()
