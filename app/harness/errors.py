"""Harness-specific exceptions."""


class HarnessError(Exception):
    """Base error for expected harness failures."""


class ConfigError(HarnessError):
    """Raised when harness configuration is missing or invalid."""


class BackendError(HarnessError):
    """Raised when repository metadata cannot be loaded."""


class StageFailedError(HarnessError):
    """Raised when a pipeline stage cannot be repaired."""

