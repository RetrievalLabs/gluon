import unittest
from pathlib import Path

from execution.paths import HarnessPaths
from integrations.gluon_cli import GluonCli


class GluonCliTests(unittest.TestCase):
    def test_builds_parse_build_command(self) -> None:
        paths = HarnessPaths.from_org_project("org/project", Path("/opt/gluon/org"))
        command = GluonCli("gluon-cli", paths).parse_build()

        self.assertEqual(command[:3], ["gluon-cli", "code-parser", "parse-build"])
        self.assertIn("--resolve", command)
        self.assertIn("--format", command)
        self.assertIn("--output-dir", command)
        self.assertIn("/opt/gluon/org/build-report", command)

    def test_builds_analyze_report_command_with_output_dir(self) -> None:
        paths = HarnessPaths.from_org_project("org/project", Path("/opt/gluon/org"))
        command = GluonCli("gluon-cli", paths).analyze_report("25")

        self.assertEqual(command[:3], ["gluon-cli", "code-parser", "analyze-report"])
        self.assertIn(str(paths.build_report), command)
        self.assertIn("--output-dir", command)
        self.assertIn("/opt/gluon/org/compatibility-report", command)

    def test_builds_classify_models_command_with_build_report(self) -> None:
        paths = HarnessPaths.from_org_project("org/project", Path("/opt/gluon/org"))
        command = GluonCli("gluon-cli", paths).classify_models()

        self.assertEqual(command[:3], ["gluon-cli", "code-parser", "classify-models"])
        self.assertIn("--build-report", command)
        self.assertIn(str(paths.build_report), command)
        self.assertIn("--source-path", command)
        self.assertIn(str(paths.repo), command)
        self.assertIn("--output-dir", command)
        self.assertIn("/opt/gluon/org/model-classification-report", command)

    def test_builds_classify_configs_command_with_build_report(self) -> None:
        paths = HarnessPaths.from_org_project("org/project", Path("/opt/gluon/org"))
        command = GluonCli("gluon-cli", paths).classify_configs()

        self.assertEqual(command[:3], ["gluon-cli", "code-parser", "classify-configs"])
        self.assertIn("--build-report", command)
        self.assertIn(str(paths.build_report), command)
        self.assertIn("--source-path", command)
        self.assertIn(str(paths.repo), command)
        self.assertIn("--output-dir", command)
        self.assertIn("/opt/gluon/org/configuration-classification-report", command)

    def test_builds_extract_business_command_with_build_report(self) -> None:
        paths = HarnessPaths.from_org_project("org/project", Path("/opt/gluon/org"))
        command = GluonCli("gluon-cli", paths).extract_business()

        self.assertEqual(command[:3], ["gluon-cli", "code-parser", "extract-business"])
        self.assertIn("--output-dir", command)
        self.assertIn(str(paths.root), command)
        self.assertIn("--build-report", command)
        self.assertIn(str(paths.build_report), command)


if __name__ == "__main__":
    unittest.main()
