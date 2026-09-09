"""Module for updating coverage shields in README.md.

This script parses coverage from cargo llvm-cov and updates the README.md
badges for both documentation and test coverage in a cross-platform manner.
"""

import shutil
import subprocess
from pathlib import Path


def get_test_coverage() -> str:
    """Runs cargo llvm-cov and extracts the test coverage percentage.

    Returns:
        str: The test coverage percentage, or 'Unknown' if unavailable.
    """
    if not shutil.which("cargo-llvm-cov"):
        return "Unknown"

    try:
        result = subprocess.run(
            ["cargo", "llvm-cov"],
            capture_output=True,
            text=True,
            check=False,
        )
        return parse_coverage_output(result.stdout)
    except Exception:
        return "Unknown"


def parse_coverage_output(output: str) -> str:
    """Parses the output of cargo llvm-cov to extract the total coverage.

    Args:
        output (str): The stdout string from the cargo llvm-cov command.

    Returns:
        str: The parsed coverage percentage, or 'Unknown' if not found.
    """
    for line in output.splitlines():
        if line.startswith("TOTAL"):
            parts = line.split()
            if len(parts) >= 10:
                return parts[9]
    return "Unknown"


def update_readme(readme_path: Path, doc_cov: str, test_cov: str) -> None:
    """Updates the README.md file with the provided coverage values.

    If the badges already exist, they are replaced with the new values.
    If they do not exist, they are inserted at the beginning of the file.

    Args:
        readme_path (Path): The pathlib.Path to the README.md file.
        doc_cov (str): The documentation coverage percentage to inject.
        test_cov (str): The test coverage percentage to inject.
    """
    if not readme_path.exists():
        return

    content = readme_path.read_text(encoding="utf-8")

    doc_badge = f"[![Doc Coverage](https://img.shields.io/badge/Doc_Coverage-{doc_cov.replace('%', '%25')}-brightgreen.svg)]()"
    test_badge = f"[![Test Coverage](https://img.shields.io/badge/Test_Coverage-{test_cov.replace('%', '%25')}-brightgreen.svg)]()"

    has_doc = "Doc Coverage" in content
    has_test = "Test Coverage" in content

    lines = content.splitlines()

    if not has_doc and not has_test:
        lines.insert(1, doc_badge)
        lines.insert(2, test_badge)
    else:
        for i, line in enumerate(lines):
            if "![Doc Coverage]" in line:
                lines[i] = doc_badge
            elif "![Test Coverage]" in line:
                lines[i] = test_badge

    readme_path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def get_doc_coverage() -> str:
    """Runs cargo rustdoc to get documentation coverage.

    Returns:
        str: The documentation coverage percentage, or 'Unknown' if unavailable.
    """
    try:
        subprocess.run(
            ["cargo", "rustdoc", "--", "-Z", "unstable-options", "--show-coverage"],
            capture_output=True,
            text=True,
            check=False,
        )
        doc_path = Path("target/doc/migratory.txt")
        if not doc_path.exists():
            return "Unknown"

        content = doc_path.read_text(encoding="utf-8")
        for line in reversed(content.splitlines()):
            if line.startswith("| Total"):
                parts = [p.strip() for p in line.split("|") if p.strip()]
                if len(parts) >= 3:
                    return parts[2]
        return "Unknown"
    except Exception:
        return "Unknown"


def main() -> None:
    """Main entry point for the script."""
    test_cov = get_test_coverage()
    doc_cov = get_doc_coverage()
    readme_path = Path("README.md")
    update_readme(readme_path, doc_cov, test_cov)


if __name__ == "__main__":  # pragma: no cover
    main()
