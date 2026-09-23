"""Tests for update_shields.py."""

import unittest
from unittest.mock import patch, MagicMock
from pathlib import Path
import tempfile

import update_shields


class TestUpdateShields(unittest.TestCase):
    """Test cases for the update_shields module."""

    def test_parse_coverage_output_valid(self) -> None:
        """Test parsing valid cargo llvm-cov output."""
        output = "TOTAL 1651 306 81.47% 221 55 75.11% 955 176 81.57% 0 0 -"
        self.assertEqual(update_shields.parse_coverage_output(output), "81.57%")

    def test_parse_coverage_output_no_total(self) -> None:
        """Test parsing output that lacks the TOTAL prefix line."""
        output = "Some other line\nAnother line"
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
        mock_result.stdout = "+-------+\n| Total | 1273 | 100.0% | 0 | 0.0% |\n+-------+"
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
        mock_read.return_value = (
            "+-------+\n| Total | 500 | 98.5% | 0 | 0.0% |\n+-------+"
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
        mock_result.stdout = "Header line\nNo matching summary"
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
            f.write("# Project\n\nSome text.")
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
                "# Project\n[![Doc Coverage](...)]()\n[![Test Coverage](...)]()\nText."
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


if __name__ == "__main__":
    unittest.main()
