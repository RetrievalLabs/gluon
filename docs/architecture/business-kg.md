# Business Knowledge Graph Build Plan

## 1. Objective

Build a `build-business-kg` command that converts the high-value
business logic identified by `business-extraction.db` into a separate,
persistent Business Knowledge Graph.

The architecture should keep LLM token consumption low:

-   `business-extraction.db` is the compressed structural/contextual
    source.
-   Claude performs semantic business reasoning.
-   Claude starts with a compact method prompt.
-   Claude may use bounded tools when additional context is genuinely
    required.
-   `business-kg.db` stores the resulting business knowledge and evidence.
-   The KG remains traceable to the extraction DB through
    `business_evidence.method_id`.

The system should initially process **high-priority methods only**.

------------------------------------------------------------------------

## 2. Architecture

``` text
business-extraction.db
        |
        | high-priority methods
        v
+-------------------------------+
|     Business KG Builder       |
|                               |
|  Compact initial prompt       |
|          +                    |
|  bounded Claude tool-use      |
+---------------+---------------+
                |
                v
        business-kg.db
```

The extraction DB must remain unchanged and should be treated as
read-only.

The implementation is split by language responsibility:

-   `app/code-parser/src/languages/business/` contains shared business
    contracts, the language-neutral code model, and KG build/validation
    logic.
-   `app/code-parser/src/languages/java/business/` contains Java-specific
    extraction, JDTLS enrichment, scoring, and extraction DB persistence.
-   New languages should implement extraction and extraction-DB input behavior
    behind the shared business traits instead of duplicating KG persistence or
    LLM orchestration.

------------------------------------------------------------------------

## 3. CLI

Use the existing hand-written CLI parsing style.

Recommended command:

``` bash
code-parser build-business-kg \
  --database /tmp/gluon-business-test/nakadi/business-extraction.db \
  --output /tmp/gluon-business-test/nakadi/business-kg.db \
  --source-path /tmp/gluon-business-test/nakadi \
  --min-priority high \
  --max-methods 100
```

### Options

  ----------------------------------------------------------------------------------------
  Option             Required          Default                           Description
  ------------------ ----------------- --------------------------------- -----------------
  `--database`       yes               ---                               Business
                                                                         extraction DB

  `--output`         no                `<database-dir>/business-kg.db`   KG output DB

  `--source-path`    yes               ---                               Root of source
                                                                         project

  `--min-priority`   no                `high`                            Initially support
                                                                         `high`, `medium`,
                                                                         `low`

  `--max-methods`    no                all selected                      Maximum methods
                                                                         sent to LLM

  `--max-failures`   no                no cap                            Stop after this
                                                                         many failed
                                                                         methods

  `--continue`       no                false                             Resume an existing
                                                                         KG by skipping
                                                                         methods with
                                                                         evidence

  `--force`          no                false                             Rebuild existing
                                                                         KG output
  ----------------------------------------------------------------------------------------

`--force` should delete and recreate the existing KG output database before
building. Without `--force`, an existing output database should be reopened and
appended to. Reused nodes, reused edges, and duplicate evidence should be
deduplicated by their stable keys.

`--continue` should skip methods that already have rows in
`business_evidence.method_id`, allowing interrupted long-running KG builds to
resume without spending LLM calls on completed methods. `--force` and
`--continue` are mutually exclusive.

Malformed LLM JSON should be repaired locally when possible and retried once
through the LLM when parsing still fails. Common malformed edge fields, such as
`source` and `target`, should be normalized into the validated
`source_client_id`/`source_node_id` and `target_client_id`/`target_node_id`
contract before edge validation.

The first implementation should use:

``` text
--min-priority high
```

as the normal migration-oriented workflow.

------------------------------------------------------------------------

## 4. Candidate Selection

Initially process only high-priority methods.

``` sql
SELECT
    m.id,
    m.module_id,
    m.class_id,
    m.name,
    m.signature,
    m.return_type,
    m.parameters_json,
    m.annotations_json,
    m.file,
    m.start_line,
    m.end_line,
    cs.score,
    cs.priority
FROM methods m
JOIN candidate_scores cs
    ON cs.method_id = m.id
WHERE cs.priority IN ('high')
ORDER BY cs.score DESC
LIMIT ?;
```

