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
4. Clone repo into `/opt/gluon/org/project/{}`.
5. Checkout source branch and create migration branch.
6. Set `JAVA_HOME` to `/opt/jdks/jdk{CURRENT_VERSION}`.
7. Run `gluon-cli parse-build` to create `/opt/gluon/org/build-report`.
8. Run `gluon-cli analyze-report` to create `/opt/gluon/org/compatibility-report`.
9. Run `gluon-cli extract-business` to create `/opt/gluon/org/extraction.db`.
10. Run `gluon-cli extract-tests` to append test extraction tables to `/opt/gluon/org/extraction.db`.
11. Run `gluon-cli build-business-kg` to create `/opt/gluon/org/business-kg.db`.
12. Run characterization test generation to create artifacts under `<repo>/gluon/tests/*`.

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
- characterization test artifacts in `<repo>/gluon/tests/*`
