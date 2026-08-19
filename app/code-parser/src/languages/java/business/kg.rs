use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-5";
pub const BUSINESS_KG_PROMPT_VERSION: &str = "v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildBusinessKgOptions {
    pub database: PathBuf,
    pub output: Option<PathBuf>,
    pub source_path: PathBuf,
    pub min_priority: Priority,
    pub max_methods: Option<usize>,
    pub force: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    High,
    Medium,
    Low,
}

impl Priority {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "high" => Ok(Self::High),
            "medium" => Ok(Self::Medium),
            "low" => Ok(Self::Low),
            _ => Err(format!("invalid --min-priority: {value}")),
        }
    }

    fn selected_values(self) -> Vec<&'static str> {
        match self {
            Self::High => vec!["high"],
            Self::Medium => vec!["high", "medium"],
            Self::Low => vec!["high", "medium", "low"],
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BuildBusinessKgSummary {
    pub database_path: String,
    pub output_path: String,
    pub candidates: usize,
    pub high_priority_candidates: usize,
    pub selected: usize,
    pub methods_processed: usize,
    pub failed: usize,
    pub tool_calls: usize,
    pub nodes: usize,
    pub edges: usize,
    pub evidence: usize,
}

#[derive(Debug, Clone)]
struct CandidateMethod {
    id: String,
    class_name: String,
    name: String,
    signature: String,
    file: String,
    start_line: usize,
    end_line: usize,
    score: i64,
    priority: String,
    source: String,
    entry_points: Vec<EntryPointContext>,
}

#[derive(Debug, Clone, Serialize)]
struct EntryPointContext {
    kind: String,
    framework: Option<String>,
    route: Option<String>,
    http_method: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MethodRequest {
    method: CandidateMethod,
    existing_nodes: Vec<ExistingNode>,
}

#[derive(Debug, Clone, Serialize)]
struct ExistingNode {
    id: String,
    kind: String,
    name: String,
    statement: String,
}

pub trait LlmClient {
    fn model(&self) -> &str;
    fn analyze_method(
        &self,
        request: &MethodRequest,
        tools: &mut dyn ToolExecutor,
    ) -> Result<LlmKgResponse, String>;
}

pub trait ToolExecutor {
    fn execute_tool(&mut self, name: &str, input: &Value) -> Result<Value, String>;
    fn call_count(&self) -> usize;
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LlmKgResponse {
    #[serde(default)]
    pub nodes: Vec<NodeProposal>,
    #[serde(default)]
    pub edges: Vec<EdgeProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeProposal {
    pub client_id: Option<String>,
    pub kind: String,
    pub name: String,
    pub statement: String,
    pub confidence: f64,
    #[serde(default)]
    pub evidence: Vec<EvidenceProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeProposal {
    pub source_client_id: Option<String>,
    pub source_node_id: Option<String>,
    pub target_client_id: Option<String>,
    pub target_node_id: Option<String>,
    pub kind: String,
    pub confidence: f64,
    #[serde(default)]
    pub evidence: Vec<EvidenceProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceProposal {
    pub method_id: String,
    pub source_lines: Vec<usize>,
    pub reason: String,
}

pub struct AnthropicLlmClient {
    api_key: String,
    api_base: String,
    model: String,
    http: Client,
}

impl AnthropicLlmClient {
    pub fn from_env() -> Result<Self, String> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "missing ANTHROPIC_API_KEY".to_string())?;
        let api_base = std::env::var("ANTHROPIC_API_BASE")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| DEFAULT_ANTHROPIC_MODEL.to_string());
        Ok(Self {
            api_key,
            api_base,
            model,
            http: Client::new(),
        })
    }
}

impl LlmClient for AnthropicLlmClient {
    fn model(&self) -> &str {
        &self.model
    }

    fn analyze_method(
        &self,
        request: &MethodRequest,
        tools: &mut dyn ToolExecutor,
    ) -> Result<LlmKgResponse, String> {
        let prompt = build_prompt(request)?;
        let url = format!("{}/v1/messages", self.api_base.trim_end_matches('/'));
        let mut messages = vec![serde_json::json!({
            "role": "user",
            "content": prompt
        })];

        loop {
            let body = serde_json::json!({
                "model": self.model,
                "max_tokens": 4096,
                "system": system_prompt(),
                "tools": tool_schemas(),
                "messages": messages
            });
            let value = self.post_message(&url, &body)?;
            let content = value
                .get("content")
                .and_then(Value::as_array)
                .ok_or_else(|| format!("Anthropic response contained no content array: {value}"))?
                .clone();
            let tool_uses = tool_uses_from_content(&content)?;
            if tool_uses.is_empty() {
                let text = anthropic_text_from_content(&content).ok_or_else(|| {
                    format!("Anthropic response contained no text content: {value}")
                })?;
                return parse_llm_json(&text);
            }

            messages.push(serde_json::json!({
                "role": "assistant",
                "content": content
            }));
            let mut tool_results = Vec::new();
            for tool_use in tool_uses {
                match tools.execute_tool(&tool_use.name, &tool_use.input) {
                    Ok(result) => tool_results.push(serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use.id,
                        "content": result.to_string()
                    })),
                    Err(error) => tool_results.push(serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use.id,
                        "content": error,
                        "is_error": true
                    })),
                }
            }
            messages.push(serde_json::json!({
                "role": "user",
                "content": tool_results
            }));
        }
    }
}

impl AnthropicLlmClient {
    fn post_message(&self, url: &str, body: &Value) -> Result<Value, String> {
        let response = self
            .http
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(body)
            .send()
            .map_err(|error| format!("Anthropic request failed: {error}"))?;
        let status = response.status();
        let value: Value = response
            .json()
            .map_err(|error| format!("Anthropic response was not valid JSON: {error}"))?;
        if !status.is_success() {
            return Err(format!(
                "Anthropic request failed with status {status}: {value}"
            ));
        }
        Ok(value)
    }
}

#[derive(Debug)]
struct ToolUse {
    id: String,
    name: String,
    input: Value,
}

pub fn build_business_kg(
    options: &BuildBusinessKgOptions,
) -> Result<BuildBusinessKgSummary, String> {
    validate_build_options(options)?;
    let client = AnthropicLlmClient::from_env()?;
    build_business_kg_with_client(options, &client)
}

