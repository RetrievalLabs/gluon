---
name: gluon-cli
description: Use the Gluon CLI, a command line interface for modernizing legacy code.
---

# Gluon CLI

Use this skill when an agent needs to run or explain Gluon CLI commands in production workflows.

## Usage

- Start by checking the local CLI help for the exact command shape before running commands.
- Prefer documented CLI commands over ad hoc scripts when the CLI supports the workflow.
- Run commands from the repository root unless a command explicitly requires another working directory.
- Capture the command, important output, and any generated files when reporting results.
- Read stderr for verbose status, failure context, and generated JSON paths.
- Do not assume a command exists because it is planned; verify it with CLI help or source code first.

## Command Discovery

- Use `cargo run -- --help` from the CLI crate when the binary is not installed.
- Use the installed binary help when available.
- Check command-specific help before using flags or positional arguments.

## Maintenance

- Update this file whenever a new Gluon CLI command, flag, output format, or required workflow is added.
- Document each command with its purpose, required inputs, important flags, outputs, and a minimal example.
- Keep examples production-safe and avoid destructive commands unless the command documentation clearly explains the risk.

## Commands

### `code-parser parse-build`

Parses Maven and Gradle project build metadata.

Required input:

- `--path <project-root>`: file or directory to scan.

Flags:

- `--format json`: output JSON. This is the only supported format.
- `--resolve`: run Maven or Gradle to collect effective dependency and plugin versions.
- `--output-dir <directory>`: write JSON to `<directory>/<project-directory-name>/build-report.json` and print the written path.

Outputs:

- Declared Java versions, build tools, plugins, and dependencies from build files.
- Resolved dependencies and plugins when `--resolve` succeeds.
- Diagnostics for malformed files, missing tools, wrapper issues, and build resolution failures.
- When `--output-dir` is set, report JSON is written to disk instead of printed to stdout. Stdout and stderr include the generated JSON path.
- If report diagnostics include errors, stderr summarizes the first error diagnostics and points to the JSON report for full details.

Example:

```bash
gluon-cli code-parser parse-build --path /path/to/project --resolve --format json
```

```bash
gluon-cli code-parser parse-build --path /path/to/project --resolve --format json --output-dir /path/to/gluon/data
```

### `code-parser analyze-report`

Analyzes a resolved Java build report against the Java compatibility knowledge base and scans source for Java migration API risks.

Required input:

- `--report <build-report.json>`: resolved or declared build report produced by `code-parser parse-build`.
- `--target-java <version>`: target Java release, such as `25`.

Flags:

- `--format json`: output JSON. This is the only supported format.
- `--source-path <project-root>`: source tree to scan. Defaults to `project_root` from the build report.
- `--output-dir <directory>`: write JSON to `<directory>/<project-directory-name>/compatibility-report.json` and print the written path.
- `--enable-jdk-tools`: compile project with the detected source Java JDK and run target JDK `jdeps` and `jdeprscan` on compiled classes.
- `--jdk-root <directory>`: root containing `jdk8`, `jdk11`, `jdk17`, `jdk21`, and `jdk25`. Defaults to `GLUON_JDK_ROOT` when set, otherwise `/opt/jdks`.
- `--classes-path <directory>`: compiled classes directory to scan. May be repeated. When omitted, the command attempts compilation and discovers common Maven/Gradle class directories.

Outputs:

- Source and target Java versions.
- Dependency and plugin update recommendations from curated KB rules.
- Removed, deprecated-for-removal, internal API, and reflective access findings from tree-sitter Java syntax scanning.
- Optional `jdk_tool_findings` from `jdeps --jdk-internals` and `jdeprscan --release <target> --for-removal`.
- Code-change recommendations derived from API findings, replacements, and incremental migration guidance.
- Unknown dependencies and plugins that need official-source verification before automated upgrades.
- Diagnostics for missing or unreadable source paths while preserving dependency and plugin analysis.
- When `--output-dir` is set, report JSON is written to disk instead of printed to stdout. Stdout and stderr include the generated JSON path.
- If report diagnostics include errors, stderr summarizes the first error diagnostics and points to the JSON report for full details.

Example:

