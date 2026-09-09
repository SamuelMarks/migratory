import json
import subprocess
import re

with open("scripts/vagrant_cli.json") as f:
    vagrant_tree = json.load(f)


def get_flags(cmd_args, is_vagrant):
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
        return []

    flags = set()
    for line in out.splitlines():
        # Match flags like -f, --force
        matches = re.findall(r"--[a-zA-Z0-9-]+", line)
        for m in matches:
            if m not in ["--help", "--version"]:
                flags.add(m)
    return flags


def walk_tree(tree, path):
    missing_flags = {}

    # check current path
    if path:
        v_flags = get_flags(["vagrant"] + path, True)
        m_flags = get_flags(path, False)
        missing = v_flags - m_flags
        if missing:
            missing_flags[" ".join(path)] = missing

    for cmd, sub in tree.items():
        missing_flags.update(walk_tree(sub, path + [cmd]))

    return missing_flags


mf = walk_tree(vagrant_tree, [])
for p, flags in mf.items():
    print(f"Command '{p}' is missing flags: {', '.join(flags)}")
