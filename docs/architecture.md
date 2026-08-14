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
