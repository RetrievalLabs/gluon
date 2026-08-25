# Java 8 to 25 Migration Skill

A Conductor skill for migrating Java 8, 11, 17, or 21 applications toward Java 25.

## Overview

This skill activates for Java upgrade and modernization work and provides guidance on:

- **Migration Strategy**: Inventory, staged LTS targets, build toolchains, scanners, and source modernization order
- **Removed APIs**: Java EE/CORBA removal, Java 25 removed APIs, and removed tools/components
- **Runtime Risks**: Strong encapsulation, charset defaults, native access warnings, Unsafe warnings, Java agent warnings
- **Modernization Boundaries**: Final features versus preview/incubator features

## Target Path

- **Source baselines**: Java 8, 11, 17, or 21
- **Recommended checkpoints**: Java 11, 17, 21, then 25
- **Target**: Java 25

## Activation

The skill automatically activates when:

- Working on Java migration or upgrade tasks
- Working with `.java`, `pom.xml`, `build.gradle`, or `build.gradle.kts` files
- Task description contains keywords: `java 8`, `java 25`, `migration`, `upgrade`, `jdeps`, `jdeprscan`, `removed api`, `--add-opens`, `--add-exports`

## Patterns Provided

| Pattern | Description |
|---------|-------------|
| [migration-strategy](patterns/migration-strategy.md) | Java 8-25 migration strategy, risks, and edit guidance from `incremental_migration.yaml` |

## Changelog

### 1.0.0

- Initial release
- Added staged Java 8 to Java 25 migration guidance
- Added release-by-release migration risks and edit guidance from `incremental_migration.yaml`
