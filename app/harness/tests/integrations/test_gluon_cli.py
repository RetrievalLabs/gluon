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


if __name__ == "__main__":
    unittest.main()

