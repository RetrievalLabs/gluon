use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

const SUPPORTED_NODE_KINDS: [&str; 5] = [
    "BusinessRule",
    "Workflow",
    "Invariant",
    "StateTransition",
    "SideEffect",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerateCharacterizationTestsOptions {
    pub business_database: PathBuf,
    pub kg_database: PathBuf,
    pub source_path: PathBuf,
    pub output_dir: PathBuf,
    pub max_behaviors: Option<usize>,
    pub node_kind: Option<String>,
    pub force: bool,
    pub resume: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GenerateCharacterizationTestsSummary {
    pub business_database_path: String,
    pub kg_database_path: String,
    pub source_path: String,
    pub output_path: String,
    pub selected_behaviors: usize,
    pub persisted_behaviors: usize,
    pub skipped_behaviors: usize,
    pub diagnostics: usize,
}

#[derive(Debug)]
struct BehaviorCandidate {
    node_id: String,
    kind: String,
    name: String,
    statement: String,
    method_ids: Vec<String>,
}

#[derive(Debug)]
struct MethodContext {
    method_id: String,
    method_name: String,
    signature: Option<String>,
    file: Option<String>,
    start_line: Option<i64>,
    end_line: Option<i64>,
    class_name: Option<String>,
    qualified_class_name: Option<String>,
    package_name: Option<String>,
}

#[derive(Debug)]
struct GeneratedScenario {
    id: String,
    name: String,
    scenario_kind: String,
    invocation_kind: String,
}

#[derive(Debug)]
struct GeneratedFile {
    scenario_id: String,
    path: PathBuf,
    relative_path: String,
    class_name: String,
    package_name: String,
    content: String,
    content_hash: String,
}

const GENERATED_MARKER: &str = "GLUON-GENERATED-CHARACTERIZATION-TEST";

pub fn generate_characterization_tests(
    options: &GenerateCharacterizationTestsOptions,
) -> Result<GenerateCharacterizationTestsSummary, String> {
    validate_options(options)?;
    let output_path = characterization_database_path(&options.source_path, &options.output_dir)?;
    if options.force && output_path.exists() {
        fs::remove_file(&output_path).map_err(|error| {
            format!(
                "failed to remove characterization database {}: {error}",
                output_path.display()
            )
        })?;
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create output directory {}: {error}",
                parent.display()
            )
        })?;
    }

    let business = Connection::open(&options.business_database).map_err(|error| {
        format!(
            "failed to open business database {}: {error}",
            options.business_database.display()
        )
    })?;
    validate_business_database(&business)?;

    let kg = Connection::open(&options.kg_database).map_err(|error| {
        format!(
            "failed to open KG database {}: {error}",
            options.kg_database.display()
        )
    })?;
    validate_kg_database(&kg)?;

    let mut store = CharacterizationStore::open(&output_path)?;
    let run_id = store.start_run(options)?;

    let candidates = select_behavior_candidates(&kg, &business, options)?;
    let mut summary = GenerateCharacterizationTestsSummary {
        business_database_path: options.business_database.display().to_string(),
        kg_database_path: options.kg_database.display().to_string(),
        source_path: options.source_path.display().to_string(),
        output_path: output_path.display().to_string(),
        selected_behaviors: candidates.len(),
        ..GenerateCharacterizationTestsSummary::default()
    };

    for candidate in candidates {
        match store.persist_behavior(run_id, &candidate) {
            Ok(persisted) => {
                if persisted {
                    summary.persisted_behaviors += 1;
                    match generate_behavior_scaffold(&business, options, run_id, &candidate) {
                        Ok((scenario, file)) => {
                            store.persist_scenario(
                                run_id,
                                &behavior_id(&candidate.node_id),
                                &scenario,
                            )?;
                            match write_generated_file(&file, options.force) {
                                Ok(()) => {
                                    store.persist_file(&file)?;
                                }
                                Err(error) => {
                                    summary.diagnostics += 1;
                                    store.record_diagnostic(
                                        run_id,
                                        Some(&behavior_id(&candidate.node_id)),
                                        Some(&candidate.node_id),
                                        Some(&scenario.id),
                                        "error",
                                        "write_file",
                                        &error,
                                    )?;
                                }
                            }
                        }
                        Err(error) => {
                            summary.diagnostics += 1;
                            store.record_diagnostic(
                                run_id,
                                Some(&behavior_id(&candidate.node_id)),
                                Some(&candidate.node_id),
                                None,
                                "error",
                                "generate",
                                &error,
                            )?;
                        }
                    }
                } else {
                    summary.skipped_behaviors += 1;
                }
            }
            Err(error) => {
                summary.diagnostics += 1;
                store.record_diagnostic(
                    run_id,
                    Some(&behavior_id(&candidate.node_id)),
                    Some(&candidate.node_id),
                    None,
                    "error",
                    "persist",
                    &error,
                )?;
            }
        }
    }

    store.finish_run(run_id, &summary)?;
    Ok(summary)
}

