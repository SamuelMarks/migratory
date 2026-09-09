import json
import subprocess
import re

with open("scripts/vagrant_cli.json") as f:
    vagrant_tree = json.load(f)


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


migratory_tree = build_migratory_tree(["cargo", "run", "-q", "--"])


def find_missing(v_tree, m_tree, path=""):
    missing = []
    for cmd, v_sub in v_tree.items():
        if cmd not in m_tree:
            missing.append(path + cmd)
        else:
            m_sub = m_tree[cmd]
            missing.extend(find_missing(v_sub, m_sub, path + cmd + " "))
    return missing


missing_cmds = find_missing(vagrant_tree, migratory_tree)
for m in missing_cmds:
    print(m)
