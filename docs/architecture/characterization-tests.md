# Characterization Tests

## Overview

Characterization tests preserve observed legacy behavior during modernization.
They are generated from the Business Knowledge Graph, run once against the
legacy application to capture current behavior, and then reused against the
modernized application to verify behavior has not changed.

The system has two phases:

1. `code-parser generate-characterization-tests` deterministically selects
   behavior abstracts, writes scaffold metadata, and initializes
   `characterization-tests.db`.
2. The Python harness runs a multi-agent workflow that turns those abstracts
   into executable characterization tests, observes legacy outputs, stores
   snapshots, and commits accepted tests.

The system uses:

- `business-extraction.db` for source structure, methods, entry points,
  relationships, and extracted integration/E2E test evidence.
- `business-kg.db` for business behaviors, rules, workflows, state transitions,
  and source evidence.
- Raw source code and LSP/JDTLS for precise symbol context.
- Harness agents for context collection, testcase input generation,
  observation, implementation, and verification.
- Existing Java build tools to compile and run generated tests.

Generated characterization tests are added as new files in the legacy
repository. Existing source and existing tests must not be modified. The Rust
code-parser runtime does not directly call a multi-agent workflow or edit full
tests; harness agents own that work after code-parser creates traceable
abstracts and database rows.

## Goal

For each selected business behavior:

1. Generate deterministic inputs that cover happy paths, edge cases, and
   boundary cases.
2. Generate executable tests that invoke the legacy behavior.
3. Use mocks or fakes for external dependencies.
4. Run those tests against the legacy code in observe mode.
5. Store generated inputs and observed outputs in `characterization-tests.db`.
6. Re-run the same tests against modernized code in assert mode.

The output is not guessed by the LLM. The legacy application is the source of
truth for expected behavior.

## CLI

Code-parser command:

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

`--continue` resumes interrupted abstract/scaffold generation by skipping
completed behavior scenarios.

If abstract generation fails, the command should print a verbose error and
stop. The user can fix the issue manually or adjust inputs, then rerun with
`--continue` to resume from the last completed scenario. Full test
implementation, observation, verification, and commits happen in the harness
multi-agent phase after this command succeeds.

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

### Code-parser phase

1. Select KG behaviors.
2. Build bounded abstract context from KG, extraction DB, source, and existing
   tests.
3. Persist selected behavior rows, scenario rows, scaffold file metadata, and
   diagnostics in `characterization-tests.db`.
4. Write traceable abstract/scaffold files under the generated test output
   area.

Code-parser does not implement full tests, run observe/assert execution, call
multi-agent workflows, or commit repository changes.

### Harness multi-agent phase

1. Harness selects one pending scenario from
   `characterization-tests.db`.
2. Harness gives the main agent seed context: scenario ID, behavior ID, KG node
   ID, abstract/scaffold path, database paths, repo path, allowed
   commands/tools, and relevant status rows.
3. Main agent gives that seed context to the Context Agent.
4. Context Agent expands the seed into a structured JSON context packet from
   the abstract, KG rows, extraction rows, characterization rows, source,
   existing tests, and JDTLS, then returns the JSON packet to the main agent.
5. Main agent gives the context packet and implementation responsibility to
   the Implementation Agent.
6. Implementation Agent writes the full executable project-native test using
   mocks or fakes for external dependencies.
7. Main agent gives the written test and context packet to the Input/Output
   Agent.
8. Input/Output Agent generates deterministic inputs, including happy path,
   edge, boundary, and failure cases, runs the written test with those inputs,
   captures observed outputs, and stores inputs, observations, fake boundary
   calls, and scenario status in `characterization-tests.db` through Gluon CLI
   database commands.
9. Main agent verifies the test with the project build/test command after the
   database writes.
10. Main agent returns control to harness after the test is accepted.
11. Harness verifies that scenario status is accepted and at least one input
   row and observation row exist for the scenario.
12. Harness commits the accepted test and related snapshot DB update, selects
   the next pending scenario, collects fresh seed context, and gives control
   back to the main agent.

Failures stop the current scenario after recording diagnostics. The next run
resumes from unfinished or failed scenarios.

## Testcase Inputs

Testcase inputs are first-class artifacts.

After the Implementation Agent writes the test, the Input/Output Agent proposes
input candidates for each selected behavior:

- happy-path inputs
- edge-case inputs
- boundary inputs
- failure inputs
- fixture setup
- fake dependency responses
- invocation parameters

The Input/Output Agent normalizes proposed inputs before storing or using them:

- input shape must match the invocation type
- required fields must be present
- values must be serializable and deterministic
- random, clock, UUID, tenant, and user values must be fixed
- external dependency configuration must point to fakes only
- generated fixtures must be safe for isolated test execution

Agents must not invent expected outputs. Expected outputs are observed by
running the written test with stored inputs against the legacy application.

```text
Implementation Agent writes test
Input/Output Agent stores input
Input/Output Agent runs test with input
legacy code produces output
```

Both generated inputs and observed outputs are stored in
`characterization-tests.db`. Assert mode reuses the stored inputs exactly and
compares modernized outputs against stored legacy observations.

## Agent And Runtime Ownership

Code-parser is a deterministic abstract/scaffold generator. Harness agents own
full test implementation and observation.

Agents may:

