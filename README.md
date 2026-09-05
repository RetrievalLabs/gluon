# Gluon

**Agentic legacy code migration**

Gluon modernizes legacy applications by combining **deterministic code
analysis**, **business-logic understanding**, **characterization
tests**, and **agent-driven migration**.

> **Goal:** Modernize legacy systems without breaking business behavior.

------------------------------------------------------------------------

## Architecture

``` text
Frontend
   │
   ▼
Backend ── GitHub auth, repo/language/target selection
   │
   ▼
VM Orchestration ── creates isolated migration microVM
   │
   ├── Gluon CLI (Rust) ── analysis & knowledge extraction
   └── Harness (Python) ── agents, migration & verification
```

### Components

-   **Frontend** --- Web UI.
-   **Backend** --- GitHub authorization, repository selection, language
    and target-version selection.
-   **VM Orchestration** --- Creates a microVM containing the
    repository, JDK, language server, Git, build tools, Gluon CLI, and
    Harness.
-   **Gluon CLI** --- Deterministic analysis and knowledge-extraction
    engine written in Rust.
-   **Harness** --- Python service that orchestrates agents and performs
    the migration.
-   **Package** --- Shared contracts/protobuf definitions.

------------------------------------------------------------------------

## Milestone 1 --- Java 8 → Java 25

Java is the first target because of its large enterprise footprint and
the number of organizations still maintaining Java 8 applications.

The migration pipeline targets **Java 25** while preserving existing
application behavior.

------------------------------------------------------------------------

## Gluon CLI

The CLI combines:

-   Tree-sitter
-   JDTLS / Java language server
-   `jdeps` and JDK tooling
-   Maven / Gradle
-   deterministic YAML migration rules
-   SQLite
-   controlled LLM analysis

### Compatibility Rules

  -----------------------------------------------------------------------
  File                                Purpose
  ----------------------------------- -----------------------------------
  `dependency_compatibility.yaml`     Compatible dependency versions

  `plugins_compatibility.yaml`        Compatible build-plugin versions

  `deprecated_for_removal.yaml`       Deprecated APIs/symbols

  `internal_apis.yaml`                Restricted JDK internal APIs

  `removed_apis.yaml`                 Removed Java APIs/modules

  `replacements.yaml`                 Replacement APIs/dependencies and
                                      Jakarta alternatives
  -----------------------------------------------------------------------

------------------------------------------------------------------------

## CLI Pipeline

### 1. Build Report

Parses Maven/Gradle projects and resolves:

-   Java version
-   build tool + version
-   modules
-   dependencies + versions
-   plugins + versions

Supports multi-module repositories.

``` text
pom.xml / build.gradle
        ↓
Maven / Gradle resolution
        ↓
build-report.json
```

### 2. Compatibility Analysis

Compares the build report and source code against migration rules.

Uses **Tree-sitter + JDTLS + JDK tools** to detect:

-   incompatible dependencies/plugins
-   removed APIs
-   deprecated APIs
-   restricted internal APIs
-   symbols requiring migration

Findings contain **file, line, symbol, issue, and recommended action**.

### 3. Business Logic Extraction

Tree-sitter converts Java source into a `CodeModel` containing classes,
methods, modules, annotations, entry points, and calls.

Unresolved calls start with low confidence and are resolved using JDTLS.

``` text
Tree-sitter call   → confidence ~0.25
        ↓ JDTLS
Resolved call      → confidence ~0.95
```

Methods are scored for business relevance using signals such as
branches, persistence calls, business terms, annotations, exceptions,
loops, state changes, external calls, authorization, and transactions.

``` text
score >= 18  → HIGH
score >= 8   → MEDIUM
score < 8    → LOW
```

### 4. Integration / E2E Test Extraction

Extracts existing behavioral tests into a `TestModel` containing:

-   suites and test cases
-   assertions
-   fixtures
-   invoked methods
-   test targets

Unit tests are skipped.

### 5. Business Knowledge Graph

High-priority methods are sent to an LLM to extract their business
meaning.

Each method receives a small seed of up to **20 existing KG nodes**. The
LLM can request more context through:

-   `find_business_nodes`
-   `get_business_node`
-   `get_business_neighbors`

**Nodes**

`BusinessRule` · `Workflow` · `Invariant` · `StateTransition` ·
`SideEffect` · `BusinessConcept`

