import json
from pathlib import Path

from models.agent import AgentAttempt
from models.command import CommandResult
from models.config import HarnessConfig


class ClaudeAgentClient:
    def __init__(self, config: HarnessConfig, log_path: Path | None = None) -> None:
        self.config = config
        self.log_path = log_path

    def validate(self) -> None:
        # Import lazily so offline tests can run without the SDK installed.
        __import__("claude_agent_sdk")

    def repair_stage(
        self,
        stage_name: str,
        attempt: int,
        repo_path: Path,
        failed: CommandResult,
    ) -> AgentAttempt:
        message = (
            f"repair requested for {stage_name} attempt {attempt} "
            f"in {repo_path}: exit {failed.exit_code}"
        )
        result = AgentAttempt(
            stage_name=stage_name,
            attempt=attempt,
            status="requested",
            message=message,
        )
        self.record(result)
        return result

    def record(self, attempt: AgentAttempt) -> None:
        if self.log_path is None:
            return
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        with self.log_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(attempt.to_dict(), sort_keys=True))
            handle.write("\n")

