# Agentic Execution Architecture

## 1. Core Principle

Design the system as a graph of independently executable and verifiable units,
not as one large retry loop.

```text
Input
  ↓
Split
  ↓
Unit A ─→ Execute ─→ Verify ──✓
Unit B ─→ Execute ─→ Verify ──✓
Unit C ─→ Execute ─→ Verify ──✗ ─→ Fix C ─→ Verify
Unit D ─→ Execute ─→ Verify ──✓
  ↓
Merge
  ↓
Final Gate
  ↓
Ship
```

The fundamental rule is:

> Return the unit, not the batch.

A failure in one unit must not invalidate already verified work.

## 2. High-Level Architecture

```text
                         ┌──────────────────┐
                         │      INPUT       │
                         └────────┬─────────┘
                                  │
                                  ▼
                         ┌──────────────────┐
                         │     PLANNER      │
                         │ Define the work  │
                         └────────┬─────────┘
                                  │
                                  ▼
                         ┌──────────────────┐
                         │     SPLITTER     │
                         │ Create units     │
                         └────────┬─────────┘
                                  │
              ┌───────────────────┼───────────────────┐
              │                   │                   │
              ▼                   ▼                   ▼
          ┌────────┐          ┌────────┐          ┌────────┐
          │ Unit A │          │ Unit B │          │ Unit C │
          └───┬────┘          └───┬────┘          └───┬────┘
              │                   │                   │
              ▼                   ▼                   ▼
          ┌────────┐          ┌────────┐          ┌────────┐
          │ Agent  │          │ Agent  │          │ Agent  │
          └───┬────┘          └───┬────┘          └───┬────┘
              │                   │                   │
              ▼                   ▼                   ▼
          ┌────────┐          ┌────────┐          ┌────────┐
          │ Verify │          │ Verify │          │ Verify │
          └───┬────┘          └───┬────┘          └───┬────┘
              │                   │                   │
        ┌─────┴─────┐       ┌─────┴─────┐       ┌─────┴─────┐
        │           │       │           │       │           │
       PASS        FAIL    PASS        FAIL    PASS        FAIL
        │           │       │           │       │           │
        │           ▼       │           ▼       │           ▼
        │        Fix Unit    │        Fix Unit    │        Fix Unit
        │           │       │           │       │           │
        │           └───────┼──→ Verify │       └────→ Verify
        │                   │           │
        └───────────────────┴───────────┴──────────────┐
                                                       │
                                                       ▼
                                                ┌────────────┐
                                                │   MERGE    │
                                                └─────┬──────┘
                                                      │
                                                      ▼
                                             ┌────────────────┐
                                             │ BLAST-RADIUS   │
                                             │     GATE       │
                                             └───────┬────────┘
                                                     │
                                          ┌──────────┴──────────┐
                                          │                     │
                                          ▼                     ▼
                                       ACCEPT                 HUMAN
                                          │                   REVIEW
                                          │                     │
                                          └──────────┬──────────┘
                                                     ▼
                                                   SHIP
                                                     │
                                                     ▼
                                             Learn Constraints
                                                     │
                                                     ▼
                                               Future Runs
```

## 3. Unit Model

A unit is the smallest piece of work that can be independently:

- executed,
- verified,
- rejected,
- corrected,
- and accepted.

Examples include:

```text
method
class
file
module
dependency migration
API migration
schema change
configuration change
```

The exact granularity depends on the task.

The important invariant is:

```text
failure(Unit X)
        ↓
retry(Unit X)
```

Never:

```text
failure(Unit X)
        ↓
retry(entire batch)
```

## 4. Return Protocol

When verification fails, the verifier returns a structured failure object.

```text
UNIT      authentication
VERDICT   red
REASON    test_auth_redirect failed
EVIDENCE  expected 302, got 200, handlers/auth.py:88
SCOPE     fix this file only
ATTEMPT   1/3
```

Each field has a specific responsibility.

| Field | Purpose |
| --- | --- |
| `UNIT` | Identifies exactly what failed |
| `VERDICT` | Determines the next graph transition |
| `REASON` | Explains why verification failed |
| `EVIDENCE` | Gives deterministic evidence |
| `SCOPE` | Restricts what the correction agent may modify |
| `ATTEMPT` | Prevents infinite correction loops |

### Scope Invariant

A returned unit must not expand its modification scope unless explicitly
re-planned.

```text
Returned:

SCOPE
handlers/auth.py
```

The correction agent may modify:

```text
handlers/auth.py
```

It must not opportunistically modify:

```text
handlers/user.py
database/auth.py
config/security.py
```

even if it notices unrelated improvements.

## 5. Retry Architecture

Each unit receives a bounded correction budget.

