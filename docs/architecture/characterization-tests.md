# Characterization Tests

## Overview

Characterization tests preserve observed legacy behavior during modernization.
They are generated from the Business Knowledge Graph, run once against the
legacy application to capture current behavior, and then reused against the
modernized application to verify behavior has not changed.

The system uses:

- `business-extraction.db` for source structure, methods, entry points,
  relationships, and extracted integration/E2E test evidence.
- `business-kg.db` for business behaviors, rules, workflows, state transitions,
  and source evidence.
- Raw source code and LSP/JDTLS for precise symbol context.
- LLM reasoning for scenario and testcase input generation.
- Existing Java build tools to compile and run generated tests.

Generated characterization tests are added as new files in the legacy
repository. Existing source and existing tests must not be modified.
The LLM must not directly edit or create files. It returns structured scenario
JSON, and Rust renders owned Java/JUnit files from validated templates.

## Goal

For each selected business behavior:

1. Generate deterministic inputs that cover happy paths, edge cases, and
   boundary cases.
2. Generate executable tests that invoke the legacy behavior.
3. Run those tests against the legacy code in observe mode.
4. Store generated inputs and observed outputs.
5. Re-run the same tests against modernized code in assert mode.

The output is not guessed by the LLM. The legacy application is the source of
truth for expected behavior.

## CLI

Planned command:

```bash
code-parser generate-characterization-tests \
  --business-database <business-extraction.db> \
  --kg-database <business-kg.db> \
  --source-path <legacy-project-root> \
  --output-dir <gluon-output-dir>
```

Optional flags:

```text
--max-behaviors <count>
--node-kind BusinessRule|Workflow|Invariant|StateTransition|SideEffect
--force
--continue
```

`--force` may replace previously generated Gluon characterization files and
snapshot rows. It must not overwrite user-authored tests.

`--continue` resumes an interrupted generation or observation run by skipping
completed behavior scenarios.

If generation, compilation, or test execution fails, the command should print a
verbose error and stop. The user can fix the issue manually or adjust inputs,
then rerun with `--continue` to resume from the last completed scenario.

## Behavior Selection

The generator selects behavior nodes from `business-kg.db`.

Supported node kinds:

- `BusinessRule`
- `Workflow`
- `Invariant`
- `StateTransition`
- `SideEffect`

Each selected behavior must trace to production source through
`business_evidence.method_id`. The generator uses that method ID to load source
context from `business-extraction.db`.

Relevant context includes:

- KG node statement and confidence.
- KG neighbors and edge reasons.
- Source methods and evidence lines.
- Entry points for the evidence methods.
- Related production methods.
- Existing integration, E2E, and acceptance tests from `test_*` tables.

Unit-test-style characterization is allowed only when no usable entry point
exists. Prefer externally visible API, messaging, command, or workflow behavior
when available.

## Generated Test Layout

Generated tests live in the legacy repository under module test source roots.

Example:

```text
src/test/java/<package>/gluon/characterization/
```

Use one generated test class per behavior group when behaviors share the same
entry point or fixture setup. Use one test method per scenario.

Generated files must include stable markers so future runs can identify files
owned by Gluon. Files without these markers are user-owned and must not be
overwritten.

## Generation Flow

1. Select KG behaviors.
2. Build bounded context from KG, extraction DB, source, and existing tests.
3. Ask the LLM for scenario intent and testcase inputs:
   - scenario name
   - setup requirements
   - generated inputs
   - invocation path
   - observable outputs
   - required fakes
   - side-effect capture points
4. Validate the LLM JSON proposal.
5. Render Java/JUnit characterization test source from Rust-owned templates.
6. Compile tests with the project build tool.
7. Run generated tests in observe mode against legacy code.
8. Capture observed behavior.
9. Persist snapshots.
10. Run generated tests in assert mode against modernized code.

There is no automatic LLM repair loop. Failures stop the run after recording
diagnostics.

## Testcase Inputs

Testcase inputs are first-class artifacts.

The LLM proposes input candidates for each selected behavior:

- happy-path inputs
- edge-case inputs
- boundary inputs
- failure inputs
- fixture setup
- fake dependency responses
- invocation parameters

Rust validates proposed inputs before any file is generated:

- input shape must match the invocation type
- required fields must be present
- values must be serializable and deterministic
- random, clock, UUID, tenant, and user values must be fixed
- external dependency configuration must point to fakes only
- generated fixtures must be safe for isolated test execution

The LLM must not provide expected outputs. Expected outputs are observed by
running the generated test against the legacy application.

```text
LLM proposes input -> Rust validates input -> legacy code produces output
```

Both generated inputs and observed outputs are stored in the snapshot database.
Assert mode reuses the stored inputs exactly and compares modernized outputs
against stored legacy observations.

## LLM And Rust Ownership

The LLM is a bounded planner, not a filesystem actor.

LLM may propose:

- scenario names
- testcase inputs
- fixtures
- fake requirements
- invocation paths
- observation points

LLM must not:

- create files
- edit files
- choose arbitrary output paths
- run build commands
- write SQLite rows
- call arbitrary shell commands
- query arbitrary SQL

Rust owns:

- schema validation
- generated file paths
- package and class names
- Java/JUnit templates
- imports
- generated markers
- overwrite rules
- snapshot IDs
- database writes
- build/test commands
- traceability enforcement

