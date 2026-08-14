# Architecture

## Flow

- Client source files are cloned into an isolated VM.
- Client source lives on a separate attached volume so project state is decoupled from the VM lifecycle.
- The VM includes all required build tools, Gluon tools, language servers, and migration support utilities.

## Parsing

- The Rust CLI parses `pom.xml` and Gradle build files to detect:
  - Java version
  - Build tool version
  - Plugins and plugin versions
  - Dependencies and dependency versions