pub fn build_business_kg_with_client(
    options: &BuildBusinessKgOptions,
    client: &dyn LlmClient,
) -> Result<BuildBusinessKgSummary, String> {
    validate_build_options(options)?;
    let output = options.output.clone().unwrap_or_else(|| {
        options
            .database
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("business-kg.db")
    });
    if options.force && output.exists() {
        fs::remove_file(&output)
            .map_err(|error| format!("failed to remove {}: {error}", output.display()))?;
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }

    let extraction = Connection::open(&options.database).map_err(|error| {
        format!(
            "failed to open extraction database {}: {error}",
            options.database.display()
        )
    })?;
    validate_extraction_db(&extraction)?;

    let mut selected = select_methods(&extraction, &options.source_path, options)?;
    if let Some(max_methods) = options.max_methods {
        selected.truncate(max_methods);
    }

    let mut store = KgStore::open(&output)?;
    let run_id = store.start_run(client.model())?;
    let mut summary = BuildBusinessKgSummary {
        database_path: options.database.display().to_string(),
        output_path: output.display().to_string(),
        candidates: count_candidates(&extraction)?,
        high_priority_candidates: count_priority(&extraction, "high")?,
        selected: selected.len(),
        ..BuildBusinessKgSummary::default()
    };

    for method in selected {
        let request = MethodRequest {
            method: method.clone(),
            existing_nodes: store.find_nodes_for_prompt(20)?,
        };
        let (analysis_result, tool_calls) = {
            let mut tools =
                MethodToolExecutor::new(&extraction, &options.source_path, &store, &method);
            let result = client.analyze_method(&request, &mut tools);
            (result, tools.call_count())
        };
        match analysis_result {
            Ok(response) => match store.commit_method_response(run_id, &method, response) {
                Ok(()) => {
                    summary.methods_processed += 1;
                }
                Err(error) => {
                    summary.failed += 1;
                    store.record_method_failure(run_id, &method.id, &error)?;
                }
            },
            Err(error) => {
                summary.failed += 1;
                store.record_method_failure(run_id, &method.id, &error)?;
            }
        }
        summary.tool_calls += tool_calls;
    }

    store.finish_run(run_id, &summary)?;
    let counts = store.counts()?;
    summary.nodes = counts.nodes;
    summary.edges = counts.edges;
    summary.evidence = counts.evidence;
    Ok(summary)
}

fn validate_build_options(options: &BuildBusinessKgOptions) -> Result<(), String> {
    if !options.database.exists() {
        return Err(format!(
            "extraction database does not exist: {}",
            options.database.display()
        ));
    }
    if !options.source_path.exists() {
        return Err(format!(
            "source path does not exist: {}",
            options.source_path.display()
        ));
    }
    Ok(())
}

fn validate_extraction_db(connection: &Connection) -> Result<(), String> {
    for table in ["methods", "classes", "candidate_scores"] {
        let exists: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("failed to inspect extraction database: {error}"))?;
        if exists.is_none() {
            return Err(format!("invalid extraction DB: missing table {table}"));
        }
    }
    Ok(())
}

fn select_methods(
    connection: &Connection,
    source_path: &Path,
    options: &BuildBusinessKgOptions,
) -> Result<Vec<CandidateMethod>, String> {
    let priorities = options.min_priority.selected_values();
    let placeholders = std::iter::repeat_n("?", priorities.len())
        .collect::<Vec<_>>()
        .join(", ");
    let limit = options.max_methods.unwrap_or(usize::MAX);
    let sql = format!(
        "SELECT
            m.id, c.qualified_name, m.name, m.signature, m.file,
            m.start_line, m.end_line, cs.score, cs.priority
         FROM methods m
         JOIN classes c ON c.id = m.class_id
         JOIN candidate_scores cs ON cs.method_id = m.id
         WHERE cs.priority IN ({placeholders})
         ORDER BY cs.score DESC
         LIMIT ?"
    );
    let limit_i64 = limit as i64;
    let mut values: Vec<&dyn rusqlite::ToSql> = priorities
        .iter()
        .map(|priority| priority as &dyn rusqlite::ToSql)
        .collect();
    values.push(&limit_i64);
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("failed to prepare candidate query: {error}"))?;
    let rows = statement
        .query_map(values.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, String>(8)?,
            ))
        })
        .map_err(|error| format!("failed to query candidate methods: {error}"))?;

    let mut methods = Vec::new();
    for row in rows {
        let (id, class_name, name, signature, file, start_line, end_line, score, priority) =
            row.map_err(|error| format!("failed to read candidate method: {error}"))?;
        let source = read_source_range(source_path, &file, start_line as usize, end_line as usize)?;
        let entry_points = entry_points_for_method(connection, &id)?;
        methods.push(CandidateMethod {
            id,
            class_name,
            name,
            signature,
            file,
            start_line: start_line as usize,
            end_line: end_line as usize,
            score,
            priority,
            source,
            entry_points,
        });
    }
    Ok(methods)
}

fn entry_points_for_method(
    connection: &Connection,
    method_id: &str,
) -> Result<Vec<EntryPointContext>, String> {
    let exists: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'entry_points'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("failed to inspect entry_points table: {error}"))?;
    if exists.is_none() {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare(
            "SELECT kind, framework, route, http_method
             FROM entry_points
             WHERE method_id = ?1
             ORDER BY id
             LIMIT 5",
        )
        .map_err(|error| format!("failed to prepare entry point query: {error}"))?;
    let rows = statement
        .query_map([method_id], |row| {
            Ok(EntryPointContext {
                kind: row.get(0)?,
                framework: row.get(1)?,
                route: row.get(2)?,
                http_method: row.get(3)?,
            })
        })
        .map_err(|error| format!("failed to query entry points: {error}"))?;
    let mut entry_points = Vec::new();
    for row in rows {
        entry_points.push(row.map_err(|error| format!("failed to read entry point: {error}"))?);
    }
    Ok(entry_points)
}

fn read_source_range(
    source_path: &Path,
    file: &str,
    start_line: usize,
    end_line: usize,
) -> Result<String, String> {
    if start_line == 0 || end_line < start_line {
        return Err(format!(
            "invalid source range {start_line}-{end_line} for {file}"
        ));
    }
    let path = source_path.join(file);
    let source_root = source_path
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize {}: {error}", source_path.display()))?;
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("failed to read source file {}: {error}", path.display()))?;
    if !canonical.starts_with(&source_root) {
        return Err(format!(
            "source file escapes source path: {}",
            path.display()
        ));
    }
    let file = fs::File::open(&canonical).map_err(|error| {
        format!(
            "failed to open source file {}: {error}",
            canonical.display()
        )
    })?;
    let reader = std::io::BufReader::new(file);
    let mut output = String::new();
    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        if line_number < start_line {
            continue;
        }
        if line_number > end_line {
            break;
        }
        let line = line.map_err(|error| {
            format!(
                "failed to read source line {} from {}: {error}",
                line_number,
                canonical.display()
            )
        })?;
        output.push_str(&format!("{line_number}: {line}\n"));
    }
    Ok(output)
}

fn count_candidates(connection: &Connection) -> Result<usize, String> {
    connection
        .query_row("SELECT COUNT(*) FROM candidate_scores", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|count| count as usize)
        .map_err(|error| format!("failed to count candidates: {error}"))
}

