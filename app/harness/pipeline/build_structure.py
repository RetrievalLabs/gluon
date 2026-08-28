from pathlib import Path

from errors import StageFailedError
from execution.paths import HarnessPaths
from integrations.claude_agent import ClaudeAgentClient


LEGACY_BUILD_FILE_NAMES = {
    "pom.xml",
    "mvnw",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "gradlew",
    "libs.versions.toml",
}


def run_build_structure_agent(
    paths: HarnessPaths,
    agent: ClaudeAgentClient,
    target_version: str,
) -> None:
    paths.rewrite_docs_dir.mkdir(parents=True, exist_ok=True)
    allowed_legacy_build_files = discover_legacy_build_files(paths.repo)
    agent.run_build_structure(
        paths.rewrite_workspace,
        paths.repo,
        paths.build_report,
        paths.compatibility_report,
        paths.legacy_tree,
        paths.dependency_selection_report,
        paths.build_structure_report,
        target_version,
        allowed_legacy_build_files,
    )
    if not root_build_file_exists(paths.rewrite_workspace):
        raise StageFailedError(
            f"build structure agent did not write a root build file in {paths.rewrite_workspace}"
        )
    if not paths.build_structure_report.exists():
        raise StageFailedError(
            "build structure agent did not write "
            f"{paths.build_structure_report}"
        )


def discover_legacy_build_files(repo_path: Path) -> list[Path]:
    if not repo_path.exists():
        return []
    files = [
        path
        for path in repo_path.rglob("*")
        if path.is_file() and is_legacy_build_file(repo_path, path)
    ]
    return sorted(files)


def is_legacy_build_file(repo_path: Path, path: Path) -> bool:
    relative = path.relative_to(repo_path)
    parts = relative.parts
    if any(part in {".git", "target", "build", ".gradle"} for part in parts):
        return False
    if path.name in LEGACY_BUILD_FILE_NAMES:
        return True
    return parts[:2] == (".mvn", "wrapper") or parts[:2] == ("gradle", "wrapper")


def root_build_file_exists(rewrite_workspace: Path) -> bool:
    return any(
        (rewrite_workspace / name).exists()
        for name in (
            "pom.xml",
            "build.gradle",
            "build.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
        )
    )
