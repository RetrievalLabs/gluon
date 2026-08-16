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
- When `--output-dir` is set, report JSON is written to disk instead of printed to stdout.

Example:

```bash
gluon-cli code-parser parse-build --path /path/to/project --resolve --format json
```

```bash
gluon-cli code-parser parse-build --path /path/to/project --resolve --format json --output-dir /path/to/gluon/data
```