fn validate_options(options: &GenerateCharacterizationTestsOptions) -> Result<(), String> {
    if !options.business_database.exists() {
        return Err(format!(
            "business database does not exist: {}",
            options.business_database.display()
        ));
    }
    if !options.kg_database.exists() {
        return Err(format!(
            "KG database does not exist: {}",
            options.kg_database.display()
        ));
    }
    if !options.source_path.exists() {
        return Err(format!(
            "source path does not exist: {}",
            options.source_path.display()
        ));
    }
    if options.force && options.resume {
        return Err("--force and --continue cannot be used together".to_string());
    }
    if options.max_behaviors == Some(0) {
        return Err("--max-behaviors must be greater than 0".to_string());
    }
    if let Some(kind) = &options.node_kind
        && !SUPPORTED_NODE_KINDS.contains(&kind.as_str())
    {
        return Err(format!("unsupported --node-kind: {kind}"));
    }
    Ok(())
}

fn validate_business_database(connection: &Connection) -> Result<(), String> {
    validate_tables(connection, "business database", &["methods"])
}

fn validate_kg_database(connection: &Connection) -> Result<(), String> {
    validate_tables(
        connection,
        "KG database",
        &["business_nodes", "business_evidence"],
    )
}

fn validate_tables(connection: &Connection, label: &str, tables: &[&str]) -> Result<(), String> {
    for table in tables {
        let exists: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("failed to inspect {label}: {error}"))?;
        if exists.is_none() {
            return Err(format!("invalid {label}: missing table {table}"));
        }
    }
    Ok(())
}

fn select_behavior_candidates(
    kg: &Connection,
    business: &Connection,
    options: &GenerateCharacterizationTestsOptions,
) -> Result<Vec<BehaviorCandidate>, String> {
    let mut sql = "
        SELECT n.id, n.kind, n.name, n.statement, GROUP_CONCAT(DISTINCT e.method_id)
        FROM business_nodes n
        JOIN business_evidence e ON e.node_id = n.id
        WHERE e.method_id IS NOT NULL
    "
    .to_string();

    if options.node_kind.is_some() {
        sql.push_str(" AND n.kind = ?1");
    } else {
        sql.push_str(" AND n.kind IN ('BusinessRule', 'Workflow', 'Invariant', 'StateTransition', 'SideEffect')");
    }
    sql.push_str(" GROUP BY n.id, n.kind, n.name, n.statement ORDER BY n.kind, n.name, n.id");

    let mut statement = kg
        .prepare(&sql)
        .map_err(|error| format!("failed to prepare behavior selection query: {error}"))?;
    let mut candidates = Vec::new();
    match options.node_kind.as_ref() {
        Some(kind) => {
            let rows = statement
                .query_map(params![kind], row_to_candidate)
                .map_err(|error| format!("failed to query behavior candidates: {error}"))?;
            collect_candidates(rows, business, &mut candidates)?;
        }
        None => {
            let rows = statement
                .query_map([], row_to_candidate)
                .map_err(|error| format!("failed to query behavior candidates: {error}"))?;
            collect_candidates(rows, business, &mut candidates)?;
        }
    }
    if let Some(limit) = options.max_behaviors {
        candidates.truncate(limit);
    }
    Ok(candidates)
}

fn row_to_candidate(row: &rusqlite::Row<'_>) -> rusqlite::Result<BehaviorCandidate> {
    let method_ids: String = row.get(4)?;
    Ok(BehaviorCandidate {
        node_id: row.get(0)?,
        kind: row.get(1)?,
        name: row.get(2)?,
        statement: row.get(3)?,
        method_ids: method_ids
            .split(',')
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .collect(),
    })
}

fn collect_candidates(
    rows: impl Iterator<Item = rusqlite::Result<BehaviorCandidate>>,
    business: &Connection,
    candidates: &mut Vec<BehaviorCandidate>,
) -> Result<(), String> {
    for row in rows {
        let candidate =
            row.map_err(|error| format!("failed to read behavior candidate row: {error}"))?;
        if candidate
            .method_ids
            .iter()
            .any(|method_id| method_exists(business, method_id).unwrap_or(false))
        {
            candidates.push(candidate);
        }
    }
    Ok(())
}

