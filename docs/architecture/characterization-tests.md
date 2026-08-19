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
- LLM reasoning for scenario generation and test source generation.
- Existing Java build tools to compile and run generated tests.

Generated characterization tests are added as new files in the legacy
repository. Existing source and existing tests must not be modified.

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
3. Ask the LLM for scenario intent:
   - scenario name
   - setup requirements
   - generated inputs
   - invocation path
   - observable outputs
   - required fakes
   - side-effect capture points
4. Generate Java/JUnit characterization test source.
5. Compile tests with the project build tool.
6. Run generated tests in observe mode against legacy code.
7. Capture observed behavior.
8. Persist snapshots.
9. Run generated tests in assert mode against modernized code.

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

The LLM produces scenario and test-generation proposals. Rust validates the
proposal, writes owned files, runs build commands, and stores snapshots.

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

## Multi-Agent Workflow

Use multi-agent execution for implementation, but keep ownership narrow to
minimize tokens and conflicts.

### Context Agent

Read-only.

Responsibilities:

- inspect current CLI patterns
- inspect KG schema and extraction DB schema
- inspect test extraction tables and tools
- inspect Java build/test runner patterns
- inspect existing generated-file conventions

Output must be short and factual. Target budget: about 1500 tokens.

### Main Agent

Locks implementation contracts before coding:

- CLI options
- snapshot DB schema
- generated file layout
- fake strategy
- LLM tool contract
- validation and test plan

### Implementation Agent

Single coding owner.

Responsibilities:

- add CLI command
- implement behavior selection
- implement snapshot DB store
- implement bounded LLM context/tools
- implement Java test source generation
- implement fake handling metadata
- add focused tests

Avoid multiple coding agents until interfaces are stable.

### Verification Agent

Read-only.

Responsibilities:

- run tests and CLI smoke checks
- inspect generated files and snapshot rows
- verify no real external calls are generated
- verify traceability to KG and extraction DB
- report only failures, risks, and missing coverage

Target output budget: about 1000 tokens.

### Final Integration

Main agent fixes integration gaps, reruns validation, and reports the final
state.

## Token-Control Rules

- Do not ask agents to broadly research the repository.
- Give each agent exact files or exact questions when possible.
- Do not paste long logs between agents.
- Do not run parallel implementation agents over the same Rust crate.
- Prefer short factual reports over narrative summaries.
- Keep generated prompts compact and tool-driven.

## Acceptance Criteria

A mature implementation is complete when:

- Characterization tests can be generated from selected KG behaviors.
- Generated tests compile in the legacy project.
- Observe mode captures legacy outputs without calling real external services.
- Assert mode compares modernized behavior against stored legacy snapshots.
- All generated artifacts trace to KG nodes and extraction DB evidence.
- Unsafe or under-specified scenarios are skipped with diagnostics instead of
  producing weak tests.
