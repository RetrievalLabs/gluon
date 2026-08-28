from pathlib import Path

from errors import StageFailedError
from execution.paths import HarnessPaths
from integrations.claude_agent import ClaudeAgentClient


def run_source_migration_agent(
    paths: HarnessPaths,
    agent: ClaudeAgentClient,
    target_version: str,
) -> None:
    paths.rewrite_docs_dir.mkdir(parents=True, exist_ok=True)
    agent.run_source_migration(
        paths.rewrite_workspace,
        paths.repo,
        paths.business_kg_db,
        paths.extraction_db,
        paths.characterization_db,
        paths.characterization_output_dir,
        target_version,
        paths.source_migration_report,
    )
    if not migrated_java_source_exists(paths.rewrite_workspace):
        raise StageFailedError(
            f"source migration agent did not write Java source in {paths.rewrite_workspace}"
        )
    if not paths.source_migration_report.exists():
        raise StageFailedError(
            "source migration agent did not write "
            f"{paths.source_migration_report}"
        )


def migrated_java_source_exists(rewrite_workspace: Path) -> bool:
    if not rewrite_workspace.exists():
        return False
    return any(
        path.is_file()
        and path.suffix == ".java"
        and "docs" not in path.relative_to(rewrite_workspace).parts
        for path in rewrite_workspace.rglob("*.java")
    )
