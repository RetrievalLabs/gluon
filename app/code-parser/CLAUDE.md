# Code Parser

A Rust-based CLI tool for parsing Java code.

## Rules

- Define a parser trait so the same interface can be implemented for different languages in the future.
- Add focused unit tests for parser behavior, fixture-based Java tests, and CLI tests for arguments, exit codes, stdout, and stderr.
- Add a regression test for each parser bug fix.
- Keep tests deterministic and offline.
- Update `../../skills/gluon-cli/SKILL.md` whenever new CLI commands are added so production agents know how to use them.
