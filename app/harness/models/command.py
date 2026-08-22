from dataclasses import asdict, dataclass


@dataclass(frozen=True)
class CommandResult:
    command: list[str]
    cwd: str | None
    exit_code: int
    stdout: str
    stderr: str
    elapsed_ms: int

    @property
    def ok(self) -> bool:
        return self.exit_code == 0

    def to_dict(self) -> dict[str, object]:
        return asdict(self)

