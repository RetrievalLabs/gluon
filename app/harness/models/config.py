from dataclasses import dataclass


@dataclass(frozen=True)
class HarnessConfig:
    backend_url: str
    language: str
    current_version: str
    target_version: str
    org_project_name: str
    anthropic_api_key: str
    anthropic_model: str
    anthropic_base_url: str
    max_agent_attempts: int = 3
    gluon_cli: str = "gluon-cli"


@dataclass(frozen=True)
class RepoInfo:
    repo_url: str
    source_branch: str
    token: str | None = None

