use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::core::error::{DatabaseError, FileError, KgError, LlmError, PathError};
use crate::proto::gluon::db::v1::{
    BusinessEdgeRow, BusinessEvidenceRow, BusinessKgTable, BusinessNodeRow, ExtractionTable,
    LlmExtractionRunRow, LlmExtractionRunStatus,
};
use crate::proto::{
    business_kg_schema_ddl, business_kg_table, extraction_table, llm_extraction_run_status,
};
use crate::proto_field;

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
    pub resume: bool,
    pub max_failures: Option<usize>,
}

pub type BuildResult<T> = Result<T, BuildError>;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error(transparent)]
    Path(#[from] PathError),

    #[error(transparent)]
    File(#[from] FileError),

    #[error(transparent)]
    Database(#[from] DatabaseError),

    #[error("--force and --continue cannot be used together")]
    ConflictingResumeOptions,

    #[error("--max-failures must be greater than 0")]
    InvalidMaxFailures,

    #[error("LLM request failed: {0}")]
    Llm(#[from] LlmError),

    #[error(transparent)]
    Kg(#[from] KgError),
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
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub total_tokens: u64,
    pub nodes: usize,
    pub edges: usize,
    pub evidence: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CandidateMethod {
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
    ) -> Result<LlmMethodResult, String>;
}

pub trait ToolExecutor {
    fn execute_tool(&mut self, name: &str, input: &Value) -> Result<Value, String>;
    fn call_count(&self) -> usize;
}

pub(crate) trait BusinessKgInput {
    fn validate_extraction_db(&self, connection: &Connection) -> Result<(), String>;
    fn select_methods(
        &self,
        connection: &Connection,
        source_path: &Path,
        options: &BuildBusinessKgOptions,
    ) -> Result<Vec<CandidateMethod>, String>;
    fn count_candidates(&self, connection: &Connection) -> Result<usize, String>;
    fn count_priority(&self, connection: &Connection, priority: &str) -> Result<usize, String>;
}

struct CommonSqliteBusinessKgInput;

impl BusinessKgInput for CommonSqliteBusinessKgInput {
    fn validate_extraction_db(&self, connection: &Connection) -> Result<(), String> {
        validate_extraction_db(connection)
    }

    fn select_methods(
        &self,
        connection: &Connection,
        source_path: &Path,
        options: &BuildBusinessKgOptions,
    ) -> Result<Vec<CandidateMethod>, String> {
        select_methods(connection, source_path, options)
    }

    fn count_candidates(&self, connection: &Connection) -> Result<usize, String> {
        count_candidates(connection)
    }

    fn count_priority(&self, connection: &Connection, priority: &str) -> Result<usize, String> {
        count_priority(connection, priority)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct LlmKgResponse {
    #[serde(default)]
    pub nodes: Vec<NodeProposal>,
    #[serde(default)]
    pub edges: Vec<EdgeProposal>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl TokenUsage {
    fn add(&mut self, other: &TokenUsage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_creation_input_tokens += other.cache_creation_input_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
    }

    fn total_tokens(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_creation_input_tokens
            + self.cache_read_input_tokens
    }

    fn update_from_anthropic_usage(&mut self, usage: &Value) {
        if let Some(value) = usage.get("input_tokens").and_then(Value::as_u64) {
            self.input_tokens = value;
        }
        if let Some(value) = usage.get("output_tokens").and_then(Value::as_u64) {
            self.output_tokens = value;
        }
        if let Some(value) = usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_u64)
        {
            self.cache_creation_input_tokens = value;
        }
        if let Some(value) = usage.get("cache_read_input_tokens").and_then(Value::as_u64) {
            self.cache_read_input_tokens = value;
        }
        if let Some(cache_creation) = usage.get("cache_creation") {
            self.cache_creation_input_tokens = cache_creation
                .get("ephemeral_5m_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                + cache_creation
                    .get("ephemeral_1h_input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
        }
    }
}

#[derive(Debug, Clone)]
pub struct LlmMethodResult {
    response: LlmKgResponse,
    usage: TokenUsage,
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
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub evidence: Vec<EvidenceProposal>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub source_lines: Vec<usize>,
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
    pub fn from_env() -> BuildResult<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| LlmError::MissingApiKey)?;
        let api_base = std::env::var("ANTHROPIC_API_BASE")
            .unwrap_or_else(|_| "https://api.anthropic.com".to_string());
        let model = std::env::var("ANTHROPIC_MODEL")
            .unwrap_or_else(|_| DEFAULT_ANTHROPIC_MODEL.to_string());
        let http = Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .map_err(|error| {
                LlmError::Operation(format!("failed to create Anthropic HTTP client: {error}"))
            })?;
        Ok(Self {
            api_key,
            api_base,
            model,
            http,
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
    ) -> Result<LlmMethodResult, String> {
        let prompt = build_prompt(request)?;
        let url = format!("{}/v1/messages", self.api_base.trim_end_matches('/'));
        let mut messages = vec![serde_json::json!({
            "role": "user",
            "content": prompt
        })];
        let mut usage = TokenUsage::default();

        loop {
            let body = serde_json::json!({
                "model": self.model,
                "max_tokens": 8192,
                "stream": true,
                "system": cached_system_prompt(),
                "tools": cached_tool_schemas(),
                "messages": messages
            });
            let message = self.post_message(&url, &body)?;
            usage.add(&message.usage);
            let value = message.value;
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
                return match parse_llm_json(&text) {
                    Ok(response) => Ok(LlmMethodResult { response, usage }),
                    Err(error) => self.repair_json_response(&url, &messages, &text, &error, usage),
                };
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
    fn post_message(&self, url: &str, body: &Value) -> Result<AnthropicMessage, String> {
        let response = self
            .http
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .json(body)
            .send()
            .map_err(|error| format!("Anthropic request failed: {error:?}"))?;
        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .unwrap_or_else(|error| format!("failed to read error body: {error}"));
            return Err(format!(
                "Anthropic request failed with status {status}: {text}"
            ));
        }
        parse_sse_response(response)
    }

    fn repair_json_response(
        &self,
        url: &str,
        original_messages: &[Value],
        bad_text: &str,
        parse_error: &str,
        mut usage: TokenUsage,
    ) -> Result<LlmMethodResult, String> {
        let mut messages = original_messages.to_vec();
        messages.push(serde_json::json!({
            "role": "assistant",
            "content": bad_text
        }));
        messages.push(serde_json::json!({
            "role": "user",
            "content": format!(
                "Your previous response was not valid JSON: {parse_error}\nReturn only a corrected JSON object with top-level keys nodes and edges. Do not explain."
            )
        }));
        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 8192,
            "stream": true,
            "system": cached_system_prompt(),
            "messages": messages
        });
        let message = self.post_message(url, &body)?;
        usage.add(&message.usage);
        let value = message.value;
        let content = value
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                format!("Anthropic repair response contained no content array: {value}")
            })?;
        let text = anthropic_text_from_content(content).ok_or_else(|| {
            format!("Anthropic repair response contained no text content: {value}")
        })?;
        parse_llm_json(&text)
            .map(|response| LlmMethodResult { response, usage })
            .map_err(|repair_error| format!("{parse_error}; repair failed: {repair_error}"))
    }
}

#[derive(Debug, Clone)]
struct AnthropicMessage {
    value: Value,
    usage: TokenUsage,
}

fn parse_sse_response(response: impl Read) -> Result<AnthropicMessage, String> {
    let mut blocks: HashMap<usize, Value> = HashMap::new();
    let mut input_buffers: HashMap<usize, String> = HashMap::new();
    let mut usage = TokenUsage::default();
    let reader = std::io::BufReader::new(response);
    for line in reader.lines() {
        let line = line.map_err(|error| format!("failed to read Anthropic stream: {error:?}"))?;
        let Some(data) = line.strip_prefix("data: ") else {
            continue;
        };
        if data == "[DONE]" {
            break;
        }
        let event: Value = serde_json::from_str(data)
            .map_err(|error| format!("invalid Anthropic stream event: {error}: {data}"))?;
        if let Some(message_usage) = event
            .get("message")
            .and_then(|message| message.get("usage"))
        {
            usage.update_from_anthropic_usage(message_usage);
        }
        if let Some(event_usage) = event.get("usage") {
            usage.update_from_anthropic_usage(event_usage);
        }
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                let index = event
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| format!("stream content block missing index: {event}"))?
                    as usize;
                let mut block = event
                    .get("content_block")
                    .cloned()
                    .ok_or_else(|| format!("stream content block missing body: {event}"))?;
                if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                    input_buffers.insert(index, String::new());
                    if let Some(object) = block.as_object_mut() {
                        object.insert("input".to_string(), Value::Object(Default::default()));
                    }
                }
                blocks.insert(index, block);
            }
            Some("content_block_delta") => {
                let index = event
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| format!("stream delta missing index: {event}"))?
                    as usize;
                let delta = event
                    .get("delta")
                    .ok_or_else(|| format!("stream delta missing body: {event}"))?;
                match delta.get("type").and_then(Value::as_str) {
                    Some("text_delta") => {
                        let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        append_text_delta(&mut blocks, index, text)?;
                    }
                    Some("input_json_delta") => {
                        let partial = delta
                            .get("partial_json")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        input_buffers.entry(index).or_default().push_str(partial);
                    }
                    _ => {}
                }
            }
            Some("content_block_stop") => {
                let index = event
                    .get("index")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| format!("stream stop missing index: {event}"))?
                    as usize;
                if let Some(input) = input_buffers.remove(&index) {
                    let parsed = if input.trim().is_empty() {
                        Value::Object(Default::default())
                    } else {
                        serde_json::from_str(&input)
                            .map_err(|error| format!("invalid tool input JSON: {error}: {input}"))?
                    };
                    if let Some(block) = blocks.get_mut(&index)
                        && let Some(object) = block.as_object_mut()
                    {
                        object.insert("input".to_string(), parsed);
                    }
                }
            }
            _ => {}
        }
    }
    let mut ordered = blocks.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(index, _)| *index);
    Ok(AnthropicMessage {
        value: serde_json::json!({
            "content": ordered.into_iter().map(|(_, block)| block).collect::<Vec<_>>()
        }),
        usage,
    })
}

