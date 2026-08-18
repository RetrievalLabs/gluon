# Business Logic Extraction from Legacy Code

## Overview

We need to extract **business logic embedded in a legacy Java codebase** and represent it in a form that an AI agent can query and reason about.

The extraction pipeline should separate:

1. **Code parsing** — understand the structure of the source code.
2. **Semantic analysis** — understand what classes, methods, types, and references mean.
3. **Entry-point detection** — identify where application execution can begin or be triggered.
4. **Business-logic detection** — identify methods/classes that are likely to contain meaningful business behavior.
5. **SQLite storage** — persist the Code Model, relationships, candidates, context packets, and diagnostics in a queryable local database.
6. **Context construction** — collect the relevant code, tests, configuration, and history.
7. **LLM extraction** — convert implementation details into explicit business rules, workflows, invariants, and state transitions.
8. **Knowledge graph construction** — connect business concepts to their implementation and evidence.

---

## Tools

### Tree-sitter

Used for fast structural analysis of the Java source code.

It extracts:

- Classes
- Methods
- Fields
- Parameters
- Method invocations
- `if` / `else`
- `switch`
- Assignments
- Exceptions
- Annotations
- Source locations

Tree-sitter answers:

> **"What does the source code structurally contain?"**

### JDTLS

Eclipse JDT Language Server is used for semantic analysis.

It can help resolve:

- Types
- Method definitions
- Method references
- Implementations
- Inheritance
- Symbols
- Cross-file relationships

JDTLS answers:

> **"What does this piece of code actually refer to?"**

Business extraction v1 requires JDTLS. If the executable is missing, the
language server cannot start, initialization fails, or semantic requests fail,
the CLI blocks extraction and prints verbose failure details.

Semantic enrichment sends bounded pipelined LSP requests within each phase
rather than waiting for every request before sending the next one. The default
window is 32 in-flight requests. Default extraction resolves document symbols
and call definitions. Deep enrichment is opt-in and additionally resolves
project-wide references and implementations; those heavier phases are capped
lower to avoid overwhelming JDTLS on large repositories.

---

## Architecture

```text
                         Legacy Repository
                                │
                                ▼
                       Repository Discovery
                                │
                    ┌───────────┴───────────┐
                    ▼                       ▼
               Tree-sitter                JDTLS
               AST Analysis          Semantic Analysis
                    │                       │
                    └───────────┬───────────┘
                                ▼
                       Method / Code Model
                                │
                                ▼
                       Entry Point Detection
                                │
                                ▼
                         Call Graph / Code
                           Relationships
                                │
                                ▼
                    Business Logic Candidates
                                │
                                ▼
                         Context Builder
                                │
                    ┌───────────┼───────────┐
                    ▼           ▼           ▼
                  Source       Tests       Git/Docs
                    │           │           │
                    └───────────┼───────────┘
                                ▼
                       SQLite Extraction DB
                                │
                                ▼
                               LLM
                                │
                                ▼
                      Business Logic IR
                                │
                                ▼
                       Knowledge Graph
```

---

## Entry Points

Entry points represent **ways in which execution can enter or be triggered within the application**.

```rust
pub enum EntryPointKind {
    // Application startup
    Main,

    // Network/API entry points
    Http,
    WebSocket,
    Rpc,
    Servlet,

    // Asynchronous entry points
    Message,
    Event,

    // Background execution
    Scheduled,
    Batch,

    // User/application commands
    Cli,

    // Framework lifecycle
    Lifecycle,

    // Persistence/infrastructure-triggered
    Database,

    // Plugin/extension mechanisms
    Plugin,
}
```

The enum represents the **general type of entry point**.

Framework-specific information should be stored separately.

For example:

```text
EntryPoint
├── kind: Message
├── framework: Kafka
├── topic: orders
└── method: OrderConsumer.process
```

Another example:

```text
EntryPoint
├── kind: Http
├── framework: Spring MVC
├── method: POST
├── route: /orders/{id}/approve
└── handler: OrderController.approve
```

---

## Code Model

The first major output of the parser and semantic analyzer is the **Code Model**.

