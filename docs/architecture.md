
## Environment Setup

- A VM is provisioned with the required Maven and Gradle versions.
- Git and Claude are installed in the VM.
- A persistent volume is attached for important files and data.
- Java 8 and Java 25 are available.

## Parsing

- The Rust CLI parses `pom.xml` and Gradle build files to detect:
  - Java version
  - Build tool version
  - Plugins and plugin versions
  - Dependencies and dependency versions