fn append_text_delta(
    blocks: &mut HashMap<usize, Value>,
    index: usize,
    text: &str,
) -> Result<(), String> {
    let block = blocks
        .get_mut(&index)
        .ok_or_else(|| format!("text delta for unknown content block {index}"))?;
    let object = block
        .as_object_mut()
        .ok_or_else(|| format!("content block {index} is not an object"))?;
    let current = object.get("text").and_then(Value::as_str).unwrap_or("");
    object.insert(
        "text".to_string(),
        Value::String(format!("{current}{text}")),
    );
    Ok(())
}

#[derive(Debug)]
struct ToolUse {
    id: String,
    name: String,
    input: Value,
}

pub fn build_business_kg(options: &BuildBusinessKgOptions) -> BuildResult<BuildBusinessKgSummary> {
    validate_build_options(options)?;
    let client: AnthropicLlmClient = AnthropicLlmClient::from_env()?;
    build_business_kg_with_client(options, &client)
}

pub fn build_business_kg_with_client(
    options: &BuildBusinessKgOptions,
    client: &dyn LlmClient,
) -> BuildResult<BuildBusinessKgSummary> {
    build_business_kg_with_input(options, client, &CommonSqliteBusinessKgInput)
}

fn build_business_kg_with_input(
    options: &BuildBusinessKgOptions,
    client: &dyn LlmClient,
    input: &dyn BusinessKgInput,
) -> BuildResult<BuildBusinessKgSummary> {
    validate_build_options(options)?;
    let output = options.output.clone().unwrap_or_else(|| {
        options
            .database
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("business-kg.db")
    });
    if options.force && output.exists() {
        fs::remove_file(&output).map_err(|source| FileError::Remove {
            path: output.clone(),
            source,
        })?;
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|source| FileError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let extraction = Connection::open(&options.database).map_err(|source| DatabaseError::Open {
        label: "extraction database",
        path: options.database.clone(),
        source,
    })?;
    input
        .validate_extraction_db(&extraction)
        .map_err(|detail| DatabaseError::InvalidSchema {
            label: "extraction DB",
            detail,
        })?;

    let mut selected = input
        .select_methods(&extraction, &options.source_path, options)
        .map_err(DatabaseError::Operation)?;
    if let Some(max_methods) = options.max_methods {
        selected.truncate(max_methods);
    }

    let mut store = KgStore::open(&output).map_err(KgError::Operation)?;
    if options.resume {
        let before = selected.len();
        selected.retain(|method| {
            store
                .method_has_evidence(&method.id)
                .map(|has_evidence| !has_evidence)
                .unwrap_or(true)
        });
        eprintln!(
            "build-business-kg: resume skipped {} methods with existing KG evidence",
            before.saturating_sub(selected.len())
        );
    }
    let mut summary = BuildBusinessKgSummary {
        database_path: options.database.display().to_string(),
        output_path: output.display().to_string(),
        candidates: input
            .count_candidates(&extraction)
            .map_err(DatabaseError::Operation)?,
        high_priority_candidates: input
            .count_priority(&extraction, "high")
            .map_err(DatabaseError::Operation)?,
        selected: selected.len(),
        ..BuildBusinessKgSummary::default()
    };
    let run_id = store
        .start_run(client.model(), summary.selected)
        .map_err(KgError::Operation)?;

    eprintln!(
        "build-business-kg: selected {} methods from {} candidates; output={}",
        summary.selected, summary.candidates, summary.output_path
    );
    let run_started_at = Instant::now();
    let selected_count = summary.selected;
    let mut last_error = None;
    for (index, method) in selected.into_iter().enumerate() {
        let method_started_at = Instant::now();
        eprintln!(
            "build-business-kg: method {}/{} start id={} class={} name={} lines={}-{} priority={} score={}",
            index + 1,
            selected_count,
            method.id,
            method.class_name,
            method.name,
            method.start_line,
            method.end_line,
            method.priority,
            method.score
        );
        let request = MethodRequest {
            method: method.clone(),
            existing_nodes: store
                .find_nodes_for_prompt(20)
                .map_err(KgError::Operation)?,
        };
        let (analysis_result, tool_calls) = {
            let mut tools =
                MethodToolExecutor::new(&extraction, &options.source_path, &store, &method);
            let result = client.analyze_method(&request, &mut tools);
            (result, tools.call_count())
        };
        match analysis_result {
            Ok(result) => match store.commit_method_response(run_id, &method, result.response) {
                Ok(()) => {
                    summary.methods_processed += 1;
                    summary.input_tokens += result.usage.input_tokens;
                    summary.output_tokens += result.usage.output_tokens;
                    summary.cache_creation_input_tokens += result.usage.cache_creation_input_tokens;
                    summary.cache_read_input_tokens += result.usage.cache_read_input_tokens;
                    summary.total_tokens += result.usage.total_tokens();
                    eprintln!(
                        "build-business-kg: method {}/{} ok elapsed_ms={} tool_calls={} input_tokens={} output_tokens={} total_tokens={} complete={} failed={}",
                        index + 1,
                        selected_count,
                        method_started_at.elapsed().as_millis(),
                        tool_calls,
                        result.usage.input_tokens,
                        result.usage.output_tokens,
                        result.usage.total_tokens(),
                        summary.methods_processed,
                        summary.failed
                    );
                }
                Err(error) => {
                    summary.failed += 1;
                    summary.input_tokens += result.usage.input_tokens;
                    summary.output_tokens += result.usage.output_tokens;
                    summary.cache_creation_input_tokens += result.usage.cache_creation_input_tokens;
                    summary.cache_read_input_tokens += result.usage.cache_read_input_tokens;
                    summary.total_tokens += result.usage.total_tokens();
                    store
                        .record_method_failure(run_id, &method.id, &error)
                        .map_err(KgError::Operation)?;
                    let reason = short_error(&error);
                    last_error = Some(format!("{}: {}", method.id, reason));
                    eprintln!(
                        "build-business-kg: method {}/{} failed elapsed_ms={} tool_calls={} input_tokens={} output_tokens={} total_tokens={}",
                        index + 1,
                        selected_count,
                        method_started_at.elapsed().as_millis(),
                        tool_calls,
                        result.usage.input_tokens,
                        result.usage.output_tokens,
                        result.usage.total_tokens()
                    );
                    eprintln!(
                        "build-business-kg: failure_reason method_id={} reason={}",
                        method.id, reason
                    );
                }
            },
            Err(error) => {
                summary.failed += 1;
                store
                    .record_method_failure(run_id, &method.id, &error)
                    .map_err(KgError::Operation)?;
                let reason = short_error(&error);
                last_error = Some(format!("{}: {}", method.id, reason));
                eprintln!(
                    "build-business-kg: method {}/{} failed elapsed_ms={} tool_calls={}",
                    index + 1,
                    selected_count,
                    method_started_at.elapsed().as_millis(),
                    tool_calls
                );
                eprintln!(
                    "build-business-kg: failure_reason method_id={} reason={}",
                    method.id, reason
                );
            }
        }
        summary.tool_calls += tool_calls;
        store
            .update_run_progress(run_id, &summary)
            .map_err(KgError::Operation)?;
        if let Some(error) = &last_error {
            eprintln!(
                "build-business-kg: progress {}/{} complete failed={} total_tool_calls={} total_tokens={} elapsed_ms={} last_error={}",
                summary.methods_processed + summary.failed,
                selected_count,
                summary.failed,
                summary.tool_calls,
                summary.total_tokens,
                run_started_at.elapsed().as_millis(),
                error
            );
        } else {
            eprintln!(
                "build-business-kg: progress {}/{} complete failed={} total_tool_calls={} total_tokens={} elapsed_ms={}",
                summary.methods_processed + summary.failed,
                selected_count,
                summary.failed,
                summary.tool_calls,
                summary.total_tokens,
                run_started_at.elapsed().as_millis()
            );
        }
        if let Some(max_failures) = options.max_failures
            && summary.failed >= max_failures
        {
            eprintln!(
                "build-business-kg: stopping because failed={} reached max_failures={}",
                summary.failed, max_failures
            );
            break;
        }
    }

    let counts = store.counts().map_err(KgError::Operation)?;
    summary.nodes = counts.nodes;
    summary.edges = counts.edges;
    summary.evidence = counts.evidence;
    store
        .finish_run(run_id, &summary)
        .map_err(KgError::Operation)?;
    eprintln!(
        "build-business-kg: done status={} methods_processed={} failed={} input_tokens={} output_tokens={} total_tokens={} nodes={} edges={} evidence={} elapsed_ms={}",
        if summary.failed == 0 {
            llm_extraction_run_status(LlmExtractionRunStatus::Completed)
        } else {
            llm_extraction_run_status(LlmExtractionRunStatus::PartialFailure)
        },
        summary.methods_processed,
        summary.failed,
        summary.input_tokens,
        summary.output_tokens,
        summary.total_tokens,
        summary.nodes,
        summary.edges,
        summary.evidence,
        run_started_at.elapsed().as_millis()
    );
    Ok(summary)
}

fn short_error(error: &str) -> String {
    const MAX_LEN: usize = 240;
    let first_line = error.lines().next().unwrap_or(error).trim();
    if first_line.len() <= MAX_LEN {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..MAX_LEN])
    }
}