Every class and method should initially be represented, even if it later turns out not to contain business logic.
Class and method IDs include source location so duplicate simple or qualified
names across modules, source sets, or generated/test helpers do not collide in
SQLite.
Business extraction scans production Java sources only. Unit tests, integration
tests, acceptance tests, common `*Test.java`/`*IT.java` files, and generated or
autogenerated Java sources are skipped before semantic enrichment.

```text
Code Model
│
├── Classes
│   ├── name
│   ├── package
│   ├── file
│   ├── superclass
│   ├── interfaces
│   └── methods
│
├── Methods
│   ├── name
│   ├── signature
│   ├── parameters
│   ├── return type
│   ├── source location
│   ├── callers
│   ├── callees
│   ├── reads
│   ├── writes
│   └── annotations
│
└── Relationships
    ├── CALLS
    ├── EXTENDS
    ├── IMPLEMENTS
    ├── REFERENCES
    ├── READS
    └── WRITES
```

The Code Model is persisted in SQLite as the v1 system of record. Tree-sitter creates complete structural records first. JDTLS then enriches those records with resolved symbols and call targets in default mode. Deep enrichment additionally resolves references and implementations when requested.

Example:

```json
{
  "id": "method:OrderService.approveOrder",
  "class": "class:OrderService",
  "name": "approveOrder",
  "file": "OrderService.java",
  "start_line": 42,
  "end_line": 58,
  "parameters": [
    {
      "name": "order",
      "type": "Order"
    }
  ],
  "return_type": "void"
}
```

---

## Entry Point Detection

Entry points are detected using a combination of:

```text
Tree-sitter
    +
Framework detection
    +
JDTLS semantic information
```

For example:

```java
@PostMapping("/orders/{id}/approve")
public void approve(Long id) {
    orderService.approve(id);
}
```

Tree-sitter detects:

```text
MethodDeclaration
 ├── Annotation: @PostMapping
 └── Method: approve
```

The analyzer classifies it as:

```text
EntryPointKind::Http
```

JDTLS can then resolve:

```text
orderService.approve()
        ↓
OrderService.approve(Order)
```

If JDTLS is unavailable, misconfigured, or unable to resolve a project, extraction fails before writing `business-extraction.db`. Tree-sitter diagnostics are persisted only after JDTLS has initialized and semantic enrichment has completed.

---

## Business Logic Candidate Detection

After constructing the Code Model, methods are analyzed for signals indicating business behavior.

### Structural signals

```text
if / else
switch
loops
exceptions
assignments
state changes
calculations
```

### Semantic signals

```text
domain entities
business-specific exceptions
database writes
external service calls
authorization
state transitions
feature flags
transactions
```

### Repository signals

```text
tests
Git history
PRs
documentation
business terminology
```

### Graph signals

```text
number of callers
number of callees
reachability from entry points
centrality
number of dependent components
```

Example:

```text
OrderService.approveOrder()

if_count          = 2
exception_count   = 2
state_changes     = 1
database_writes   = 1
business_terms    = 3
test_references   = 4
caller_count      = 7
```

This method becomes a **high-priority business-logic candidate**.

Candidate scoring must be deterministic. The LLM does not assign v1 candidate scores.

Each candidate stores:

```text
method_id
score
priority
raw signal counts
weighted score breakdown
evidence ranges
```

The raw signals and weighted breakdown are stored so agents can explain why a method ranked high and recompute scores after scoring rules change.

---

## Context Construction

The LLM should not receive the entire repository.

For each candidate, construct a context packet containing the relevant information.

```text
Candidate:
    OrderService.approveOrder()

Source:
    OrderService.java:42-58

Relevant callees:
    ApprovalService.check()
    CustomerService.validate()
    OrderRepository.save()

Relevant models:
    Order
    Customer

Tests:
    OrderApprovalTest
    HighValueOrderTest

Database:
    orders

Git:
    PR #821
    Commit abc123
```

The context builder is responsible for deciding what information is relevant.

---

## SQLite Storage

Business extraction v1 writes a SQLite database as the primary artifact.

```text
business-extraction.db
```

The CLI should print a short summary to stdout, including database path, module count, class and method counts, relationship counts, candidate counts by priority, and diagnostic count. It should not write a duplicate JSON report in v1.

