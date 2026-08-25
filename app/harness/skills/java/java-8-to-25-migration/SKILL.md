---
name: java-8-to-25-migration
description: Use this skill when migrating Java 8, 11, 17, or 21 applications to Java 25, handling removed APIs, JDK internals, dependency upgrades, runtime flags, preview features, or staged Java LTS upgrades.
version: 1.0.0
metadata:
  mcpmarket-version: 1.0.0
---
# Java 8 to 25 Migration

Guidance for Java 8 to Java 25 migrations. Focuses on staged LTS upgrades, removed APIs, dependency and build-tool readiness, runtime warnings, and behavior-preserving modernization.

Use `patterns/migration-strategy.md` for release-by-release migration guidance.
## Core Principles

1. **Inventory first**: Parse build files, resolve dependencies, and scan internal or removed APIs before editing source.
2. **LTS staging**: Prefer Java 11, 17, 21, then 25 as migration checkpoints.
3. **Build before modernization**: Make code compile and tests pass before optional syntax upgrades.
4. **Dependency readiness**: Upgrade bytecode, mocking, ORM, application-server, native-access, serialization, and observability libraries before large source edits.
5. **Preview caution**: Treat preview/incubator features as opt-in only.
6. **Behavior preservation**: Add explicit charset, locale, timezone, native-access, and module-boundary assumptions when defaults changed.

## Quick Commands

```bash
gluon-cli code-parser parse-build --path <project> --resolve --format json --output-dir <gluon-data>
jdeps --jdk-internals <jar-or-classes>
jdeprscan --release 25 --for-removal --class-path <classpath> <classes-or-jar>
```

## Migration Rules

- Keep legacy applications on classpath first; add `module-info.java` only when dependencies are module-ready.
- Use `--add-opens` and `--add-exports` only as tracked temporary migration flags.
- Add standalone dependencies for Java EE/CORBA APIs removed from the JDK, or migrate to Jakarta packages when framework version requires it.
- Use Maven/Gradle toolchains and `--release` so source level, bytecode level, and runtime JDK are explicit.
- Run tests at each LTS checkpoint before applying Java 25-only source features.
- Use `removed_apis.yaml` for exact Java 25 removed symbol detection and replacements.

## Related Skill

Use `java-best-practices` after migration blockers are resolved and optional source modernization is in scope.
