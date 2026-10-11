"""Deterministic unit checks for the LCOV gate itself (no Rust build required)."""

from pathlib import Path
import runpy
import tempfile
import unittest

MODULE = runpy.run_path(str(Path(__file__).with_name("check-coverage.py")))
read_lcov = MODULE["read_lcov"]
audit = MODULE["audit"]


class LcovEvidenceTests(unittest.TestCase):
    def write(self, text: str) -> Path:
        self.addCleanup(self.tmp.cleanup)
        path = Path(self.tmp.name) / "coverage.lcov"
        path.write_text(text, encoding="utf-8")
        return path

    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()

    def test_parses_complete_record_and_normalizes_absolute_source(self) -> None:
        path = self.write(
            "TN:\nSF:/tmp/checkout/crates/playback/src/record.rs\n"
            "DA:1,1\nLF:10\nLH:9\nend_of_record\n"
        )
        self.assertEqual(read_lcov(path), {"crates/playback/src/record.rs": (10, 9)})

    def test_duplicate_source_is_not_silently_counted_twice(self) -> None:
        path = self.write(
            "SF:crates/playback/src/lib.rs\nLF:10\nLH:9\nend_of_record\n"
            "SF:/tmp/checkout/crates/playback/src/lib.rs\nLF:10\nLH:9\nend_of_record\n"
        )
        with self.assertRaisesRegex(ValueError, "duplicate source"):
            read_lcov(path)

    def test_missing_and_incomplete_records_are_rejected(self) -> None:
        with self.assertRaisesRegex(ValueError, "missing or empty"):
            read_lcov(Path(self.tmp.name) / "missing.lcov")
        path = self.write("SF:src/main.rs\nLF:10\nend_of_record\n")
        with self.assertRaisesRegex(ValueError, "incomplete LCOV"):
            read_lcov(path)

    def test_impossible_counts_and_unterminated_records_are_rejected(self) -> None:
        path = self.write("SF:src/main.rs\nLF:2\nLH:3\nend_of_record\n")
        with self.assertRaisesRegex(ValueError, "impossible coverage count"):
            read_lcov(path)
        path = self.write("SF:src/main.rs\nLF:2\nLH:1\n")
        with self.assertRaisesRegex(ValueError, "truncated LCOV"):
            read_lcov(path)

    def test_passing_overall_ratio_is_not_enough_if_critical_modules_are_missing(self) -> None:
        path = self.write("SF:src/large.rs\nLF:100\nLH:100\nend_of_record\n")
        self.assertEqual(audit(path), 1, "missing critical modules must fail the gate")

    def test_empty_instrumentation_is_rejected(self) -> None:
        path = self.write("SF:src/empty.rs\nLF:0\nLH:0\nend_of_record\n")
        with self.assertRaisesRegex(ValueError, "zero instrumented lines"):
            audit(path)


if __name__ == "__main__":
    unittest.main()
