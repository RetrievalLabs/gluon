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

No stable Gluon CLI commands are documented yet.
