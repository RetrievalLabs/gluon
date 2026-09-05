# Gluon

**Agentic Legacy Code Migration**

> Understand the code → understand the business → capture behavior → migrate → verify.

---

# 1. Platform Architecture

```text
Frontend
   │
   ▼
Backend
   │
   ├── GitHub authorization
   ├── Repository selection
   ├── Language selection
   └── Target version selection
   │
   ▼
VM Orchestration
   │
   ▼
┌─────────────────────────────────┐
│            microVM              │
│                                 │
│ Source Code                     │
│ Language Runtime / JDK          │
│ Language Server                 │
│ Git                             │
│ Build Tools                     │
│ Gluon CLI                       │
│ Harness                         │
└─────────────────────────────────┘
```

### Components

| Component | Responsibility |
|---|---|
| Frontend | Web interface |
| Backend | Authentication and migration configuration |
| VM Orchestration | Creates isolated migration environments |
| Gluon CLI | Analyzes the legacy application |
| Harness | Performs migration and verification |
| Package | Shared contracts and protobuf definitions |

---

# 2. First Milestone — Java 8 → Java 25

```text
Legacy Java 8
      │
      ▼
    Gluon
      │
      ▼
Modern Java 25
```

Java is the first target because of its large enterprise footprint and the number of organizations still maintaining legacy Java applications.

---

# 3. Competitor — Moderne / OpenRewrite

```text
Source Code
     │
     ▼
  LST / AST
     │
     ▼
Migration Recipe
     │
     ▼
Tree Transformation
     │
     ▼
Updated Source
```

Moderne/OpenRewrite is strong at deterministic, predefined transformations.

Gluon adds business understanding and behavior preservation on top of deterministic analysis.

---

# 4. Gluon CLI

The **Gluon CLI** is a Rust-based analysis and knowledge-extraction engine.

Its responsibility is to understand the legacy application before migration begins.

## 4.1 Migration Rules

```text
Migration Rules
│
├── dependency_compatibility.yaml
│      └── Dependency versions
│
├── plugins_compatibility.yaml
│      └── Plugin versions
│
├── deprecated_for_removal.yaml
│      └── Deprecated APIs / symbols
│
├── internal_apis.yaml
│      └── Restricted JDK APIs
│
├── removed_apis.yaml
│      └── Removed APIs / modules
│
└── replacements.yaml
       └── Replacement APIs / dependencies
```

---

## 4.2 CLI Pipeline

```text
Legacy Repository
       │
       ▼
Build Report
       │
       ▼
Compatibility Analysis
       │
       ▼
CodeModel Extraction
       │
       ▼
JDTLS Resolution
       │
       ▼
Business-Relevance Scoring
       │
       ▼
TestModel Extraction
       │
       ▼
Business Knowledge Graph
       │
       ▼
Characterization Scenarios
```

---

## 4.3 Build Report

```text
pom.xml / build.gradle
        │
        ▼
Detect Build Tool
        │
   ┌────┴────┐
   ▼         ▼
 Maven     Gradle
   │         │
   └────┬────┘
        ▼
Resolve Build
        │
        ▼
┌──────────────────────────┐
│       Build Report       │
│                          │
│ • Build tool + version   │
│ • Java version           │
│ • Modules                │
│ • Dependencies           │
│ • Plugins                │
└──────────────────────────┘
```

Supports multi-module Maven and Gradle repositories.

---

## 4.4 Compatibility Analysis

```text
Build Report ───────────┐
                        │
Migration Rules ────────┼──► Compatibility Analysis
                        │
Source Code ────────────┘
                              │
                   ┌──────────┼──────────┐
                   ▼          ▼          ▼
              Tree-sitter   JDTLS      jdeps
                              │
                              ▼
                    Compatibility Report
```

The report identifies:

- Dependency upgrades
- Plugin upgrades
- Removed APIs
- Deprecated APIs
- Restricted internal APIs
- Symbols requiring migration

Source findings contain:

```text
File → Line → Symbol → Problem → Recommendation
```

---

## 4.5 Business Logic Extraction

### CodeModel

```text
Java Source
     │
     ▼
Tree-sitter
     │
     ▼
┌────────────────────┐
│     CodeModel      │
│                    │
│ Classes            │
│ Methods            │
│ Modules            │
│ Annotations        │
│ Entry Points       │
│ Method Calls       │
└─────────┬──────────┘
          │
          ▼
        SQLite
```

### Method Resolution

```text
Method Invocation
       │
       ▼
Tree-sitter
       │
       ▼
Unresolved Call
Confidence ≈ 0.25
       │
       ▼
JDTLS
       │
       ▼
Resolved Call
Confidence ≈ 0.95
```

### Business-Relevance Scoring

```text
Method
  │
  ▼
Detect Signals
  │
  ├── Branches
  ├── Persistence Calls
  ├── Business Terms
  ├── Annotations
  ├── Exceptions
  ├── Loops
  ├── State Changes
  └── Transactions
  │
  ▼
Total Score
  │
  ├── >= 18 → HIGH
  ├── >= 8  → MEDIUM
  └── < 8   → LOW
```

High-priority methods are selected for business-knowledge extraction.

---

## 4.6 Integration / E2E Test Extraction