## External Dependencies

Generated tests must not call real external services.

When legacy code calls an external dependency, the generator should fake that
dependency.

Fake selection order:

1. Use existing project fakes, stubs, mocks, embedded servers, test containers,
   or in-memory implementations when already present.
2. Use existing framework-supported test configuration when available.
3. Generate minimal test-local fakes under the characterization test package.
4. If no safe fake can be created, mark the scenario as `needs_fake` and skip
   test source generation for that scenario.

Fakes should record behavior-relevant boundary interactions:

- dependency name or interface
- request payload or message
- headers or metadata when business-relevant
- topic, route, command, or operation name
- configured fake response
- call count

Do not snapshot unstable low-level details unless they are business-relevant:

- object identity
- random values
- timestamps
- thread names
- connection metadata
- unordered map/set iteration order

External dependency calls are part of characterized behavior only at the
boundary. The test should assert what the application sent to the dependency
and how the configured fake response affected application behavior.

## Snapshot Database

Generated inputs and observed outputs are stored outside the legacy repository
in `characterization-tests.db` under the Gluon output directory.

Suggested tables:

### `characterization_runs`

One generation, observation, or assertion run.

Fields:

- run ID
- mode: `generate`, `observe`, or `assert`
- source path
- business DB path
- KG DB path
- status
- started and finished timestamps

### `characterization_behaviors`

Selected KG behaviors.

Fields:

- behavior ID
- KG node ID
- node kind
- node name
- node statement
- source method IDs
- status

### `characterization_scenarios`

Generated scenarios for one behavior.

Fields:

- scenario ID
- behavior ID
- name
- scenario kind: `happy_path`, `edge`, `boundary`, or `failure`
- invocation kind
- status
- diagnostic reason when skipped

### `characterization_inputs`

Stable generated inputs for a scenario.

Fields:

- input ID
- scenario ID
- input JSON
- fixture JSON
- deterministic seed data

### `characterization_observations`

Observed legacy behavior.

Fields:

- observation ID
- scenario ID
- input ID
- status code or return value
- response body
- exception type and message
- emitted events or messages
- database side effects where supported
- fake boundary calls
- normalized output JSON

### `characterization_files`

Generated test source files.

Fields:

- file ID
- scenario ID
- path
- class name
- package name
- content hash
- generated marker

### `characterization_fakes`

Fake dependency metadata.

Fields:

- fake ID
- scenario ID
- dependency name
- fake strategy: `existing`, `framework`, or `generated`
- source file path when generated
- boundary calls captured

### `characterization_diagnostics`

Failures and skipped work.

Fields:

- diagnostic ID
- run ID
- behavior ID
- scenario ID
- severity
- category
- message

Every behavior, scenario, generated file, fake, and observation must trace back
to `business_nodes.id`, `business_evidence.method_id`, and source file/line
evidence where available.

## LLM Tooling

Do not expose arbitrary SQL or unrestricted repository writes to the LLM.

Use bounded tools:

```text
get_business_node(node_id)
get_business_neighbors(node_id, limit)
get_evidence_method(method_id)
read_method_source(method_id)
get_entry_points_for_method(method_id)
get_tests_for_method(method_id, limit)
get_test_case(test_case_id)
```

The LLM produces structured scenario and input proposals. Rust validates the
proposal, renders owned files, runs approved build commands, and stores
snapshots.

## Determinism Rules

Generated tests must be deterministic.

Control:

- clocks
- UUIDs
- random seeds
- current user/tenant context
- locale and timezone when relevant
- temp directories
- in-memory or isolated databases
- fake external dependencies

If deterministic setup is not possible, mark the scenario as skipped with a
diagnostic instead of generating a flaky test.

## Failure Handling

The runtime must fail fast on generation, compile, or execution errors.

On failure, print verbose diagnostics:

- behavior ID and scenario ID
- KG node ID and evidence method IDs
- generated file path
- phase: `generate`, `compile`, `observe`, or `assert`
- failing command when applicable
- exit status
- concise stdout/stderr excerpt
- full diagnostic path or DB row ID when persisted

The command must persist run progress before stopping. A later invocation with
`--continue` resumes by skipping completed scenarios and starting at the first
incomplete or failed scenario.

Do not ask the LLM to repair failed generated code automatically. Do not modify
production source or user-authored tests during failure handling.

## Runtime LLM Loop

Do not use multi-agent orchestration inside the
`generate-characterization-tests` runtime.

Runtime generation should use one bounded LLM loop per behavior:

```text
Rust selects behavior
Rust builds bounded context
LLM proposes scenario/input JSON
Rust validates proposal
Rust renders Java/JUnit files
Rust runs observe/assert mode
Rust stores snapshots
```

This keeps generation deterministic, traceable, token-efficient, and easier to
audit. Runtime verification comes from compiling and running generated tests,
not from another LLM agent.

## Acceptance Criteria

A mature implementation is complete when:

- Characterization tests can be generated from selected KG behaviors.
- Generated tests compile in the legacy project.
- Observe mode captures legacy outputs without calling real external services.
- Assert mode compares modernized behavior against stored legacy snapshots.
- All generated artifacts trace to KG nodes and extraction DB evidence.
- Unsafe or under-specified scenarios are skipped with diagnostics instead of
  producing weak tests.
