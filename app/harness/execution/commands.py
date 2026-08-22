import json
import subprocess
import time
from pathlib import Path

from models.command import CommandResult


class CommandRunner:
    def __init__(self, log_path: Path | None = None) -> None:
        self.log_path = log_path

    def run(
        self,
        command: list[str],
        cwd: Path | None = None,
        env: dict[str, str] | None = None,
    ) -> CommandResult:
        started = time.monotonic()
        completed = subprocess.run(
            command,
            cwd=str(cwd) if cwd else None,
            env=env,
            text=True,
            capture_output=True,
            check=False,
        )
        elapsed_ms = int((time.monotonic() - started) * 1000)
        result = CommandResult(
            command=command,
            cwd=str(cwd) if cwd else None,
            exit_code=completed.returncode,
            stdout=completed.stdout,
            stderr=completed.stderr,
            elapsed_ms=elapsed_ms,
        )
        self.record(result)
        return result

    def record(self, result: CommandResult) -> None:
        if self.log_path is None:
            return
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        with self.log_path.open("a", encoding="utf-8") as handle:
            handle.write(json.dumps(result.to_dict(), sort_keys=True))
            handle.write("\n")