fn validate_build_options(options: &BuildBusinessKgOptions) -> BuildResult<()> {
    if !options.database.exists() {
        return Err(PathError::NotFound(options.database.clone()).into());
    }
    if !options.source_path.exists() {
        return Err(PathError::InvalidSourcePath(options.source_path.clone()).into());
    }
    if options.force && options.resume {
        return Err(BuildError::ConflictingResumeOptions);
    }
    if options.max_failures == Some(0) {
        return Err(BuildError::InvalidMaxFailures);
    }
    Ok(())
}

fn validate_extraction_db(connection: &Connection) -> Result<(), String> {
    for table in [
        extraction_table(ExtractionTable::Methods),
        extraction_table(ExtractionTable::Classes),
        extraction_table(ExtractionTable::CandidateScores),
    ] {
        let exists: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("failed to inspect extraction database: {error}"))?;
        if exists.is_none() {
            return Err(format!("missing table {table}"));
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

fn truncate_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("\n...[truncated]");
    truncated
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
Test tools expose integration/E2E/acceptance evidence when test extraction has been run. Use tests only as supporting validation; do not create business facts from tests alone without production source evidence from the current or related methods.

Return only JSON with this shape:
{{
  "nodes": [
    {{
      "client_id": "n1",
      "kind": "BusinessRule",
      "name": "...",
      "statement": "...",
      "confidence": 0.95,
      "evidence": [{{"method_id":"{method_id}","source_lines":[{start_line}],"reason":"..."}}]
    }}
  ],
  "edges": [
    {{
      "source_client_id": "n1",
      "target_client_id": "n2",
      "kind": "DEPENDS_ON",
      "confidence": 0.85,
      "evidence": [{{"method_id":"{method_id}","source_lines":[{start_line}],"reason":"source and target are related because..."}}]
    }},
    {{
      "source_client_id": "n1",
      "target_node_id": "business-node:existing-id-from-existing_nodes",
      "kind": "MENTIONS",
      "confidence": 0.75,
      "evidence": [{{"method_id":"{method_id}","source_lines":[{start_line}],"reason":"new fact overlaps existing node..."}}]
    }}
  ]
}}

Create at most 5 nodes and 8 edges for one method. Prefer highest-confidence business facts only.
Use supported node kinds only: BusinessRule, Workflow, Invariant, StateTransition, SideEffect, BusinessConcept.
Use supported edge kinds only: SUPPORTED_BY, DEPENDS_ON, TRIGGERS, TRANSITIONS_TO, MENTIONS.
Edges must use source_client_id/source_node_id and target_client_id/target_node_id. Do not use source or target fields.
Every edge must include confidence and evidence. Evidence source_lines must be sorted, unique, and inside the current method lines.
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
    "You are a business logic analyst. Extract only business meaning supported by source evidence. Never invent requirements. Ignore technical plumbing, logging, configuration, and CRUD-only code unless it encodes a business rule. Every node and edge must have evidence and confidence. Return only valid JSON that exactly follows the requested schema."
}

fn prompt_cache_control() -> Value {
    serde_json::json!({ "type": "ephemeral" })
}

fn cached_system_prompt() -> Value {
    serde_json::json!([
        {
            "type": "text",
            "text": system_prompt(),
            "cache_control": prompt_cache_control()
        }
    ])
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
        tool_schema(
            "get_tests_for_method",
            "Return bounded integration/E2E/acceptance tests linked to a production method.",
            &[("method_id", "string")],
        ),
        tool_schema(
            "get_test_case",
            "Return compact metadata and bounded body text for one test case.",
            &[("test_case_id", "string")],
        ),
        tool_schema(
            "get_test_assertions",
            "Return bounded assertions for one test case.",
            &[("test_case_id", "string")],
        ),
        tool_schema(
            "get_test_entry_points",
            "Return bounded HTTP, messaging, or command entry points exercised by one test case.",
            &[("test_case_id", "string")],
        ),
        tool_schema(
            "get_test_fixtures",
            "Return bounded suite and case fixtures for one test case.",
            &[("test_case_id", "string")],
        ),
    ]
}