- return structured JSON context packets from bounded database rows, source,
  existing tests, and JDTLS results
- propose deterministic testcase inputs, fixtures, fake requirements,
  invocation paths, and observation points
- write generated characterization test files
- run project-local build/test commands for verification
- use git status, diff, add, and commit for accepted generated tests
- use Gluon CLI database commands documented in the `gluon-cli` skill for
  bounded DB inspection and focused snapshot/status updates

Agents must not:

- choose which scenario to process
- modify production source or user-authored tests
- call real external services from generated tests
- invent expected outputs
- run harness-owned Gluon pipeline stages
- query arbitrary SQL

Code-parser owns:

- behavior selection from `business-kg.db`
- abstract/scaffold metadata
- `characterization-tests.db` schema
- generated markers and user-file overwrite protection
- traceability from behavior/scenario rows to KG and extraction evidence

Harness owns:

- scenario selection and seed context construction
- context packet orchestration
- generated test orchestration
- mocks/fakes policy for external dependencies
- project build/test verification
- one git commit per accepted verified test

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

The shared protobuf contract lives at
`app/package/gluon/db/v1/characterization_tests.proto`. Code-parser and harness
use that contract for shared table, row, lifecycle names, and SQLite metadata.
Code-parser materializes DDL, foreign keys, and defaults from protobuf
`sqlite_table` and `sqlite_column` options.

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

## Agent Tooling

Do not expose arbitrary SQL or unrestricted repository writes to agents.

Context and Input/Output agents return structured JSON only. They use bounded
database and source access. The preferred database interface is the public
Gluon CLI DB command set documented in the `gluon-cli` skill.

Use bounded conceptual tools or equivalent Gluon CLI DB reads:

```text
get_business_node(node_id)
get_business_neighbors(node_id, limit)
get_evidence_method(method_id)
read_method_source(method_id)
get_entry_points_for_method(method_id)
get_tests_for_method(method_id, limit)
get_test_case(test_case_id)
```

Expose JDTLS through bounded tools when source context is not enough to
choose a correct invocation path, fixture, fake, or observation point:

```text
lsp_definition(file, line, column)
lsp_references(symbol_id, limit)
lsp_implementations(symbol_id, limit)
lsp_document_symbols(file, limit)
lsp_call_hierarchy(method_id, direction, limit)
```

JDTLS tool inputs must be anchored to files, methods, call sites, or symbols
that code-parser selected from KG evidence, extraction tables, or prior bounded
tool results. Do not expose broad workspace symbol search, raw arbitrary LSP
requests, or unbounded reference scans to agents.

JDTLS tool results must be normalized before returning to agents:

- stable symbol ID where available
- symbol kind
- qualified name where available
- file
- start and end lines
- short source excerpt when needed
- relationship to the selected behavior context

Context Agent produces JSON context packets. Input/Output Agent produces JSON
input proposals and observations, then writes inputs and outputs to
`characterization-tests.db` through Gluon CLI database commands.
Harness orchestrates agent order, runs approved project test commands, and
commits accepted generated tests. Implementation Agent is the only agent that
may edit generated test files.

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

Code-parser must fail fast on abstract/scaffold generation errors. Harness must
fail the current scenario on full-test implementation, compile, observe, or
assert errors after recording diagnostics.

On failure, print verbose diagnostics:

- behavior ID and scenario ID
- KG node ID and evidence method IDs
- generated file path
- phase: `abstract`, `context`, `input`, `implementation`, `compile`,
  `observe`, or `assert`
- failing command when applicable
- exit status
- concise stdout/stderr excerpt
- full diagnostic path or DB row ID when persisted

Code-parser and harness must persist run progress before stopping. A later run
resumes by skipping completed scenarios and starting at the first incomplete or
failed scenario.

Do not modify production source or user-authored tests during failure handling.
Harness may invoke repair agents only for generated characterization files and
snapshot metadata owned by Gluon.

## Harness Multi-Agent Loop

Do not use multi-agent orchestration inside the Rust
`generate-characterization-tests` runtime. Multi-agent orchestration belongs in
the Python harness after code-parser produces abstracts and database rows.

Harness uses one bounded loop per scenario:

```text
Harness selects pending scenario and collects seed context
Harness gives control to main agent
main agent gives seed context to Context Agent
Context Agent returns JSON context packet to main agent
main agent gives implementation responsibility to Implementation Agent
Implementation Agent writes full test with mocks/fakes
main agent gives written test and context to Input/Output Agent
Input/Output Agent generates inputs, runs test, and captures outputs
harness verifies project test command
harness commits accepted generated test
control returns to harness for next scenario
```

This keeps code-parser deterministic while allowing agents to do repository-
specific implementation work. Verification comes from compiling and running
generated tests against the legacy project.

## Acceptance Criteria

A mature implementation is complete when:

- Characterization tests can be generated from selected KG behaviors.
- Generated tests compile in the legacy project.
- Observe mode captures legacy outputs without calling real external services.
- Assert mode compares modernized behavior against stored legacy snapshots.
- All generated artifacts trace to KG nodes and extraction DB evidence.
- Unsafe or under-specified scenarios are skipped with diagnostics instead of
  producing weak tests.
- Each accepted generated test is committed separately with related
  `characterization-tests.db` snapshot updates.
