#!/usr/bin/env python3
import subprocess
import re
import json
import sys
import os

VAGRANT_CLI_JSON = os.path.join(os.path.dirname(__file__), "vagrant_cli.json")


def get_vagrant_top_level_commands():
    try:
        out = subprocess.check_output(["vagrant", "list-commands"], text=True)
    except subprocess.CalledProcessError as e:
        out = e.output
    except FileNotFoundError:
        print("Vagrant not installed. Cannot build vagrant_cli.json.")
        sys.exit(1)

    commands = []
    for line in out.splitlines():
        if (
            line.strip() == ""
            or line.startswith("Below is")
            or line.startswith("description")
            or line.startswith("To see all subcommands")
        ):
            continue
        parts = line.strip().split()
        if parts:
            cmd = parts[0]
            if not cmd.startswith("-"):
                commands.append(cmd)
    return commands


def get_vagrant_subcommands(base_cmd):
    try:
        out = subprocess.check_output(
            base_cmd + ["--help"], text=True, stderr=subprocess.STDOUT
        )
    except subprocess.CalledProcessError as e:
        out = e.output
    except FileNotFoundError:
        return []

    commands = []
    lines = out.splitlines()
    in_commands_section = False
    for line in lines:
        if line.strip() == "Available subcommands:":
            in_commands_section = True
            continue
        if in_commands_section:
            if (
                line.strip() == ""
                or line.startswith("For help on")
                or line.startswith("-")
                or "Options:" in line
            ):
                if line.startswith("For help on") or "Options:" in line:
                    break
                continue
            parts = line.strip().split()
            if parts:
                cmd = parts[0]
                if not cmd.startswith("-"):
                    commands.append(cmd)
    return commands


def build_vagrant_tree(base_cmd):
    if len(base_cmd) == 1 and base_cmd[0] == "vagrant":
        cmds = get_vagrant_top_level_commands()
    else:
        cmds = get_vagrant_subcommands(base_cmd)

    tree = {}
    for cmd in cmds:
        sub = build_vagrant_tree(base_cmd + [cmd])
        tree[cmd] = sub
    return tree


def get_migratory_subcommands(base_cmd):
    try:
        out = subprocess.check_output(
            base_cmd + ["--help"], text=True, stderr=subprocess.STDOUT
        )
    except subprocess.CalledProcessError as e:
        out = e.output

    commands = []
    lines = out.splitlines()
    in_commands_section = False
    for line in lines:
        if line.strip() == "Commands:":
            in_commands_section = True
            continue
        if in_commands_section:
            if (
                line.strip() == ""
                or line.startswith("Options:")
                or line.startswith("  -")
            ):
                if line.startswith("Options:"):
                    break
                continue
            match = re.match(r"^\s+([a-z0-9-]+)\s+", line)
            if match:
                cmd = match.group(1)
                commands.append(cmd)
    return commands


def build_migratory_tree(base_cmd):
    cmds = get_migratory_subcommands(base_cmd)
    tree = {}
    for cmd in cmds:
        if cmd == "help":
            continue
        sub = build_migratory_tree(base_cmd + [cmd])
        tree[cmd] = sub
    return tree


def compare_trees(migratory_tree, vagrant_tree, path=""):
    errors = []
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


def main():
    if not os.path.exists(VAGRANT_CLI_JSON):
        print("vagrant_cli.json not found. Building it from system vagrant...")
        vagrant_tree = build_vagrant_tree(["vagrant"])
        with open(VAGRANT_CLI_JSON, "w") as f:
            json.dump(vagrant_tree, f, indent=2)
        print("vagrant_cli.json created.")
    else:
        with open(VAGRANT_CLI_JSON, "r") as f:
            vagrant_tree = json.load(f)

    print("Building Migratory CLI tree...")
    migratory_tree = build_migratory_tree(["cargo", "run", "-q", "--"])

    print("Comparing Migratory CLI against Vagrant CLI...")
    errors = compare_trees(migratory_tree, vagrant_tree)

    if errors:
        print("\nCLI Validation Failed:")
        for err in errors:
            print(f" - {err}")
        sys.exit(1)

    print("CLI Validation Passed! Migratory interface aligns with Vagrant.")


if __name__ == "__main__":
    main()
