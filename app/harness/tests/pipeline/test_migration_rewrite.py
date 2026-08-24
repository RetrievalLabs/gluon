import tempfile
import unittest
from pathlib import Path

from execution.paths import HarnessPaths
from models.command import CommandResult
from models.config import RepoInfo
from pipeline.migration_rewrite import run_migration_rewrite_setup


class FakeRunner:
    def __init__(self) -> None:
        self.commands = []

    def run(self, command, cwd=None, env=None):
        self.commands.append((command, cwd))
        stdout = "legacy\n`-- pom.xml\n" if command[0] == "tree" else ""
        return CommandResult(command, str(cwd) if cwd else None, 0, stdout, "", 1)


class MigrationRewriteTests(unittest.TestCase):
    def test_scaffolds_rewrite_workspace_deterministically(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            paths = HarnessPaths.from_org_project("org/project", Path(directory))
            paths.repo.mkdir(parents=True)
            runner = FakeRunner()

            run_migration_rewrite_setup(
                paths,
                RepoInfo("https://repo.test/project", "main"),
                "25",
                runner,
            )

            self.assertEqual(
                runner.commands[:3],
                [
                    (["git", "init"], paths.rewrite_workspace),
                    (
                        ["git", "checkout", "-B", "gluon/java-25"],
                        paths.rewrite_workspace,
                    ),
                    (
                        [
                            "git",
                            "remote",
                            "add",
                            "origin",
                            "https://repo.test/project",
                        ],
                        paths.rewrite_workspace,
                    ),
                ],
            )
            self.assertEqual(
                runner.commands[3],
                (["tree", str(paths.repo)], paths.rewrite_workspace),
            )
            self.assertTrue((paths.rewrite_workspace / "Makefile").exists())
            self.assertTrue((paths.rewrite_workspace / ".gitignore").exists())
            self.assertTrue((paths.rewrite_workspace / "CLAUDE.md").exists())
            self.assertTrue((paths.rewrite_workspace / "AGENTS.md").exists())
            self.assertTrue((paths.rewrite_workspace / "src").is_dir())
            self.assertEqual(
                paths.legacy_tree.read_text(encoding="utf-8"),
                "legacy\n`-- pom.xml\n",
            )


if __name__ == "__main__":
    unittest.main()
