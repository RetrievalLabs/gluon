from dataclasses import dataclass

from models.command import CommandResult


@dataclass(frozen=True)
class Stage:
    name: str
    command: list[str]
    cwd: str


@dataclass(frozen=True)
class StageResult:
    stage_name: str
    attempts: int
    command_result: CommandResult

    @property
    def ok(self) -> bool:
        return self.command_result.ok

