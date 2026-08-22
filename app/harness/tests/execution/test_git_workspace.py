import unittest
from pathlib import Path

from execution.git_workspace import GitWorkspace
from models.config import RepoInfo


class FakeRunner:
    def __init__(self) -> None:
        self.commands = []

    def run(self, command, cwd=None, env=None):
        self.commands.append((command, cwd))


class GitWorkspaceTests(unittest.TestCase):
    def test_prepares_migration_branch(self) -> None:
        runner = FakeRunner()
        repo = RepoInfo("https://repo.test/project", "main")

        GitWorkspace(runner).prepare(repo, Path("/tmp/project"), "25")

        self.assertEqual(
            runner.commands,
            [
                (["git", "clone", "https://repo.test/project", "/tmp/project"], None),
                (["git", "checkout", "main"], Path("/tmp/project")),
                (["git", "checkout", "-B", "gluon/java-25"], Path("/tmp/project")),
            ],
        )


if __name__ == "__main__":
    unittest.main()

