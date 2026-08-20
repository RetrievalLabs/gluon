# Harness

## Overview

Python AI code harness for running migration agents against target repositories.
It coordinates agent prompts, repository access, commands, validation, and
result collection.

## Rules

- Isolate Claude Agent SDK usage behind narrow adapters.
- Treat target repositories as untrusted input. Validate paths and avoid shell
  interpolation.
- Preserve target project behavior unless explicitly required.
- Prefer small, resumable workflow steps.
- Record command, working directory, exit status, stdout, stderr, and elapsed
  time.
- Keep tests offline and deterministic. Mock agent SDK calls.
- Add regression tests for workflow bugs.
