import json
from dataclasses import asdict, dataclass
from typing import Any


@dataclass(frozen=True)
class AgentAttempt:
    stage_name: str
    attempt: int
    status: str
    message: str

    def to_dict(self) -> dict[str, object]:
        return asdict(self)


@dataclass(frozen=True)
class AgentVerification:
    command: str
    status: str


@dataclass(frozen=True)
class AgentJsonResponse:
    status: str
    changed_files: list[str]
    verification: list[AgentVerification]
    blockers: list[str]

    def to_dict(self) -> dict[str, object]:
        return asdict(self)

    def to_json(self) -> str:
        return json.dumps(self.to_dict(), separators=(",", ":"))


def parse_agent_json_response(value: str) -> AgentJsonResponse:
    text = value.strip()
    if text.startswith("```") or text.endswith("```"):
        raise ValueError("agent response must be JSON only")

    decoder = json.JSONDecoder()
    try:
        payload, index = decoder.raw_decode(text)
    except json.JSONDecodeError as error:
        raise ValueError("agent response must be valid JSON") from error
    if text[index:].strip():
        raise ValueError("agent response must contain one JSON object")
    if not isinstance(payload, dict):
        raise ValueError("agent response must be a JSON object")

    expected_keys = {"status", "changed_files", "verification", "blockers"}
    if set(payload) != expected_keys:
        raise ValueError("agent response fields do not match schema")

    status = payload["status"]
    if status not in {"completed", "blocked"}:
        raise ValueError("agent response status must be completed or blocked")

    changed_files = required_string_list(payload["changed_files"], "changed_files")
    blockers = required_string_list(payload["blockers"], "blockers")
    verification = parse_verification(payload["verification"])

    if status == "blocked" and not blockers:
        raise ValueError("blocked agent response must include blocker")

    return AgentJsonResponse(
        status=status,
        changed_files=changed_files,
        verification=verification,
        blockers=blockers,
    )


def required_string_list(value: Any, field_name: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        raise ValueError(f"agent response {field_name} must be string array")
    return value


def parse_verification(value: Any) -> list[AgentVerification]:
    if not isinstance(value, list):
        raise ValueError("agent response verification must be array")
    verification: list[AgentVerification] = []
    for item in value:
        if not isinstance(item, dict) or set(item) != {"command", "status"}:
            raise ValueError("agent response verification item must match schema")
        command = item["command"]
        status = item["status"]
        if not isinstance(command, str) or not isinstance(status, str):
            raise ValueError("agent response verification fields must be strings")
        verification.append(AgentVerification(command=command, status=status))
    return verification
