## Initial

Build the first Java project detector slice. The goal is to point the tool at a
Java repository and emit reliable project metadata before doing symbol
extraction or migration work.

### Scope

- Detect Java version
- Detect build system: Maven, Gradle, or mixed
- Detect single-module vs multi-module project structure
- Resolve source roots:
  - `src/main/java`
  - `src/test/java`
  - `src/main/resources`
  - `src/test/resources`
- Capture declared dependencies
- Emit project metadata as JSON

### Java Version Detection

Check version signals in this order:

1. Maven `pom.xml`
   - `maven.compiler.release`
   - `maven.compiler.source`
   - `maven.compiler.target`
   - `java.version`
   - `maven-compiler-plugin` configuration
2. Gradle build files
   - `java.toolchain.languageVersion`
   - `sourceCompatibility`
   - `targetCompatibility`
3. Fallback
   - mark version as `unknown`
   - include the file/signals checked
   - include a confidence level

### Output

Write detected metadata to:

```text
.renovate-analysis/project.json
```

The JSON should include:

- repository root
- build system
- modules
- source roots
- test roots
- resource roots
- Java version
- dependencies
- confidence/warnings for incomplete detection

### Not In Scope Yet

- Java symbol extraction
- call graph generation
- business/dependency labeling
- characterization tests
- migration agents
