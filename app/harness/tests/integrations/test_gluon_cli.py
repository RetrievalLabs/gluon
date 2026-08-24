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


if __name__ == "__main__":
    unittest.main()