For v1, `--min-priority high` maps to `high` only. Later support can map
`medium` to `high, medium` and `low` to `high, medium, low`.

Do not automatically include low-scoring entry points.

Entry-point information can be provided as context when the selected
method is associated with an entry point.

Example progress output:

``` text
build-business-kg select: candidates=523 high=187 selected=100
```

------------------------------------------------------------------------

# 5. Initial LLM Context

The initial prompt should be intentionally small.

It should contain:

-   method ID
-   class name
-   method name/signature
-   source file
-   source line range
-   candidate score/priority
-   source code for the current method
-   concise entry-point information if available

Example:

``` text
You are a business logic analyst.

Your task is to build a business knowledge graph from a Java codebase.

Current method:
method_id: method:...
class: OrderService
method: approve
file: OrderService.java
lines: 42-67
priority: high
score: 18.5

Source evidence:
--------------------------------
42: if (order.getStatus() != PENDING) {
43:     throw new InvalidOrderException(...);
44: }
45:
46: order.setStatus(APPROVED);
47: paymentProcessor.charge(order.getTotal());
48: repository.save(order);
--------------------------------

The extraction database contains structural information about this
codebase. Use the available tools when additional context is required.

Your goal is to identify business meaning, not technical implementation details.

Do not invent requirements.
Do not infer business rules without evidence.
If the method contains no meaningful business logic, create nothing.
```

The model should not receive the entire extraction DB schema or large
amounts of unrelated data on every request.

------------------------------------------------------------------------

# 6. Extraction Database Context

Claude should know the conceptual structure of the extraction database
so it can request the right information.

The relevant tables include:

``` text
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

Important relationships:

``` text
methods.id
    |
    +--> classes.id
    |
    +--> modules.id
    |
    +--> relationships.source_id / target_id
    |
    +--> entry_points.method_id
    |
    +--> candidate_scores.method_id
    |
    +--> candidate_signals.method_id
    |
    +--> evidence_ranges.method_id