fn count_priority(connection: &Connection, priority: &str) -> Result<usize, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM candidate_scores WHERE priority = ?1",
            [priority],
            |row| row.get::<_, i64>(0),
        )
        .map(|count| count as usize)
        .map_err(|error| format!("failed to count {priority} candidates: {error}"))
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| format!("failed to inspect table {table}: {error}"))
}

fn collect_json_rows(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Value>>,
) -> Result<Vec<Value>, String> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row.map_err(|error| format!("failed to read tool row: {error}"))?);
    }
    Ok(values)
}

fn arg_str<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("tool argument {key} is required"))
}

fn arg_limit(input: &Value) -> usize {
    input
        .get("limit")
        .and_then(Value::as_u64)
        .map(|value| clamp_limit(value as usize))
        .unwrap_or(20)
}

fn clamp_limit(limit: usize) -> usize {
    limit.clamp(1, 20)
}

fn build_prompt(request: &MethodRequest) -> Result<String, String> {
    let entry_points = serde_json::to_string(&request.method.entry_points)
        .map_err(|error| format!("failed to serialize entry points: {error}"))?;
    let existing_nodes = serde_json::to_string(&request.existing_nodes)
        .map_err(|error| format!("failed to serialize existing KG nodes: {error}"))?;
    Ok(format!(
        r#"prompt_version: {BUSINESS_KG_PROMPT_VERSION}

Current method:
method_id: {method_id}
class: {class_name}
method: {method_name}
signature: {signature}
file: {file}
lines: {start_line}-{end_line}
priority: {priority}
score: {score}
entry_points_json: {entry_points}

Source evidence:
--------------------------------
{source}--------------------------------

Existing KG nodes available for reuse:
{existing_nodes}

Use available tools only when this method needs more context. Stop once enough evidence has been collected.

Return only JSON with this shape:
{{"nodes":[{{"client_id":"n1","kind":"BusinessRule","name":"...","statement":"...","confidence":0.95,"evidence":[{{"method_id":"{method_id}","source_lines":[1],"reason":"..."}}]}}],"edges":[]}}

Use supported node kinds only: BusinessRule, Workflow, Invariant, StateTransition, SideEffect, BusinessConcept.
Use supported edge kinds only: SUPPORTED_BY, DEPENDS_ON, TRIGGERS, TRANSITIONS_TO, MENTIONS.
If no meaningful business logic is present, return {{"nodes":[],"edges":[]}}.
"#,
        method_id = request.method.id,
        class_name = request.method.class_name,
        method_name = request.method.name,
        signature = request.method.signature,
        file = request.method.file,
        start_line = request.method.start_line,
        end_line = request.method.end_line,
        priority = request.method.priority,
        score = request.method.score,
        source = request.method.source,
    ))
}

fn system_prompt() -> &'static str {
    "You are a business logic analyst. Extract only business meaning supported by source evidence. Never invent requirements. Ignore technical plumbing, logging, configuration, and CRUD-only code unless it encodes a business rule. Every node and edge must have evidence and confidence. Return only valid JSON."
}

fn anthropic_text_from_content(content: &[Value]) -> Option<String> {
    let mut text = String::new();
    for item in content {
        if item.get("type").and_then(Value::as_str) == Some("text")
            && let Some(part) = item.get("text").and_then(Value::as_str)
        {
            text.push_str(part);
        }
    }
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

fn tool_uses_from_content(content: &[Value]) -> Result<Vec<ToolUse>, String> {
    let mut tools = Vec::new();
    for item in content {
        if item.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("tool_use block missing id: {item}"))?
            .to_string();
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("tool_use block missing name: {item}"))?
            .to_string();
        let input = item
            .get("input")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        tools.push(ToolUse { id, name, input });
    }
    Ok(tools)
}

fn tool_schemas() -> Vec<Value> {
    vec![
        tool_schema(
            "get_method",
            "Return compact method metadata.",
            &[("method_id", "string")],
        ),
        tool_schema(
            "get_method_relationships",
            "Return bounded incoming and outgoing method relationships.",
            &[("method_id", "string")],
        ),
        tool_schema(
            "get_method_analysis",
            "Return candidate score, priority, and signals for a method.",
            &[("method_id", "string")],
        ),
        tool_schema(
            "read_method_source",
            "Read source for current or explicitly discovered related method.",
            &[("method_id", "string")],
        ),
        tool_schema(
            "get_related_method",
            "Return compact metadata for an authorized related method.",
            &[("method_id", "string")],
        ),
        tool_schema(
            "search_methods",
            "Search methods by name or signature with bounded results.",
            &[("query", "string")],
        ),
        tool_schema(
            "search_classes",
            "Search classes by name with bounded results.",
            &[("query", "string")],
        ),
        tool_schema(
            "find_business_nodes",
            "Search existing business nodes by name, statement, and optional kind.",
            &[("query", "string")],
        ),
        tool_schema(
            "get_business_node",
            "Return one existing business node.",
            &[("node_id", "string")],
        ),
        tool_schema(
            "get_business_neighbors",
            "Return bounded incoming and outgoing KG neighbors.",
            &[("node_id", "string")],
        ),
    ]
}

fn tool_schema(name: &str, description: &str, required: &[(&str, &str)]) -> Value {
    let mut properties = serde_json::Map::new();
    let mut required_names = Vec::new();
    for (field, field_type) in required {
        properties.insert(
            (*field).to_string(),
            serde_json::json!({
                "type": field_type
            }),
        );
        required_names.push(Value::String((*field).to_string()));
    }
    properties.insert(
        "limit".to_string(),
        serde_json::json!({
            "type": "integer",
            "minimum": 1,
            "maximum": 20
        }),
    );
    properties.insert(
        "kind".to_string(),
        serde_json::json!({
            "type": "string"
        }),
    );
    serde_json::json!({
        "name": name,
        "description": description,
        "input_schema": {
            "type": "object",
            "properties": properties,
            "required": required_names
        }
    })
}

fn parse_llm_json(text: &str) -> Result<LlmKgResponse, String> {
    let trimmed = text.trim();
    let json_text = if trimmed.starts_with("```") {
        extract_fenced_json(trimmed).unwrap_or(trimmed)
    } else {
        trimmed
    };
    serde_json::from_str(json_text)
        .map_err(|error| format!("failed to parse LLM JSON response: {error}: {json_text}"))
}

fn extract_fenced_json(text: &str) -> Option<&str> {
    let start = text.find('\n')? + 1;
    let end = text.rfind("```")?;
    text.get(start..end).map(str::trim)
}

struct MethodToolExecutor<'a> {
    extraction: &'a Connection,
    source_path: &'a Path,
    store: &'a KgStore,
    authorized_methods: HashSet<String>,
    call_count: usize,
    max_calls: usize,
}

impl<'a> MethodToolExecutor<'a> {
    fn new(
        extraction: &'a Connection,
        source_path: &'a Path,
        store: &'a KgStore,
        current_method: &CandidateMethod,
    ) -> Self {
        let mut authorized_methods = HashSet::new();
        authorized_methods.insert(current_method.id.clone());
        Self {
            extraction,
            source_path,
            store,
            authorized_methods,
            call_count: 0,
            max_calls: 5,
        }
    }

