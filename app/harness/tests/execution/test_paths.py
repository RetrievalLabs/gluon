import unittest
from pathlib import Path

from execution.paths import HarnessPaths


class HarnessPathsTests(unittest.TestCase):
    def test_fixed_output_paths(self) -> None:
        paths = HarnessPaths.from_org_project("org/project", Path("/opt/gluon/org"))

        self.assertEqual(paths.repo, Path("/opt/gluon/org/project/project"))
        self.assertEqual(paths.rewrite_workspace, Path("/opt/gluon/org/rewrite/project"))
        self.assertEqual(
            paths.rewrite_docs_dir,
            Path("/opt/gluon/org/rewrite/project/docs/migration"),
        )
        self.assertEqual(
            paths.legacy_tree,
            Path("/opt/gluon/org/rewrite/project/docs/migration/legacy-tree"),
        )
        self.assertEqual(
            paths.dependency_selection_report,
            Path("/opt/gluon/org/rewrite/project/docs/migration/dependency-selection.md"),
        )
        self.assertEqual(
            paths.build_structure_report,
            Path("/opt/gluon/org/rewrite/project/docs/migration/build-structure.md"),
        )
        self.assertEqual(
            paths.source_migration_report,
            Path("/opt/gluon/org/rewrite/project/docs/migration/source-migration.md"),
        )
        self.assertEqual(
            paths.build_report,
            Path("/opt/gluon/org/build-report/project/build-report.json"),
        )
        self.assertEqual(
            paths.build_report_output_dir,
            Path("/opt/gluon/org/build-report"),
        )
        self.assertEqual(
            paths.compatibility_report,
            Path("/opt/gluon/org/compatibility-report/project/compatibility-report.json"),
        )
        self.assertEqual(
            paths.compatibility_report_output_dir,
            Path("/opt/gluon/org/compatibility-report"),
        )
        self.assertEqual(paths.extraction_db, Path("/opt/gluon/org/extraction.db"))
        self.assertEqual(paths.business_kg_db, Path("/opt/gluon/org/business-kg.db"))
        self.assertEqual(
            paths.characterization_output_dir,
            Path("/opt/gluon/org/project/project/gluon/tests"),
        )


if __name__ == "__main__":
    unittest.main()