```

The LLM should not need to know every column of every table initially.
Tool schemas should expose the useful fields.

------------------------------------------------------------------------

# 7. Tool Design

Do not expose arbitrary SQL as the primary tool.

Avoid giving Claude:

``` text
query_db(path, arbitrary_sql)
```

because this gives the agent too much control and makes behavior harder
to validate.

Instead expose narrow, purpose-specific tools.

## 7.1 `get_method`

``` text
get_method(method_id)
```

Returns:

``` json
{
  "id": "...",
  "name": "...",
  "signature": "...",
  "class_id": "...",
  "class_name": "...",
  "module_id": "...",
  "file": "...",
  "start_line": 42,
  "end_line": 67,
  "annotations": [...]
}
```

## 7.2 `get_method_relationships`

``` text
get_method_relationships(method_id)
```

Returns:

``` json
{
  "calls": [...],
  "called_by": [...]
}
```

Limit results to a reasonable maximum, such as 20 per direction.

## 7.3 `get_method_analysis`

``` text
get_method_analysis(method_id)
```

Returns:

``` json
{
  "score": 18.5,
  "priority": "high",
  "signals": [...]
}
```

## 7.4 `read_method_source`

``` text
read_method_source(method_id)
```

The Rust implementation uses:

``` text
methods.file
methods.start_line
methods.end_line
```

and `--source-path`.

Only the current method's source should be returned by default.

Do not expose arbitrary whole-file reads by default.

## 7.5 `get_related_method`

``` text
get_related_method(method_id)
```

Returns compact metadata for a related method.

If the model needs the implementation, it can subsequently call:

``` text
read_method_source(method_id)
```

The implementation should only allow this for method IDs returned by a
bounded relationship or search tool during the same method analysis. This
permits targeted context without creating whole-repository source access.

## 7.6 Search tools

Provide bounded search tools such as:

``` text
search_methods(query, limit)
search_classes(query, limit)
search_business_terms(query, limit)
```

Each should have a strict result limit.

A source-level search tool can be added if needed:

``` text
search_source(pattern, limit)
```

with a maximum result count.

------------------------------------------------------------------------

# 8. Knowledge Graph Tools

The LLM should be able to read the existing KG because the KG is built
incrementally.

## Read

``` text
find_business_nodes(query, kind, limit)
get_business_node(node_id)
get_business_neighbors(node_id)
```

These allow Claude to reuse existing business concepts instead of
repeatedly creating duplicates.

## Write

``` text
propose_business_node(kind, name, statement, confidence, evidence)
```

and:

``` text
propose_business_edge(source_id, target_id, kind, confidence, evidence)
```

`evidence` must include the supporting `method_id`, source lines, and reason.

Rust must validate every proposed write.

The LLM must never execute raw `INSERT`, `UPDATE`, or `DELETE`
statements.

Writes should be staged per analyzed method. Rust should validate the final
structured result and commit all accepted nodes, edges, and evidence in one
transaction. Failed validation must not leave partial KG rows for that method.

Final LLM output should be structured JSON:

``` json
{
  "nodes": [
    {
      "client_id": "n1",
      "kind": "BusinessRule",
      "name": "Pending order approval rule",
      "statement": "An order can only be approved when its status is PENDING.",
      "confidence": 0.95,
      "evidence": [
        {
          "method_id": "method:OrderService#approve",
          "source_lines": [42, 43],
          "reason": "The method rejects approval unless the order status is PENDING."
        }
      ]
    }
  ],
  "edges": [
    {
      "source_client_id": "n1",
      "target_node_id": "business-node:...",
      "kind": "SUPPORTED_BY",
      "confidence": 0.9,
      "evidence": [
        {
          "method_id": "method:OrderService#approve",
          "source_lines": [42, 43],
          "reason": "The validation supports the relationship between approval and pending status."
        }
      ]
    }
  ]
}
```

Edges can reference nodes created in the same response by `client_id` or
existing KG nodes by `target_node_id` / `source_node_id`. Each edge must set
exactly one source reference and exactly one target reference.

------------------------------------------------------------------------

# 9. Business Node Types

Supported node kinds:

### `BusinessRule`

A business constraint, validation, policy, or decision.

Example:

``` text
An order can only be approved when its status is PENDING.
```

### `Workflow`

A sequence or orchestration of business operations.

Example:

``` text
Validate order -> charge payment -> approve order -> persist order.
```

### `Invariant`

A condition that must remain true.

Example:

``` text
An approved order must have APPROVED status.
```

### `StateTransition`

A meaningful business state change.

Example:

``` text
Order: PENDING -> APPROVED
```

### `SideEffect`

An observable business consequence.

Example:

``` text
Charge the customer's payment.
```

### `BusinessConcept`

A domain concept or terminology.

Example:

``` text
Order
Payment
Approval
Customer
```

The prompt must clearly distinguish these categories.

------------------------------------------------------------------------

# 10. Business Edge Types

Supported edge kinds:

``` text
SUPPORTED_BY
DEPENDS_ON
TRIGGERS
TRANSITIONS_TO
MENTIONS
```

The prompt must define edge direction explicitly.

Examples:

``` text
BusinessRule
    --SUPPORTED_BY-->
BusinessConcept
```

For state changes:

``` text
Pending Order
    --TRANSITIONS_TO-->
Approved Order
```

For business dependencies:

``` text
Order Approval
    --TRIGGERS-->