    fn get_method(&mut self, method_id: &str) -> Result<Value, String> {
        let mut statement = self
            .extraction
            .prepare(
                "SELECT
                    m.id, m.name, m.signature, m.class_id, c.qualified_name,
                    m.module_id, m.file, m.start_line, m.end_line, m.annotations_json
                 FROM methods m
                 JOIN classes c ON c.id = m.class_id
                 WHERE m.id = ?1",
            )
            .map_err(|error| format!("failed to prepare get_method: {error}"))?;
        statement
            .query_row([method_id], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "signature": row.get::<_, String>(2)?,
                    "class_id": row.get::<_, String>(3)?,
                    "class_name": row.get::<_, String>(4)?,
                    "module_id": row.get::<_, String>(5)?,
                    "file": row.get::<_, String>(6)?,
                    "start_line": row.get::<_, i64>(7)?,
                    "end_line": row.get::<_, i64>(8)?,
                    "annotations_json": row.get::<_, String>(9)?,
                }))
            })
            .optional()
            .map_err(|error| format!("failed to query method {method_id}: {error}"))?
            .ok_or_else(|| format!("method not found: {method_id}"))
    }

    fn get_method_relationships(&mut self, method_id: &str) -> Result<Value, String> {
        if !table_exists(self.extraction, "relationships")? {
            return Ok(serde_json::json!({"calls": [], "called_by": []}));
        }
        let calls = self.relationships(method_id, true)?;
        let called_by = self.relationships(method_id, false)?;
        for item in calls.iter().chain(called_by.iter()) {
            if let Some(id) = item.get("method_id").and_then(Value::as_str) {
                self.authorized_methods.insert(id.to_string());
            }
        }
        Ok(serde_json::json!({
            "calls": calls,
            "called_by": called_by
        }))
    }

    fn relationships(&self, method_id: &str, outgoing: bool) -> Result<Vec<Value>, String> {
        let (where_column, select_column) = if outgoing {
            ("source_id", "target_id")
        } else {
            ("target_id", "source_id")
        };
        let sql = format!(
            "SELECT r.{select_column}, r.kind, r.confidence, r.source, m.name, m.signature
             FROM relationships r
             LEFT JOIN methods m ON m.id = r.{select_column}
             WHERE r.{where_column} = ?1
             ORDER BY r.confidence DESC, r.id
             LIMIT 20"
        );
        let mut statement = self
            .extraction
            .prepare(&sql)
            .map_err(|error| format!("failed to prepare relationships query: {error}"))?;
        let rows = statement
            .query_map([method_id], |row| {
                Ok(serde_json::json!({
                    "method_id": row.get::<_, String>(0)?,
                    "kind": row.get::<_, String>(1)?,
                    "confidence": row.get::<_, f64>(2)?,
                    "source": row.get::<_, String>(3)?,
                    "name": row.get::<_, Option<String>>(4)?,
                    "signature": row.get::<_, Option<String>>(5)?,
                }))
            })
            .map_err(|error| format!("failed to query relationships: {error}"))?;
        collect_json_rows(rows)
    }

    fn get_method_analysis(&self, method_id: &str) -> Result<Value, String> {
        let score = self
            .extraction
            .query_row(
                "SELECT score, priority FROM candidate_scores WHERE method_id = ?1",
                [method_id],
                |row| {
                    Ok(serde_json::json!({
                        "score": row.get::<_, i64>(0)?,
                        "priority": row.get::<_, String>(1)?,
                    }))
                },
            )
            .optional()
            .map_err(|error| format!("failed to query candidate score: {error}"))?;
        let signals = if table_exists(self.extraction, "candidate_signals")? {
            let mut statement = self
                .extraction
                .prepare(
                    "SELECT name, count, weight
                     FROM candidate_signals
                     WHERE method_id = ?1
                     ORDER BY weight DESC, name
                     LIMIT 20",
                )
                .map_err(|error| format!("failed to prepare candidate signals query: {error}"))?;
            let rows = statement
                .query_map([method_id], |row| {
                    Ok(serde_json::json!({
                        "name": row.get::<_, String>(0)?,
                        "count": row.get::<_, i64>(1)?,
                        "weight": row.get::<_, i64>(2)?,
                    }))
                })
                .map_err(|error| format!("failed to query candidate signals: {error}"))?;
            collect_json_rows(rows)?
        } else {
            Vec::new()
        };
        Ok(serde_json::json!({
            "analysis": score,
            "signals": signals
        }))
    }

    fn read_method_source(&self, method_id: &str) -> Result<Value, String> {
        if !self.authorized_methods.contains(method_id) {
            return Err(format!("method source not authorized: {method_id}"));
        }
        let (file, start_line, end_line): (String, i64, i64) = self
            .extraction
            .query_row(
                "SELECT file, start_line, end_line FROM methods WHERE id = ?1",
                [method_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| format!("failed to query method source range: {error}"))?
            .ok_or_else(|| format!("method not found: {method_id}"))?;
        let source = read_source_range(
            self.source_path,
            &file,
            start_line as usize,
            end_line as usize,
        )?;
        Ok(serde_json::json!({
            "method_id": method_id,
            "file": file,
            "start_line": start_line,
            "end_line": end_line,
            "source": source
        }))
    }

    fn search_methods(&mut self, query: &str, limit: usize) -> Result<Value, String> {
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut statement = self
            .extraction
            .prepare(
                "SELECT m.id, m.name, m.signature, c.qualified_name, m.file, m.start_line, m.end_line
                 FROM methods m
                 JOIN classes c ON c.id = m.class_id
                 WHERE m.name LIKE ?1 ESCAPE '\\' OR m.signature LIKE ?1 ESCAPE '\\'
                 ORDER BY m.name, m.id
                 LIMIT ?2",
            )
            .map_err(|error| format!("failed to prepare method search: {error}"))?;
        let rows = statement
            .query_map(params![pattern, clamp_limit(limit) as i64], |row| {
                Ok(serde_json::json!({
                    "method_id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "signature": row.get::<_, String>(2)?,
                    "class_name": row.get::<_, String>(3)?,
                    "file": row.get::<_, String>(4)?,
                    "start_line": row.get::<_, i64>(5)?,
                    "end_line": row.get::<_, i64>(6)?,
                }))
            })
            .map_err(|error| format!("failed to search methods: {error}"))?;
        let results = collect_json_rows(rows)?;
        for result in &results {
            if let Some(id) = result.get("method_id").and_then(Value::as_str) {
                self.authorized_methods.insert(id.to_string());
            }
        }
        Ok(serde_json::json!({ "results": results }))
    }

    fn search_classes(&self, query: &str, limit: usize) -> Result<Value, String> {
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut statement = self
            .extraction
            .prepare(
                "SELECT id, name, qualified_name, file, start_line, end_line
                 FROM classes
                 WHERE name LIKE ?1 ESCAPE '\\' OR qualified_name LIKE ?1 ESCAPE '\\'
                 ORDER BY qualified_name
                 LIMIT ?2",
            )
            .map_err(|error| format!("failed to prepare class search: {error}"))?;
        let rows = statement
            .query_map(params![pattern, clamp_limit(limit) as i64], |row| {
                Ok(serde_json::json!({
                    "class_id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "qualified_name": row.get::<_, String>(2)?,
                    "file": row.get::<_, String>(3)?,
                    "start_line": row.get::<_, i64>(4)?,
                    "end_line": row.get::<_, i64>(5)?,
                }))
            })
            .map_err(|error| format!("failed to search classes: {error}"))?;
        Ok(serde_json::json!({ "results": collect_json_rows(rows)? }))
    }

    fn find_business_nodes(&self, input: &Value) -> Result<Value, String> {
        let query = arg_str(input, "query")?;
        let kind = input.get("kind").and_then(Value::as_str);
        let limit = arg_limit(input);
        self.store.find_business_nodes(query, kind, limit)
    }
}

