from dataclasses import asdict, dataclass


@dataclass(frozen=True)
class AgentAttempt:
    stage_name: str
    attempt: int
    status: str
    message: str

    def to_dict(self) -> dict[str, object]:
        return asdict(self)

