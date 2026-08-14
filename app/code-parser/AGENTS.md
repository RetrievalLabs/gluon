# Code Parser

A Rust-based CLI tool for parsing Java build metadata.

## Structure

- `src/languages/<language>/` - language-specific parsers and orchestration.
- `src/languages/java/build/` - Java build metadata parsing and resolution.

## Rules

- Define language-level parser traits before build-system-specific traits.
- Define build-system parser traits so the same Java interface can be extended for different build systems.
- Add focused unit tests for parser behavior, fixture-based Maven and Gradle tests, and CLI tests for arguments, exit codes, stdout, and stderr.
- Add a regression test for each parser bug fix.
- Keep tests deterministic and offline.
- Update `../../skills/gluon-cli/SKILL.md` whenever new CLI commands are added so production agents know how to use them.