impl ToolExecutor for MethodToolExecutor<'_> {
    fn execute_tool(&mut self, name: &str, input: &Value) -> Result<Value, String> {
        if self.call_count >= self.max_calls {
            return Err(format!("tool call limit exceeded: {}", self.max_calls));
        }
        self.call_count += 1;
        match name {
            "get_method" => self.get_method(arg_str(input, "method_id")?),
            "get_method_relationships" => {
                self.get_method_relationships(arg_str(input, "method_id")?)
            }
            "get_method_analysis" => self.get_method_analysis(arg_str(input, "method_id")?),
            "read_method_source" => self.read_method_source(arg_str(input, "method_id")?),
            "get_related_method" => {
                let method_id = arg_str(input, "method_id")?;
                if !self.authorized_methods.contains(method_id) {
                    return Err(format!("related method not authorized: {method_id}"));
                }
                self.get_method(method_id)
            }
            "search_methods" => self.search_methods(arg_str(input, "query")?, arg_limit(input)),
            "search_classes" => self.search_classes(arg_str(input, "query")?, arg_limit(input)),
            "find_business_nodes" => self.find_business_nodes(input),
            "get_business_node" => self.store.get_business_node(arg_str(input, "node_id")?),
            "get_business_neighbors" => self
                .store
                .get_business_neighbors(arg_str(input, "node_id")?, arg_limit(input)),
            other => Err(format!("unsupported tool: {other}")),
        }
    }

    fn call_count(&self) -> usize {
        self.call_count
    }
}

#[derive(Debug)]
struct KgCounts {
    nodes: usize,
    edges: usize,
    evidence: usize,
}

struct KgStore {
    connection: Connection,
}