```text
Execute
   ↓
Verify
   │
   ├── PASS ─────────────→ Accepted
   │
   └── FAIL
         ↓
      Correct
         ↓
      Verify
         │
         ├── PASS ───────→ Accepted
         └── FAIL
               ↓
            Correct
               ↓
            Verify
               │
               ├── PASS → Accepted
               └── FAIL
                     ↓
                  Correct
                     ↓
                  Verify
                     │
                     ├── PASS
                     └── FAIL
                           ↓
                    RETRY EXHAUSTED
                           ↓
                       RE-PLAN
```

Recommended maximum:

```text
MAX_CORRECTION_ATTEMPTS = 3
```

After repeated failure, assume the problem may be upstream.

```text
Repeated implementation failure
             ↓
      Possible bad plan
             ↓
         Re-planning
```

The correction loop should not endlessly attempt to repair a task produced by
an incorrect plan.

## 6. Verification Gate

A gate is not a reporting mechanism.

A gate must change what executes next.

Bad:

```text
Execute
   ↓
Tests fail
   ↓
Record failure
   ↓
Continue
```

Good:

```text
Execute
   ↓
Verify
  ↙    ↘
FAIL   PASS
 ↓       ↓
Fix    Continue
```

Therefore:

> A verdict that does not change execution is only a report.

## 7. Evidence Hierarchy

The gate should not primarily trust model confidence.

Evidence should be evaluated in this order:

```text
1. Deterministic verification
2. Current-run trajectory
3. Historical reliability
4. Model assessment
```

### 7.1 Deterministic Verification

Examples:

```text
compilation
unit tests
integration tests
static analysis
schema validation
API compatibility
linting
type checking
security policies
```

These provide the strongest evidence.

### 7.2 Current-Run Trajectory

Evaluate how the unit reached its current state.

Example:

```text
Unit A
execute
  ↓
PASS
```

is stronger than:

```text
Unit B
execute
  ↓
FAIL
  ↓
fix
  ↓
FAIL
  ↓
fix
  ↓
PASS
```

Both may currently pass, but their trajectories carry different risk.

### 7.3 Historical Reliability

Track previous outcomes for the node or transformation.

Example:

```text
Transformation: javax → jakarta

Runs:      120
Accepted:  112
Rollback:    8

Rollback rate = 6.7%
```

Historical information becomes another signal for the gate.

### 7.4 Model Confidence

Model confidence may be considered, but only after stronger evidence.

Never use:

```text
if model_confidence > 0.95:
    merge()
```

Confidence is advisory evidence, not authorization.

## 8. Blast-Radius Gate

Automation should be determined primarily by the consequence of being wrong.

### Lane 1 - Reversible and Contained

Examples:

```text
copy change
test addition
isolated function
small refactor with coverage
```

Architecture:

```text
Change
  ↓
Deterministic Checks
  ↓
PASS
  ↓
Auto Accept
```

This lane can open early.

### Lane 2 - Reversible but Wide

Examples:

```text
shared utility
public API
common interface
schema addition
shared dependency
```

Architecture:

```text
Change
  ↓
Deterministic Checks
  ↓
Dependency Analysis
  ↓
Trajectory Analysis
  ↓
Historical Reliability
  ↓
Gate
```

Require stronger evidence before automatic acceptance.

### Lane 3 - Hard to Reverse

Examples:

```text
production database migration
data deletion
irreversible schema transformation
money movement
security-critical destructive action
```

This lane is closed to autonomous execution.

```text
Change
  ↓
Verification
  ↓
Human Approval
  ↓
Execute
```

Do not implement this as:

```text
confidence > 99.9%
```

Implement it as:

```text
AUTO_EXECUTION = false
```

This distinction prevents thresholds from gradually weakening safety
guarantees.

## 9. Merge Architecture

Only accepted units reach the merge stage.

```text
Unit A ─ PASS ─┐
Unit B ─ PASS ─┤
Unit C ─ PASS ─┼──→ MERGE
Unit D ─ PASS ─┘

Unit E ─ FAIL ─────X
```

The merge itself must also be verified because independently correct units can
interact incorrectly.

```text
Verified Units
      ↓
     Merge
      ↓
Integration Verification
      ↓
Blast-Radius Gate
```

## 10. Human Placement

Humans should be placed at the point of:

```text
highest consequence
        +
lowest reversibility
```

Avoid:

```text
Agent
 ↓
Human review
 ↓
Agent
 ↓
Human review
 ↓
Agent
 ↓
Human review
```

The human becomes the throughput bottleneck.

Prefer:

```text
Automated execution graph
          ↓
     Verified merge
          ↓
High-consequence decision
          ↓
        Human
          ↓
         Ship
```

Humans approve consequences, not every intermediate token produced by an
agent.

## 11. Learning Edge

Once the basic execution and verification graph works, introduce learning.

```text
Failure
   ↓
Correction
   ↓
Verification
   ↓
Accepted
   ↓
Extract Constraint
   ↓
Constraint Store
   ↓
Future Planning
```

Example:

Initial failure:

```text
Spring Boot 3 migration

javax.servlet.*
      ↓
compilation failure
```

Correction:

```text
javax.servlet.*
      ↓
jakarta.servlet.*
```

Verification:

