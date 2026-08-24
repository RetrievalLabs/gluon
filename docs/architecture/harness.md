# Harness

Python orchestration layer for one Java migration run.

## Contract

Harness owns deterministic setup and pipeline execution. Agent is invoked only to repair a failed stage, then harness resumes from same stage.

## Required Environment

- `BACKEND_URL`
- `LANGUAGE`
- `CURRENT_VERSION`
- `TARGET_VERSION`
- `ORG_PROJECT_NAME`
- `ANTHROPIC_API_KEY`
- `ANTHROPIC_MODEL`
- `ANTHROPIC_BASE_URL`

Missing or invalid required config is fatal.

## Flow

1. Validate required environment variables.
2. Validate Anthropic API key and base URL with a test request.
3. Request repo URL, source branch, and token from backend. Mock this until backend is ready.
4. Clone repo, checkout source branch, create migration branch.
5. Set `JAVA_HOME` to `/opt/jdks/jdk{CURRENT_VERSION}`.
6. Run `gluon-cli parse-build` and `gluon-cli analyze-report`, passing
   configured output directories so reports are written by the CLI.
7. Run `gluon-cli extract-business` to create `/opt/gluon/org/extraction.db`.
8. Run `gluon-cli extract-tests` to append test extraction tables to `/opt/gluon/org/extraction.db`.
9. Run `gluon-cli build-business-kg` to create `business-kg.db`.
10. Run characterization test generation.
11. Create migration rewrite workspace and hand off rewrite setup to agents.

## Error Handling

For any failed `gluon-cli` stage:

1. Capture command, exit code, stdout, stderr, and relevant artifact paths.
2. Parse error category when possible.
3. Invoke Claude agent SDK with repo checkout, shell tools, failed command, and parsed error.
4. After agent fix, rerun same failed stage.
5. Continue pipeline only after stage succeeds.

Do not skip stages after failure.

## Outputs

- repo clone in `/opt/gluon/org/project/{}`
- `/opt/gluon/org/build-report/<project>/build-report.json`
- `/opt/gluon/org/compatibility-report/<project>/compatibility-report.json`
- `/opt/gluon/org/extraction.db`
- test extraction tables in `/opt/gluon/org/extraction.db`
- `/opt/gluon/org/business-kg.db`
- characterization test artifacts in `<repo-path>/gluon/tests/*`
- rewrite workspace in `/opt/gluon/org/rewrite/<project>`
- legacy tree snapshot in `/opt/gluon/org/rewrite/<project>/docs/legacy-tree.txt`

## Characterization Test Generation

`gluon-cli code-parser generate-characterization-tests` creates behavior
abstracts, scaffold files, and `characterization-tests.db`. Harness then owns
the full-test generation workflow that turns those abstracts into executable
tests.

Full-test generation uses a multi-agent workflow:

1. Harness selects one pending characterization scenario from
   `characterization-tests.db`.
2. Harness gives the main agent seed context: scenario ID, behavior ID, KG node
   ID, abstract/scaffold path, database paths, repo path, allowed
   commands/tools, and relevant status rows.
3. Main agent gives that seed context to the Context Agent.
4. Context Agent expands the seed into a structured JSON context packet by
   reading bounded rows from `business-kg.db`, `business-extraction.db`, and
   `characterization-tests.db`, plus source files, existing tests, and JDTLS
   symbol context, then returns the JSON packet to the main agent.
5. Main agent gives the context packet and implementation responsibility to
   the Implementation Agent.
6. Implementation Agent writes the executable project-native test, uses mocks
   or fakes for external dependencies, and verifies it with the project
   build/test command.
7. Main agent gives the written test and context packet to the Input/Output
   Agent.
8. Input/Output Agent generates deterministic inputs including happy path,
   edge, boundary, and failure cases, runs the written test with those inputs,
   captures observed outputs, inserts input and observation rows through Gluon
   CLI database commands, and updates the scenario status to `accepted`.
9. Main agent returns control to harness after the test is accepted.
10. Harness verifies that the scenario is accepted and has stored input and
   observation rows, then checks `git status`, commits the accepted test and
   related `characterization-tests.db` changes, selects the next pending
   scenario, collects fresh seed context, and gives control back to the main
   agent.

Agents may use:

- Git status, diff, add, and commit for generated characterization work.
- Java build and test commands needed to verify the generated test.
- JDTLS from `PATH` for Java symbol context.
- Gluon CLI database commands documented in the `gluon-cli` skill for bounded
  database inspection and focused edits.

Agents must not run harness-owned Gluon stages. Harness runs `gluon-cli`
pipeline commands and resumes failed stages after repair.

## Migration Rewrite

1. Create a separate rewrite workspace for the modernized project.
2. Initialize git in the rewrite workspace, create branch
   `gluon/java-<target-version>`, and set `origin` to the source repo URL.
3. Inspect the legacy checkout with `tree` and save the output, for example
   `docs/legacy-tree.txt`, so agents can compare legacy structure with the new
   scaffold.
4. Scaffold only the initial structure needed for rewrite work:
   `Makefile`, `.gitignore`, `docs/`, `src/`, `CLAUDE.md`, and `AGENTS.md`.
5. Use the compatibility report as the source of migration requirements.
   Preserve behavior and avoid unrelated refactors.
6. Use a multi-agent handoff: Context Agent reads reports and legacy structure,
   Rewrite Agent updates scaffold, and Review Agent checks traceability.
