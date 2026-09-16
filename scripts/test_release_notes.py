#!/usr/bin/env python3
"""Exercise the release reader's boundary independently of publication."""
import sys
import unittest

sys.dont_write_bytecode = True
from render_release_notes import render


class ReleaseNotesTests(unittest.TestCase):
    def test_selects_exact_dated_section_without_other_versions(self):
        text = "## [Unreleased]\n\n- Future.\n\n## [1.2.3] - 2026-09-16\n\n### Added\n\n- Exact change.\n\n## [1.2.2] - 2026-09-15\n\n- Old.\n"
        result = render("1.2.3", text)
        self.assertIn("- Exact change.", result)
        self.assertNotIn("Future", result)
        self.assertNotIn("Old.", result)
        self.assertIn("SHA-256", result)

    def test_missing_or_undated_section_refuses(self):
        for text in ["## [1.2.30] - 2026-09-16\n- Wrong", "## [1.2.3]\n- Undated"]:
            with self.assertRaisesRegex(ValueError, "exactly one dated"):
                render("1.2.3", text)

    def test_empty_and_heading_only_sections_refuse(self):
        for body in ["", "\n\n", "\n### Added\n"]:
            with self.assertRaisesRegex(ValueError, "no release notes"):
                render("1.2.3", "## [1.2.3] - 2026-09-16\n" + body)

    def test_duplicate_section_refuses(self):
        with self.assertRaisesRegex(ValueError, "exactly one dated"):
            render("1.2.3", "## [1.2.3] - 2026-09-16\n- One\n" * 2)

    def test_encoding_artifacts_refuse_but_typography_survives(self):
        prefix = "## [1.2.3] - 2026-09-16\n- "
        for artifact in ["\u00e2\u20ac\u201d", "\u0081"]:
            with self.assertRaisesRegex(ValueError, "encoding artifacts"):
                render("1.2.3", prefix + artifact)
        self.assertIn("\u2014", render("1.2.3", prefix + "Text \u2014 valid."))


if __name__ == "__main__":
    unittest.main()