fn method_exists(connection: &Connection, method_id: &str) -> Result<bool, String> {
    connection
        .query_row("SELECT 1 FROM methods WHERE id = ?1", [method_id], |_| {
            Ok(())
        })
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| format!("failed to verify method {method_id}: {error}"))
}

fn generate_behavior_scaffold(
    business: &Connection,
    options: &GenerateCharacterizationTestsOptions,
    run_id: i64,
    candidate: &BehaviorCandidate,
) -> Result<(GeneratedScenario, GeneratedFile), String> {
    let method_id = candidate
        .method_ids
        .first()
        .ok_or_else(|| format!("behavior {} has no source methods", candidate.node_id))?;
    let method = load_method_context(business, method_id)?;
    let scenario_id = scenario_id(&candidate.node_id, method_id);
    let scenario = GeneratedScenario {
        id: scenario_id.clone(),
        name: format!("Characterize {}", candidate.name),
        scenario_kind: "scaffold".to_string(),
        invocation_kind: invocation_kind(business, method_id),
    };
    let package_name = generated_package_name(method.package_name.as_deref());
    let class_name = generated_class_name(&candidate.name, &candidate.node_id);
    let relative_path = generated_relative_path(&method, &package_name, &class_name);
    let path = options.source_path.join(&relative_path);
    let content = render_scaffold_test(run_id, candidate, &method, &package_name, &class_name);
    let content_hash = sha256_hex(&content);

    Ok((
        scenario,
        GeneratedFile {
            scenario_id,
            path,
            relative_path: relative_path.display().to_string(),
            class_name,
            package_name,
            content,
            content_hash,
        },
    ))
}

fn load_method_context(connection: &Connection, method_id: &str) -> Result<MethodContext, String> {
    let method_name = query_text_column(connection, "methods", "name", "id", method_id)?
        .ok_or_else(|| format!("method not found: {method_id}"))?;
    let class_id = query_text_column(connection, "methods", "class_id", "id", method_id)?;
    let signature = query_text_column(connection, "methods", "signature", "id", method_id)?;
    let file = query_text_column(connection, "methods", "file", "id", method_id)?;
    let start_line = query_i64_column(connection, "methods", "start_line", "id", method_id)?;
    let end_line = query_i64_column(connection, "methods", "end_line", "id", method_id)?;

    let class_name = class_id.as_deref().and_then(|id| {
        query_text_column(connection, "classes", "name", "id", id)
            .ok()
            .flatten()
    });
    let qualified_class_name = class_id.as_deref().and_then(|id| {
        query_text_column(connection, "classes", "qualified_name", "id", id)
            .ok()
            .flatten()
    });
    let package_name = class_id.as_deref().and_then(|id| {
        query_text_column(connection, "classes", "package_name", "id", id)
            .ok()
            .flatten()
    });

    Ok(MethodContext {
        method_id: method_id.to_string(),
        method_name,
        signature,
        file,
        start_line,
        end_line,
        class_name,
        qualified_class_name,
        package_name,
    })
}

fn query_text_column(
    connection: &Connection,
    table: &str,
    column: &str,
    key_column: &str,
    key: &str,
) -> Result<Option<String>, String> {
    if !column_exists(connection, table, column)? || !column_exists(connection, table, key_column)?
    {
        return Ok(None);
    }
    let sql = format!("SELECT {column} FROM {table} WHERE {key_column} = ?1");
    connection
        .query_row(&sql, [key], |row| row.get(0))
        .optional()
        .map_err(|error| format!("failed to query {table}.{column}: {error}"))
}

