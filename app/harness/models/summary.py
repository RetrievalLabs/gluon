from dataclasses import asdict, dataclass, field


@dataclass(frozen=True)
class RunSummary:
    status: str
    completed_stages: list[str] = field(default_factory=list)
    failed_stage: str | None = None
    attempts_used: int = 0
    message: str = ""

    def to_dict(self) -> dict[str, object]:
        return asdict(self)

