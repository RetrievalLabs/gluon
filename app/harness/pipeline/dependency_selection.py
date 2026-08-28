from errors import StageFailedError
from execution.paths import HarnessPaths
from integrations.claude_agent import ClaudeAgentClient


def run_dependency_selection_agent(
    paths: HarnessPaths,
    agent: ClaudeAgentClient,
    target_version: str,
) -> None:
    paths.rewrite_docs_dir.mkdir(parents=True, exist_ok=True)
    agent.run_dependency_selection(
        paths.rewrite_workspace,
        paths.repo,
        paths.build_report,
        paths.compatibility_report,
        target_version,
        paths.dependency_selection_report,
    )
    if not paths.dependency_selection_report.exists():
        raise StageFailedError(
            "dependency selection agent did not write "
            f"{paths.dependency_selection_report}"
        )