Payment Charge
```

Do not create artificial method nodes merely to represent source code.

Evidence rows provide the connection back to the extraction DB.

------------------------------------------------------------------------

# 11. KG Database Schema

Create a separate SQLite database.

Create `llm_extraction_runs` before graph tables that reference it.

## `llm_extraction_runs`

``` sql
CREATE TABLE IF NOT EXISTS llm_extraction_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    model TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    error TEXT,
    methods_total INTEGER DEFAULT 0,
    methods_processed INTEGER DEFAULT 0,
    failed INTEGER DEFAULT 0,
    nodes_created INTEGER DEFAULT 0,
    edges_created INTEGER DEFAULT 0,
    evidence_created INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_creation_input_tokens INTEGER DEFAULT 0,
    cache_read_input_tokens INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0
);
```

## `business_nodes`

``` sql
CREATE TABLE IF NOT EXISTS business_nodes (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    name TEXT NOT NULL,
    statement TEXT NOT NULL,
    confidence REAL NOT NULL,
    created_by_run_id INTEGER,
    created_at TEXT NOT NULL,

    FOREIGN KEY (created_by_run_id) REFERENCES llm_extraction_runs(id)
);
```

Nodes represent reusable business knowledge. Do not tie a node to exactly one
method, because the same business rule or concept can be supported by multiple
methods.

Node IDs should be deterministic:

``` text
business-node:<sha256(kind + normalized_name + normalized_statement)>
```

Normalization should trim whitespace, collapse internal whitespace, and compare
case-insensitively for duplicate detection. If a proposed node has the same
normalized kind, name, and statement as an existing node, reuse the existing
node and add new evidence instead of creating a duplicate.

## `business_edges`

``` sql
CREATE TABLE IF NOT EXISTS business_edges (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    confidence REAL NOT NULL,
    created_by_run_id INTEGER,
    created_at TEXT NOT NULL,

    FOREIGN KEY (source_id) REFERENCES business_nodes(id),
    FOREIGN KEY (target_id) REFERENCES business_nodes(id),
    FOREIGN KEY (created_by_run_id) REFERENCES llm_extraction_runs(id)
);
```

Edges should be unique by normalized `(source_id, target_id, kind)`. If the
same edge is proposed again, reuse it and add new evidence.

## `business_evidence`

``` sql
CREATE TABLE IF NOT EXISTS business_evidence (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    node_id TEXT,
    edge_id INTEGER,
    method_id TEXT NOT NULL,
    source_lines_json TEXT NOT NULL,
    reason TEXT NOT NULL,
    created_by_run_id INTEGER,
    created_at TEXT NOT NULL,

    FOREIGN KEY (node_id) REFERENCES business_nodes(id),
    FOREIGN KEY (edge_id) REFERENCES business_edges(id),
    FOREIGN KEY (created_by_run_id) REFERENCES llm_extraction_runs(id),
    CHECK (
        (node_id IS NOT NULL AND edge_id IS NULL)
        OR (node_id IS NULL AND edge_id IS NOT NULL)
    )
);
```

`method_id` links evidence back to `business-extraction.db`. Source file,
class, method name, and line range can be retrieved from the extraction DB.
Do not duplicate that metadata unless a later requirement demonstrates a need.
`created_by_run_id` allows later cleanup or quality comparison by extraction
run without changing the reusable node identity.

SQLite connections must enable foreign-key enforcement:

``` sql
PRAGMA foreign_keys = ON;
```

Add indexes:

``` sql
CREATE INDEX IF NOT EXISTS idx_business_nodes_kind_name
    ON business_nodes(kind, name);
CREATE INDEX IF NOT EXISTS idx_business_edges_source
    ON business_edges(source_id);
