"""Tests for scripts in the scripts/ directory."""

from __future__ import annotations

import json
import subprocess
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

import check_cli
import find_missing
import find_missing_flags
import update_shields

NL = chr(10)


class TestUpdateShields(unittest.TestCase):
    """Test cases for the update_shields module."""

    def test_parse_coverage_output_valid(self) -> None:
        """Test parsing valid cargo llvm-cov output."""
        output = "TOTAL 1651 306 81.47% 221 55 75.11% 955 176 81.57% 0 0 -"
        self.assertEqual(update_shields.parse_coverage_output(output), "81.57%")

    def test_parse_coverage_output_no_total(self) -> None:
        """Test parsing output that lacks the TOTAL prefix line."""
        output = NL.join(["Some other line", "Another line"])
        self.assertEqual(update_shields.parse_coverage_output(output), "Unknown")

    def test_parse_coverage_output_invalid(self) -> None:
        """Test parsing invalid output (too short)."""
        output = "TOTAL 1 2 3"
        self.assertEqual(update_shields.parse_coverage_output(output), "Unknown")

    def test_parse_coverage_output_empty(self) -> None:
        """Test parsing empty output."""
        self.assertEqual(update_shields.parse_coverage_output(""), "Unknown")

    @patch("update_shields.shutil.which")
    def test_get_test_coverage_not_installed(self, mock_which: MagicMock) -> None:
        """Test get_test_coverage when cargo-llvm-cov is missing."""
        mock_which.return_value = None
        self.assertEqual(update_shields.get_test_coverage(), "Unknown")

    @patch("update_shields.shutil.which")
    @patch("update_shields.subprocess.run")
    def test_get_test_coverage_success(
        self, mock_run: MagicMock, mock_which: MagicMock
    ) -> None:
        """Test get_test_coverage on successful execution."""
        mock_which.return_value = "/usr/bin/cargo-llvm-cov"
        mock_result = MagicMock()
        mock_result.stdout = "TOTAL 1651 306 81.47% 221 55 75.11% 955 176 99.99% 0 0 -"
        mock_run.return_value = mock_result
        self.assertEqual(update_shields.get_test_coverage(), "99.99%")
        self.assertEqual(mock_run.call_count, 2)
        mock_run.assert_any_call(
            ["cargo", "llvm-cov", "clean"],
            capture_output=True,
            text=True,
            check=False,
        )
        mock_run.assert_any_call(
            ["cargo", "llvm-cov", "--", "--test-threads=1"],
            capture_output=True,
            text=True,
            check=False,
        )

    @patch("update_shields.shutil.which")
    @patch("update_shields.subprocess.run")
    def test_get_test_coverage_exception(
        self, mock_run: MagicMock, mock_which: MagicMock
    ) -> None:
        """Test get_test_coverage on exception."""
        mock_which.return_value = "/usr/bin/cargo-llvm-cov"
        mock_run.side_effect = Exception("Simulated error")
        self.assertEqual(update_shields.get_test_coverage(), "Unknown")

    @patch("update_shields.subprocess.run")
    def test_get_doc_coverage_stdout_success(self, mock_run: MagicMock) -> None:
        """Test get_doc_coverage extracting percentage from stdout."""
        mock_result = MagicMock()
        mock_result.stdout = NL.join(
            ["+-------+", "| Total | 1273 | 100.0% | 0 | 0.0% |", "+-------+"]
        )
        mock_run.return_value = mock_result
        self.assertEqual(update_shields.get_doc_coverage(), "100.0%")

    @patch("update_shields.Path.exists")
    @patch("update_shields.Path.read_text")
    @patch("update_shields.subprocess.run")
    def test_get_doc_coverage_file_fallback(
        self, mock_run: MagicMock, mock_read: MagicMock, mock_exists: MagicMock
    ) -> None:
        """Test get_doc_coverage extracting percentage from target/doc/migratory.txt when stdout is empty."""
        mock_result = MagicMock()
        mock_result.stdout = ""
        mock_run.return_value = mock_result
        mock_exists.return_value = True
        mock_read.return_value = NL.join(
            ["+-------+", "| Total | 500 | 98.5% | 0 | 0.0% |", "+-------+"]
        )
        self.assertEqual(update_shields.get_doc_coverage(), "98.5%")

    @patch("update_shields.Path.exists")
    @patch("update_shields.subprocess.run")
    def test_get_doc_coverage_no_total(
        self, mock_run: MagicMock, mock_exists: MagicMock
    ) -> None:
        """Test get_doc_coverage when output has no total line."""
        mock_exists.return_value = False
        mock_result = MagicMock()
        mock_result.stdout = NL.join(["Header line", "No matching summary"])
        mock_run.return_value = mock_result
        self.assertEqual(update_shields.get_doc_coverage(), "Unknown")

    @patch("update_shields.Path.exists")
    @patch("update_shields.subprocess.run")
    def test_get_doc_coverage_short_total_line(
        self, mock_run: MagicMock, mock_exists: MagicMock
    ) -> None:
        """Test get_doc_coverage when a '| Total' line has fewer than 3 columns."""
        mock_exists.return_value = False
        mock_result = MagicMock()
        mock_result.stdout = "| Total | 100 |" + NL
        mock_run.return_value = mock_result
        self.assertEqual(update_shields.get_doc_coverage(), "Unknown")

    @patch("update_shields.subprocess.run")
    def test_get_doc_coverage_exception(self, mock_run: MagicMock) -> None:
        """Test get_doc_coverage when an exception is raised."""
        mock_run.side_effect = Exception("rustdoc error")
        self.assertEqual(update_shields.get_doc_coverage(), "Unknown")

    def test_update_readme_no_file(self) -> None:
        """Test update_readme when README does not exist."""
        update_shields.update_readme(Path("does_not_exist_file.md"), "100%", "50%")

    def test_update_readme_insert_new(self) -> None:
        """Test update_readme inserting new shields."""
        with tempfile.NamedTemporaryFile(mode="w+", delete=False) as f:
            f.write(NL.join(["# Project", "", "Some text."]))
            filepath = Path(f.name)

        try:
            update_shields.update_readme(filepath, "100%", "81.57%")
            content = filepath.read_text(encoding="utf-8")
            self.assertIn("Doc_Coverage-100%25", content)
            self.assertIn("Test_Coverage-81.57%25", content)
        finally:
            filepath.unlink()

    def test_update_readme_replace_existing(self) -> None:
        """Test update_readme replacing existing shields."""
        with tempfile.NamedTemporaryFile(mode="w+", delete=False) as f:
            f.write(
                NL.join(
                    [
                        "# Project",
                        "[![Doc Coverage](...)]()",
                        "[![Test Coverage](...)]()",
                        "Text.",
                    ]
                )
            )
            filepath = Path(f.name)

        try:
            update_shields.update_readme(filepath, "100%", "90.00%")
            content = filepath.read_text(encoding="utf-8")
            self.assertIn("Doc_Coverage-100%25", content)
            self.assertIn("Test_Coverage-90.00%25", content)
            self.assertEqual(content.count("Doc Coverage"), 1)
            self.assertEqual(content.count("Test Coverage"), 1)
        finally:
            filepath.unlink()

    def test_update_readme_unchanged(self) -> None:
        """Test update_readme when shields are already up to date."""
        initial = (
            NL.join(
                [
                    "# Project",
                    "[![Doc Coverage](https://img.shields.io/badge/Doc_Coverage-100%25-brightgreen.svg)]()",
                    "[![Test Coverage](https://img.shields.io/badge/Test_Coverage-90.00%25-brightgreen.svg)]()",
                    "Text.",
                ]
            )
            + NL
        )
        with tempfile.NamedTemporaryFile(mode="w+", delete=False) as f:
            f.write(initial)
            filepath = Path(f.name)

        try:
            update_shields.update_readme(filepath, "100%", "90.00%")
            content = filepath.read_text(encoding="utf-8")
            self.assertEqual(content, initial)
        finally:
            filepath.unlink()

    @patch("update_shields.update_readme")
    @patch("update_shields.get_doc_coverage")
    @patch("update_shields.get_test_coverage")
    def test_main(
        self, mock_get_cov: MagicMock, mock_get_doc: MagicMock, mock_update: MagicMock
    ) -> None:
        """Test the main entry point function."""
        mock_get_cov.return_value = "80%"
        mock_get_doc.return_value = "100%"
        update_shields.main()
        mock_update.assert_called_once_with(Path("README.md"), "100%", "80%")


