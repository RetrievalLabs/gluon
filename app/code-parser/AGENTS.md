# Code Parser

A Rust-based CLI tool for parsing Java build metadata.

## Structure

- `src/languages/<language>/` - language-specific parsers and orchestration.
- `src/languages/java/build/` - Java build metadata parsing and resolution.
- `src/languages/java/build/model.rs` - shared build report data model.
- `src/languages/java/build/maven.rs` - static Maven `pom.xml` parsing.
- `src/languages/java/build/gradle.rs` - static Gradle build, wrapper, and version catalog parsing.
- `src/languages/java/build/resolver/` - optional Maven and Gradle command-based resolution.
- `src/languages/java/build/resolver/runner.rs` - command execution, executable checks, and command diagnostics.
- `src/languages/java/build/resolver/maven.rs` - Maven resolution commands and output parsing.
- `src/languages/java/build/resolver/gradle.rs` - Gradle resolution commands and output parsing.

## Rules

- Define language-level parser traits before build-system-specific traits.
- Define build-system parser traits so the same Java interface can be extended for different build systems.
- Add focused unit tests for parser behavior, fixture-based Maven and Gradle tests, and CLI tests for arguments, exit codes, stdout, and stderr.
- Add a regression test for each parser bug fix.
- Keep tests deterministic and offline.
- Update this file's Structure section whenever code-parser directories or module responsibilities change.
- Update `../../skills/gluon-cli/SKILL.md` whenever new CLI commands are added so production agents know how to use them.

## Rust Documentation

Prefer project-local documentation when available.

- Use `cargo doc --open` to generate and open documentation for the project's dependencies.
- Use `cargo doc --no-deps` for documentation of the current crate.
- Use `cargo doc --open --package <package>` for a specific package.
- Use the official Rust documentation when local documentation is insufficient:
  - https://doc.rust-lang.org/book/
  - https://doc.rust-lang.org/reference/
  - https://doc.rust-lang.org/std/
  - https://doc.rust-lang.org/cargo/
