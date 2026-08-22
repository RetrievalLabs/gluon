from pathlib import Path

from errors import StageFailedError
from execution.commands import CommandRunner
from integrations.claude_agent import ClaudeAgentClient
from models.stage import Stage, StageResult


def run_stage_with_repair(
    stage: Stage,
    runner: CommandRunner,
    agent: ClaudeAgentClient,
    repo_path: Path,
    max_attempts: int,
    env: dict[str, str],
) -> StageResult:
    attempts = 0
    while True:
        result = runner.run(stage.command, cwd=Path(stage.cwd), env=env)
        if result.ok:
            return StageResult(stage.name, attempts, result)
        if attempts >= max_attempts:
            raise StageFailedError(
                f"{stage.name} failed after {max_attempts} repair attempts"
            )
        attempts += 1
        agent.repair_stage(stage.name, attempts, repo_path, result)

