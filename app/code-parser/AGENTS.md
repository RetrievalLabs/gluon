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
- `src/languages/java/business/` - Java business logic extraction, JDTLS semantic enrichment, candidate scoring, and SQLite storage.
- `src/languages/java/business/model.rs` - business extraction code model, relationships, candidates, context packet, and summary data structures.
- `src/languages/java/business/tree_sitter.rs` - tree-sitter Java structural extraction for classes, methods, annotations, entry points, and call sites.
- `src/languages/java/business/jdtls.rs` - required Eclipse JDTLS LSP client for semantic definitions and references.
- `src/languages/java/business/modules.rs` - Maven and Gradle multi-module discovery and source-file module ownership mapping.
- `src/languages/java/business/scoring.rs` - deterministic business-logic candidate scoring.
- `src/languages/java/business/store.rs` - SQLite schema creation and persistence for `business-extraction.db`.
- `src/languages/java/compatibility/` - Java compatibility analysis from resolved build reports and curated KB files.
- `src/languages/java/compatibility/model.rs` - compatibility report data model.
- `src/languages/java/compatibility/knowledge_base.rs` - YAML knowledge base loading and loose rule structs.
- `src/languages/java/compatibility/analyzer.rs` - dependency, plugin, and source-finding recommendation logic.
- `src/languages/java/compatibility/jdk_tools.rs` - optional VM JDK compile, `jdeps`, and `jdeprscan` enrichment.
- `src/languages/java/compatibility/source_scan.rs` - tree-sitter Java syntax scanner for removed, deprecated, internal, and reflective API usage.
- `data/java/` - curated Java migration compatibility knowledge base, including incremental migration guidance, removed APIs, deprecated-for-removal APIs, internal APIs, replacements, dependency compatibility, and plugin compatibility.

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
