# Harness

## Overview

Python orchestration layer for one Java migration run. It validates runtime
configuration, prepares the target repository, runs Gluon CLI stages, invokes
Claude Agent SDK only for failed-stage repair, records logs, and writes run
summaries.

## Structure

- `cli.py` - CLI entrypoint and top-level error handling.
- `main.py` - thin compatibility wrapper around `cli.main`.
- `config/` - environment parsing and validation.
- `execution/` - subprocess command runner, git workspace, and fixed paths.
- `integrations/` - backend, Claude Agent SDK, and Gluon CLI adapters.
- `models/` - dataclasses shared across harness components.
- `pipeline/` - stage list, retry loop, coordinator, and summary writer.
- `tests/` - offline unit tests mirroring production module structure.
- `skills/` - contains skills used by claude agent sdk during migration.