fn cached_tool_schemas() -> Vec<Value> {
    let mut tools = tool_schemas();
    if let Some(last) = tools.last_mut()
        && let Some(object) = last.as_object_mut()
    {
        object.insert("cache_control".to_string(), prompt_cache_control());
    }
    tools
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
    let candidates = json_parse_candidates(trimmed);
    let mut last_error = None;
    for json_text in candidates {
        match serde_json::from_str(&json_text) {
            Ok(response) => return Ok(response),
            Err(error) => last_error = Some(format!("{error}: {json_text}")),
        }
    }
    Err(format!(
        "failed to parse LLM JSON response: {}",
        last_error.unwrap_or_else(|| trimmed.to_string())
    ))
}

fn extract_fenced_json(text: &str) -> Option<&str> {
    let start = text.find('\n')? + 1;
    let end = text.rfind("```")?;
    text.get(start..end).map(str::trim)
}

fn json_parse_candidates(text: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(fenced) = extract_fenced_json(text) {
        candidates.push(fenced.to_string());
    }
    candidates.push(text.to_string());
    if let Some(object) = extract_outer_json_object(text) {
        candidates.push(object.to_string());
    }
    if let Some(repaired) = repair_truncated_json_object(text) {
        candidates.push(repaired);
    }
    candidates
}

fn extract_outer_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end <= start {
        return None;
    }
    text.get(start..=end).map(str::trim)
}