impl KgStore {
    fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path)
            .map_err(|error| format!("failed to open KG database {}: {error}", path.display()))?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|error| format!("failed to enable KG foreign keys: {error}"))?;
        create_kg_schema(&connection)?;
        Ok(Self { connection })
    }

    fn start_run(&mut self, model: &str) -> Result<i64, String> {
        self.connection
            .execute(
                "INSERT INTO llm_extraction_runs (model, status, started_at)
                 VALUES (?1, 'running', ?2)",
                params![model, timestamp()],
            )
            .map_err(|error| format!("failed to create KG run: {error}"))?;
        Ok(self.connection.last_insert_rowid())
    }

    fn finish_run(&mut self, run_id: i64, summary: &BuildBusinessKgSummary) -> Result<(), String> {
        let status = if summary.failed == 0 {
            "completed"
        } else {
            "partial_failure"
        };
        self.connection
            .execute(
                "UPDATE llm_extraction_runs
                 SET status = ?1, finished_at = ?2, methods_processed = ?3, failed = ?4
                 WHERE id = ?5",
                params![
                    status,
                    timestamp(),
                    summary.methods_processed as i64,
                    summary.failed as i64,
                    run_id,
                ],
            )
            .map_err(|error| format!("failed to finish KG run {run_id}: {error}"))?;
        Ok(())
    }

    fn record_method_failure(
        &mut self,
        run_id: i64,
        method_id: &str,
        error: &str,
    ) -> Result<(), String> {
        let existing: Option<String> = self
            .connection
            .query_row(
                "SELECT error FROM llm_extraction_runs WHERE id = ?1",
                [run_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map(|value| value.flatten())
            .map_err(|error| format!("failed to read run error state: {error}"))?;
        let message = format!("{method_id}: {error}");
        let combined = match existing.filter(|value| !value.is_empty()) {
            Some(value) => format!("{value}\n{message}"),
            None => message,
        };
        self.connection
            .execute(
                "UPDATE llm_extraction_runs SET error = ?1 WHERE id = ?2",
                params![combined, run_id],
            )
            .map_err(|error| format!("failed to record KG method failure: {error}"))?;
        Ok(())
    }

    fn find_nodes_for_prompt(&self, limit: usize) -> Result<Vec<ExistingNode>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, kind, name, statement
                 FROM business_nodes
                 ORDER BY created_at DESC, id
                 LIMIT ?1",
            )
            .map_err(|error| format!("failed to prepare KG node lookup: {error}"))?;
        let rows = statement
            .query_map([limit as i64], |row| {
                Ok(ExistingNode {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    name: row.get(2)?,
                    statement: row.get(3)?,
                })
            })
            .map_err(|error| format!("failed to query KG nodes: {error}"))?;
        let mut nodes = Vec::new();
        for row in rows {
            nodes.push(row.map_err(|error| format!("failed to read KG node: {error}"))?);
        }
        Ok(nodes)
    }

    fn find_business_nodes(
        &self,
        query: &str,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Value, String> {
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let limit = clamp_limit(limit) as i64;
        let results = if let Some(kind) = kind.filter(|value| !value.trim().is_empty()) {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, kind, name, statement, confidence
                     FROM business_nodes
                     WHERE kind = ?1
                       AND (name LIKE ?2 ESCAPE '\\' OR statement LIKE ?2 ESCAPE '\\')
                     ORDER BY created_at DESC, id
                     LIMIT ?3",
                )
                .map_err(|error| format!("failed to prepare business node search: {error}"))?;
            let rows = statement
                .query_map(params![kind, pattern, limit], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "kind": row.get::<_, String>(1)?,
                        "name": row.get::<_, String>(2)?,
                        "statement": row.get::<_, String>(3)?,
                        "confidence": row.get::<_, f64>(4)?,
                    }))
                })
                .map_err(|error| format!("failed to search business nodes: {error}"))?;
            collect_json_rows(rows)?
        } else {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT id, kind, name, statement, confidence
                     FROM business_nodes
                     WHERE name LIKE ?1 ESCAPE '\\' OR statement LIKE ?1 ESCAPE '\\'
                     ORDER BY created_at DESC, id
                     LIMIT ?2",
                )
                .map_err(|error| format!("failed to prepare business node search: {error}"))?;
            let rows = statement
                .query_map(params![pattern, limit], |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "kind": row.get::<_, String>(1)?,
                        "name": row.get::<_, String>(2)?,
                        "statement": row.get::<_, String>(3)?,
                        "confidence": row.get::<_, f64>(4)?,
                    }))
                })
                .map_err(|error| format!("failed to search business nodes: {error}"))?;
            collect_json_rows(rows)?
        };
        Ok(serde_json::json!({ "results": results }))
    }

    fn get_business_node(&self, node_id: &str) -> Result<Value, String> {
        self.connection
            .query_row(
                "SELECT id, kind, name, statement, confidence
                 FROM business_nodes
                 WHERE id = ?1",
                [node_id],
                |row| {
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "kind": row.get::<_, String>(1)?,
                        "name": row.get::<_, String>(2)?,
                        "statement": row.get::<_, String>(3)?,
                        "confidence": row.get::<_, f64>(4)?,
                    }))
                },
            )
            .optional()
            .map_err(|error| format!("failed to query business node {node_id}: {error}"))?
            .ok_or_else(|| format!("business node not found: {node_id}"))
    }

    fn get_business_neighbors(&self, node_id: &str, limit: usize) -> Result<Value, String> {
        let limit = clamp_limit(limit) as i64;
        let outgoing = self.neighbor_edges(node_id, true, limit)?;
        let incoming = self.neighbor_edges(node_id, false, limit)?;
        Ok(serde_json::json!({
            "outgoing": outgoing,
            "incoming": incoming
        }))
    }

    fn neighbor_edges(
        &self,
        node_id: &str,
        outgoing: bool,
        limit: i64,
    ) -> Result<Vec<Value>, String> {
        let (where_column, join_column) = if outgoing {
            ("source_id", "target_id")
        } else {
            ("target_id", "source_id")
        };
        let sql = format!(
            "SELECT e.id, e.source_id, e.target_id, e.kind, e.confidence,
                    n.id, n.kind, n.name, n.statement
             FROM business_edges e
             JOIN business_nodes n ON n.id = e.{join_column}
             WHERE e.{where_column} = ?1
             ORDER BY e.id
             LIMIT ?2"
        );
        let mut statement = self
            .connection
            .prepare(&sql)
            .map_err(|error| format!("failed to prepare KG neighbor query: {error}"))?;
        let rows = statement
            .query_map(params![node_id, limit], |row| {
                Ok(serde_json::json!({
                    "edge_id": row.get::<_, i64>(0)?,
                    "source_id": row.get::<_, String>(1)?,
                    "target_id": row.get::<_, String>(2)?,
                    "edge_kind": row.get::<_, String>(3)?,
                    "edge_confidence": row.get::<_, f64>(4)?,
                    "neighbor": {
                        "id": row.get::<_, String>(5)?,
                        "kind": row.get::<_, String>(6)?,
                        "name": row.get::<_, String>(7)?,
                        "statement": row.get::<_, String>(8)?,
                    }
                }))
            })
            .map_err(|error| format!("failed to query KG neighbors: {error}"))?;
        collect_json_rows(rows)
    }

    fn commit_method_response(
        &mut self,
        run_id: i64,
        method: &CandidateMethod,
        response: LlmKgResponse,
    ) -> Result<(), String> {
        validate_response(method, &response)?;
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| format!("failed to start KG transaction: {error}"))?;
        let mut client_nodes = HashMap::new();
        for node in response.nodes {
            let node_id = deterministic_node_id(&node.kind, &node.name, &node.statement);
            transaction
                .execute(
                    "INSERT INTO business_nodes (
                        id, kind, name, statement, confidence, created_by_run_id, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(id) DO NOTHING",
                    params![
                        node_id,
                        node.kind,
                        node.name,
                        node.statement,
                        node.confidence,
                        run_id,
                        timestamp(),
                    ],
                )
                .map_err(|error| format!("failed to insert KG node: {error}"))?;
            if let Some(client_id) = node.client_id {
                client_nodes.insert(client_id, node_id.clone());
            }
            for evidence in node.evidence {
                insert_evidence(&transaction, run_id, Some(&node_id), None, evidence)?;
            }
        }
        for edge in response.edges {
            let source_id = resolve_edge_node(
                edge.source_node_id.as_deref(),
                edge.source_client_id.as_deref(),
                &client_nodes,
            )?;
            let target_id = resolve_edge_node(
                edge.target_node_id.as_deref(),
                edge.target_client_id.as_deref(),
                &client_nodes,
            )?;
            transaction
                .execute(
                    "INSERT INTO business_edges (
                        source_id, target_id, kind, confidence, created_by_run_id, created_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(source_id, target_id, kind) DO NOTHING",
                    params![
                        source_id,
                        target_id,
                        edge.kind,
                        edge.confidence,
                        run_id,
                        timestamp(),
                    ],
                )
                .map_err(|error| format!("failed to insert KG edge: {error}"))?;
            let edge_id: i64 = transaction
                .query_row(
                    "SELECT id FROM business_edges
                     WHERE source_id = ?1 AND target_id = ?2 AND kind = ?3",
                    params![source_id, target_id, edge.kind],
                    |row| row.get(0),
                )
                .map_err(|error| format!("failed to resolve KG edge ID: {error}"))?;
            for evidence in edge.evidence {
                insert_evidence(&transaction, run_id, None, Some(edge_id), evidence)?;
            }
        }
        transaction
            .commit()
            .map_err(|error| format!("failed to commit KG transaction: {error}"))?;
        Ok(())
    }

    fn counts(&self) -> Result<KgCounts, String> {
        Ok(KgCounts {
            nodes: count_table(&self.connection, "business_nodes")?,
            edges: count_table(&self.connection, "business_edges")?,
            evidence: count_table(&self.connection, "business_evidence")?,
        })
    }
}

fn create_kg_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS llm_extraction_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                model TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                error TEXT,
                methods_processed INTEGER DEFAULT 0,
                failed INTEGER DEFAULT 0
            );

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
            ",
        )
        .map_err(|error| format!("failed to create KG schema: {error}"))
}

fn validate_response(method: &CandidateMethod, response: &LlmKgResponse) -> Result<(), String> {
    let mut client_ids = HashSet::new();
    for node in &response.nodes {
        validate_node(node)?;
        if let Some(client_id) = &node.client_id
            && !client_ids.insert(client_id.clone())
        {
            return Err(format!("duplicate node client_id: {client_id}"));
        }
        for evidence in &node.evidence {
            validate_evidence(method, evidence)?;
        }
    }
    for edge in &response.edges {
        validate_edge(edge, &client_ids)?;
        for evidence in &edge.evidence {
            validate_evidence(method, evidence)?;
        }
    }
    Ok(())
}