**Edges**

`SUPPORTED_BY` · `DEPENDS_ON` · `TRIGGERS` · `TRANSITIONS_TO` ·
`MENTIONS`

The graph is validated and stored in SQLite.

### 6. Characterization Scenarios

Behavioral KG nodes with source evidence are converted into abstract
characterization scenarios.

The CLI generates:

-   scenario/scaffold files
-   `characterization-tests.db`

The Harness later turns them into executable tests.

------------------------------------------------------------------------

## Harness

The Harness performs the actual migration and coordinates agents.

``` text
Clone legacy repo
      ↓
Run Gluon CLI pipeline
      ↓
Create characterization tests
      ↓
Create clean target project
      ↓
Select target dependencies
      ↓
Create Maven/Gradle structure
      ↓
Migrate source code
      ↓
Verify behavior
```

### CLI Error Repair

Every CLI stage follows a repair loop:

``` text
Run command
    ↓
Success? ── Yes ──→ Next stage
    │
    No
    ↓
Give error + context to agent
    ↓
Agent repairs repository
    ↓
Retry command
```

------------------------------------------------------------------------

## Characterization Test Loop

The purpose is to capture legacy behavior **before rewriting the
application**.

``` text
Pending scenario
      ↓
Seed context
      ↓
Context Agent
      ↓
Implementation Agent
      ↓
Executable characterization test
      ↓
Input/Output Agent
      ↓
Run + record observations
      ↓
Harness verifies + commits
```

### Agents

-   **Context Agent** --- Expands context using DB rows, source, tests,
    and JDTLS.
-   **Implementation Agent** --- Writes executable tests with
    mocks/fakes.
-   **Input/Output Agent** --- Runs deterministic inputs and records
    observed behavior.

------------------------------------------------------------------------

## Target Project Creation

After characterization, the Harness creates a new project while keeping
the legacy repository intact.

### Dependency Selection

The agent reads Gluon build/compatibility reports and writes:

`docs/migration/dependency-selection.md`

It selects target-compatible dependency and plugin versions.

### Build Structure

The next agent creates:

-   root/module Maven or Gradle files
-   Java 25 configuration
-   selected dependencies
-   compatible plugins

------------------------------------------------------------------------

## Source Migration --- WIP


------------------------------------------------------------------------

## Agent Skills

  Skill                                        Focus
  -------------------------------------------- -----------------------------
  `gluon-cli`                                  Gluon CLI usage
  `java-best-practices`                        Java 8/11/17/21 → 25
  `java-build-tool-best-practices`             Maven / Gradle
  `java-dependency-selection-best-practices`   Dependency selection
  `database-orm-best-practices`                JPA / ORM / persistence
  `jakarta-ee-best-practices`                  Jakarta EE
  `junit-mockito-testing-best-practices`       Testing
  `spring-boot-best-practices`                 Spring Boot
  `spring-mvc-best-practices`                  Spring MVC
  `spring-security-best-practices`             Spring Security
  `version-rewrite-modernization`              Behavior-preserving rewrite

------------------------------------------------------------------------

## Core Design

Gluon separates modernization into three layers:

``` text
1. Deterministic Understanding
   Build + AST + JDTLS + jdeps + YAML rules
                  ↓
2. Behavioral Understanding
   Business KG + Characterization Tests
                  ↓
3. Agentic Transformation
   Dependencies + Build + Source + Verification
```

**Deterministic tools discover facts.**\
**LLMs interpret application-specific business meaning.**\
**Characterization tests capture existing behavior.**\
**Agents perform the migration within those constraints.**

------------------------------------------------------------------------

## Current Status

  Stage                          Status
  ------------------------------ --------
  Build analysis                 ✅
  Compatibility analysis         ✅
  CodeModel + JDTLS resolution   ✅
  Business scoring               ✅
  TestModel extraction           ✅
  Business Knowledge Graph       ✅
  Characterization scaffolding   ✅
  Characterization agent loop    ✅
  Dependency selection           ✅
  Target build structure         ✅
  Source migration               🚧 WIP

------------------------------------------------------------------------

## Gluon in One Line

> **Deterministic analysis + business understanding + characterization
> tests + specialized agents → behavior-preserving legacy
> modernization.**