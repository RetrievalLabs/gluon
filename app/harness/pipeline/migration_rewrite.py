from pathlib import Path

from errors import StageFailedError
from execution.commands import CommandRunner
from execution.paths import HarnessPaths
from integrations.claude_agent import ClaudeAgentClient
from models.config import RepoInfo


def run_migration_rewrite_setup(
    paths: HarnessPaths,
    repo: RepoInfo,
    target_version: str,
    runner: CommandRunner,
    agent: ClaudeAgentClient,
) -> None:
    paths.rewrite_workspace.mkdir(parents=True, exist_ok=True)
    run_or_fail(runner, ["git", "init"], paths.rewrite_workspace)
    run_or_fail(
        runner,
        ["git", "checkout", "-B", f"gluon/java-{target_version}"],
        paths.rewrite_workspace,
    )
    set_remote_origin(runner, paths.rewrite_workspace, repo.repo_url)

    scaffold_rewrite_workspace(paths)
    tree = run_or_fail(runner, ["tree", str(paths.repo)], paths.rewrite_workspace)
    paths.legacy_tree.write_text(tree.stdout, encoding="utf-8")

    seed_context = {
        "legacy_repo_path": str(paths.repo),
        "rewrite_workspace": str(paths.rewrite_workspace),
        "compatibility_report": str(paths.compatibility_report),
        "legacy_tree_path": str(paths.legacy_tree),
        "target_branch": f"gluon/java-{target_version}",
        "repo_url": repo.repo_url,
        "scaffold": [
            "Makefile",
            ".gitignore",
            "docs/",
            "src/",
            "CLAUDE.md",
            "AGENTS.md",
        ],
    }
    agent.run_migration_rewrite(paths.rewrite_workspace, seed_context)


def scaffold_rewrite_workspace(paths: HarnessPaths) -> None:
    paths.rewrite_docs_dir.mkdir(parents=True, exist_ok=True)
    (paths.rewrite_workspace / "src").mkdir(parents=True, exist_ok=True)
    write_if_missing(paths.rewrite_workspace / "Makefile", "SHELL := /usr/bin/env bash\n")
    write_if_missing(
        paths.rewrite_workspace / ".gitignore",
        ".DS_Store\nbuild/\ntarget/\n.gradle/\n",
    )
    write_if_missing(
        paths.rewrite_workspace / "CLAUDE.md",
        "# CLAUDE.md\n\nPreserve legacy behavior during Java modernization.\n",
    )
    write_if_missing(
        paths.rewrite_workspace / "AGENTS.md",
        "# AGENTS.md\n\nUse compatibility report findings as migration requirements.\n",
    )


def write_if_missing(path: Path, content: str) -> None:
    if not path.exists():
        path.write_text(content, encoding="utf-8")


def set_remote_origin(runner: CommandRunner, cwd: Path, repo_url: str) -> None:
    result = runner.run(["git", "remote", "add", "origin", repo_url], cwd=cwd)
    if result.ok:
        return
    run_or_fail(runner, ["git", "remote", "set-url", "origin", repo_url], cwd)


def run_or_fail(runner: CommandRunner, command: list[str], cwd: Path):
    result = runner.run(command, cwd=cwd)
    if not result.ok:
        raise StageFailedError(
            f"migration rewrite command failed: {' '.join(command)}"
        )
    return result