fn validate_node(node: &NodeProposal) -> Result<(), String> {
    if !matches!(
        node.kind.as_str(),
        "BusinessRule"
            | "Workflow"
            | "Invariant"
            | "StateTransition"
            | "SideEffect"
            | "BusinessConcept"
    ) {
        return Err(format!("unsupported business node kind: {}", node.kind));
    }
    validate_confidence(node.confidence)?;
    if node.name.trim().is_empty() {
        return Err("business node name is empty".to_string());
    }
    if node.statement.trim().is_empty() {
        return Err("business node statement is empty".to_string());
    }
    if node.evidence.is_empty() {
        return Err(format!("business node {} has no evidence", node.name));
    }
    Ok(())
}

fn validate_edge(edge: &EdgeProposal, client_ids: &HashSet<String>) -> Result<(), String> {
    if !matches!(
        edge.kind.as_str(),
        "SUPPORTED_BY" | "DEPENDS_ON" | "TRIGGERS" | "TRANSITIONS_TO" | "MENTIONS"
    ) {
        return Err(format!("unsupported business edge kind: {}", edge.kind));
    }
    validate_confidence(edge.confidence)?;
    validate_one_edge_reference(
        edge.source_node_id.as_deref(),
        edge.source_client_id.as_deref(),
    )?;
    validate_one_edge_reference(
        edge.target_node_id.as_deref(),
        edge.target_client_id.as_deref(),
    )?;
    if let Some(client_id) = &edge.source_client_id
        && !client_ids.contains(client_id)
    {
        return Err(format!("unknown source_client_id: {client_id}"));
    }
    if let Some(client_id) = &edge.target_client_id
        && !client_ids.contains(client_id)
    {
        return Err(format!("unknown target_client_id: {client_id}"));
    }
    if edge.evidence.is_empty() {
        return Err(format!("business edge {} has no evidence", edge.kind));
    }
    Ok(())
}

fn validate_one_edge_reference(
    node_id: Option<&str>,
    client_id: Option<&str>,
) -> Result<(), String> {
    match (node_id, client_id) {
        (Some(_), None) | (None, Some(_)) => Ok(()),
        _ => Err("edge must set exactly one node reference per side".to_string()),
    }
}

fn validate_evidence(method: &CandidateMethod, evidence: &EvidenceProposal) -> Result<(), String> {
    if evidence.method_id != method.id {
        return Err(format!(
            "evidence method_id {} does not match current method {}",
            evidence.method_id, method.id
        ));
    }
    if evidence.reason.trim().is_empty() {
        return Err("evidence reason is empty".to_string());
    }
    if evidence.source_lines.is_empty() {
        return Err("evidence source_lines is empty".to_string());
    }
    let mut previous = 0;
    for line in &evidence.source_lines {
        if *line < method.start_line || *line > method.end_line {
            return Err(format!(
                "evidence line {} outside method range {}-{}",
                line, method.start_line, method.end_line
            ));
        }
        if *line <= previous {
            return Err("evidence source_lines must be sorted and unique".to_string());
        }
        previous = *line;
    }
    Ok(())
}

fn validate_confidence(confidence: f64) -> Result<(), String> {
    if !(0.0..=1.0).contains(&confidence) {
        return Err(format!("confidence out of range: {confidence}"));
    }
    Ok(())
}

fn insert_evidence(
    transaction: &rusqlite::Transaction<'_>,
    run_id: i64,
    node_id: Option<&str>,
    edge_id: Option<i64>,
    evidence: EvidenceProposal,
) -> Result<(), String> {
    let source_lines = serde_json::to_string(&evidence.source_lines)
        .map_err(|error| format!("failed to serialize evidence source lines: {error}"))?;
    transaction
        .execute(
            "INSERT INTO business_evidence (
                node_id, edge_id, method_id, source_lines_json, reason, created_by_run_id, created_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT DO NOTHING",
            params![
                node_id,
                edge_id,
                evidence.method_id,
                source_lines,
                evidence.reason,
                run_id,
                timestamp(),
            ],
        )
        .map_err(|error| format!("failed to insert KG evidence: {error}"))?;
    Ok(())
}

fn resolve_edge_node(
    node_id: Option<&str>,
    client_id: Option<&str>,
    client_nodes: &HashMap<String, String>,
) -> Result<String, String> {
    if let Some(node_id) = node_id {
        return Ok(node_id.to_string());
    }
    let client_id = client_id.ok_or_else(|| "missing edge node reference".to_string())?;
    client_nodes
        .get(client_id)
        .cloned()
        .ok_or_else(|| format!("unknown edge client node reference: {client_id}"))
}

fn deterministic_node_id(kind: &str, name: &str, statement: &str) -> String {
    let input = format!(
        "{}\0{}\0{}",
        normalize(kind),
        normalize(name),
        normalize(statement)
    );
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("business-node:{:x}", hasher.finalize())
}

fn normalize(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn count_table(connection: &Connection, table: &str) -> Result<usize, String> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    connection
        .query_row(&sql, [], |row| row.get::<_, i64>(0))
        .map(|count| count as usize)
        .map_err(|error| format!("failed to count {table}: {error}"))
}

fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    seconds.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockClient {
        response: LlmKgResponse,
    }

    impl LlmClient for MockClient {
        fn model(&self) -> &str {
            "mock"
        }

        fn analyze_method(
            &self,
            _request: &MethodRequest,
            _tools: &mut dyn ToolExecutor,
        ) -> Result<LlmKgResponse, String> {
            Ok(self.response.clone())
        }
    }

    #[test]
    fn deterministic_node_id_normalizes_content() {
        assert_eq!(
            deterministic_node_id("BusinessRule", " Pending  Rule ", "A   B"),
            deterministic_node_id("businessrule", "pending rule", "a b")
        );
    }

    #[test]
    fn builds_kg_with_deduped_nodes_edges_and_evidence() {
        let root = test_dir("business-kg");
        let source = root.join("OrderService.java");
        fs::write(
            &source,
            "class OrderService {\n  void approve() {\n    if (status != PENDING) throw new RuntimeException();\n  }\n}\n",
        )
        .unwrap();
        let extraction_db = root.join("business-extraction.db");
        write_extraction_db(&extraction_db, "OrderService.java");
        let output = root.join("business-kg.db");
        let response = LlmKgResponse {
            nodes: vec![NodeProposal {
                client_id: Some("n1".to_string()),
                kind: "BusinessRule".to_string(),
                name: "Pending approval rule".to_string(),
                statement: "Approval requires PENDING status.".to_string(),
                confidence: 0.95,
                evidence: vec![EvidenceProposal {
                    method_id: "method:OrderService#approve".to_string(),
                    source_lines: vec![3],
                    reason: "The method rejects non-pending status.".to_string(),
                }],
            }],
            edges: Vec::new(),
        };
        let options = BuildBusinessKgOptions {
            database: extraction_db,
            output: Some(output.clone()),
            source_path: root.clone(),
            min_priority: Priority::High,
            max_methods: Some(1),
            force: false,
        };

        let summary = build_business_kg_with_client(&options, &MockClient { response }).unwrap();
        assert_eq!(summary.nodes, 1);
        assert_eq!(summary.evidence, 1);

        let second = build_business_kg_with_client(
            &options,
            &MockClient {
                response: LlmKgResponse {
                    nodes: vec![NodeProposal {
                        client_id: Some("n1".to_string()),
                        kind: "BusinessRule".to_string(),
                        name: "Pending approval rule".to_string(),
                        statement: "Approval requires PENDING status.".to_string(),
                        confidence: 0.95,
                        evidence: vec![EvidenceProposal {
                            method_id: "method:OrderService#approve".to_string(),
                            source_lines: vec![3],
                            reason: "The method rejects non-pending status.".to_string(),
                        }],
                    }],
                    edges: Vec::new(),
                },
            },
        )
        .unwrap();
        assert_eq!(second.nodes, 1);
        assert_eq!(second.evidence, 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_response_rolls_back_method_rows() {
        let root = test_dir("business-kg-rollback");
        fs::write(
            root.join("OrderService.java"),
            "class OrderService {\n  void approve() {\n    return;\n  }\n}\n",
        )
        .unwrap();
        let extraction_db = root.join("business-extraction.db");
        write_extraction_db(&extraction_db, "OrderService.java");
        let output = root.join("business-kg.db");
        let response = LlmKgResponse {
            nodes: vec![NodeProposal {
                client_id: Some("n1".to_string()),
                kind: "NotSupported".to_string(),
                name: "Bad".to_string(),
                statement: "Bad".to_string(),
                confidence: 0.9,
                evidence: vec![EvidenceProposal {
                    method_id: "method:OrderService#approve".to_string(),
                    source_lines: vec![3],
                    reason: "Bad".to_string(),
                }],
            }],
            edges: Vec::new(),
        };
        let options = BuildBusinessKgOptions {
            database: extraction_db,
            output: Some(output.clone()),
            source_path: root.clone(),
            min_priority: Priority::High,
            max_methods: Some(1),
            force: false,
        };

        let summary = build_business_kg_with_client(&options, &MockClient { response }).unwrap();
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.nodes, 0);
        assert_eq!(summary.evidence, 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn tool_executor_bounds_source_reads_and_call_count() {
        let root = test_dir("business-kg-tools");
        fs::write(
            root.join("OrderService.java"),
            "class OrderService {\n  void approve() {\n    validate();\n  }\n  void validate() {\n    if (status != PENDING) throw new RuntimeException();\n  }\n}\n",
        )
        .unwrap();
        let extraction_db = root.join("business-extraction.db");
        write_extraction_db(&extraction_db, "OrderService.java");
        let extraction = Connection::open(&extraction_db).unwrap();
        let store = KgStore::open(&root.join("business-kg.db")).unwrap();
        let current = CandidateMethod {
            id: "method:OrderService#approve".to_string(),
            class_name: "OrderService".to_string(),
            name: "approve".to_string(),
            signature: "approve()".to_string(),
            file: "OrderService.java".to_string(),
            start_line: 2,
            end_line: 4,
            score: 10,
            priority: "high".to_string(),
            source: String::new(),
            entry_points: Vec::new(),
        };
        let mut tools = MethodToolExecutor::new(&extraction, &root, &store, &current);

        assert!(
            tools
                .execute_tool(
                    "read_method_source",
                    &serde_json::json!({"method_id": "method:OrderService#approve"})
                )
                .unwrap()["source"]
                .as_str()
                .unwrap()
                .contains("validate();")
        );
        assert!(
            tools
                .execute_tool(
                    "read_method_source",
                    &serde_json::json!({"method_id": "method:OrderService#validate"})
                )
                .unwrap_err()
                .contains("not authorized")
        );
        let relationships = tools
            .execute_tool(
                "get_method_relationships",
                &serde_json::json!({"method_id": "method:OrderService#approve"}),
            )
            .unwrap();
        assert_eq!(
            relationships["calls"][0]["method_id"],
            "method:OrderService#validate"
        );
        assert!(
            tools
                .execute_tool(
                    "read_method_source",
                    &serde_json::json!({"method_id": "method:OrderService#validate"})
                )
                .unwrap()["source"]
                .as_str()
                .unwrap()
                .contains("PENDING")
        );
        tools
            .execute_tool(
                "get_method_analysis",
                &serde_json::json!({"method_id": "method:OrderService#approve"}),
            )
            .unwrap();
        assert!(
            tools
                .execute_tool(
                    "get_method_analysis",
                    &serde_json::json!({"method_id": "method:OrderService#approve"}),
                )
                .unwrap_err()
                .contains("tool call limit exceeded")
        );

        let _ = fs::remove_dir_all(root);
    }

    fn write_extraction_db(path: &Path, file: &str) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE classes (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    module_id TEXT NOT NULL,
                    qualified_name TEXT NOT NULL
                );
                CREATE TABLE methods (
                    id TEXT PRIMARY KEY,
                    module_id TEXT NOT NULL,
                    class_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    signature TEXT NOT NULL,
                    annotations_json TEXT NOT NULL,
                    file TEXT NOT NULL,
                    start_line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL
                );
                CREATE TABLE candidate_scores (
                    method_id TEXT PRIMARY KEY,
                    score INTEGER NOT NULL,
                    priority TEXT NOT NULL
                );
                CREATE TABLE relationships (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source_id TEXT NOT NULL,
                    target_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    source TEXT NOT NULL
                );
                CREATE TABLE entry_points (
                    id TEXT PRIMARY KEY,
                    method_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    framework TEXT,
                    route TEXT,
                    http_method TEXT
                );
                ",
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO classes (id, name, module_id, qualified_name)
                 VALUES ('class:OrderService', 'OrderService', 'module:.', 'OrderService')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO methods (
                    id, module_id, class_id, name, signature, annotations_json, file, start_line, end_line
                 ) VALUES (
                    'method:OrderService#approve', 'module:.', 'class:OrderService', 'approve',
                    'approve()', '[]', ?1, 2, 4
                 )",
                [file],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO methods (
                    id, module_id, class_id, name, signature, annotations_json, file, start_line, end_line
                 ) VALUES (
                    'method:OrderService#validate', 'module:.', 'class:OrderService', 'validate',
                    'validate()', '[]', ?1, 5, 7
                 )",
                [file],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO candidate_scores (method_id, score, priority)
                 VALUES ('method:OrderService#approve', 10, 'high')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO relationships (source_id, target_id, kind, confidence, source)
                 VALUES ('method:OrderService#approve', 'method:OrderService#validate', 'CALLS', 0.9, 'tree-sitter')",
                [],
            )
            .unwrap();
    }

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("code-parser-{name}-{}", timestamp()));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
