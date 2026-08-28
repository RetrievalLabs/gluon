# Source Code Migration

## Overview

Source code migration moves the legacy Java application into the rewrite
workspace after dependency selection and build structure are already complete.
The stage preserves behavior while making source code compile and run on the
target Java version.

This stage does not choose dependencies, redesign the build, or perform
optional cleanup. Existing rewrite workspace build files are authoritative.

The migration uses two read-only databases:

- `business-kg.db` ranks business behavior and links business rules,
  workflows, invariants, state transitions, and side effects to source
  evidence.
- `extraction.db` maps source evidence to modules, classes, methods, entry
  points, relationships, tests, assertions, and source locations.

## Inputs

- Legacy repository path, read-only.
- Rewrite workspace path, writable.
- Target Java version.
- `business-kg.db`.
- `extraction.db`.
- `characterization-tests.db`, when present.
- Characterization test artifacts, when present.
- Java migration skills available to the source migration agent.
- Multi-agent orchestration through one main agent, one Context Agent, one
  Implementation Agent, and one Verification Agent.
- Web search and fetch for official target Java, framework, and library
  documentation when local evidence is insufficient.

## Migration Order

Start with runtime entry into the application, then expand only through code
needed by startup, compile, tests, or business behavior.

```text
1. Select main/runtime entrypoint -> verify: exact method, class, and file are known.
2. Copy entrypoint source -> verify: package path and module path are preserved.
3. Compile rewrite workspace -> verify: missing symbols identify next files.
4. Copy direct runtime dependencies -> verify: startup path compile errors decrease.
5. Migrate external entrypoints -> verify: HTTP, CLI, scheduled, message, or lifecycle methods compile.
6. Migrate high-priority business methods -> verify: methods map to business KG evidence.
7. Copy required domain, DTO, persistence, and utility code -> verify: each file is reachable or compile-required.
8. Run focused tests and characterization checks -> verify: behavior matches legacy observations.
```

Do not bulk-copy unrelated packages. Shared utilities are migrated only when
reachable from migrated code or required by compiler/test feedback.

## Multi-Agent Workflow

Harness invokes one source migration main agent. The main agent coordinates
bounded specialist agents through the Task tool:

```text
1. Main Agent receives DB paths, legacy repo, rewrite workspace, and target Java.
2. Context Agent reads business-kg.db, extraction.db, characterization-tests.db,
   repository docs, relevant configuration, bounded source context, and
   official web documentation when needed, then returns a JSON context packet.
3. Implementation Agent uses the context packet and Java skills to migrate
   source code and write integration tests in the rewrite workspace.
4. Verification Agent compiles, runs focused tests, verifies business behavior
   through characterization-tests.db, and writes or repairs characterization
   tests needed for migrated business logic.
5. Main Agent writes docs/migration/source-migration.md and returns control to
   harness.
```

The Context Agent does not edit files. The Implementation Agent writes only in
the rewrite workspace. The Verification Agent may run build and test commands
and may use bounded Gluon database commands for characterization DB work.
Web research should prefer official documentation, migration guides, release
notes, and stable API docs.

## Database Usage

Use `extraction.db` to find entrypoints:

```sql
SELECT
  ep.id,
  ep.kind,
  ep.framework,
  ep.route,
  ep.http_method,
  ep.topic,
  ep.command,
  m.id AS method_id,
  c.qualified_name,
  m.file,
  m.start_line,
  m.end_line
FROM entry_points ep
JOIN methods m ON m.id = ep.method_id
JOIN classes c ON c.id = m.class_id
ORDER BY
  CASE ep.kind
    WHEN 'Main' THEN 0
    WHEN 'Lifecycle' THEN 1
    WHEN 'Http' THEN 2
    WHEN 'Cli' THEN 3
    WHEN 'Scheduled' THEN 4
    WHEN 'Message' THEN 5
    ELSE 6
  END,
  m.file;
```

Use `extraction.db` to resolve one method to source:

```sql
SELECT
  m.id,
  m.module_id,
  c.qualified_name,
  c.package_name,
  m.name,
  m.signature,
  m.file,
  m.start_line,
  m.end_line
FROM methods m
JOIN classes c ON c.id = m.class_id
WHERE m.id = ?;
```

Use `extraction.db` to expand reachable code:

```sql
SELECT source_id, target_id, kind, confidence, source
FROM relationships
WHERE source_id = ?
ORDER BY confidence DESC;
```

Use `business-kg.db` to rank business behavior:

```sql
SELECT
  n.id AS node_id,
  n.kind,
  n.name,
  n.statement,
  n.confidence,
  e.method_id,
  e.source_lines_json,
  e.reason
FROM business_nodes n
JOIN business_evidence e ON e.node_id = n.id
ORDER BY n.confidence DESC;
```

Use `extraction.db` to find tests for migrated methods:

```sql
SELECT
  tc.id,
  tc.name,
  tc.file,
  tt.relationship,
  tt.confidence
FROM test_targets tt
JOIN test_cases tc ON tc.id = tt.test_case_id
WHERE tt.target_id = ?
ORDER BY tt.confidence DESC;
```

Use `characterization-tests.db` to verify migrated business behavior. The
Verification Agent should find scenarios linked to migrated business methods,
run or repair their generated tests, and compare modernized behavior to stored
legacy observations. Expected outputs must come from stored observations, not
from agent guesses.

## Agent Rules

The source migration agent must:

- Treat the legacy repository as read-only.
- Write only inside the rewrite workspace.
- Read `version-rewrite-modernization` and `java-best-practices` before source
  edits.
- Read repository docs and relevant configuration when needed to understand
  behavior.
- Use web search/fetch for official target Java, framework, or library docs
  when local evidence is not enough.
- Use framework and domain skills only when touched code requires them, such as
  Spring Boot, Spring Security, Jakarta EE, persistence, or testing.
- Write integration tests for migrated entrypoints or business slices when
  existing tests do not cover the migrated path.
- Use `characterization-tests.db` to write, repair, or run characterization
  tests for migrated business logic when artifacts are present.
- Preserve package names, public APIs, resource paths, configuration keys,
  serialization formats, security behavior, transactions, null handling,
  ordering, exceptions, and concurrency semantics.
- Fix target Java compatibility issues discovered by compile/test feedback.
- Avoid optional syntax modernization unless benefit is clear, semantics are
  known, and verification is available.

## Verification

Compile after each migrated slice. A slice should be one entrypoint path, one
business behavior, or one tightly related group of files required by compile or
startup.

Run focused tests selected from `extraction.db.test_targets` when available.
Run integration tests written for migrated source paths. Run characterization
tests in assert mode when characterization artifacts exist.
Compilation proves type compatibility only; behavior is accepted only after
tests or characterization checks confirm it.

Source migration is complete when:

- Target Java build succeeds in the rewrite workspace.
- Application startup path compiles and reaches expected startup checks.
- Migrated entrypoints compile and their focused tests pass.
- High-priority business behavior linked from `business-kg.db` is migrated or
  explicitly recorded as skipped.
- Characterization checks pass where available.
- Important source changes and blockers are documented.

## Migration Report

The stage writes `docs/migration/source-migration.md` in the rewrite workspace.
The report must include:

- Migrated entrypoints.
- Migrated business nodes and method IDs.
- Source roots, resources, and tests copied.
- Java compatibility changes made.
- Java skills used.
- Integration tests written.
- Characterization tests written or reused.
- Verification commands and outcomes.
- Skipped files or behaviors with reasons.
- Remaining blockers.
