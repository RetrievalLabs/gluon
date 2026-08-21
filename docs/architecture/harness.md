# Harness

## Overview

The harness is a Python orchestration layer for Java migration runs. It prepares
an isolated target repository, runs Gluon CLI analysis commands, coordinates
Claude Agent SDK agents, records execution details, and validates the migrated
code.

The harness does not parse Java source or encode migration rules. Gluon CLI is
the source of truth for build reports, compatibility reports, extraction
databases, business knowledge graphs, and characterization-test artifacts.

## Goals

- Run a complete migration workflow from a target repository path.
- Keep the migrated code on a dedicated git branch.
- Use Gluon CLI outputs to ground migration planning and agent prompts.
- Use a multi-agent setup only when work can be split into isolated scopes.
- Preserve existing behavior unless the migration explicitly requires a change.
- Record commands, working directories, exit statuses, stdout, stderr, and
  elapsed time for auditability and resumability.
- Produce a run summary that explains completed work, validation results, and
  remaining blockers.

## Non-Goals

- Reimplement Java build parsing, compatibility analysis, source extraction, or
  knowledge graph construction in Python.
- Let LLM agents write unbounded changes without scoped prompts and validation.
- Hide failed commands or environment problems behind generic migration errors.
- Modify user-authored tests or source outside the migration branch.

## Inputs and Environment

Each run requires:

- Target repository path.
- Harness work directory for reports, databases, logs, and summaries.
- Target Java version, defaulting to Java 25.
- Migration branch name.
- `gluon-cli` executable.
- Java build tools required by the target repository.
- JDTLS when business or test extraction is enabled.
- Claude Agent SDK credentials and runtime configuration.

The target repository should live in a sandboxed VM or equivalent isolated
environment with all migration tools installed.

## Architecture Components

### Migration Coordinator

Owns the end-to-end run. It validates inputs, creates the run state, executes
workflow steps in order, delegates agent work, and writes the final summary.

### Git Workspace

Manages the target repository checkout. It verifies the repo, checks worktree
state, creates or checks out the migration branch, and prevents accidental
changes outside the selected branch.

### Command Runner

Runs shell commands through a narrow subprocess wrapper. It records the command,
working directory, exit status, stdout, stderr, start time, end time, and
elapsed time for every invocation.

### Gluon CLI Adapter

Builds and runs documented Gluon CLI commands. It should not construct ad hoc
analysis scripts when an existing CLI command supports the workflow.

Core commands:

- `code-parser parse-build`
- `code-parser analyze-report`
- `code-parser extract-business`
- `code-parser extract-tests`
- `code-parser build-business-kg`
- `code-parser generate-characterization-tests`

### Agent Client

Isolates Claude Agent SDK usage behind a narrow adapter. The rest of the
harness should depend on harness-level request and response types, not SDK
types.

### Run State

Tracks paths, generated artifacts, command results, agent outcomes, validation
results, and resumable step status.

## Migration Workflow

1. Preflight
   - Validate repository, work directory, branch name, CLI path, and required
     tools.
   - Check git status and create or switch to the migration branch.

2. Build and compatibility analysis
   - Run `gluon-cli code-parser parse-build --resolve --format json`.
   - Run `gluon-cli code-parser analyze-report --target-java 25 --format json`.
   - Store generated reports under the run work directory.

3. Business context extraction
   - Run `extract-business` to create `business-extraction.db`.
   - Run `extract-tests` when test sources and JDTLS are available.
   - Run `build-business-kg` when extraction succeeds and LLM credentials are
     available.

4. Characterization artifacts
   - Run `generate-characterization-tests` when the business extraction
     database and business knowledge graph are available.
   - Treat generation as resumable. Existing user-authored tests must not be
     overwritten.

5. Agent migration
   - Ask the Context Agent to read reports, databases, and source context and
     produce scoped migration work items.
   - Ask implementation agents to make scoped changes only when work items are
     independent.
   - Let the main agent review changes, resolve conflicts, and keep final
     responsibility for the migration branch.

6. Validation
   - Run generated characterization tests when present.
   - Run the target repository's existing test or build command.
   - Record failures with the shortest decisive error plus full command logs.

7. Summary
   - Write completed steps, changed files, validation results, skipped steps,
     blockers, and next actions.

## Multi-Agent Model

The main agent is the administrator for the migration run. It owns planning,
delegation, integration, validation, and final reporting.

A Context Agent is read-only. It researches the target repository, Gluon CLI
reports, architecture, dependencies, tests, and likely migration risks before
implementation starts.

Implementation agents receive isolated scopes such as dependency updates,
specific API replacements, or test fixes. They should not edit the same files
unless the main agent explicitly serializes the work.

The main agent can spawn as many agents as needed, but the harness should cap
concurrent agents by configuration so runs remain debuggable and resource usage
is bounded.

## Artifacts and Logging

Recommended run layout:

```text
<work-dir>/<repo-name>/
  reports/
    build-report.json
    compatibility-report.json
  data/
    business-extraction.db
    business-kg.db
    characterization-tests.db
  logs/
    commands.jsonl
    agents.jsonl
  summary.json
```

`commands.jsonl` records deterministic command metadata. `agents.jsonl` records
agent prompts, scoped assignments, final messages, and status summaries without
requiring callers to parse SDK-specific message shapes.

## Failure Handling

Each workflow step should fail with a specific reason and preserve artifacts
already produced. Dependent steps should be skipped when prerequisites are
missing.

Examples:

- Build resolution failure keeps offline build parse results and records the
  resolver diagnostic.
- Missing JDTLS skips business and test extraction with a clear environment
  blocker.
- LLM or Claude Agent SDK failure stops agent-driven migration but preserves
  CLI reports.
- Validation failure records failed command details and leaves the migration
  branch for manual inspection.

## Validation Strategy

Validation should prefer project-native commands discovered from Maven or Gradle
wrappers. Generated characterization tests, when available, run against the
legacy behavior first and then against migrated code.

The harness should distinguish:

- Environment failures, such as missing JDKs or tools.
- Migration failures, such as compile errors caused by edits.
- Behavior failures, such as characterization-test mismatches.
- Existing failures already present before migration.

## Implementation Phases

1. Build the Python package structure, CLI argument parsing, run state model,
   command runner, and git workspace handling.
2. Add Gluon CLI adapter methods for documented commands and command logging.
3. Add Claude Agent SDK adapter and mocked offline tests.
4. Implement coordinator workflow through reports, extraction, agent migration,
   validation, and summary writing.
5. Add resumability for completed steps and partial artifacts.


## Enviroment variables vm contains

- URL to communicate with backend service.
- Language
- TARGET - VERSION
- ORG/PROJECT NAME
