"""Deterministic tests of the per-file and per-component LLVM inventory."""

from pathlib import Path
import runpy
import tempfile
import unittest

MODULE = runpy.run_path(str(Path(__file__).with_name("coverage-inventory.py")))
generate = MODULE["generate"]
relative_path = MODULE["relative_path"]
component = MODULE["component"]


class CoverageInventoryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path = Path(self.tmp.name) / "coverage.lcov"

    def lcov(self, *records: tuple[str, int, int]) -> Path:
        self.path.write_text("".join(
            f"SF:{name}\nLF:{lines}\nLH:{hit}\nend_of_record\n"
            for name, lines, hit in records
        ), encoding="utf-8")
        return self.path

    def test_cross_platform_paths_and_component_keys(self) -> None:
        self.assertEqual(relative_path(r"C:\ci\src\crates\mix\src\lib.rs"), "crates/mix/src/lib.rs")
        self.assertEqual(relative_path("/home/runner/work/apps/soundcraft/src/main.rs"), "apps/soundcraft/src/main.rs")
        self.assertEqual(relative_path("xtask/src/main.rs"), "xtask/src/main.rs")
        self.assertEqual(component("crates/mix/src/lib.rs"), "crates/mix")
        self.assertEqual(component("apps/soundcraft/src/main.rs"), "apps/soundcraft")
        self.assertEqual(component("xtask/src/main.rs"), "xtask")

    def test_exact_totals_order_and_component_aggregation(self) -> None:
        result = generate(self.lcov(
            ("crates/mix/src/a.rs", 10, 2),
            ("crates/mix/src/b.rs", 20, 15),
            ("apps/soundcraft/src/main.rs", 5, 5),
        ))
        self.assertIn("**Workspace: 22/35 lines (62.86%), 13 lines not executed.**", result)
        self.assertIn("**Source files measured: 3; components measured: 2.**", result)
        self.assertIn("| `crates/mix` | 17 / 30 | 56.67% | 13 |", result)
        self.assertIn("| `apps/soundcraft` | 5 / 5 | 100.00% | 0 |", result)
        self.assertLess(result.index("| `crates/mix/src/a.rs`"), result.index("| `crates/mix/src/b.rs`"))
        self.assertIn("not** proof of behavioral correctness", result)

    def test_output_is_stable_independent_of_record_order(self) -> None:
        records = [
            ("crates/z/src/lib.rs", 3, 1),
            ("crates/a/src/lib.rs", 3, 1),
            ("apps/tool/src/main.rs", 8, 8),
        ]
        one = generate(self.lcov(*records))
        two = generate(self.lcov(*reversed(records)))
        self.assertEqual(one, two)

    def test_rejects_duplicate_after_absolute_path_normalization(self) -> None:
        with self.assertRaisesRegex(ValueError, "duplicate source record|source recorded twice"):
            generate(self.lcov(
                ("/checkout/crates/mix/src/lib.rs", 10, 9),
                ("crates/mix/src/lib.rs", 10, 9),
            ))

    def test_missing_corrupt_and_zero_coverage_evidence_fail_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "missing or empty"):
            generate(self.path)
        with self.assertRaisesRegex(ValueError, "no instrumented source lines"):
            generate(self.lcov(("crates/mix/src/lib.rs", 0, 0)))
        with self.assertRaisesRegex(ValueError, "impossible coverage count"):
            generate(self.lcov(("crates/mix/src/lib.rs", 1, 2)))


if __name__ == "__main__":
    unittest.main()