```text
Existing Tests
      │
      ▼
Tree-sitter + JDTLS
      │
      ▼
┌────────────────────┐
│     TestModel      │
│                    │
│ Test Suites        │
│ Test Cases         │
│ Assertions         │
│ Fixtures           │
│ Invoked Methods    │
│ Test Targets       │
└──────────┬─────────┘
           │
           ▼
         SQLite
```

Unit tests are skipped.

---

## 4.7 Business Knowledge Graph

```text
High-Priority Method
        │
        ▼
Seed Context
        │
        ├── Method
        ├── Source Evidence
        └── Up to 20 Existing KG Nodes
        │
        ▼
       LLM
        │
        │ Need more context?
        │
        ├── find_business_nodes
        ├── get_business_node
        └── get_business_neighbors
        │
        ▼
Nodes + Edges
        │
        ▼
Validation
        │
        ▼
Business Knowledge Graph
        │
        ▼
      SQLite
```

### Nodes

```text
BusinessRule
Workflow
Invariant
StateTransition
SideEffect
BusinessConcept
```

### Edges

```text
SUPPORTED_BY
DEPENDS_ON
TRIGGERS
TRANSITIONS_TO
MENTIONS
```

---

## 4.8 Characterization Scenario Generation

```text
Business Knowledge Graph
          │
          ▼
Select Behavioral Nodes
          │
          ▼
Require Source Evidence
          │
          ▼
Build Scaffold Context
          │
          ▼
Characterization Scenario
          │
     ┌────┴─────┐
     ▼          ▼
TODO Files   characterization-tests.db
```

These are abstract scenarios. The Harness turns them into executable tests.

---

# 5. Harness

The **Harness** is the Python service that performs the actual migration and coordinates agents.

## 5.1 Agent Skills

```text
Harness Skills
│
├── gluon-cli
│
├── java/
│   ├── java-best-practices
│   ├── java-build-tool-best-practices
│   ├── java-dependency-selection-best-practices
│   ├── database-orm-best-practices
│   ├── jakarta-ee-best-practices
│   ├── junit-mockito-testing-best-practices
│   ├── spring-boot-best-practices
│   ├── spring-mvc-best-practices
│   └── spring-security-best-practices
│
└── version-rewrite-modernization
```

---

## 5.2 Harness Pipeline

```text
Clone Legacy Repository
        │
        ▼
Run Gluon CLI Stages
        │
        ▼
Implement Characterization Tests
        │
        ▼
Create Target Project
        │
        ▼
Select Dependencies
        │
        ▼
Create Build Structure
        │
        ▼
Migrate Source
        │
        ▼
Verify Behavior
```

---

## 5.3 CLI Repair Loop

If a CLI command fails, the Harness gives the error and relevant context to an agent.

```text
Run CLI Command
      │
      ▼
   Success?
   │      │
  Yes     No
   │      │
   ▼      ▼
 Next   Capture Error
Stage      │
           ▼
      Agent Repair
           │
           ▼
      Retry Command
```

---

## 5.4 Characterization Test Implementation

```text
characterization-tests.db
          │
          ▼
Pending Scenario
          │
          ▼
Seed Context
          │
          ▼
Context Agent
          │
          ├── DB
          ├── Source
          ├── Existing Tests
          └── JDTLS
          │
          ▼
Implementation Agent
          │
          ▼
Executable Characterization Test
          │
          ▼
Input / Output Agent
          │
          ├── Run deterministic inputs
          ├── Capture observations
          └── Mark scenario accepted
          │
          ▼
Harness Verification
          │
          ▼
Commit
```

---

## 5.5 Target Project Creation

```text
Legacy Project Structure
          │
          ▼
Create New Project Folder
          │
          ├── git init
          ├── configure remote
          └── checkout branch
          │
          ▼
Recreate High-Level Structure
```

---

## 5.6 Dependency Selection

```text
Build Report ────────────────┐
                             │
Compatibility Report ────────┼──► Dependency Selection Agent
                             │
Migration Skill ─────────────┘
                                      │
                                      ▼
                         Target-Compatible
                         Dependencies + Plugins
                                      │
                                      ▼
                  docs/migration/dependency-selection.md
```

Skill:

`java-dependency-selection-best-practices`

---

## 5.7 Build Structure

```text
dependency-selection.md
          │
          ▼
Build Agent
          │
          ▼
┌──────────────────────────┐
│ Target Build Structure   │
│                          │
│ Root Build File          │
│ Module Build Files       │
│ Java 25 Configuration    │
│ Dependencies             │
│ Plugins                  │
└──────────────────────────┘
```

Skill:

`java-build-tool-best-practices`

---

## 5.8 Source Migration — WIP

The objective is to preserve legacy behavior, not only to make the project compile on Java 25.

---

# 6. Current Status

| Stage | Status |
|---|---|
| Build Report | ✅ |
| Compatibility Analysis | ✅ |
| CodeModel Extraction | ✅ |
| JDTLS Resolution | ✅ |
| Business Scoring | ✅ |
| TestModel Extraction | ✅ |
| Business Knowledge Graph | ✅ |
| Characterization Scenarios | ✅ |
| Characterization Agent Loop | ✅ |
| Dependency Selection | ✅ |
| Build Structure | ✅ |
| Source Migration | 🚧 WIP |