fn query_i64_column(
    connection: &Connection,
    table: &str,
    column: &str,
    key_column: &str,
    key: &str,
) -> Result<Option<i64>, String> {
    if !column_exists(connection, table, column)? || !column_exists(connection, table, key_column)?
    {
        return Ok(None);
    }
    let sql = format!("SELECT {column} FROM {table} WHERE {key_column} = ?1");
    connection
        .query_row(&sql, [key], |row| row.get(0))
        .optional()
        .map_err(|error| format!("failed to query {table}.{column}: {error}"))
}

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| format!("failed to inspect table {table}: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to inspect columns for {table}: {error}"))?;
    for row in rows {
        let name = row.map_err(|error| format!("failed to read column for {table}: {error}"))?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn invocation_kind(connection: &Connection, method_id: &str) -> String {
    if table_exists(connection, "entry_points").unwrap_or(false)
        && let Ok(Some(kind)) =
            query_text_column(connection, "entry_points", "kind", "method_id", method_id)
    {
        return kind;
    }
    "direct_method_scaffold".to_string()
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

fn generated_package_name(source_package: Option<&str>) -> String {
    source_package
        .filter(|package| !package.is_empty())
        .map(|package| format!("{package}.gluon.characterization"))
        .unwrap_or_else(|| "gluon.characterization".to_string())
}

fn generated_class_name(name: &str, node_id: &str) -> String {
    let mut class_name = String::from("Gluon");
    let mut capitalize_next = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if capitalize_next {
                class_name.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                class_name.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }
    if class_name == "Gluon" {
        class_name.push_str("Behavior");
    }
    class_name.push_str(&short_hash(node_id));
    class_name.push_str("CharacterizationTest");
    class_name
}

fn generated_relative_path(
    method: &MethodContext,
    package_name: &str,
    class_name: &str,
) -> PathBuf {
    let source_prefix = method
        .file
        .as_deref()
        .and_then(|file| file.split_once("src/main/java").map(|(prefix, _)| prefix));
    let mut path = source_prefix
        .map(PathBuf::from)
        .unwrap_or_else(PathBuf::new)
        .join("src/test/java");
    for segment in package_name.split('.') {
        path.push(segment);
    }
    path.push(format!("{class_name}.java"));
    path
}

fn render_scaffold_test(
    run_id: i64,
    candidate: &BehaviorCandidate,
    method: &MethodContext,
    package_name: &str,
    class_name: &str,
) -> String {
    let class_ref = method
        .qualified_class_name
        .as_deref()
        .or(method.class_name.as_deref())
        .unwrap_or("<unknown class>");
    let signature = method.signature.as_deref().unwrap_or(&method.method_name);
    let file = method.file.as_deref().unwrap_or("<unknown file>");
    let line_range = match (method.start_line, method.end_line) {
        (Some(start), Some(end)) => format!("{start}-{end}"),
        _ => "unknown".to_string(),
    };

    format!(
        r#"package {package_name};

import org.junit.Ignore;
import org.junit.Test;

/**
 * {GENERATED_MARKER}
 * Behavior: {behavior_name}
 * KG node: {kg_node}
 * Characterization run: {run_id}
 * Source method: {method_id}
 * Source location: {file}:{line_range}
 *
 * This scaffold is disabled until fixture generation and observation capture
 * are available for this behavior.
 */
@Ignore("Generated characterization scaffold requires fixture and observation support.")
public final class {class_name} {{
    @Test
    public void characterizesBehavior() {{
        throw new UnsupportedOperationException("{message}");
    }}
}}
"#,
        behavior_name = java_comment_text(&candidate.name),
        kg_node = java_comment_text(&candidate.node_id),
        method_id = java_comment_text(&method.method_id),
        message = java_string(&format!(
            "Generate fixture for {}.{} from {}",
            class_ref, signature, candidate.kind
        )),
    )
}

fn write_generated_file(file: &GeneratedFile, force: bool) -> Result<(), String> {
    if file.path.exists() {
        let existing = fs::read_to_string(&file.path).map_err(|error| {
            format!(
                "failed to read existing generated file {}: {error}",
                file.path.display()
            )
        })?;
        if !existing.contains(GENERATED_MARKER) {
            return Err(format!(
                "refusing to overwrite non-generated file {}",
                file.path.display()
            ));
        }
        if !force {
            return Ok(());
        }
    }
    if let Some(parent) = file.path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create generated test directory {}: {error}",
                parent.display()
            )
        })?;
    }
    fs::write(&file.path, &file.content).map_err(|error| {
        format!(
            "failed to write generated file {}: {error}",
            file.path.display()
        )
    })
}

fn scenario_id(node_id: &str, method_id: &str) -> String {
    format!("scenario:{}:{}", short_hash(node_id), short_hash(method_id))
}

fn short_hash(value: &str) -> String {
    sha256_hex(value).chars().take(12).collect()
}

fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn java_comment_text(value: &str) -> String {
    value.replace("*/", "* /").replace('\n', " ")
}

