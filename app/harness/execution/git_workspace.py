from pathlib import Path

from execution.commands import CommandRunner
from models.config import RepoInfo


class GitWorkspace:
    def __init__(self, runner: CommandRunner) -> None:
        self.runner = runner

    def prepare(self, repo: RepoInfo, destination: Path, target_version: str) -> None:
        destination.parent.mkdir(parents=True, exist_ok=True)
        self.runner.run(["git", "clone", repo.repo_url, str(destination)])
        self.runner.run(["git", "checkout", repo.source_branch], cwd=destination)
        branch = f"gluon/java-{target_version}"
        self.runner.run(["git", "checkout", "-B", branch], cwd=destination)