The CLI command is:

```bash
code-parser extract-business --path <project-root> --output-dir <directory> [--database <path>] [--jdtls-command <command>] [--jdtls-workspace <directory>] [--jdtls-max-in-flight <count>] [--jdtls-deep]
```

`--jdtls-command` defaults to `jdtls`. The command is mandatory for v1 extraction.

The schema groups are:

```text
modules
classes
methods
relationships
entry_points
candidate_scores
candidate_signals
evidence_ranges
context_packets
diagnostics
```

For multi-module Maven and Gradle repositories, module ownership is persisted directly. The `modules` table records root and discovered modules, and classes and methods store `module_id` so agents can query boundaries without inferring them from paths.

Graph-like relationships are stored as edges:

```text
source_id
target_id
kind        // CALLS, EXTENDS, IMPLEMENTS, REFERENCES, READS, WRITES, TESTED_BY
confidence
source      // tree_sitter, jdtls, heuristic
```

SQLite is the stable interface for agents that need to query extraction results. A JSON export command can be added later if an external interchange format becomes necessary.

---

## Business Logic Extraction

The LLM converts implementation context from SQLite into a structured representation.

Example source:

```java
public void approveOrder(Order order) {

    if (order.getTotal() > 100000) {
        throw new ApprovalRequiredException();
    }

    if (order.getCustomer().getStatus() != ACTIVE) {
        throw new InvalidCustomerException();
    }

    order.setStatus(APPROVED);
    repository.save(order);
}
```

The LLM may extract:

```text
Business Rule:
    Orders above ₹100,000 require additional approval.

Business Rule:
    Only active customers can have orders approved.

State Transition:
    Order.PENDING → Order.APPROVED.

Side Effect:
    Approved order is persisted.
```

Every extracted rule should contain evidence.

```json
{
  "id": "rule:high-value-order-approval",
  "type": "BusinessRule",
  "statement": "Orders above ₹100,000 require additional approval.",
  "implemented_by": [
    "method:OrderService.approveOrder"
  ],
  "evidence": [
    {
      "file": "OrderService.java",
      "start_line": 42,
      "end_line": 45
    }
  ],
  "confidence": 0.96
}
```

LLM-generated business rules, workflows, invariants, and state transitions are downstream of deterministic extraction in v1. They should reference SQLite method IDs and evidence ranges rather than duplicating source content as the primary record.

---

## Knowledge Graph

The final business knowledge can be represented as a graph after LLM extraction.

```text
                 BusinessRule
                      │
               IMPLEMENTED_BY
                      │
                      ▼
                Java Method
                      │
              ┌───────┼────────┐
              ▼       ▼        ▼
            CALLS   WRITES   TESTED_BY
              │       │        │
              ▼       ▼        ▼
           Service   Entity    Test
```

Example:

```text
HighValueOrderApproval
          │
   IMPLEMENTED_BY
          ↓
OrderService.approveOrder()
          │
     ┌────┼───────────┐
     ↓    ↓           ↓
 Approval Order     ApprovalTest
 Service  status
```

The knowledge graph should focus on **meaningful semantic entities and relationships**, rather than storing every AST node.

---

## Overall Flow

```text
1. Repository
       ↓
2. Detect build system / Java version / frameworks
       ↓
3. Tree-sitter parses source
       ↓
4. JDTLS resolves semantic information
       ↓
5. Build Code Model
       ↓
6. Detect Entry Points
       ↓
7. Build Call / Reference Graph
       ↓
8. Write Code Model, Entry Points, and Relationships to SQLite
       ↓
9. Score Business Logic Candidates
       ↓
10. Write Candidate Scores and Signals to SQLite
       ↓
11. Build and Store Context Packets
       ↓
12. LLM extracts Business Logic IR from SQLite context
       ↓
13. Validate extracted logic against evidence
       ↓
14. Store Business Knowledge in Knowledge Graph
```

The key principle is:

> **Tree-sitter understands structure, JDTLS understands relationships, the candidate detector finds what is worth analyzing, and the LLM interprets the business meaning.**
