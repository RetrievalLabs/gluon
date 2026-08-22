import os
from collections.abc import Mapping

from errors import ConfigError
from models.config import HarnessConfig

REQUIRED_ENV = (
    "BACKEND_URL",
    "LANGUAGE",
    "CURRENT_VERSION",
    "TARGET_VERSION",
    "ORG_PROJECT_NAME",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_BASE_URL",
)


def load_config(env: Mapping[str, str] | None = None) -> HarnessConfig:
    source = os.environ if env is None else env
    missing = [key for key in REQUIRED_ENV if not source.get(key)]
    if missing:
        raise ConfigError(f"missing required env: {', '.join(missing)}")

    language = source["LANGUAGE"].strip().lower()
    if language != "java":
        raise ConfigError("LANGUAGE must be java")

    max_attempts_raw = source.get("MAX_AGENT_ATTEMPTS", "3")
    try:
        max_attempts = int(max_attempts_raw)
    except ValueError as error:
        raise ConfigError("MAX_AGENT_ATTEMPTS must be an integer") from error
    if max_attempts < 1:
        raise ConfigError("MAX_AGENT_ATTEMPTS must be at least 1")

    return HarnessConfig(
        backend_url=source["BACKEND_URL"],
        language=language,
        current_version=source["CURRENT_VERSION"],
        target_version=source["TARGET_VERSION"],
        org_project_name=source["ORG_PROJECT_NAME"],
        anthropic_api_key=source["ANTHROPIC_API_KEY"],
        anthropic_model=source["ANTHROPIC_MODEL"],
        anthropic_base_url=source["ANTHROPIC_BASE_URL"],
        max_agent_attempts=max_attempts,
        gluon_cli=source.get("GLUON_CLI", "gluon-cli"),
    )

