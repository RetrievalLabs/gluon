from pathlib import Path

from errors import StageFailedError
from execution.commands import CommandRunner


def commit_rewrite_workspace(
    runner: CommandRunner,
    rewrite_workspace: Path,
    message: str,
) -> None:
    status = runner.run(["git", "status", "--short"], cwd=rewrite_workspace)
    if not status.stdout.strip():
        return

    add = runner.run(["git", "add", "."], cwd=rewrite_workspace)
    if not add.ok:
        raise StageFailedError("git add failed for migration rewrite workspace")
    commit = runner.run(
        ["git", "commit", "-m", message],
        cwd=rewrite_workspace,
    )
    if not commit.ok:
        raise StageFailedError("git commit failed for migration rewrite workspace")
