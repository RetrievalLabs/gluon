# Architecture

## Flow

- Client source files are cloned into an isolated VM.
- Client source lives on a separate attached volume so project state is decoupled from the VM lifecycle.
- The VM includes all required build tools, Gluon tools, language servers, and migration support utilities.

## Parsing

- The Rust CLI is organized by language first, with Java parsing under `languages/java`.
- Java build metadata parsing is separated by build system under `languages/java/build`.
- Java build parsing detects:
  - Java version
  - Build tool version
  - Plugins and plugin versions
  - Dependencies and dependency versions
- `LanguageParser` is the public language-level abstraction.
- `BuildSystemParser` is the Java build-system abstraction implemented by Maven and Gradle parsers.
- Offline parsing reads `pom.xml`, Gradle build files, Gradle wrapper properties, and Gradle version catalogs without executing the build.
- Resolved parsing optionally runs Maven or Gradle to extract effective dependency and plugin versions.
- Maven resolution prefers `./mvnw` when present and falls back to `mvn`.
- Gradle resolution prefers `./gradlew` when present and falls back to `gradle`.
- Build resolution failures are returned as diagnostics while preserving offline parse results.
- Java migration compatibility knowledge is stored as curated YAML under `app/code-parser/data/java`.
- Compatibility data tracks incremental migration guidance, removed APIs, deprecated-for-removal APIs, internal API risks, replacement dependencies, dependency compatibility, and build plugin compatibility separately from parser logic.
- Dependency compatibility coverage is curated and inventory-driven; unmatched dependencies are treated as requiring official-source verification before automated upgrades.
- `code-parser analyze-report` consumes a resolved `build-report.json`, loads Java compatibility KB files, and produces a separate `compatibility-report.json` with dependency, plugin, API, source-change, unknown-inventory, and diagnostic sections.
- Compatibility analysis prefers resolved dependencies and plugins when present, falls back to declared inventory, and includes declared source metadata when available.
- Java source analysis is lightweight text scanning over `.java` files. It ignores build output and VCS directories and reports findings only; it does not edit source files.
- Compatibility recommendations are advisory. Automated source or build-file rewrites happen in later migration steps after report review and test-backed planning.