fn java_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn characterization_database_path(
    source_path: &Path,
    output_dir: &Path,
) -> Result<PathBuf, String> {
    let project_name = source_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(sanitize_path_segment)
        .ok_or_else(|| {
            format!(
                "path has no usable directory name: {}",
                source_path.display()
            )
        })?;
    Ok(output_dir
        .join(project_name)
        .join("characterization-tests.db"))
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    seconds.to_string()
}

fn behavior_id(node_id: &str) -> String {
    format!("behavior:{node_id}")
}

struct CharacterizationStore {
    connection: Connection,
}

impl CharacterizationStore {
    fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|error| {
            format!(
                "failed to open characterization database {}: {error}",
                path.display()
            )
        })?;
        let store = Self { connection };
        store.create_schema()?;
        Ok(store)
    }

    fn start_run(&mut self, options: &GenerateCharacterizationTestsOptions) -> Result<i64, String> {
        self.connection
            .execute(
                "INSERT INTO characterization_runs (
                    mode, source_path, business_database_path, kg_database_path,
                    status, started_at
                 ) VALUES ('generate', ?1, ?2, ?3, 'running', ?4)",
                params![
                    options.source_path.display().to_string(),
                    options.business_database.display().to_string(),
                    options.kg_database.display().to_string(),
                    timestamp(),
                ],
            )
            .map_err(|error| format!("failed to start characterization run: {error}"))?;
        Ok(self.connection.last_insert_rowid())
    }

    fn persist_behavior(
        &mut self,
        run_id: i64,
        candidate: &BehaviorCandidate,
    ) -> Result<bool, String> {
        let changed = self
            .connection
            .execute(
                "INSERT OR IGNORE INTO characterization_behaviors (
                    id, run_id, kg_node_id, node_kind, node_name, node_statement,
                    source_method_ids_json, status
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'selected')",
                params![
                    behavior_id(&candidate.node_id),
                    run_id,
                    candidate.node_id,
                    candidate.kind,
                    candidate.name,
                    candidate.statement,
                    serde_json::to_string(&candidate.method_ids)
                        .map_err(|error| format!("failed to serialize method IDs: {error}"))?,
                ],
            )
            .map_err(|error| {
                format!("failed to persist behavior {}: {error}", candidate.node_id)
            })?;
        Ok(changed > 0)
    }

    fn persist_scenario(
        &mut self,
        run_id: i64,
        behavior_id: &str,
        scenario: &GeneratedScenario,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT OR REPLACE INTO characterization_scenarios (
                    id, run_id, behavior_id, name, scenario_kind, invocation_kind,
                    status, diagnostic_reason
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'generated_scaffold', NULL)",
                params![
                    scenario.id,
                    run_id,
                    behavior_id,
                    scenario.name,
                    scenario.scenario_kind,
                    scenario.invocation_kind,
                ],
            )
            .map_err(|error| format!("failed to persist scenario {}: {error}", scenario.id))?;
        Ok(())
    }

    fn persist_file(&mut self, file: &GeneratedFile) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO characterization_files (
                    scenario_id, path, class_name, package_name, content_hash,
                    generated_marker
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    file.scenario_id,
                    file.relative_path,
                    file.class_name,
                    file.package_name,
                    file.content_hash,
                    GENERATED_MARKER,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to persist generated file {}: {error}",
                    file.relative_path
                )
            })?;
        Ok(())
    }

    fn record_diagnostic(
        &mut self,
        run_id: i64,
        behavior_id: Option<&str>,
        kg_node_id: Option<&str>,
        scenario_id: Option<&str>,
        severity: &str,
        category: &str,
        message: &str,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "INSERT INTO characterization_diagnostics (
                    run_id, behavior_id, kg_node_id, scenario_id, severity,
                    category, message
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    run_id,
                    behavior_id,
                    kg_node_id,
                    scenario_id,
                    severity,
                    category,
                    message,
                ],
            )
            .map_err(|error| format!("failed to record characterization diagnostic: {error}"))?;
        Ok(())
    }

    fn finish_run(
        &mut self,
        run_id: i64,
        summary: &GenerateCharacterizationTestsSummary,
    ) -> Result<(), String> {
        self.connection
            .execute(
                "UPDATE characterization_runs
                 SET status = ?1,
                     finished_at = ?2,
                     selected_behaviors = ?3,
                     persisted_behaviors = ?4,
                     skipped_behaviors = ?5,
                     diagnostics = ?6
                 WHERE id = ?7",
                params![
                    if summary.diagnostics == 0 {
                        "completed"
                    } else {
                        "partial_failure"
                    },
                    timestamp(),
                    summary.selected_behaviors as i64,
                    summary.persisted_behaviors as i64,
                    summary.skipped_behaviors as i64,
                    summary.diagnostics as i64,
                    run_id,
                ],
            )
            .map_err(|error| format!("failed to finish characterization run: {error}"))?;
        Ok(())
    }

    fn create_schema(&self) -> Result<(), String> {
        self.connection
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;

                CREATE TABLE IF NOT EXISTS characterization_runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    mode TEXT NOT NULL,
                    source_path TEXT NOT NULL,
                    business_database_path TEXT NOT NULL,
                    kg_database_path TEXT NOT NULL,
                    status TEXT NOT NULL,
                    started_at TEXT NOT NULL,
                    finished_at TEXT,
                    selected_behaviors INTEGER DEFAULT 0,
                    persisted_behaviors INTEGER DEFAULT 0,
                    skipped_behaviors INTEGER DEFAULT 0,
                    diagnostics INTEGER DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS characterization_behaviors (
                    id TEXT PRIMARY KEY,
                    run_id INTEGER NOT NULL,
                    kg_node_id TEXT NOT NULL,
                    node_kind TEXT NOT NULL,
                    node_name TEXT NOT NULL,
                    node_statement TEXT NOT NULL,
                    source_method_ids_json TEXT NOT NULL,
                    status TEXT NOT NULL,
                    FOREIGN KEY (run_id) REFERENCES characterization_runs(id)
                );

                CREATE TABLE IF NOT EXISTS characterization_scenarios (
                    id TEXT PRIMARY KEY,
                    run_id INTEGER NOT NULL,
                    behavior_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    scenario_kind TEXT NOT NULL,
                    invocation_kind TEXT,
                    status TEXT NOT NULL,
                    diagnostic_reason TEXT,
                    FOREIGN KEY (run_id) REFERENCES characterization_runs(id),
                    FOREIGN KEY (behavior_id) REFERENCES characterization_behaviors(id)
                );

                CREATE TABLE IF NOT EXISTS characterization_inputs (
                    id TEXT PRIMARY KEY,
                    scenario_id TEXT NOT NULL,
                    input_json TEXT NOT NULL,
                    fixture_json TEXT NOT NULL,
                    deterministic_seed_json TEXT NOT NULL,
                    FOREIGN KEY (scenario_id) REFERENCES characterization_scenarios(id)
                );

                CREATE TABLE IF NOT EXISTS characterization_observations (
                    id TEXT PRIMARY KEY,
                    scenario_id TEXT NOT NULL,
                    input_id TEXT NOT NULL,
                    status TEXT NOT NULL,
                    return_value_json TEXT,
                    response_body TEXT,
                    exception_type TEXT,
                    exception_message TEXT,
                    emitted_events_json TEXT NOT NULL DEFAULT '[]',
                    database_side_effects_json TEXT NOT NULL DEFAULT '[]',
                    fake_boundary_calls_json TEXT NOT NULL DEFAULT '[]',
                    normalized_output_json TEXT NOT NULL,
                    FOREIGN KEY (scenario_id) REFERENCES characterization_scenarios(id),
                    FOREIGN KEY (input_id) REFERENCES characterization_inputs(id)
                );

                CREATE TABLE IF NOT EXISTS characterization_files (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    scenario_id TEXT NOT NULL,
                    path TEXT NOT NULL,
                    class_name TEXT NOT NULL,
                    package_name TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    generated_marker TEXT NOT NULL,
                    FOREIGN KEY (scenario_id) REFERENCES characterization_scenarios(id)
                );

                CREATE TABLE IF NOT EXISTS characterization_fakes (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    scenario_id TEXT NOT NULL,
                    dependency_name TEXT NOT NULL,
                    fake_strategy TEXT NOT NULL,
                    source_file_path TEXT,
                    boundary_calls_json TEXT NOT NULL DEFAULT '[]',
                    FOREIGN KEY (scenario_id) REFERENCES characterization_scenarios(id)
                );

                CREATE TABLE IF NOT EXISTS characterization_diagnostics (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id INTEGER NOT NULL,
                    behavior_id TEXT,
                    kg_node_id TEXT,
                    scenario_id TEXT,
                    severity TEXT NOT NULL,
                    category TEXT NOT NULL,
                    message TEXT NOT NULL,
                    FOREIGN KEY (run_id) REFERENCES characterization_runs(id)
                );
                ",
            )
            .map_err(|error| format!("failed to create characterization schema: {error}"))
    }
}
