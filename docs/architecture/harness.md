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
6. Run `gluon-cli parse-build` and `gluon-cli analyze-report`.
7. Run `gluon-cli extract-business` to create `/opt/gluon/org/extraction.db`.
8. Run `gluon-cli extract-tests` to append test extraction tables to `/opt/gluon/org/extraction.db`.
9. Run `gluon-cli build-business-kg` to create `business-kg.db`.
10. Run characterization test generation.

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
- `/opt/gluon/org/build-report`
- `/opt/gluon/org/compatibility-report`
- `/opt/gluon/org/extraction.db`
- test extraction tables in `/opt/gluon/org/extraction.db`
- `/opt/gluon/org/business-kg.db`
- characterization test artifacts in `<repo-path>/gluon/tests/*`

## Characterization Test Generation

`gluon-cli code-parser generate-characterization-tests` creates behavior
abstracts, scaffold files, and `characterization-tests.db`. Harness then owns
the full-test generation workflow that turns those abstracts into executable
tests.

Full-test generation uses a multi-agent workflow:

1. Main harness agent selects one pending characterization scenario from
   `characterization-tests.db`.
2. Context agent reads the behavior abstract, `business-kg.db`,
   `business-extraction.db`, source files, existing tests, and JDTLS symbol
   context. It returns a bounded context packet for one test.
3. Input/output agent receives the context packet, generates deterministic
   inputs including happy path, edge, boundary, and failure cases, runs the
   test path against the legacy behavior, and stores generated inputs plus
   observed outputs in `characterization-tests.db`.
4. Implementation agent receives the context packet and stored observations,
   writes the executable project-native test, uses mocks or fakes for external
   dependencies, and verifies it with the project build/test command.
5. Main harness agent checks `git status`, commits the accepted test and
   related `characterization-tests.db` changes, then moves to the next
   scenario.

Agents may use:

- Git status, diff, add, and commit for generated characterization work.
- Java build and test commands needed to verify the generated test.
- JDTLS from `PATH` for Java symbol context.
- Gluon CLI database commands documented in the `gluon-cli` skill for bounded
  database inspection and focused edits.

Agents must not run harness-owned Gluon stages. Harness runs `gluon-cli`
pipeline commands and resumes failed stages after repair.