fn repair_truncated_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut object = text[start..].trim().to_string();
    if object.is_empty() {
        return None;
    }
    let mut stack = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in object.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                if stack.pop() != Some(ch) {
                    return None;
                }
            }
            _ => {}
        }
    }
    if in_string {
        object.push('"');
    }
    while let Some(ch) = stack.pop() {
        object.push(ch);
    }
    Some(object)
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

    fn get_tests_for_method(&self, method_id: &str, limit: usize) -> Result<Value, String> {
        if !table_exists(self.extraction, "test_targets")?
            || !table_exists(self.extraction, "test_cases")?
        {
            return Ok(serde_json::json!({ "tests": [] }));
        }
        let mut statement = self
            .extraction
            .prepare(
                "SELECT tc.id, tc.name, tc.display_name, tc.test_kind, tc.file,
                        tc.start_line, tc.end_line, tt.relationship, tt.confidence, tt.source
                 FROM test_targets tt
                 JOIN test_cases tc ON tc.id = tt.test_case_id
                 WHERE tt.target_kind = 'method' AND tt.target_id = ?1
                 ORDER BY tt.confidence DESC, tc.file, tc.start_line, tc.id
                 LIMIT ?2",
            )
            .map_err(|error| format!("failed to prepare test target query: {error}"))?;
        let rows = statement
            .query_map(params![method_id, clamp_limit(limit) as i64], |row| {
                Ok(serde_json::json!({
                    "test_case_id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "display_name": row.get::<_, Option<String>>(2)?,
                    "test_kind": row.get::<_, String>(3)?,
                    "file": row.get::<_, String>(4)?,
                    "start_line": row.get::<_, i64>(5)?,
                    "end_line": row.get::<_, i64>(6)?,
                    "relationship": row.get::<_, String>(7)?,
                    "confidence": row.get::<_, f64>(8)?,
                    "source": row.get::<_, String>(9)?,
                }))
            })
            .map_err(|error| format!("failed to query tests for method {method_id}: {error}"))?;
        Ok(serde_json::json!({ "tests": collect_json_rows(rows)? }))
    }

    fn get_test_case(&self, test_case_id: &str) -> Result<Value, String> {
        if !table_exists(self.extraction, "test_cases")? {
            return Ok(serde_json::json!({ "test_case": null }));
        }
        let test_case = self
            .extraction
            .query_row(
                "SELECT tc.id, tc.suite_id, tc.name, tc.display_name, tc.test_kind,
                        tc.file, tc.start_line, tc.end_line, tc.annotations_json,
                        tc.body_text, ts.qualified_name
                 FROM test_cases tc
                 LEFT JOIN test_suites ts ON ts.id = tc.suite_id
                 WHERE tc.id = ?1",
                [test_case_id],
                |row| {
                    let body_text: String = row.get(9)?;
                    Ok(serde_json::json!({
                        "id": row.get::<_, String>(0)?,
                        "suite_id": row.get::<_, String>(1)?,
                        "name": row.get::<_, String>(2)?,
                        "display_name": row.get::<_, Option<String>>(3)?,
                        "test_kind": row.get::<_, String>(4)?,
                        "file": row.get::<_, String>(5)?,
                        "start_line": row.get::<_, i64>(6)?,
                        "end_line": row.get::<_, i64>(7)?,
                        "annotations_json": row.get::<_, String>(8)?,
                        "body_text": truncate_text(&body_text, 4000),
                        "suite_qualified_name": row.get::<_, Option<String>>(10)?,
                    }))
                },
            )
            .optional()
            .map_err(|error| format!("failed to query test case {test_case_id}: {error}"))?;
        Ok(serde_json::json!({ "test_case": test_case }))
    }

    fn get_test_assertions(&self, test_case_id: &str, limit: usize) -> Result<Value, String> {
        if !table_exists(self.extraction, "test_assertions")? {
            return Ok(serde_json::json!({ "assertions": [] }));
        }
        let mut statement = self
            .extraction
            .prepare(
                "SELECT assertion_kind, expression, expected_value, file, line
                 FROM test_assertions
                 WHERE test_case_id = ?1
                 ORDER BY line, id
                 LIMIT ?2",
            )
            .map_err(|error| format!("failed to prepare test assertion query: {error}"))?;
        let rows = statement
            .query_map(params![test_case_id, clamp_limit(limit) as i64], |row| {
                let expression: String = row.get(1)?;
                Ok(serde_json::json!({
                    "assertion_kind": row.get::<_, String>(0)?,
                    "expression": truncate_text(&expression, 1000),
                    "expected_value": row.get::<_, Option<String>>(2)?,
                    "file": row.get::<_, String>(3)?,
                    "line": row.get::<_, i64>(4)?,
                }))
            })
            .map_err(|error| format!("failed to query test assertions {test_case_id}: {error}"))?;
        Ok(serde_json::json!({ "assertions": collect_json_rows(rows)? }))
    }

    fn get_test_entry_points(&self, test_case_id: &str, limit: usize) -> Result<Value, String> {
        if !table_exists(self.extraction, "test_entry_points")? {
            return Ok(serde_json::json!({ "entry_points": [] }));
        }
        let mut statement = self
            .extraction
            .prepare(
                "SELECT kind, framework, route, http_method, topic, command, source
                 FROM test_entry_points
                 WHERE test_case_id = ?1
                 ORDER BY id
                 LIMIT ?2",
            )
            .map_err(|error| format!("failed to prepare test entry point query: {error}"))?;
        let rows = statement
            .query_map(params![test_case_id, clamp_limit(limit) as i64], |row| {
                Ok(serde_json::json!({
                    "kind": row.get::<_, String>(0)?,
                    "framework": row.get::<_, Option<String>>(1)?,
                    "route": row.get::<_, Option<String>>(2)?,
                    "http_method": row.get::<_, Option<String>>(3)?,
                    "topic": row.get::<_, Option<String>>(4)?,
                    "command": row.get::<_, Option<String>>(5)?,
                    "source": row.get::<_, String>(6)?,
                }))
            })
            .map_err(|error| {
                format!("failed to query test entry points {test_case_id}: {error}")
            })?;
        Ok(serde_json::json!({ "entry_points": collect_json_rows(rows)? }))
    }

    fn get_test_fixtures(&self, test_case_id: &str, limit: usize) -> Result<Value, String> {
        if !table_exists(self.extraction, "test_cases")?
            || !table_exists(self.extraction, "test_fixtures")?
        {
            return Ok(serde_json::json!({ "fixtures": [] }));
        }
        let suite_id = self
            .extraction
            .query_row(
                "SELECT suite_id FROM test_cases WHERE id = ?1",
                [test_case_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("failed to query test case suite {test_case_id}: {error}"))?;
        let Some(suite_id) = suite_id else {
            return Ok(serde_json::json!({ "fixtures": [] }));
        };
        let mut statement = self
            .extraction
            .prepare(
                "SELECT fixture_kind, name, details_json, file, line,
                        CASE WHEN test_case_id IS NULL THEN 'suite' ELSE 'case' END
                 FROM test_fixtures
                 WHERE test_case_id = ?1 OR suite_id = ?2
                 ORDER BY line, id
                 LIMIT ?3",
            )
            .map_err(|error| format!("failed to prepare test fixture query: {error}"))?;
        let rows = statement
            .query_map(
                params![test_case_id, suite_id, clamp_limit(limit) as i64],
                |row| {
                    let details_json: String = row.get(2)?;
                    Ok(serde_json::json!({
                        "fixture_kind": row.get::<_, String>(0)?,
                        "name": row.get::<_, String>(1)?,
                        "details_json": truncate_text(&details_json, 1000),
                        "file": row.get::<_, String>(3)?,
                        "line": row.get::<_, i64>(4)?,
                        "scope": row.get::<_, String>(5)?,
                    }))
                },
            )
            .map_err(|error| format!("failed to query test fixtures {test_case_id}: {error}"))?;
        Ok(serde_json::json!({ "fixtures": collect_json_rows(rows)? }))
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
            "get_tests_for_method" => {
                self.get_tests_for_method(arg_str(input, "method_id")?, arg_limit(input))
            }
            "get_test_case" => self.get_test_case(arg_str(input, "test_case_id")?),
            "get_test_assertions" => {
                self.get_test_assertions(arg_str(input, "test_case_id")?, arg_limit(input))
            }
            "get_test_entry_points" => {
                self.get_test_entry_points(arg_str(input, "test_case_id")?, arg_limit(input))
            }
            "get_test_fixtures" => {
                self.get_test_fixtures(arg_str(input, "test_case_id")?, arg_limit(input))
            }
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

    fn start_run(&mut self, model: &str, methods_total: usize) -> Result<i64, String> {
        self.connection
            .execute(
                &format!(
                    "INSERT INTO {} ({}, {}, {}, {})
                 VALUES (?1, ?2, ?3, ?4)",
                    business_kg_table(BusinessKgTable::LlmExtractionRuns),
                    proto_field!(LlmExtractionRunRow, model),
                    proto_field!(LlmExtractionRunRow, status),
                    proto_field!(LlmExtractionRunRow, started_at),
                    proto_field!(LlmExtractionRunRow, methods_total),
                ),
                params![
                    model,
                    llm_extraction_run_status(LlmExtractionRunStatus::Running),
                    timestamp(),
                    methods_total as i64,
                ],
            )
            .map_err(|error| format!("failed to create KG run: {error}"))?;
        Ok(self.connection.last_insert_rowid())
    }

    fn finish_run(&mut self, run_id: i64, summary: &BuildBusinessKgSummary) -> Result<(), String> {
        let status = if summary.failed == 0 {
            llm_extraction_run_status(LlmExtractionRunStatus::Completed)
        } else {
            llm_extraction_run_status(LlmExtractionRunStatus::PartialFailure)
        };
        self.connection
            .execute(
                &format!(
                    "UPDATE {}
                 SET {} = ?1,
                     {} = ?2,
                     {} = ?3,
                     {} = ?4,
                     {} = ?5,
                     {} = ?6,
                     {} = ?7,
                     {} = ?8,
                     {} = ?9,
                     {} = ?10,
                     {} = ?11,
                     {} = ?12
                 WHERE {} = ?13",
                    business_kg_table(BusinessKgTable::LlmExtractionRuns),
                    proto_field!(LlmExtractionRunRow, status),
                    proto_field!(LlmExtractionRunRow, finished_at),
                    proto_field!(LlmExtractionRunRow, methods_processed),
                    proto_field!(LlmExtractionRunRow, failed),
                    proto_field!(LlmExtractionRunRow, nodes_created),
                    proto_field!(LlmExtractionRunRow, edges_created),
                    proto_field!(LlmExtractionRunRow, evidence_created),
                    proto_field!(LlmExtractionRunRow, input_tokens),
                    proto_field!(LlmExtractionRunRow, output_tokens),
                    proto_field!(LlmExtractionRunRow, cache_creation_input_tokens),
                    proto_field!(LlmExtractionRunRow, cache_read_input_tokens),
                    proto_field!(LlmExtractionRunRow, total_tokens),
                    proto_field!(LlmExtractionRunRow, id),
                ),
                params![
                    status,
                    timestamp(),
                    summary.methods_processed as i64,
                    summary.failed as i64,
                    summary.nodes as i64,
                    summary.edges as i64,
                    summary.evidence as i64,
                    summary.input_tokens as i64,
                    summary.output_tokens as i64,
                    summary.cache_creation_input_tokens as i64,
                    summary.cache_read_input_tokens as i64,
                    summary.total_tokens as i64,
                    run_id,
                ],
            )
            .map_err(|error| format!("failed to finish KG run {run_id}: {error}"))?;
        Ok(())
    }

    fn update_run_progress(
        &mut self,
        run_id: i64,
        summary: &BuildBusinessKgSummary,
    ) -> Result<(), String> {
        let counts = self.counts()?;
        self.connection
            .execute(
                &format!(
                    "UPDATE {}
                 SET {} = ?1,
                     {} = ?2,
                     {} = ?3,
                     {} = ?4,
                     {} = ?5,
                     {} = ?6,
                     {} = ?7,
                     {} = ?8,
                     {} = ?9,
                     {} = ?10
                 WHERE {} = ?11",
                    business_kg_table(BusinessKgTable::LlmExtractionRuns),
                    proto_field!(LlmExtractionRunRow, methods_processed),
                    proto_field!(LlmExtractionRunRow, failed),
                    proto_field!(LlmExtractionRunRow, nodes_created),
                    proto_field!(LlmExtractionRunRow, edges_created),
                    proto_field!(LlmExtractionRunRow, evidence_created),
                    proto_field!(LlmExtractionRunRow, input_tokens),
                    proto_field!(LlmExtractionRunRow, output_tokens),
                    proto_field!(LlmExtractionRunRow, cache_creation_input_tokens),
                    proto_field!(LlmExtractionRunRow, cache_read_input_tokens),
                    proto_field!(LlmExtractionRunRow, total_tokens),
                    proto_field!(LlmExtractionRunRow, id),
                ),
                params![
                    summary.methods_processed as i64,
                    summary.failed as i64,
                    counts.nodes as i64,
                    counts.edges as i64,
                    counts.evidence as i64,
                    summary.input_tokens as i64,
                    summary.output_tokens as i64,
                    summary.cache_creation_input_tokens as i64,
                    summary.cache_read_input_tokens as i64,
                    summary.total_tokens as i64,
                    run_id,
                ],
            )
            .map_err(|error| format!("failed to update KG run progress {run_id}: {error}"))?;
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
                &format!(
                    "SELECT {} FROM {} WHERE {} = ?1",
                    proto_field!(LlmExtractionRunRow, error),
                    business_kg_table(BusinessKgTable::LlmExtractionRuns),
                    proto_field!(LlmExtractionRunRow, id),
                ),
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
                &format!(
                    "UPDATE {} SET {} = ?1 WHERE {} = ?2",
                    business_kg_table(BusinessKgTable::LlmExtractionRuns),
                    proto_field!(LlmExtractionRunRow, error),
                    proto_field!(LlmExtractionRunRow, id),
                ),
                params![combined, run_id],
            )
            .map_err(|error| format!("failed to record KG method failure: {error}"))?;
        Ok(())
    }

    fn method_has_evidence(&self, method_id: &str) -> Result<bool, String> {
        self.connection
            .query_row(
                &format!(
                    "SELECT 1 FROM {} WHERE {} = ?1 LIMIT 1",
                    business_kg_table(BusinessKgTable::BusinessEvidence),
                    proto_field!(BusinessEvidenceRow, method_id),
                ),
                [method_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map(|value| value.is_some())
            .map_err(|error| format!("failed to query KG evidence for method {method_id}: {error}"))
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
        let response = normalize_response(method, response);
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
                    &format!(
                        "INSERT INTO {} (
                        {}, {}, {}, {}, {}, {}, {}
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT({}) DO NOTHING",
                        business_kg_table(BusinessKgTable::BusinessNodes),
                        proto_field!(BusinessNodeRow, id),
                        proto_field!(BusinessNodeRow, kind),
                        proto_field!(BusinessNodeRow, name),
                        proto_field!(BusinessNodeRow, statement),
                        proto_field!(BusinessNodeRow, confidence),
                        proto_field!(BusinessNodeRow, created_by_run_id),
                        proto_field!(BusinessNodeRow, created_at),
                        proto_field!(BusinessNodeRow, id),
                    ),
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
        let client_ids = client_nodes.keys().cloned().collect::<HashSet<_>>();
        for edge in response.edges {
            if !valid_edge_for_method(method, &edge, &client_ids) {
                continue;
            }
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
                    &format!(
                        "INSERT INTO {} (
                        {}, {}, {}, {}, {}, {}
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT({}, {}, {}) DO NOTHING",
                        business_kg_table(BusinessKgTable::BusinessEdges),
                        proto_field!(BusinessEdgeRow, source_id),
                        proto_field!(BusinessEdgeRow, target_id),
                        proto_field!(BusinessEdgeRow, kind),
                        proto_field!(BusinessEdgeRow, confidence),
                        proto_field!(BusinessEdgeRow, created_by_run_id),
                        proto_field!(BusinessEdgeRow, created_at),
                        proto_field!(BusinessEdgeRow, source_id),
                        proto_field!(BusinessEdgeRow, target_id),
                        proto_field!(BusinessEdgeRow, kind),
                    ),
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
                    &format!(
                        "SELECT {} FROM {}
                     WHERE {} = ?1 AND {} = ?2 AND {} = ?3",
                        proto_field!(BusinessEdgeRow, id),
                        business_kg_table(BusinessKgTable::BusinessEdges),
                        proto_field!(BusinessEdgeRow, source_id),
                        proto_field!(BusinessEdgeRow, target_id),
                        proto_field!(BusinessEdgeRow, kind),
                    ),
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
            nodes: count_table(
                &self.connection,
                business_kg_table(BusinessKgTable::BusinessNodes),
            )?,
            edges: count_table(
                &self.connection,
                business_kg_table(BusinessKgTable::BusinessEdges),
            )?,
            evidence: count_table(
                &self.connection,
                business_kg_table(BusinessKgTable::BusinessEvidence),
            )?,
        })
    }
}

fn create_kg_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(&business_kg_schema_ddl())
        .map_err(|error| format!("failed to create KG schema: {error}"))?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, methods_total),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, methods_total)
        ),
    )?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, nodes_created),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, nodes_created)
        ),
    )?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, edges_created),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, edges_created)
        ),
    )?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, evidence_created),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, evidence_created)
        ),
    )?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, input_tokens),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, input_tokens)
        ),
    )?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, output_tokens),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, output_tokens)
        ),
    )?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, cache_creation_input_tokens),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, cache_creation_input_tokens)
        ),
    )?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, cache_read_input_tokens),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, cache_read_input_tokens)
        ),
    )?;
    ensure_run_column(
        connection,
        proto_field!(LlmExtractionRunRow, total_tokens),
        &format!(
            "ALTER TABLE {} ADD COLUMN {} INTEGER DEFAULT 0",
            business_kg_table(BusinessKgTable::LlmExtractionRuns),
            proto_field!(LlmExtractionRunRow, total_tokens)
        ),
    )?;
    Ok(())
}

