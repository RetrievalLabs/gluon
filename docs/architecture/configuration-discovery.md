# Configuration Discovery

- `code-parser classify-configs` consumes `build-report.json`, uses module boundaries and direct dependency inventory, and writes `configuration-classification-report.json`.
- The command discovers Java application/runtime configuration with deterministic rules from `app/code-parser/data/java/configuration_classification.yaml`.
- Build, container, orchestration, deployment, and Java toolchain configuration are excluded because they belong to separate Gluon stages.
- Reports are nested by module like other code-parser sidecars. Each configuration file includes path, type, format, framework, profile, scope, extracted properties, evidence, and linked dependencies.
- Dependency links are best-effort direct dependency matches from `build-report.json`. Transitive dependency resolution and Java consumer graph linking are out of scope for this command.
- Secret-looking keys are marked sensitive. Literal secret values are not emitted; references such as `${DB_PASSWORD}` are recorded as references only.