```bash
gluon-cli code-parser analyze-report --report /path/to/gluon/data/project/build-report.json --target-java 25 --format json
```

```bash
gluon-cli code-parser analyze-report --report /path/to/gluon/data/project/build-report.json --target-java 25 --format json --output-dir /path/to/gluon/data
```

```bash
gluon-cli code-parser analyze-report --report /path/to/gluon/data/project/build-report.json --target-java 25 --source-path /path/to/project --enable-jdk-tools
```

### `code-parser extract-business`

Extracts Java business logic structure into a SQLite database.

Required input:

- `--path <project-root>`: Java project root or Java source file.
- `--output-dir <directory>`: base output directory. Default database path is `<directory>/<project-directory-name>/business-extraction.db`.

Flags:

- `--database <path>`: write SQLite database to an explicit path instead of the default output path.
- `--jdtls-command <command>`: JDTLS executable. Defaults to `jdtls`.
- `--jdtls-workspace <directory>`: JDTLS workspace directory. Defaults beside the database path.
- `--jdtls-max-in-flight <count>`: maximum concurrent JDTLS requests within an enrichment phase. Defaults to `32`; references and implementations are capped at `16`.
- `--jdtls-deep`: also resolve project-wide method references and interface implementations. Default mode resolves call definitions only, which is faster for large repositories.

Outputs:

- SQLite `business-extraction.db` with modules, classes, methods, relationships, entry points, candidate scores, signals, evidence ranges, context packets, and diagnostics.
- Stdout summary with database path, module count, class count, method count, relationship count, candidate counts by priority, and diagnostic count.
- Stderr phase status for tree-sitter scan, skipped source counts, JDTLS progress, scoring, database write, and total elapsed time.
- Multi-module Maven and Gradle projects are stored as one database with `modules` rows and `module_id` on classes and methods.
- Unit tests, integration tests, acceptance tests, common test-suffixed files, and generated Java sources are skipped.
- No JSON report in v1.
- JDTLS is required. Missing executable, startup failure, initialization failure, or semantic request failure blocks extraction and prints verbose stderr with command, path, phase, and available failure details.
- JDTLS enrichment uses bounded pipelined LSP requests and writes progress to stderr for long phases.
- Default enrichment is fast mode: document symbols and call definitions. Deep mode adds references and implementations.

Example:

```bash
gluon-cli code-parser extract-business --path /path/to/project --output-dir /path/to/gluon/data
```

```bash
gluon-cli code-parser extract-business --path /path/to/project --output-dir /path/to/gluon/data --jdtls-command /opt/jdtls/bin/jdtls
```

### `code-parser build-business-kg`

Builds `business-kg.db` from high-value methods in `business-extraction.db`.
KG build logic lives in `app/code-parser/src/languages/business/kg.rs`; Java
extraction remains in `app/code-parser/src/languages/java/business/`.

Required input:

- `--database <business-extraction.db>`: SQLite database produced by `code-parser extract-business`.
- `--source-path <project-root>`: source tree used to read method source ranges.

Flags:

- `--output <business-kg.db>`: KG SQLite output path. Defaults to `<database-dir>/business-kg.db`.
- `--min-priority high|medium|low`: minimum candidate priority. Defaults to `high`.
- `--max-methods <count>`: cap methods sent to LLM.
- `--force`: delete and recreate existing KG output database before building.

Environment:

- `ANTHROPIC_API_KEY`: required.
- `ANTHROPIC_API_BASE`: optional. Defaults to `https://api.anthropic.com`.
- `ANTHROPIC_MODEL`: optional. Defaults to `claude-sonnet-5`.

Outputs:

- SQLite `business-kg.db` with reusable business nodes, business edges, evidence rows, and LLM run metadata.
- Stdout selection, LLM, and database summary.
- Stderr live progress for long runs: selected count, current method, success/failure, tool calls, elapsed time, and final status.
- Bounded LLM tools can read compact extraction DB context, related method source, and existing KG nodes; the default limit is 5 tool calls per method.
- No persistent LLM cache in v1.

Example:

```bash
gluon-cli code-parser build-business-kg \
  --database /path/to/gluon/data/project/business-extraction.db \
  --source-path /path/to/project \
  --max-methods 5
```
