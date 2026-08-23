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

- gluon command has given abstract for characterization teast
- Now we have to generate full tests using that abstract
- We have to use mocks for external depedencies
- We have to capture business behaviour
- We will use a multi-agent setup for this,
- One agent will collect the important context and create a context packet for a test.
- The context packet is passed to implementation agent.
- The implementation agent writes the test, and verifies it.
- The input output agent -> the main agent pass the context to this agent, this agent generate inputs, including bounday conditions, the give this inputs to the test, the test gives output
- this agent store input and output for the test, in the extraction db.
- again do the same for other tests.
- gluon-cli has commands to interact with database.