CREATE INDEX IF NOT EXISTS idx_business_edges_target
    ON business_edges(target_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_business_edges_unique
    ON business_edges(source_id, target_id, kind);
CREATE INDEX IF NOT EXISTS idx_business_evidence_method
    ON business_evidence(method_id);
CREATE INDEX IF NOT EXISTS idx_business_evidence_node
    ON business_evidence(node_id);
CREATE INDEX IF NOT EXISTS idx_business_evidence_edge
    ON business_evidence(edge_id);
CREATE INDEX IF NOT EXISTS idx_business_nodes_run
    ON business_nodes(created_by_run_id);
CREATE INDEX IF NOT EXISTS idx_business_edges_run
    ON business_edges(created_by_run_id);
CREATE INDEX IF NOT EXISTS idx_business_evidence_run
    ON business_evidence(created_by_run_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_business_evidence_node_unique
    ON business_evidence(node_id, method_id, source_lines_json, reason)
    WHERE node_id IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_business_evidence_edge_unique
    ON business_evidence(edge_id, method_id, source_lines_json, reason)
    WHERE edge_id IS NOT NULL;
```

Do not add persistent LLM cache tables in v1. Cache is operational state, not
business knowledge. Add a separate cache database later only if repeated KG
builds prove expensive enough to justify it.

------------------------------------------------------------------------

# 12. Evidence Format

Every generated node and edge must retain evidence.

Example:

``` json
{
  "method_id": "method:OrderService#approve",
  "source_lines": [42, 43, 46, 47],
  "reason": "Status validation followed by APPROVED transition and payment charge"
}
```

Evidence must be grounded in actual source/extraction information.
`source_lines` should be sorted, unique, and within the source method's line
range.

This allows migration agents to later trace:

``` text
Business Rule
    |
    v
business_evidence.method_id
    |
    v
business-extraction.db
    |
    v
source file + source lines
```

The KG must therefore remain explainable.

------------------------------------------------------------------------

# 13. LLM Rules

The system prompt should enforce:

1.  Extract only business meaning supported by evidence.
2.  Never invent requirements.
3.  Never assume undocumented business intent.
4.  Technical plumbing should normally create no business nodes.
5.  CRUD alone is not necessarily business logic.
6.  Logging/configuration/boilerplate should normally be ignored.
7.  Every node must have confidence.
8.  Every node/edge must have evidence.
9.  Only supported node kinds may be used.
10. Only supported edge kinds may be used.
11. Existing KG nodes should be reused when appropriate.
12. Use tools only when additional information is necessary.
13. Stop once enough evidence has been collected.

Confidence guidelines:

``` text
0.90 - 1.00
Directly visible in source/evidence.

0.75 - 0.89
Strongly supported by source plus structural context.

0.60 - 0.74
Reasonable inference supported by multiple pieces of evidence.

< 0.60
Do not create the business knowledge unless there is a compelling reason.
```

------------------------------------------------------------------------

# 14. Agent Loop

For every selected high-priority method:

``` text
1. Select method.
2. Build compact initial prompt.
3. Call Claude.
4. Claude can make bounded tool calls.
5. Rust executes and validates each tool call.
6. Claude returns proposed business nodes, edges, and evidence.
7. Rust validates the final structured result.
8. Commit accepted rows in one transaction.
9. Record method/run statistics.
```

Maximum tool usage should be bounded.

Recommended initial limit:

``` text
max_tool_calls_per_method = 5
```

Also limit individual tool result sizes:

``` text
relationships <= 20
search results <= 20
KG neighbors <= 20
source reads = current method or explicitly discovered related methods only
```

The initial implementation should remain sequential.

------------------------------------------------------------------------

# 15. Prompt Versioning

Define:

``` text
BUSINESS_KG_PROMPT_VERSION = "v1"
```

Increment the version when the extraction strategy or prompt contract
changes materially.

------------------------------------------------------------------------

# 16. Cache Semantics

Do not implement persistent LLM caching in v1.

If caching is added later, keep it outside `business-kg.db` and make cache
entries reconstruct complete nodes, edges, and evidence. Cache keys should use
versioned semantic inputs such as prompt version, method ID, method metadata,
source content, and relevant extraction context. Do not hash database paths as
semantic inputs.

------------------------------------------------------------------------

# 17. LLM Interface

Use a trait-based interface so the LLM layer can be tested without
Anthropic.

Conceptually:

``` rust
trait LlmClient {
    // Send prompt + tools and execute the bounded tool-use loop.
}
```

Production implementation:

``` text
AnthropicLlmClient
```

Test implementation:

``` text
MockLlmClient
```

Environment configuration:

``` text
ANTHROPIC_API_KEY
ANTHROPIC_API_BASE
ANTHROPIC_MODEL
```

No API credentials should be accepted through CLI arguments or stored in
SQLite.

------------------------------------------------------------------------

# 18. Rust Module Structure

Suggested structure:

``` text
business/
├── extraction/
│   └── ...
└── kg/
    ├── mod.rs
    ├── agent.rs
    ├── tools.rs
    ├── prompt.rs
    ├── schema.rs
    └── store.rs
```

Responsibilities:

### `agent.rs`

-   Claude agent loop
-   tool-call handling
-   iteration limits
-   finalization

### `tools.rs`

-   extraction DB tools
-   source-reading tools
-   KG read tools
-   KG write tools
-   validation

### `prompt.rs`

-   system prompt
-   initial method prompt
-   prompt version

### `schema.rs`

-   node/edge types
-   tool argument schemas
-   validation structures

### `store.rs`

-   extraction DB reads
-   KG DB schema
-   KG queries/writes
-   evidence
-   run metadata

### `mod.rs`

-   public business KG API

------------------------------------------------------------------------

# 19. Dependencies

Add only the dependencies actually required by the implementation.

Expected:

``` text
anthropic
tokio
sha2
serde
serde_json
```

Use the project's existing versions/conventions where possible.

The Anthropic client should support:

``` text
ANTHROPIC_API_KEY
ANTHROPIC_API_BASE
ANTHROPIC_MODEL
```

------------------------------------------------------------------------

# 20. CLI Progress Reporting

The command should expose useful progress without excessive output.

Initial selection:

``` text
build-business-kg select:
  candidates=523
  high_priority=187
  selected=100
```

During execution:

``` text
build-business-kg llm:
  25/100 complete
  tool_calls=8
  failed=0
  elapsed_ms=12500
```

Final:

``` text
build-business-kg database:
  nodes=187
  edges=94
  evidence=241
```

Also record the same statistics in `llm_extraction_runs`.
Token usage should be captured from Anthropic stream usage events, printed in
progress/final summaries, and stored on `llm_extraction_runs`.

------------------------------------------------------------------------

# 21. Failure Behavior

A failure on one method should not normally terminate the entire KG
build.

For a failed method:

``` text
status = failed
error = ...
```

Continue processing the remaining methods.

The overall run should indicate partial failure.

If a run is abandoned or fails before completion, rows tied to
`created_by_run_id` can be inspected or removed without affecting older
evidence from previous runs.

Fatal errors should include:

-   missing `ANTHROPIC_API_KEY`
-   invalid extraction DB
-   invalid KG DB
-   inability to initialize the source path
-   unrecoverable database errors

Tool failures should be returned to Claude when possible.

------------------------------------------------------------------------

# 22. Validation

Rust must validate all LLM-generated data.

Validate:

### Node

``` text
kind is supported
confidence is 0..1
name is non-empty
statement is non-empty
```

### Evidence

``` text
exactly one of node_id or edge_id is set
referenced node or edge exists
method_id exists in extraction DB
source lines are within the source method range
reason is non-empty
```

### Edge

``` text
kind is supported
confidence is 0..1
source_id exists
target_id exists
evidence is present
evidence supports the relationship, not only the source and target nodes
```

The LLM should not be trusted as the database validator.

------------------------------------------------------------------------

# 23. Testing

## CLI

Test:

-   `build-business-kg` routing
-   required arguments
-   default output path
-   `--min-priority`
-   `--max-methods`
-   `--force`
-   `--force` recreates the output DB
-   invalid priority values

## Database

Test:

-   KG schema creation
-   existing KG DB reopening
-   node creation
-   edge creation
-   duplicate node handling
-   deterministic node IDs
-   duplicate edge handling
-   duplicate evidence handling
-   foreign-key validation
-   `PRAGMA foreign_keys = ON`
-   evidence creation
-   index creation
-   transaction rollback on invalid proposed output

## LLM

Use `MockLlmClient`.

Test:

-   business rule extraction
-   state transition extraction
-   side effect extraction
-   technical method returning no nodes
-   invalid LLM output
-   invalid edge references
-   confidence validation
-   unsupported node/edge kind

## Tool loop

Test:

-   zero-tool method
-   one-tool method
-   multiple-tool method
-   tool-call limit
-   invalid tool arguments
-   tool failure
-   existing KG node lookup
-   cross-method edge creation

------------------------------------------------------------------------

# 24. Integration Smoke Test

Start small:

``` bash
code-parser build-business-kg \
  --database /tmp/gluon-business-test/nakadi/business-extraction.db \
  --output /tmp/gluon-business-test/nakadi/business-kg.db \
  --source-path /tmp/gluon-business-test/nakadi \
  --min-priority high \
  --max-methods 5
```

Inspect:

``` sql
SELECT * FROM business_nodes;
SELECT * FROM business_edges;
SELECT * FROM llm_extraction_runs;
SELECT * FROM business_evidence;
```

For the first five methods manually verify:

1.  Nodes represent actual business meaning.
2.  BusinessRule and Invariant are distinguished correctly.
3.  State transitions are correct.
4.  Side effects are correctly identified.
5.  Edges are meaningful.
6.  Evidence points to correct source lines.
7.  Existing business concepts are reused.
8.  Tool usage is limited.
9.  Token consumption is acceptable.
10. No technical plumbing is polluting the KG.

Only after this passes should the build be expanded to 50+ methods.

------------------------------------------------------------------------

# 25. Implementation Order

Implement in this order:

### Phase 1 --- KG storage

1.  Add KG schema.
2.  Add KG models.
3.  Add node/edge CRUD.
4.  Add evidence storage.
5.  Add run metadata.

### Phase 2 --- Candidate selection

6.  Add `build-business-kg` CLI.
7.  Read high-priority methods from extraction DB.
8.  Read source snippets.
9.  Add progress reporting.

### Phase 3 --- LLM abstraction

10. Add `LlmClient` trait.
11. Add Anthropic implementation.
12. Add mock implementation.
13. Add environment configuration.
14. Add prompt/versioning.

### Phase 4 --- Tools

15. Implement `get_method`.
16. Implement `get_method_relationships`.
17. Implement `get_method_analysis`.
18. Implement `read_method_source`.
19. Implement related-method tools.
20. Implement bounded search tools.
21. Implement KG read tools.
22. Implement KG write tools.
23. Implement evidence writes.

### Phase 5 --- Agent

24. Implement Claude tool-use loop.
25. Add maximum tool-call limit.
26. Add tool result limits.
27. Add validation.
28. Add per-method transaction handling.
29. Add run statistics.

### Phase 6 --- Testing

30. Unit tests.
31. Mock LLM integration tests.
32. Evidence/provenance tests.
33. Tool-loop tests.
34. Transaction rollback tests.
35. Nakadi smoke test with `--max-methods 5`.

### Phase 7 --- Quality evaluation

36. Run 5 methods.
37. Manually inspect KG.
38. Measure tokens/method.
39. Measure tool calls/method.
40. Measure duplicate-node rate.
41. Adjust prompt/tool boundaries.
42. Run 50 methods.
43. Evaluate migration usefulness.
44. Process the complete high-priority set.

------------------------------------------------------------------------

# 26. Key Design Principles

The implementation should follow these principles:

1.  **Extraction DB first** --- do not make Claude rediscover structural
    information.
2.  **High-priority first** --- migration value is more important than
    graph completeness initially.
3.  **Compact initial prompt** --- keep default token usage low.
4.  **On-demand tools** --- Claude can obtain additional context when
    necessary.
5.  **Bounded tools** --- every tool has strict result limits.
6.  **Separate databases** --- extraction DB remains immutable; KG is
    independently rebuildable.
7.  **Reusable nodes** --- business nodes should not be tied to exactly
    one method.
8.  **Evidence everywhere** --- every business claim must be traceable.
9.  **Incremental KG awareness** --- Claude can read the existing KG and
    reuse concepts.
10. **Validated writes** --- Rust, not Claude, enforces KG integrity.
11. **No persistent cache in v1** --- add cache later only if measured
    rebuild cost requires it.
12. **Sequential initially** --- optimize concurrency only after
    measuring.
13. **Migration-oriented quality** --- the KG is useful only if a future
    migration agent can trust and trace its business knowledge.