fn ensure_run_column(connection: &Connection, name: &str, ddl: &str) -> Result<(), String> {
    let mut statement = connection
        .prepare(&format!(
            "PRAGMA table_info({})",
            business_kg_table(BusinessKgTable::LlmExtractionRuns)
        ))
        .map_err(|error| format!("failed to inspect KG run schema: {error}"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read KG run schema: {error}"))?;
    for column in columns {
        if column.map_err(|error| format!("failed to read KG run column: {error}"))? == name {
            return Ok(());
        }
    }
    connection
        .execute(ddl, [])
        .map_err(|error| format!("failed to migrate KG run schema: {error}"))?;
    Ok(())
}

fn normalize_response(method: &CandidateMethod, mut response: LlmKgResponse) -> LlmKgResponse {
    let node_evidence_lines = response
        .nodes
        .iter()
        .filter_map(|node| {
            node.client_id
                .as_ref()
                .map(|client_id| (client_id.clone(), evidence_lines_for_node(node)))
        })
        .collect::<HashMap<_, _>>();
    for edge in &mut response.edges {
        normalize_edge_reference(
            &mut edge.source_client_id,
            &mut edge.source_node_id,
            &edge.source,
        );
        normalize_edge_reference(
            &mut edge.target_client_id,
            &mut edge.target_node_id,
            &edge.target,
        );
        if edge.confidence == 0.0 {
            edge.confidence = 0.75;
        }
        if edge.evidence.is_empty()
            && let Some(reason) = edge
                .reason
                .as_ref()
                .filter(|value| !value.trim().is_empty())
            && let Some(source_lines) = edge_source_lines(method, edge, &node_evidence_lines)
        {
            edge.evidence.push(EvidenceProposal {
                method_id: method.id.clone(),
                source_lines,
                reason: reason.clone(),
            });
        }
    }
    response
}

fn normalize_edge_reference(
    client_id: &mut Option<String>,
    node_id: &mut Option<String>,
    fallback: &Option<String>,
) {
    if client_id.is_some() || node_id.is_some() {
        return;
    }
    let Some(value) = fallback
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    if value.starts_with("business-node:") {
        *node_id = Some(value.to_string());
    } else {
        *client_id = Some(value.to_string());
    }
}

fn edge_source_lines(
    method: &CandidateMethod,
    edge: &EdgeProposal,
    node_evidence_lines: &HashMap<String, Vec<usize>>,
) -> Option<Vec<usize>> {
    if !edge.source_lines.is_empty() {
        return Some(normalize_source_lines(method, edge.source_lines.clone()));
    }
    let mut lines = Vec::new();
    if let Some(client_id) = &edge.source_client_id
        && let Some(source_lines) = node_evidence_lines.get(client_id)
    {
        lines.extend(source_lines);
    }
    if let Some(client_id) = &edge.target_client_id
        && let Some(source_lines) = node_evidence_lines.get(client_id)
    {
        lines.extend(source_lines);
    }
    let lines = normalize_source_lines(method, lines);
    if lines.is_empty() { None } else { Some(lines) }
}

fn evidence_lines_for_node(node: &NodeProposal) -> Vec<usize> {
    let mut lines = node
        .evidence
        .iter()
        .flat_map(|evidence| evidence.source_lines.iter().copied())
        .collect::<Vec<_>>();
    lines.sort_unstable();
    lines.dedup();
    lines
}

fn normalize_source_lines(method: &CandidateMethod, mut lines: Vec<usize>) -> Vec<usize> {
    lines.retain(|line| *line >= method.start_line && *line <= method.end_line);
    lines.sort_unstable();
    lines.dedup();
    lines
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
    if edge.confidence == 0.0 {
        return Err("business edge confidence is missing or zero".to_string());
    }
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

fn valid_edge_for_method(
    method: &CandidateMethod,
    edge: &EdgeProposal,
    client_ids: &HashSet<String>,
) -> bool {
    validate_edge(edge, client_ids).is_ok()
        && edge
            .evidence
            .iter()
            .all(|evidence| validate_evidence(method, evidence).is_ok())
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
            &format!(
                "INSERT INTO {} (
                {}, {}, {}, {}, {}, {}, {}
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT DO NOTHING",
                business_kg_table(BusinessKgTable::BusinessEvidence),
                proto_field!(BusinessEvidenceRow, node_id),
                proto_field!(BusinessEvidenceRow, edge_id),
                proto_field!(BusinessEvidenceRow, method_id),
                proto_field!(BusinessEvidenceRow, source_lines_json),
                proto_field!(BusinessEvidenceRow, reason),
                proto_field!(BusinessEvidenceRow, created_by_run_id),
                proto_field!(BusinessEvidenceRow, created_at),
            ),
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
        usage: TokenUsage,
    }

    impl MockClient {
        fn new(response: LlmKgResponse) -> Self {
            Self {
                response,
                usage: TokenUsage::default(),
            }
        }
    }

    impl LlmClient for MockClient {
        fn model(&self) -> &str {
            "mock"
        }

        fn analyze_method(
            &self,
            _request: &MethodRequest,
            _tools: &mut dyn ToolExecutor,
        ) -> Result<LlmMethodResult, String> {
            Ok(LlmMethodResult {
                response: self.response.clone(),
                usage: self.usage.clone(),
            })
        }
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
                CREATE TABLE test_suites (
                    id TEXT PRIMARY KEY,
                    module_id TEXT,
                    class_name TEXT NOT NULL,
                    package_name TEXT,
                    qualified_name TEXT NOT NULL,
                    test_kind TEXT NOT NULL,
                    file TEXT NOT NULL,
                    start_line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL,
                    annotations_json TEXT NOT NULL
                );
                CREATE TABLE test_cases (
                    id TEXT PRIMARY KEY,
                    suite_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    display_name TEXT,
                    test_kind TEXT NOT NULL,
                    file TEXT NOT NULL,
                    start_line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL,
                    annotations_json TEXT NOT NULL,
                    body_text TEXT NOT NULL
                );
                CREATE TABLE test_targets (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    test_case_id TEXT NOT NULL,
                    target_kind TEXT NOT NULL,
                    target_id TEXT NOT NULL,
                    relationship TEXT NOT NULL,
                    confidence REAL NOT NULL,
                    source TEXT NOT NULL
                );
                CREATE TABLE test_assertions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    test_case_id TEXT NOT NULL,
                    assertion_kind TEXT NOT NULL,
                    expression TEXT NOT NULL,
                    expected_value TEXT,
                    file TEXT NOT NULL,
                    line INTEGER NOT NULL
                );
                CREATE TABLE test_fixtures (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    suite_id TEXT,
                    test_case_id TEXT,
                    fixture_kind TEXT NOT NULL,
                    name TEXT NOT NULL,
                    details_json TEXT NOT NULL,
                    file TEXT NOT NULL,
                    line INTEGER NOT NULL
                );
                CREATE TABLE test_entry_points (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    test_case_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    framework TEXT,
                    route TEXT,
                    http_method TEXT,
                    topic TEXT,
                    command TEXT,
                    source TEXT NOT NULL
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
        connection
            .execute(
                "INSERT INTO test_suites (
                    id, module_id, class_name, package_name, qualified_name, test_kind,
                    file, start_line, end_line, annotations_json
                 ) VALUES (
                    'test-suite:OrderServiceIT', 'module:.', 'OrderServiceIT', NULL,
                    'OrderServiceIT', 'integration', 'OrderServiceIT.java', 1, 20, '[]'
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO test_cases (
                    id, suite_id, name, display_name, test_kind, file, start_line,
                    end_line, annotations_json, body_text
                 ) VALUES (
                    'test-case:OrderServiceIT#approvePending', 'test-suite:OrderServiceIT',
                    'approvePending', 'approves pending orders', 'integration',
                    'OrderServiceIT.java', 5, 12, '[]',
                    'void approvePending() { approve(); assertThat(status).isEqualTo(APPROVED); }'
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO test_targets (
                    test_case_id, target_kind, target_id, relationship, confidence, source
                 ) VALUES (
                    'test-case:OrderServiceIT#approvePending', 'method',
                    'method:OrderService#approve', 'exercises', 0.95, 'jdtls_definition'
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO test_assertions (
                    test_case_id, assertion_kind, expression, expected_value, file, line
                 ) VALUES (
                    'test-case:OrderServiceIT#approvePending', 'assertThat',
                    'assertThat(status).isEqualTo(APPROVED)', 'APPROVED',
                    'OrderServiceIT.java', 10
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO test_fixtures (
                    suite_id, test_case_id, fixture_kind, name, details_json, file, line
                 ) VALUES (
                    'test-suite:OrderServiceIT', NULL, 'setup', 'createPendingOrder',
                    '{\"status\":\"PENDING\"}', 'OrderServiceIT.java', 3
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO test_fixtures (
                    suite_id, test_case_id, fixture_kind, name, details_json, file, line
                 ) VALUES (
                    'test-suite:OrderServiceIT', 'test-case:OrderServiceIT#approvePending',
                    'given', 'pendingOrder', '{\"status\":\"PENDING\"}',
                    'OrderServiceIT.java', 6
                 )",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO test_entry_points (
                    test_case_id, kind, framework, route, http_method, topic, command, source
                 ) VALUES (
                    'test-case:OrderServiceIT#approvePending', 'http', 'spring',
                    '/orders/{id}/approve', 'POST', NULL, NULL, 'mockMvc'
                 )",
                [],
            )
            .unwrap();
    }

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("code-parser-{name}-{}", timestamp()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn cached_system_prompt_marks_static_prompt_cacheable() {
        let system = cached_system_prompt();

        assert_eq!(system[0]["type"], "text");
        assert_eq!(system[0]["text"], system_prompt());
        assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn cached_tool_schemas_marks_last_tool_cacheable() {
        let tools = cached_tool_schemas();

        assert!(tools.len() > 1);
        assert!(
            tools[..tools.len() - 1]
                .iter()
                .all(|tool| tool.get("cache_control").is_none())
        );
        assert_eq!(tools.last().unwrap()["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn deterministic_node_id_normalizes_content() {
        assert_eq!(
            deterministic_node_id("BusinessRule", " Pending  Rule ", "A   B"),
            deterministic_node_id("businessrule", "pending rule", "a b")
        );
    }

    #[test]
    fn malformed_edges_do_not_block_json_parsing() {
        let parsed = parse_llm_json(
            r#"{
                "nodes": [],
                "edges": [
                    {
                        "source": "n1",
                        "target": "business-node:existing",
                        "kind": "MENTIONS",
                        "reason": "Malformed edge from model."
                    }
                ]
            }"#,
        )
        .unwrap();
        assert_eq!(parsed.edges.len(), 1);
        assert_eq!(parsed.edges[0].confidence, 0.0);
    }

    #[test]
    fn sse_parser_captures_anthropic_usage() {
        let stream = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":100,\"cache_creation\":{\"ephemeral_5m_input_tokens\":7},\"cache_read_input_tokens\":3,\"output_tokens\":1}}}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"{\\\"nodes\\\":[],\\\"edges\\\":[]}\"}}\n\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":20}}\n\n",
            "data: [DONE]\n\n",
        );

        let parsed = parse_sse_response(stream.as_bytes()).unwrap();

        assert_eq!(parsed.usage.input_tokens, 100);
        assert_eq!(parsed.usage.output_tokens, 20);
        assert_eq!(parsed.usage.cache_creation_input_tokens, 7);
        assert_eq!(parsed.usage.cache_read_input_tokens, 3);
        assert_eq!(parsed.usage.total_tokens(), 130);
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
            resume: false,
            max_failures: None,
        };

        let usage = TokenUsage {
            input_tokens: 120,
            output_tokens: 45,
            cache_creation_input_tokens: 10,
            cache_read_input_tokens: 5,
        };
        let summary = build_business_kg_with_client(
            &options,
            &MockClient {
                response,
                usage: usage.clone(),
            },
        )
        .unwrap();
        assert_eq!(summary.nodes, 1);
        assert_eq!(summary.evidence, 1);
        assert_eq!(summary.input_tokens, usage.input_tokens);
        assert_eq!(summary.output_tokens, usage.output_tokens);
        assert_eq!(summary.total_tokens, usage.total_tokens());
        let connection = Connection::open(&output).unwrap();
        let stored_tokens: (i64, i64, i64) = connection
            .query_row(
                "SELECT input_tokens, output_tokens, total_tokens
                 FROM llm_extraction_runs
                 ORDER BY id DESC
                 LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            stored_tokens,
            (
                usage.input_tokens as i64,
                usage.output_tokens as i64,
                usage.total_tokens() as i64,
            )
        );

        let second = build_business_kg_with_client(
            &options,
            &MockClient::new(LlmKgResponse {
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
            }),
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
            resume: false,
            max_failures: None,
        };

        let summary = build_business_kg_with_client(&options, &MockClient::new(response)).unwrap();
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.nodes, 0);
        assert_eq!(summary.evidence, 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_edges_are_skipped_without_losing_valid_nodes() {
        let root = test_dir("business-kg-invalid-edge");
        fs::write(
            root.join("OrderService.java"),
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
            edges: vec![EdgeProposal {
                source_client_id: Some("n1".to_string()),
                source_node_id: None,
                target_client_id: None,
                target_node_id: None,
                kind: "DEPENDS_ON".to_string(),
                confidence: 0.8,
                evidence: vec![EvidenceProposal {
                    method_id: "method:OrderService#approve".to_string(),
                    source_lines: vec![3],
                    reason: "Invalid edge should be ignored.".to_string(),
                }],
                source: None,
                target: None,
                reason: None,
                source_lines: Vec::new(),
            }],
        };
        let options = BuildBusinessKgOptions {
            database: extraction_db,
            output: Some(output.clone()),
            source_path: root.clone(),
            min_priority: Priority::High,
            max_methods: Some(1),
            force: false,
            resume: false,
            max_failures: None,
        };

        let summary = build_business_kg_with_client(&options, &MockClient::new(response)).unwrap();
        assert_eq!(summary.failed, 0);
        assert_eq!(summary.nodes, 1);
        assert_eq!(summary.edges, 0);
        assert_eq!(summary.evidence, 1);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_edge_fields_are_normalized_and_inserted() {
        let root = test_dir("business-kg-normalized-edge");
        fs::write(
            root.join("OrderService.java"),
            "class OrderService {\n  void approve() {\n    if (status != PENDING) throw new RuntimeException();\n  }\n}\n",
        )
        .unwrap();
        let extraction_db = root.join("business-extraction.db");
        write_extraction_db(&extraction_db, "OrderService.java");
        let output = root.join("business-kg.db");
        let node_evidence = |reason: &str| {
            vec![EvidenceProposal {
                method_id: "method:OrderService#approve".to_string(),
                source_lines: vec![3],
                reason: reason.to_string(),
            }]
        };
        let response = LlmKgResponse {
            nodes: vec![
                NodeProposal {
                    client_id: Some("n1".to_string()),
                    kind: "BusinessRule".to_string(),
                    name: "Pending approval rule".to_string(),
                    statement: "Approval requires PENDING status.".to_string(),
                    confidence: 0.95,
                    evidence: node_evidence("The method checks pending state."),
                },
                NodeProposal {
                    client_id: Some("n2".to_string()),
                    kind: "SideEffect".to_string(),
                    name: "Rejected approval".to_string(),
                    statement: "Non-pending approval is rejected.".to_string(),
                    confidence: 0.9,
                    evidence: node_evidence("The method throws on non-pending state."),
                },
            ],
            edges: vec![EdgeProposal {
                source_client_id: None,
                source_node_id: None,
                target_client_id: None,
                target_node_id: None,
                kind: "TRIGGERS".to_string(),
                confidence: 0.0,
                evidence: Vec::new(),
                source: Some("n1".to_string()),
                target: Some("n2".to_string()),
                reason: Some("Pending rule triggers rejection side effect.".to_string()),
                source_lines: Vec::new(),
            }],
        };
        let options = BuildBusinessKgOptions {
            database: extraction_db,
            output: Some(output.clone()),
            source_path: root.clone(),
            min_priority: Priority::High,
            max_methods: Some(1),
            force: false,
            resume: false,
            max_failures: None,
        };

        let summary = build_business_kg_with_client(&options, &MockClient::new(response)).unwrap();
        assert_eq!(summary.nodes, 2);
        assert_eq!(summary.edges, 1);
        assert_eq!(summary.evidence, 3);

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

    #[test]
    fn tool_executor_reads_bounded_test_evidence() {
        let root = test_dir("business-kg-test-tools");
        fs::write(
            root.join("OrderService.java"),
            "class OrderService {\n  void approve() {\n    validate();\n  }\n  void validate() {}\n}\n",
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

        let tests = tools
            .execute_tool(
                "get_tests_for_method",
                &serde_json::json!({"method_id": "method:OrderService#approve", "limit": 1}),
            )
            .unwrap();
        assert_eq!(
            tests["tests"][0]["test_case_id"],
            "test-case:OrderServiceIT#approvePending"
        );
        assert_eq!(tests["tests"][0]["source"], "jdtls_definition");

        let test_case = tools
            .execute_tool(
                "get_test_case",
                &serde_json::json!({"test_case_id": "test-case:OrderServiceIT#approvePending"}),
            )
            .unwrap();
        assert_eq!(test_case["test_case"]["name"], "approvePending");
        assert!(
            test_case["test_case"]["body_text"]
                .as_str()
                .unwrap()
                .contains("approve();")
        );

        let assertions = tools
            .execute_tool(
                "get_test_assertions",
                &serde_json::json!({"test_case_id": "test-case:OrderServiceIT#approvePending"}),
            )
            .unwrap();
        assert_eq!(assertions["assertions"][0]["assertion_kind"], "assertThat");

        let entry_points = tools
            .execute_tool(
                "get_test_entry_points",
                &serde_json::json!({"test_case_id": "test-case:OrderServiceIT#approvePending"}),
            )
            .unwrap();
        assert_eq!(
            entry_points["entry_points"][0]["route"],
            "/orders/{id}/approve"
        );

        let fixtures = tools
            .execute_tool(
                "get_test_fixtures",
                &serde_json::json!({"test_case_id": "test-case:OrderServiceIT#approvePending"}),
            )
            .unwrap();
        assert_eq!(fixtures["fixtures"].as_array().unwrap().len(), 2);
        assert_eq!(fixtures["fixtures"][0]["scope"], "suite");

        let _ = fs::remove_dir_all(root);
    }
}