class TestCheckCli(unittest.TestCase):
    """Test cases for the check_cli module."""

    @patch("check_cli.subprocess.check_output")
    def test_get_vagrant_top_level_commands_success(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test parsing top-level commands from vagrant list-commands."""
        mock_check_output.return_value = NL.join(
            [
                "Below is a listing of all available Vagrant commands",
                "",
                "box             manages boxes",
                "up              starts and provisions",
                "-h              help flag",
                "description of commands",
                "To see all subcommands, run...",
            ]
        )
        cmds = check_cli.get_vagrant_top_level_commands()
        self.assertEqual(cmds, ["box", "up"])

    @patch("check_cli.subprocess.check_output")
    def test_get_vagrant_top_level_commands_called_process_error(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test handling CalledProcessError when listing Vagrant commands."""
        err = subprocess.CalledProcessError(1, ["vagrant", "list-commands"])
        err.output = "halt   stops the machine" + NL
        mock_check_output.side_effect = err
        cmds = check_cli.get_vagrant_top_level_commands()
        self.assertEqual(cmds, ["halt"])

    @patch("check_cli.sys.exit")
    @patch("check_cli.subprocess.check_output")
    def test_get_vagrant_top_level_commands_not_found(
        self, mock_check_output: MagicMock, mock_exit: MagicMock
    ) -> None:
        """Test exiting when Vagrant executable is not installed."""
        mock_check_output.side_effect = FileNotFoundError()
        mock_exit.side_effect = SystemExit(1)
        with self.assertRaises(SystemExit):
            check_cli.get_vagrant_top_level_commands()
        mock_exit.assert_called_once_with(1)

    @patch("check_cli.subprocess.check_output")
    def test_get_vagrant_subcommands_success_and_breaks(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test parsing Vagrant subcommands and stopping at footer/options."""
        mock_check_output.return_value = NL.join(
            [
                "Usage: vagrant box <subcommand>",
                "Available subcommands:",
                "",
                "-v",
                "  -h, --help",
                "     add",
                "     list",
                "For help on any individual subcommand",
                "     ignored_after_break",
            ]
        )
        cmds = check_cli.get_vagrant_subcommands(["vagrant", "box"])
        self.assertEqual(cmds, ["add", "list"])

    @patch("check_cli.subprocess.check_output")
    def test_get_vagrant_subcommands_called_process_error_and_options_break(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test CalledProcessError fallback and breaking on Options:."""
        err = subprocess.CalledProcessError(1, ["vagrant", "cloud", "--help"])
        err.output = NL.join(
            [
                "Available subcommands:",
                "     auth",
                "Options:",
                "     after_options",
            ]
        )
        mock_check_output.side_effect = err
        cmds = check_cli.get_vagrant_subcommands(["vagrant", "cloud"])
        self.assertEqual(cmds, ["auth"])

    @patch("check_cli.subprocess.check_output")
    def test_get_vagrant_subcommands_no_footer(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test parsing Vagrant subcommands when output ends without footer."""
        mock_check_output.return_value = NL.join(
            ["Available subcommands:", "     remove"]
        )
        cmds = check_cli.get_vagrant_subcommands(["vagrant", "box"])
        self.assertEqual(cmds, ["remove"])

    @patch("check_cli.subprocess.check_output")
    def test_get_vagrant_subcommands_file_not_found(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test returning empty list when Vagrant binary is missing."""
        mock_check_output.side_effect = FileNotFoundError()
        self.assertEqual(check_cli.get_vagrant_subcommands(["vagrant", "box"]), [])

    @patch("check_cli.get_vagrant_subcommands")
    @patch("check_cli.get_vagrant_top_level_commands")
    def test_build_vagrant_tree(
        self, mock_top: MagicMock, mock_sub: MagicMock
    ) -> None:
        """Test recursive building of the Vagrant command tree."""
        mock_top.return_value = ["box"]
        mock_sub.side_effect = lambda cmd: ["add"] if cmd == ["vagrant", "box"] else []
        tree = check_cli.build_vagrant_tree(["vagrant"])
        self.assertEqual(tree, {"box": {"add": {}}})

    @patch("check_cli.subprocess.check_output")
    def test_get_migratory_subcommands_success(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test parsing Migratory subcommands from clap help output."""
        mock_check_output.return_value = NL.join(
            [
                "Usage: migratory [COMMAND]",
                "",
                "Commands:",
                "",
                "  -f, --flag",
                "  !!!invalid line",
                "  up      Starts the machine",
                "  halt    Stops the machine",
                "Options:",
                "  -h, --help",
            ]
        )
        cmds = check_cli.get_migratory_subcommands(["cargo", "run", "--"])
        self.assertEqual(cmds, ["up", "halt"])

    @patch("check_cli.subprocess.check_output")
    def test_get_migratory_subcommands_error_and_eof(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test handling CalledProcessError and reaching EOF in Commands section."""
        err = subprocess.CalledProcessError(1, ["cargo"])
        err.output = NL.join(["Commands:", "  box   Manages boxes"])
        mock_check_output.side_effect = err
        cmds = check_cli.get_migratory_subcommands(["cargo"])
        self.assertEqual(cmds, ["box"])

    @patch("check_cli.get_migratory_subcommands")
    def test_build_migratory_tree(self, mock_sub: MagicMock) -> None:
        """Test building Migratory command tree while skipping 'help'."""
        mock_sub.side_effect = (
            lambda cmd: ["up", "help"] if cmd == ["migratory"] else []
        )
        tree = check_cli.build_migratory_tree(["migratory"])
        self.assertEqual(tree, {"up": {}})

    def test_compare_trees(self) -> None:
        """Test comparing Migratory tree against Vagrant tree for hallucinations."""
        vag = {"box": {"add": {}}, "up": {}}
        mig = {"box": {"add": {}, "fake-sub": {}}, "up": {}, "fake-top": {}}
        errors = check_cli.compare_trees(mig, vag)
        self.assertEqual(len(errors), 2)
        self.assertIn("box fake-sub", errors[0])
        self.assertIn("fake-top", errors[1])

    @patch("check_cli.build_migratory_tree")
    @patch("check_cli.build_vagrant_tree")
    def test_main_creates_json_and_passes(
        self, mock_vag_tree: MagicMock, mock_mig_tree: MagicMock
    ) -> None:
        """Test check_cli.main when vagrant_cli.json does not exist and validation passes."""
        with tempfile.TemporaryDirectory() as tmpdir:
            json_path = str(Path(tmpdir) / "vagrant_cli.json")
            mock_vag_tree.return_value = {"up": {}}
            mock_mig_tree.return_value = {"up": {}}
            with patch.object(check_cli, "VAGRANT_CLI_JSON", json_path):
                check_cli.main()
                self.assertTrue(Path(json_path).exists())

    @patch("check_cli.sys.exit")
    @patch("check_cli.build_migratory_tree")
    def test_main_loads_json_and_fails(
        self, mock_mig_tree: MagicMock, mock_exit: MagicMock
    ) -> None:
        """Test check_cli.main when vagrant_cli.json exists and validation fails."""
        with tempfile.NamedTemporaryFile(mode="w+", suffix=".json", delete=False) as f:
            json.dump({"up": {}}, f)
            json_path = f.name

        try:
            mock_mig_tree.return_value = {"hallucinated": {}}
            mock_exit.side_effect = SystemExit(1)
            with patch.object(check_cli, "VAGRANT_CLI_JSON", json_path):
                with self.assertRaises(SystemExit):
                    check_cli.main()
            mock_exit.assert_called_once_with(1)
        finally:
            Path(json_path).unlink()


class TestFindMissing(unittest.TestCase):
    """Test cases for the find_missing module."""

    @patch("find_missing.subprocess.check_output")
    def test_get_migratory_subcommands_all_branches(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test parsing Migratory subcommands across all conditional branches."""
        mock_check_output.return_value = NL.join(
            [
                "Header",
                "Commands:",
                "",
                "  -h",
                "  ???invalid",
                "  up    Start VM",
                "Options:",
                "  --help",
            ]
        )
        self.assertEqual(find_missing.get_migratory_subcommands(["mig"]), ["up"])

        err = subprocess.CalledProcessError(1, ["mig"])
        err.output = NL.join(["Commands:", "  halt  Stop VM"])
        mock_check_output.side_effect = err
        self.assertEqual(find_missing.get_migratory_subcommands(["mig"]), ["halt"])

    @patch("find_missing.get_migratory_subcommands")
    def test_build_migratory_tree(self, mock_sub: MagicMock) -> None:
        """Test building Migratory tree in find_missing."""
        mock_sub.side_effect = lambda cmd: ["up", "help"] if cmd == ["mig"] else []
        self.assertEqual(find_missing.build_migratory_tree(["mig"]), {"up": {}})

    def test_find_missing(self) -> None:
        """Test finding missing commands between Vagrant and Migratory trees."""
        v_tree = {"box": {"add": {}, "repackage": {}}, "up": {}, "cloud": {}}
        m_tree = {"box": {"add": {}}, "up": {}}
        missing = find_missing.find_missing(v_tree, m_tree)
        self.assertEqual(missing, ["box repackage", "cloud"])

    @patch("builtins.print")
    @patch("find_missing.build_migratory_tree")
    def test_main(self, mock_mig_tree: MagicMock, mock_print: MagicMock) -> None:
        """Test find_missing.main reading JSON and printing missing commands."""
        with tempfile.NamedTemporaryFile(mode="w+", suffix=".json", delete=False) as f:
            json.dump({"up": {}, "missing-cmd": {}}, f)
            json_path = f.name

        try:
            mock_mig_tree.return_value = {"up": {}}
            with patch.object(find_missing, "VAGRANT_CLI_JSON", json_path):
                find_missing.main()
            mock_print.assert_called_once_with("missing-cmd")
        finally:
            Path(json_path).unlink()


class TestFindMissingFlags(unittest.TestCase):
    """Test cases for the find_missing_flags module."""

    @patch("find_missing_flags.subprocess.check_output")
    def test_get_flags_vagrant_and_migratory(
        self, mock_check_output: MagicMock
    ) -> None:
        """Test extracting CLI flags for both Vagrant and Migratory invocations."""
        mock_check_output.return_value = NL.join(
            [
                "  -f, --force   Force execution",
                "  -h, --help    Print help",
                "  -V, --version Print version",
            ]
        )
        self.assertEqual(
            find_missing_flags.get_flags(["vagrant", "up"], True), {"--force"}
        )
        self.assertEqual(
            find_missing_flags.get_flags(["up"], False), {"--force"}
        )

    @patch("find_missing_flags.subprocess.check_output")
    def test_get_flags_errors(self, mock_check_output: MagicMock) -> None:
        """Test get_flags handling CalledProcessError and FileNotFoundError."""
        err = subprocess.CalledProcessError(1, ["vagrant", "up"])
        err.output = "  --provision" + NL
        mock_check_output.side_effect = err
        self.assertEqual(
            find_missing_flags.get_flags(["vagrant", "up"], True), {"--provision"}
        )

        mock_check_output.side_effect = FileNotFoundError()
        self.assertEqual(find_missing_flags.get_flags(["vagrant", "up"], True), set())

    @patch("find_missing_flags.get_flags")
    def test_walk_tree(self, mock_get_flags: MagicMock) -> None:
        """Test walking command tree to collect missing flags."""
        def side_effect(cmd_args: list[str], is_vagrant: bool) -> set[str]:
            if is_vagrant:
                if cmd_args == ["vagrant", "up"]:
                    return {"--provision", "--parallel"}
                return {"--force"}
            if cmd_args == ["up"]:
                return {"--provision"}
            return {"--force"}

        mock_get_flags.side_effect = side_effect
        tree = {"up": {}, "halt": {}}
        result = find_missing_flags.walk_tree(tree, [])
        self.assertEqual(result, {"up": {"--parallel"}})

    @patch("builtins.print")
    @patch("find_missing_flags.walk_tree")
    def test_main(self, mock_walk: MagicMock, mock_print: MagicMock) -> None:
        """Test find_missing_flags.main printing missing flags."""
        with tempfile.NamedTemporaryFile(mode="w+", suffix=".json", delete=False) as f:
            json.dump({"up": {}}, f)
            json_path = f.name

        try:
            mock_walk.return_value = {"up": {"--parallel"}}
            with patch.object(find_missing_flags, "VAGRANT_CLI_JSON", json_path):
                find_missing_flags.main()
            mock_print.assert_called_once_with(
                "Command 'up' is missing flags: --parallel"
            )
        finally:
            Path(json_path).unlink()


if __name__ == "__main__":  # pragma: no cover
    unittest.main()
