import tempfile
import unittest
from pathlib import Path

from errors import StageFailedError
from models.command import CommandResult
from pipeline.migration_commit import commit_rewrite_workspace


class FakeRunner:
    def __init__(self, status: str = " M pom.xml\n", fail_command: str | None = None) -> None:
        self.status = status
        self.fail_command = fail_command
        self.commands = []

    def run(self, command, cwd=None, env=None):
        self.commands.append((command, cwd))
        stdout = self.status if command == ["git", "status", "--short"] else ""
        exit_code = 1 if command[:2] == ["git", self.fail_command] else 0
        return CommandResult(command, str(cwd) if cwd else None, exit_code, stdout, "", 1)


class MigrationCommitTests(unittest.TestCase):
    def test_commits_dirty_rewrite_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "rewrite"
            runner = FakeRunner()

            commit_rewrite_workspace(runner, workspace, "Commit rewrite changes")

        self.assertEqual(
            runner.commands,
            [
                (["git", "status", "--short"], workspace),
                (["git", "add", "."], workspace),
                (
                    ["git", "commit", "-m", "Commit rewrite changes"],
                    workspace,
                ),
            ],
        )

    def test_skips_clean_rewrite_workspace(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "rewrite"
            runner = FakeRunner(status="")

            commit_rewrite_workspace(runner, workspace, "Commit rewrite changes")

        self.assertEqual(
            runner.commands,
            [(["git", "status", "--short"], workspace)],
        )

    def test_fails_when_add_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "rewrite"

            with self.assertRaises(StageFailedError):
                commit_rewrite_workspace(
                    runner=FakeRunner(" M x\n", "add"),
                    rewrite_workspace=workspace,
                    message="Commit rewrite changes",
                )

    def test_fails_when_commit_fails(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory) / "rewrite"

            with self.assertRaises(StageFailedError):
                commit_rewrite_workspace(
                    runner=FakeRunner(" M x\n", "commit"),
                    rewrite_workspace=workspace,
                    message="Commit rewrite changes",
                )