```text
compile PASS
tests PASS
```

Extract a permanent constraint:

```text
RULE:

When migrating to Spring Boot 3+,
detect javax.servlet usage and migrate
to jakarta.servlet before compilation.
```

Future execution becomes:

```text
New Repository
      ↓
Planner
      ↓
Load Known Constraints
      ↓
Detect javax.servlet
      ↓
Plan jakarta.servlet migration
      ↓
Execute
```

The system no longer needs to rediscover the same rule through failure.

## 12. Constraint Store

The learning system can maintain constraints such as:

```yaml
id: springboot3-jakarta-servlet

applies_when:
  spring_boot_target: ">=3"

detect:
  imports:
    - javax.servlet.*

action:
  migrate_to:
    - jakarta.servlet.*

evidence:
  - compilation
  - tests

source:
  type: verified_failure

confidence:
  verified_runs: 34
  rollback_count: 0
```

Constraints should originate from verified outcomes, not merely from agent
observations.

## 13. Complete Execution Flow

```text
INPUT
  │
  ▼
PLAN
  │
  ▼
LOAD CONSTRAINTS
  │
  ▼
SPLIT INTO UNITS
  │
  ├──────────────┬──────────────┬──────────────┐
  ▼              ▼              ▼              ▼
Unit A         Unit B         Unit C         Unit D
  │              │              │              │
  ▼              ▼              ▼              ▼
Execute        Execute        Execute        Execute
  │              │              │              │
  ▼              ▼              ▼              ▼
Verify         Verify         Verify         Verify
  │              │              │              │
 PASS           PASS           FAIL           PASS
  │              │              │              │
  │              │              ▼              │
  │              │           Return Unit       │
  │              │              │              │
  │              │              ▼              │
  │              │          Scoped Fix         │
  │              │              │              │
  │              │              ▼              │
  │              │           Verify            │
  │              │              │              │
  │              │             PASS            │
  │              │              │              │
  └──────────────┴──────────────┴──────────────┘
                         │
                         ▼
                       MERGE
                         │
                         ▼
               Integration Verification
                         │
                         ▼
                 Blast-Radius Classifier
                         │
              ┌──────────┼──────────┐
              ▼          ▼          ▼
          Contained     Wide    Irreversible
              │          │          │
              ▼          ▼          ▼
           Auto       Stronger     Human
           Gate         Gate        Gate
              │          │          │
              └──────────┴──────────┘
                         │
                         ▼
                        SHIP
                         │
                         ▼
                  Observe Outcome
                         │
                         ▼
                Extract Constraints
                         │
                         ▼
                  Constraint Store
                         │
                         └──────→ Future Runs
```

## 14. System Invariants

The architecture should enforce the following invariants.

### Invariant 1 - Failure Isolation

```text
Failure(Unit X)
must not invalidate
Accepted(Unit Y)
```

### Invariant 2 - Scoped Correction

```text
Correction scope
<=
returned unit scope
```

unless explicitly re-planned.

### Invariant 3 - Bounded Retries

```text
attempts <= MAX_ATTEMPTS
```

Repeated failure escalates to planning rather than infinite correction.

### Invariant 4 - Evidence Before Confidence

```text
deterministic evidence
>
trajectory
>
historical reliability
>
model confidence
```

### Invariant 5 - Blast Radius Controls Autonomy

```text
autonomy = f(reversibility, blast_radius, evidence)
```

not:

```text
autonomy = f(model_confidence)
```

### Invariant 6 - Closed Lanes Stay Closed

Irreversible operations require explicit authorization regardless of confidence.

### Invariant 7 - Merge Is Re-Verified

```text
verified(A)
+
verified(B)

does not imply

verified(A + B)
```

Integration verification is mandatory.

### Invariant 8 - Learning Requires Verification

Only accepted and verified outcomes may become permanent constraints.

## 15. Recommended Build Order

Build the architecture incrementally.

```text
1. Gate
   ↓
2. Execution units
   ↓
3. Splitter
   ↓
4. Scoped return path
   ↓
5. Retry budget
   ↓
6. Merge verification
   ↓
7. Blast-radius lanes
   ↓
8. Human gate
   ↓
9. Constraint learning
```

Do not start with sophisticated multi-agent coordination.

Start with the ability to fail correctly.

## 16. Core Architecture Principles

The architecture can be summarized by three rules:

> Measure the path, not only the final answer.

A result that passed immediately and one that required repeated correction carry
different risk.

> A verdict that does not change what executes next is only a report.

Verification must control the execution graph.

> A failure that does not become a permanent constraint will eventually happen
> again.

The goal is therefore not merely to build an agent that can retry.

The goal is to build a system that can:

```text
split
  ↓
execute
  ↓
verify
  ↓
isolate failure
  ↓
repair locally
  ↓
merge safely
  ↓
control blast radius
  ↓
learn from verified outcomes
```

The agent loop is only one node inside this architecture.

The graph around the loop is what turns individual agents into a reliable
agentic system.
