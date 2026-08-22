import unittest
from pathlib import Path

from execution.paths import HarnessPaths


class HarnessPathsTests(unittest.TestCase):
    def test_fixed_output_paths(self) -> None:
        paths = HarnessPaths.from_org_project("org/project", Path("/opt/gluon/org"))

        self.assertEqual(paths.repo, Path("/opt/gluon/org/project/project"))
        self.assertEqual(paths.build_report, Path("/opt/gluon/org/build-report"))
        self.assertEqual(paths.extraction_db, Path("/opt/gluon/org/extraction.db"))
        self.assertEqual(paths.business_kg_db, Path("/opt/gluon/org/business-kg.db"))
        self.assertEqual(
            paths.characterization_output_dir,
            Path("/opt/gluon/org/project/project/gluon/tests"),
        )


if __name__ == "__main__":
    unittest.main()

