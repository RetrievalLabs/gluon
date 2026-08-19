use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use tree_sitter::{Node, Parser, TreeCursor};
use walkdir::{DirEntry, WalkDir};

use crate::languages::java::build::model::Diagnostic;
use crate::languages::java::business::jdtls::{
    JdtlsDefinition, JdtlsDefinitionRequest, JdtlsOptions, resolve_test_definitions,
};
use crate::languages::java::business::modules::{discover_modules, module_id_for_file};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestExtractionOptions {
    pub path: PathBuf,
    pub database: PathBuf,
    pub jdtls_command: String,
    pub jdtls_workspace: Option<PathBuf>,
    pub jdtls_max_in_flight: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TestExtractionSummary {
    pub database_path: String,
    pub suites: usize,
    pub cases: usize,
    pub targets: usize,
    pub assertions: usize,
    pub fixtures: usize,
    pub entry_points: usize,
    pub diagnostics: usize,
}

#[derive(Debug, Default)]
struct TestModel {
    suites: Vec<TestSuite>,
    cases: Vec<TestCase>,
    targets: Vec<TestTarget>,
    assertions: Vec<TestAssertion>,
    fixtures: Vec<TestFixture>,
    entry_points: Vec<TestEntryPoint>,
    invocations: Vec<TestInvocation>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
struct TestSuite {
    id: String,
    module_id: String,
    class_name: String,
    package_name: Option<String>,
    qualified_name: String,
    test_kind: String,
    file: String,
    start_line: usize,
    end_line: usize,
    annotations: Vec<String>,
}

#[derive(Debug, Clone)]
struct TestCase {
    id: String,
    suite_id: String,
    name: String,
    display_name: Option<String>,
    test_kind: String,
    file: String,
    start_line: usize,
    end_line: usize,
    annotations: Vec<String>,
    body_text: String,
}

#[derive(Debug, Clone)]
struct TestTarget {
    test_case_id: String,
    target_kind: String,
    target_id: String,
    relationship: String,
    confidence: f64,
    source: String,
}

#[derive(Debug, Clone)]
struct TestAssertion {
    test_case_id: String,
    assertion_kind: String,
    expression: String,
    expected_value: Option<String>,
    file: String,
    line: usize,
}

#[derive(Debug, Clone)]
struct TestFixture {
    suite_id: Option<String>,
    test_case_id: Option<String>,
    fixture_kind: String,
    name: String,
    details_json: String,
    file: String,
    line: usize,
}

#[derive(Debug, Clone)]
struct TestEntryPoint {
    test_case_id: String,
    kind: String,
    framework: Option<String>,
    route: Option<String>,
    http_method: Option<String>,
    topic: Option<String>,
    command: Option<String>,
    source: String,
}

#[derive(Debug, Clone)]
struct TestInvocation {
    test_case_id: String,
    file: String,
    name: String,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone)]
struct ClassScope {
    suite: TestSuite,
}

pub fn extract_tests(options: &TestExtractionOptions) -> Result<TestExtractionSummary, String> {
    let started_at = Instant::now();
    if !options.path.exists() {
        return Err(format!("path does not exist: {}", options.path.display()));
    }
    if options.jdtls_max_in_flight == 0 {
        return Err("--jdtls-max-in-flight must be greater than 0".to_string());
    }
    let project_root = if options.path.is_file() {
        options
            .path
            .parent()
            .ok_or_else(|| format!("path has no parent: {}", options.path.display()))?
            .to_path_buf()
    } else {
        options.path.clone()
    };

    eprintln!(
        "extract-tests tree-sitter: start path={}",
        project_root.display()
    );
    let phase_started_at = Instant::now();
    let mut model = extract_test_model(&project_root)?;
    eprintln!(
        "extract-tests tree-sitter: done suites={} cases={} assertions={} fixtures={} entry_points={} diagnostics={} elapsed_ms={}",
        model.suites.len(),
        model.cases.len(),
        model.assertions.len(),
        model.fixtures.len(),
        model.entry_points.len(),
        model.diagnostics.len(),
        phase_started_at.elapsed().as_millis()
    );

    let mut connection = Connection::open(&options.database).map_err(|error| {
        format!(
            "failed to open database {}: {error}",
            options.database.display()
        )
    })?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|error| format!("failed to enable foreign keys: {error}"))?;

    eprintln!("extract-tests jdtls targets: start");
    let phase_started_at = Instant::now();
    link_targets_with_jdtls(&mut model, &connection, &project_root, options)?;
    eprintln!(
        "extract-tests jdtls targets: done targets={} elapsed_ms={}",
        model.targets.len(),
        phase_started_at.elapsed().as_millis()
    );

    eprintln!(
        "extract-tests database: start path={}",
        options.database.display()
    );
    let phase_started_at = Instant::now();
    write_test_tables(&mut connection, &model)?;
    eprintln!(
        "extract-tests database: done path={} elapsed_ms={}",
        options.database.display(),
        phase_started_at.elapsed().as_millis()
    );
    eprintln!(
        "extract-tests done: total_elapsed_ms={}",
        started_at.elapsed().as_millis()
    );

    Ok(TestExtractionSummary {
        database_path: options.database.display().to_string(),
        suites: model.suites.len(),
        cases: model.cases.len(),
        targets: model.targets.len(),
        assertions: model.assertions.len(),
        fixtures: model.fixtures.len(),
        entry_points: model.entry_points.len(),
        diagnostics: model.diagnostics.len(),
    })
}

fn extract_test_model(project_root: &Path) -> Result<TestModel, String> {
    let modules = discover_modules(project_root);
    let mut model = TestModel::default();
    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                model.diagnostics.push(Diagnostic::warning(
                    "test_tree_sitter",
                    error.to_string(),
                    None,
                ));
                continue;
            }
        };
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|ext| ext.to_str()) != Some("java")
        {
            continue;
        }
        let display_path = relative_path(project_root, entry.path());
        if !is_included_test_source(&display_path) || is_generated_path(&display_path) {
            continue;
        }
        match fs::read_to_string(entry.path()) {
            Ok(contents) => {
                if is_generated_source(&contents) {
                    continue;
                }
                let module_id = module_id_for_file(&display_path, &modules);
                parse_test_file(&display_path, &module_id, &contents, &mut model);
            }
            Err(error) => model.diagnostics.push(Diagnostic::warning(
                "test_tree_sitter",
                format!("failed to read {}: {error}", entry.path().display()),
                Some(entry.path().display().to_string()),
            )),
        }
    }

    model.suites.sort_by(|left, right| left.id.cmp(&right.id));
    model.cases.sort_by(|left, right| left.id.cmp(&right.id));
    model.assertions.sort_by(|left, right| {
        (
            left.test_case_id.as_str(),
            left.line,
            left.expression.as_str(),
        )
            .cmp(&(
                right.test_case_id.as_str(),
                right.line,
                right.expression.as_str(),
            ))
    });
    model.fixtures.sort_by(|left, right| {
        (
            left.suite_id.as_deref(),
            left.test_case_id.as_deref(),
            left.line,
            left.name.as_str(),
        )
            .cmp(&(
                right.suite_id.as_deref(),
                right.test_case_id.as_deref(),
                right.line,
                right.name.as_str(),
            ))
    });
    model.entry_points.sort_by(|left, right| {
        (
            left.test_case_id.as_str(),
            left.kind.as_str(),
            left.route.as_deref(),
            left.topic.as_deref(),
            left.command.as_deref(),
        )
            .cmp(&(
                right.test_case_id.as_str(),
                right.kind.as_str(),
                right.route.as_deref(),
                right.topic.as_deref(),
                right.command.as_deref(),
            ))
    });
    Ok(model)
}

fn parse_test_file(file: &str, module_id: &str, contents: &str, model: &mut TestModel) {
    let mut parser = Parser::new();
    if let Err(error) = parser.set_language(&tree_sitter_java::LANGUAGE.into()) {
        model.diagnostics.push(Diagnostic::error(
            "test_tree_sitter",
            format!("failed to initialize Java parser: {error}"),
            Some(file.to_string()),
        ));
        return;
    }
    let Some(tree) = parser.parse(contents, None) else {
        model.diagnostics.push(Diagnostic::warning(
            "test_tree_sitter",
            format!("parser returned no syntax tree for {file}"),
            Some(file.to_string()),
        ));
        return;
    };
    if tree.root_node().has_error() {
        model.diagnostics.push(Diagnostic::warning(
            "test_tree_sitter",
            format!("failed to parse Java source {file}"),
            Some(file.to_string()),
        ));
        return;
    }

    let package_name = package_name(tree.root_node(), contents);
    let test_kind = classify_test_kind(file, &[]);
    let mut class_stack = Vec::new();
    let mut cursor = tree.walk();
    collect_test_nodes(
        contents,
        file,
        module_id,
        package_name.as_deref(),
        &test_kind,
        &mut cursor,
        &mut class_stack,
        model,
    );
}

fn collect_test_nodes(
    contents: &str,
    file: &str,
    module_id: &str,
    package_name: Option<&str>,
    file_test_kind: &str,
    cursor: &mut TreeCursor<'_>,
    class_stack: &mut Vec<ClassScope>,
    model: &mut TestModel,
) {
    let node = cursor.node();
    if is_class_node(node.kind()) {
        let suite = test_suite(
            node,
            contents,
            file,
            module_id,
            package_name,
            file_test_kind,
        );
        collect_suite_fixtures(&suite, model);
        class_stack.push(ClassScope {
            suite: suite.clone(),
        });
        model.suites.push(suite);
    } else if is_method_node(node.kind()) {
        if let Some(scope) = class_stack.last() {
            let annotations = annotations(node, contents);
            if is_test_case_method(node, contents, &annotations) {
                let case = test_case(node, contents, file, &scope.suite, &annotations);
                collect_method_details(node, contents, &case, model);
                model.cases.push(case);
            } else if is_fixture_method(&annotations) {
                let fixture = method_fixture(node, contents, file, &scope.suite, &annotations);
                model.fixtures.push(fixture);
            }
        }
    }

    if cursor.goto_first_child() {
        loop {
            collect_test_nodes(
                contents,
                file,
                module_id,
                package_name,
                file_test_kind,
                cursor,
                class_stack,
                model,
            );
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }

    if is_class_node(node.kind()) {
        class_stack.pop();
    }
}

fn test_suite(
    node: Node<'_>,
    contents: &str,
    file: &str,
    module_id: &str,
    package_name: Option<&str>,
    file_test_kind: &str,
) -> TestSuite {
    let annotations = annotations(node, contents);
    let class_name = node_text(child_by_field(node, "name"), contents).unwrap_or("Anonymous");
    let qualified_name = match package_name {
        Some(package_name) if !package_name.is_empty() => format!("{package_name}.{class_name}"),
        _ => class_name.to_string(),
    };
    TestSuite {
        id: format!(
            "test-suite:{qualified_name}@{}:{}",
            file,
            node.start_position().row + 1
        ),
        module_id: module_id.to_string(),
        class_name: class_name.to_string(),
        package_name: package_name.map(str::to_string),
        qualified_name,
        test_kind: classify_test_kind(file, &annotations),
        file: file.to_string(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        annotations,
    }
    .with_default_kind(file_test_kind)
}

trait WithDefaultKind {
    fn with_default_kind(self, default_kind: &str) -> Self;
}

impl WithDefaultKind for TestSuite {
    fn with_default_kind(mut self, default_kind: &str) -> Self {
        if self.test_kind == "unknown_test" {
            self.test_kind = default_kind.to_string();
        }
        self
    }
}

fn test_case(
    node: Node<'_>,
    contents: &str,
    file: &str,
    suite: &TestSuite,
    annotations: &[String],
) -> TestCase {
    let name_node = child_by_field(node, "name");
    let name = node_text(name_node, contents).unwrap_or("<anonymous>");
    let body_text = child_by_field(node, "body")
        .and_then(|body| node_text(Some(body), contents).map(str::to_string))
        .unwrap_or_default();
    TestCase {
        id: format!(
            "test-case:{}#{}@{}:{}",
            suite.qualified_name,
            name,
            file,
            node.start_position().row + 1
        ),
        suite_id: suite.id.clone(),
        name: name.to_string(),
        display_name: display_name(annotations),
        test_kind: suite.test_kind.clone(),
        file: file.to_string(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        annotations: annotations.to_vec(),
        body_text,
    }
}

fn collect_method_details(node: Node<'_>, contents: &str, case: &TestCase, model: &mut TestModel) {
    let mut cursor = node.walk();
    collect_method_detail_nodes(contents, case, &mut cursor, model);
}

fn collect_method_detail_nodes(
    contents: &str,
    case: &TestCase,
    cursor: &mut TreeCursor<'_>,
    model: &mut TestModel,
) {
    let node = cursor.node();
    if node.kind() == "method_invocation" {
        let name_node = child_by_field(node, "name").unwrap_or(node);
        let name = node_text(Some(name_node), contents).unwrap_or("");
        if !name.is_empty() {
            let position = name_node.start_position();
            model.invocations.push(TestInvocation {
                test_case_id: case.id.clone(),
                file: case.file.clone(),
                name: name.to_string(),
                line: position.row + 1,
                column: position.column,
            });
        }
        if let Some(assertion_kind) = assertion_kind(name) {
            model.assertions.push(TestAssertion {
                test_case_id: case.id.clone(),
                assertion_kind: assertion_kind.to_string(),
                expression: node_text(Some(node), contents)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                expected_value: first_string_literal(node, contents),
                file: case.file.clone(),
                line: node.start_position().row + 1,
            });
        }
        if let Some(entry_point) = entry_point_for_invocation(node, contents, case, name) {
            model.entry_points.push(entry_point);
        }
        if let Some(fixture) = fixture_for_invocation(node, contents, case, name) {
            model.fixtures.push(fixture);
        }
    }
    if cursor.goto_first_child() {
        loop {
            collect_method_detail_nodes(contents, case, cursor, model);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn collect_suite_fixtures(suite: &TestSuite, model: &mut TestModel) {
    for annotation in &suite.annotations {
        let name = annotation_name(annotation);
        if let Some(kind) = fixture_kind_for_annotation(&name) {
            model.fixtures.push(TestFixture {
                suite_id: Some(suite.id.clone()),
                test_case_id: None,
                fixture_kind: kind.to_string(),
                name,
                details_json: json(annotation).unwrap_or_else(|_| "{}".to_string()),
                file: suite.file.clone(),
                line: suite.start_line,
            });
        }
    }
}

fn method_fixture(
    node: Node<'_>,
    contents: &str,
    file: &str,
    suite: &TestSuite,
    annotations: &[String],
) -> TestFixture {
    let name = child_by_field(node, "name")
        .and_then(|name| node_text(Some(name), contents))
        .unwrap_or("<anonymous>");
    TestFixture {
        suite_id: Some(suite.id.clone()),
        test_case_id: None,
        fixture_kind: annotations
            .iter()
            .map(|annotation| annotation_name(annotation))
            .find_map(|name| fixture_kind_for_annotation(&name).map(str::to_string))
            .unwrap_or_else(|| "lifecycle".to_string()),
        name: name.to_string(),
        details_json: json(&annotations).unwrap_or_else(|_| "[]".to_string()),
        file: file.to_string(),
        line: node.start_position().row + 1,
    }
}

fn entry_point_for_invocation(
    node: Node<'_>,
    contents: &str,
    case: &TestCase,
    name: &str,
) -> Option<TestEntryPoint> {
    let invocation = node_text(Some(node), contents).unwrap_or("");
    let lower = invocation.to_ascii_lowercase();
    let http_method = match name {
        "get" | "getForEntity" | "getForObject" => Some("GET"),
        "post" | "postForEntity" | "postForObject" => Some("POST"),
        "put" => Some("PUT"),
        "patch" | "patchForObject" => Some("PATCH"),
        "delete" => Some("DELETE"),
        "perform" if lower.contains("get(") => Some("GET"),
        "perform" if lower.contains("post(") => Some("POST"),
        "perform" if lower.contains("put(") => Some("PUT"),
        "perform" if lower.contains("patch(") => Some("PATCH"),
        "perform" if lower.contains("delete(") => Some("DELETE"),
        _ => None,
    };
    if let Some(http_method) = http_method {
        return Some(TestEntryPoint {
            test_case_id: case.id.clone(),
            kind: "Http".to_string(),
            framework: Some(if name == "perform" {
                "MockMvc".to_string()
            } else {
                "HTTP client".to_string()
            }),
            route: first_string_literal(node, contents),
            http_method: Some(http_method.to_string()),
            topic: None,
            command: None,
            source: "tree_sitter".to_string(),
        });
    }
    if matches!(
        name,
        "send" | "sendMessage" | "receive" | "receiveAndConvert"
    ) {
        return Some(TestEntryPoint {
            test_case_id: case.id.clone(),
            kind: "Message".to_string(),
            framework: None,
            route: None,
            http_method: None,
            topic: first_string_literal(node, contents),
            command: None,
            source: "tree_sitter".to_string(),
        });
    }
    None
}

fn fixture_for_invocation(
    node: Node<'_>,
    contents: &str,
    case: &TestCase,
    name: &str,
) -> Option<TestFixture> {
    let kind = match name {
        "mock" | "spy" | "when" | "given" => "mock",
        "withInitScript" | "withExposedPorts" | "start" => "container",
        "execute" | "executeSql" | "runScript" => "database",
        _ => return None,
    };
    Some(TestFixture {
        suite_id: None,
        test_case_id: Some(case.id.clone()),
        fixture_kind: kind.to_string(),
        name: name.to_string(),
        details_json: json(&node_text(Some(node), contents).unwrap_or(""))
            .unwrap_or_else(|_| "\"\"".to_string()),
        file: case.file.clone(),
        line: node.start_position().row + 1,
    })
}

fn link_targets_with_jdtls(
    model: &mut TestModel,
    connection: &Connection,
    project_root: &Path,
    options: &TestExtractionOptions,
) -> Result<(), String> {
    if model.invocations.is_empty() {
        return Ok(());
    }
    let workspace = options.jdtls_workspace.clone().unwrap_or_else(|| {
        options
            .database
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".jdtls-test-workspace")
    });
    let java_files = java_files_for_jdtls(connection, model)?;
    let requests = model
        .invocations
        .iter()
        .map(|invocation| JdtlsDefinitionRequest {
            owner_id: invocation.test_case_id.clone(),
            file: invocation.file.clone(),
            name: invocation.name.clone(),
            line: invocation.line,
            column: invocation.column,
        })
        .collect::<Vec<_>>();
    let definitions = resolve_test_definitions(
        project_root,
        &JdtlsOptions {
            command: options.jdtls_command.clone(),
            workspace,
            max_in_flight: options.jdtls_max_in_flight,
            deep_enrichment: false,
        },
        &java_files,
        &requests,
    )?;
    let mut seen = HashSet::new();
    for definition in definitions {
        if let Some(target) = production_target_for_definition(connection, &definition)? {
            push_target(&definition.owner_id, target, &definition, &mut seen, model);
        }
    }
    model.targets.sort_by(|left, right| {
        (
            left.test_case_id.as_str(),
            left.target_kind.as_str(),
            left.target_id.as_str(),
        )
            .cmp(&(
                right.test_case_id.as_str(),
                right.target_kind.as_str(),
                right.target_id.as_str(),
            ))
    });
    Ok(())
}

#[derive(Debug, Clone)]
struct ResolvedTarget {
    kind: String,
    id: String,
    relationship: String,
}

fn push_target(
    test_case_id: &str,
    target: ResolvedTarget,
    definition: &JdtlsDefinition,
    seen: &mut HashSet<String>,
    model: &mut TestModel,
) {
    let key = format!(
        "{test_case_id}\0{}\0{}\0{}",
        target.kind, target.id, target.relationship
    );
    if !seen.insert(key) {
        return;
    }
    model.targets.push(TestTarget {
        test_case_id: test_case_id.to_string(),
        target_kind: target.kind,
        target_id: target.id,
        relationship: target.relationship,
        confidence: 0.95,
        source: format!(
            "jdtls_definition:{}@{}:{}",
            definition.name, definition.file, definition.line
        ),
    });
}

fn production_target_for_definition(
    connection: &Connection,
    definition: &JdtlsDefinition,
) -> Result<Option<ResolvedTarget>, String> {
    if table_exists(connection, "methods")?
        && table_has_column(connection, "methods", "file")?
        && table_has_column(connection, "methods", "start_line")?
        && table_has_column(connection, "methods", "end_line")?
    {
        let method = connection
            .query_row(
                "SELECT id FROM methods
                 WHERE file = ?1 AND start_line <= ?2 AND end_line >= ?2
                 ORDER BY (end_line - start_line), id
                 LIMIT 1",
                params![definition.file, definition.line as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("failed to resolve JDTLS method target: {error}"))?;
        if let Some(id) = method {
            return Ok(Some(ResolvedTarget {
                kind: "method".to_string(),
                id,
                relationship: "exercises".to_string(),
            }));
        }
    }
    if table_exists(connection, "classes")? && table_has_column(connection, "classes", "file")? {
        let class = connection
            .query_row(
                "SELECT id FROM classes
                 WHERE file = ?1 AND start_line <= ?2 AND end_line >= ?2
                 ORDER BY (end_line - start_line), id
                 LIMIT 1",
                params![definition.file, definition.line as i64],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("failed to resolve JDTLS class target: {error}"))?;
        if let Some(id) = class {
            return Ok(Some(ResolvedTarget {
                kind: "class".to_string(),
                id,
                relationship: "mentions".to_string(),
            }));
        }
    }
    Ok(None)
}

fn java_files_for_jdtls(connection: &Connection, model: &TestModel) -> Result<Vec<String>, String> {
    let mut files = model
        .suites
        .iter()
        .map(|suite| suite.file.clone())
        .collect::<BTreeSet<_>>();
    if table_exists(connection, "methods")? && table_has_column(connection, "methods", "file")? {
        let mut statement = connection
            .prepare("SELECT DISTINCT file FROM methods")
            .map_err(|error| format!("failed to prepare method file query: {error}"))?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("failed to query method files: {error}"))?;
        for row in rows {
            files.insert(row.map_err(|error| format!("failed to read method file: {error}"))?);
        }
    }
    Ok(files.into_iter().collect())
}

fn write_test_tables(connection: &mut Connection, model: &TestModel) -> Result<(), String> {
    create_test_schema(connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("failed to start test extraction transaction: {error}"))?;
    transaction
        .execute_batch(
            "
            DELETE FROM test_entry_points;
            DELETE FROM test_fixtures;
            DELETE FROM test_assertions;
            DELETE FROM test_targets;
            DELETE FROM test_cases;
            DELETE FROM test_suites;
            DELETE FROM test_diagnostics;
            ",
        )
        .map_err(|error| format!("failed to clear test extraction rows: {error}"))?;

    for suite in &model.suites {
        transaction
            .execute(
                "INSERT INTO test_suites (
                    id, module_id, class_name, package_name, qualified_name, test_kind,
                    file, start_line, end_line, annotations_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    suite.id,
                    suite.module_id,
                    suite.class_name,
                    suite.package_name,
                    suite.qualified_name,
                    suite.test_kind,
                    suite.file,
                    suite.start_line as i64,
                    suite.end_line as i64,
                    json(&suite.annotations)?,
                ],
            )
            .map_err(|error| format!("failed to insert test suite {}: {error}", suite.id))?;
    }

    for case in &model.cases {
        transaction
            .execute(
                "INSERT INTO test_cases (
                    id, suite_id, name, display_name, test_kind, file, start_line,
                    end_line, annotations_json, body_text
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    case.id,
                    case.suite_id,
                    case.name,
                    case.display_name,
                    case.test_kind,
                    case.file,
                    case.start_line as i64,
                    case.end_line as i64,
                    json(&case.annotations)?,
                    case.body_text,
                ],
            )
            .map_err(|error| format!("failed to insert test case {}: {error}", case.id))?;
    }

    for target in &model.targets {
        transaction
            .execute(
                "INSERT INTO test_targets (
                    test_case_id, target_kind, target_id, relationship, confidence, source
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    target.test_case_id,
                    target.target_kind,
                    target.target_id,
                    target.relationship,
                    target.confidence,
                    target.source,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert test target for {}: {error}",
                    target.test_case_id
                )
            })?;
    }

    for assertion in &model.assertions {
        transaction
            .execute(
                "INSERT INTO test_assertions (
                    test_case_id, assertion_kind, expression, expected_value, file, line
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    assertion.test_case_id,
                    assertion.assertion_kind,
                    assertion.expression,
                    assertion.expected_value,
                    assertion.file,
                    assertion.line as i64,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert test assertion for {}: {error}",
                    assertion.test_case_id
                )
            })?;
    }

    for fixture in &model.fixtures {
        transaction
            .execute(
                "INSERT INTO test_fixtures (
                    suite_id, test_case_id, fixture_kind, name, details_json, file, line
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    fixture.suite_id,
                    fixture.test_case_id,
                    fixture.fixture_kind,
                    fixture.name,
                    fixture.details_json,
                    fixture.file,
                    fixture.line as i64,
                ],
            )
            .map_err(|error| format!("failed to insert test fixture {}: {error}", fixture.name))?;
    }

    for entry_point in &model.entry_points {
        transaction
            .execute(
                "INSERT INTO test_entry_points (
                    test_case_id, kind, framework, route, http_method, topic, command, source
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    entry_point.test_case_id,
                    entry_point.kind,
                    entry_point.framework,
                    entry_point.route,
                    entry_point.http_method,
                    entry_point.topic,
                    entry_point.command,
                    entry_point.source,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert test entry point for {}: {error}",
                    entry_point.test_case_id
                )
            })?;
    }

    for diagnostic in &model.diagnostics {
        transaction
            .execute(
                "INSERT INTO test_diagnostics (severity, category, message, file)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    diagnostic.severity,
                    diagnostic.category,
                    diagnostic.message,
                    diagnostic.file,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert test diagnostic {}: {error}",
                    diagnostic.category
                )
            })?;
    }

    transaction
        .commit()
        .map_err(|error| format!("failed to commit test extraction transaction: {error}"))?;
    Ok(())
}

fn create_test_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE IF NOT EXISTS test_suites (
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

            CREATE TABLE IF NOT EXISTS test_cases (
                id TEXT PRIMARY KEY,
                suite_id TEXT NOT NULL,
                name TEXT NOT NULL,
                display_name TEXT,
                test_kind TEXT NOT NULL,
                file TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                annotations_json TEXT NOT NULL,
                body_text TEXT NOT NULL,
                FOREIGN KEY (suite_id) REFERENCES test_suites(id)
            );

            CREATE TABLE IF NOT EXISTS test_targets (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                test_case_id TEXT NOT NULL,
                target_kind TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relationship TEXT NOT NULL,
                confidence REAL NOT NULL,
                source TEXT NOT NULL,
                FOREIGN KEY (test_case_id) REFERENCES test_cases(id)
            );

            CREATE TABLE IF NOT EXISTS test_assertions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                test_case_id TEXT NOT NULL,
                assertion_kind TEXT NOT NULL,
                expression TEXT NOT NULL,
                expected_value TEXT,
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                FOREIGN KEY (test_case_id) REFERENCES test_cases(id)
            );

            CREATE TABLE IF NOT EXISTS test_fixtures (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                suite_id TEXT,
                test_case_id TEXT,
                fixture_kind TEXT NOT NULL,
                name TEXT NOT NULL,
                details_json TEXT NOT NULL,
                file TEXT NOT NULL,
                line INTEGER NOT NULL,
                FOREIGN KEY (suite_id) REFERENCES test_suites(id),
                FOREIGN KEY (test_case_id) REFERENCES test_cases(id)
            );

            CREATE TABLE IF NOT EXISTS test_entry_points (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                test_case_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                framework TEXT,
                route TEXT,
                http_method TEXT,
                topic TEXT,
                command TEXT,
                source TEXT NOT NULL,
                FOREIGN KEY (test_case_id) REFERENCES test_cases(id)
            );

            CREATE TABLE IF NOT EXISTS test_diagnostics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                severity TEXT NOT NULL,
                category TEXT NOT NULL,
                message TEXT NOT NULL,
                file TEXT
            );

            CREATE INDEX IF NOT EXISTS idx_test_cases_suite ON test_cases(suite_id);
            CREATE INDEX IF NOT EXISTS idx_test_targets_case ON test_targets(test_case_id);
            CREATE INDEX IF NOT EXISTS idx_test_targets_target ON test_targets(target_kind, target_id);
            CREATE INDEX IF NOT EXISTS idx_test_assertions_case ON test_assertions(test_case_id);
            CREATE INDEX IF NOT EXISTS idx_test_fixtures_suite ON test_fixtures(suite_id);
            CREATE INDEX IF NOT EXISTS idx_test_fixtures_case ON test_fixtures(test_case_id);
            CREATE INDEX IF NOT EXISTS idx_test_entry_points_case ON test_entry_points(test_case_id);
            ",
        )
        .map_err(|error| format!("failed to create test extraction schema: {error}"))
}

fn table_exists(connection: &Connection, table: &str) -> Result<bool, String> {
    connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            [table],
            |_| Ok(()),
        )
        .optional()
        .map(|value| value.is_some())
        .map_err(|error| format!("failed to inspect database schema: {error}"))
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> Result<bool, String> {
    let sql = format!("PRAGMA table_info({table})");
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("failed to inspect table {table}: {error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| format!("failed to read table {table} columns: {error}"))?;
    for row in rows {
        if row.map_err(|error| format!("failed to read table {table} column: {error}"))? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_included_test_source(path: &str) -> bool {
    let components = path
        .split('/')
        .map(|component| component.to_ascii_lowercase())
        .collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            component.as_str(),
            "integrationtest"
                | "integration-test"
                | "integrationtests"
                | "integration-tests"
                | "acceptancetest"
                | "acceptance-test"
                | "acceptancetests"
                | "acceptance-tests"
                | "e2e"
                | "e2etest"
                | "e2e-test"
                | "e2etests"
                | "e2e-tests"
        )
    }) {
        return true;
    }
    path.rsplit('/')
        .next()
        .map(is_integration_test_file_name)
        .unwrap_or(false)
}

fn is_integration_test_file_name(file_name: &str) -> bool {
    [
        "IT.java",
        "ITCase.java",
        "IntegrationTest.java",
        "IntegrationTests.java",
        "AcceptanceTest.java",
        "AcceptanceTests.java",
        "E2ETest.java",
        "E2ETests.java",
    ]
    .iter()
    .any(|suffix| file_name.ends_with(suffix))
}

fn is_generated_path(path: &str) -> bool {
    path.split('/').any(|component| {
        matches!(
            component.to_ascii_lowercase().as_str(),
            "generated"
                | "generated-sources"
                | "generated-src"
                | "generatedsources"
                | "gensrc"
                | "autogen"
                | "auto-generated"
                | ".openapi-generator"
        )
    })
}

fn is_generated_source(contents: &str) -> bool {
    let header = contents.chars().take(4096).collect::<String>();
    header.contains("@Generated")
        || header.contains("@javax.annotation.Generated")
        || header.contains("@jakarta.annotation.Generated")
        || header.contains("<auto-generated")
        || header.contains("AUTO-GENERATED")
        || header.contains("Auto-generated")
        || header.contains("Generated by")
        || header.contains("This file was generated")
        || header.contains("do not edit")
        || header.contains("DO NOT EDIT")
}

fn classify_test_kind(file: &str, annotations: &[String]) -> String {
    let lower = file.to_ascii_lowercase();
    if lower.contains("e2e") {
        return "e2e".to_string();
    }
    if lower.contains("acceptance") {
        return "acceptance".to_string();
    }
    if lower.contains("integration") || is_integration_test_file_name(file) {
        return "integration".to_string();
    }
    if annotations
        .iter()
        .map(|annotation| annotation_name(annotation))
        .any(|name| matches!(name.as_str(), "SpringBootTest" | "Testcontainers"))
    {
        return "integration".to_string();
    }
    "unknown_test".to_string()
}

fn is_test_case_method(node: Node<'_>, contents: &str, annotations: &[String]) -> bool {
    if annotations
        .iter()
        .map(|annotation| annotation_name(annotation))
        .any(|name| {
            matches!(
                name.as_str(),
                "Test" | "ParameterizedTest" | "RepeatedTest" | "TestFactory" | "TestTemplate"
            )
        })
    {
        return true;
    }
    let name = child_by_field(node, "name")
        .and_then(|name| node_text(Some(name), contents))
        .unwrap_or("");
    name.starts_with("test") || name.starts_with("should")
}

fn is_fixture_method(annotations: &[String]) -> bool {
    annotations
        .iter()
        .map(|annotation| annotation_name(annotation))
        .any(|name| fixture_kind_for_annotation(&name).is_some())
}

fn fixture_kind_for_annotation(name: &str) -> Option<&'static str> {
    match name {
        "BeforeEach" | "BeforeAll" | "Before" | "BeforeClass" => Some("setup"),
        "AfterEach" | "AfterAll" | "After" | "AfterClass" => Some("teardown"),
        "SpringBootTest" | "WebMvcTest" | "DataJpaTest" | "ContextConfiguration" => {
            Some("framework")
        }
        "Testcontainers" | "Container" => Some("container"),
        "MockBean" | "Mock" | "SpyBean" => Some("mock"),
        "ActiveProfiles" | "TestPropertySource" => Some("configuration"),
        _ => None,
    }
}

fn assertion_kind(name: &str) -> Option<&'static str> {
    match name {
        "assertEquals" | "isEqualTo" | "isSameAs" => Some("equality"),
        "assertTrue" | "isTrue" | "exists" => Some("truthy"),
        "assertFalse" | "isFalse" | "doesNotExist" => Some("falsey"),
        "assertThrows" | "isThrownBy" | "expectThrows" => Some("exception"),
        "assertThat" | "then" => Some("assertion"),
        "andExpect" | "expectStatus" | "status" => Some("status_or_response"),
        "verify" | "thenVerify" => Some("verification"),
        _ if name.starts_with("assert") => Some("assertion"),
        _ => None,
    }
}

fn display_name(annotations: &[String]) -> Option<String> {
    annotations.iter().find_map(|annotation| {
        if annotation_name(annotation) == "DisplayName" {
            first_quoted_value(annotation)
        } else {
            None
        }
    })
}

fn first_string_literal(node: Node<'_>, contents: &str) -> Option<String> {
    let mut cursor = node.walk();
    first_string_literal_node(contents, &mut cursor)
}

fn first_string_literal_node(contents: &str, cursor: &mut TreeCursor<'_>) -> Option<String> {
    let node = cursor.node();
    if matches!(node.kind(), "string_literal" | "text_block") {
        return node_text(Some(node), contents).and_then(first_quoted_value);
    }
    if cursor.goto_first_child() {
        loop {
            if let Some(value) = first_string_literal_node(contents, cursor) {
                cursor.goto_parent();
                return Some(value);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
    None
}

fn first_quoted_value(value: &str) -> Option<String> {
    let start = value.find('"')?;
    let rest = &value[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn package_name(root: Node<'_>, contents: &str) -> Option<String> {
    let mut cursor = root.walk();
    if !cursor.goto_first_child() {
        return None;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "package_declaration" {
            return child
                .child_by_field_name("name")
                .and_then(|name| node_text(Some(name), contents).map(str::to_string));
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    None
}

fn annotations(node: Node<'_>, contents: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut cursor = node.walk();
    collect_annotations(contents, &mut cursor, &mut values);
    values.sort();
    values.dedup();
    values
}

fn collect_annotations(contents: &str, cursor: &mut TreeCursor<'_>, values: &mut Vec<String>) {
    let node = cursor.node();
    if matches!(node.kind(), "annotation" | "marker_annotation") {
        if let Some(value) = node_text(Some(node), contents) {
            values.push(value.trim().to_string());
        }
        return;
    }
    if cursor.goto_first_child() {
        loop {
            collect_annotations(contents, cursor, values);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn annotation_name(annotation: &str) -> String {
    let before_args = annotation
        .trim()
        .trim_start_matches('@')
        .split_once('(')
        .map(|(name, _)| name)
        .unwrap_or_else(|| annotation.trim().trim_start_matches('@'));
    before_args.rsplit('.').next().unwrap_or("").to_string()
}

fn is_class_node(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration"
    )
}

fn is_method_node(kind: &str) -> bool {
    matches!(kind, "method_declaration" | "constructor_declaration")
}

fn child_by_field<'a>(node: Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

fn node_text<'a>(node: Option<Node<'a>>, contents: &'a str) -> Option<&'a str> {
    node.and_then(|node| node.utf8_text(contents.as_bytes()).ok())
}

fn relative_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && matches!(
            entry.file_name().to_string_lossy().as_ref(),
            ".git" | "target" | "build" | ".gradle" | ".idea"
        )
}

fn json<T: Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value)
        .map_err(|error| format!("failed to serialize test extraction JSON: {error}"))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn includes_integration_e2e_and_acceptance_tests_only() {
        assert!(is_included_test_source(
            "src/integrationTest/java/demo/OrderServiceIT.java"
        ));
        assert!(is_included_test_source(
            "src/e2e/java/demo/CheckoutE2ETest.java"
        ));
        assert!(is_included_test_source(
            "src/acceptanceTest/java/demo/OrderAcceptanceTest.java"
        ));
        assert!(!is_included_test_source(
            "src/test/java/demo/OrderServiceTest.java"
        ));
        assert!(!is_included_test_source(
            "src/main/java/demo/OrderService.java"
        ));
    }

    #[test]
    fn extracts_test_tables_and_replaces_old_rows() {
        let root = test_dir("test-extraction");
        fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
        fs::create_dir_all(root.join("src/integrationTest/java/demo")).unwrap();
        fs::create_dir_all(root.join("src/test/java/demo")).unwrap();
        fs::write(
            root.join("src/main/java/demo/OrderService.java"),
            "package demo; public class OrderService { public void approve() {} }\n",
        )
        .unwrap();
        fs::write(
            root.join("src/integrationTest/java/demo/OrderServiceIT.java"),
            r#"package demo;
import org.junit.jupiter.api.Test;
import org.springframework.boot.test.context.SpringBootTest;
@SpringBootTest
class OrderServiceIT {
  @Test
  void approvesOrder() {
    OrderService service = new OrderService();
    service.approve();
    assertEquals("ok", "ok");
  }
}
"#,
        )
        .unwrap();
        fs::write(
            root.join("src/test/java/demo/OrderServiceTest.java"),
            "package demo; class OrderServiceTest { @org.junit.jupiter.api.Test void unit() {} }\n",
        )
        .unwrap();
        let db = root.join("business-extraction.db");
        seed_business_db(&db);
        let fake_jdtls = write_fake_jdtls(&root);

        let options = TestExtractionOptions {
            path: root.clone(),
            database: db.clone(),
            jdtls_command: fake_jdtls.display().to_string(),
            jdtls_workspace: None,
            jdtls_max_in_flight: 32,
        };
        let summary = extract_tests(&options).unwrap();
        assert_eq!(summary.suites, 1);
        assert_eq!(summary.cases, 1);
        assert_eq!(summary.assertions, 1);
        assert!(summary.fixtures >= 1);
        assert!(summary.targets >= 1);

        let second = extract_tests(&options).unwrap();
        assert_eq!(second.suites, 1);
        let connection = Connection::open(&db).unwrap();
        assert_eq!(count(&connection, "test_suites"), 1);
        assert_eq!(count(&connection, "test_cases"), 1);
        assert_eq!(count(&connection, "test_assertions"), 1);
        assert!(count(&connection, "test_targets") >= 1);

        let _ = fs::remove_dir_all(root);
    }

    fn seed_business_db(path: &Path) {
        let connection = Connection::open(path).unwrap();
        connection
            .execute_batch(
                "
                CREATE TABLE classes (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    qualified_name TEXT NOT NULL,
                    file TEXT NOT NULL,
                    start_line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL
                );
                CREATE TABLE methods (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    file TEXT NOT NULL,
                    start_line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL
                );
                INSERT INTO classes (id, name, qualified_name, file, start_line, end_line)
                VALUES ('class:demo.OrderService@src/main/java/demo/OrderService.java:1', 'OrderService', 'demo.OrderService', 'src/main/java/demo/OrderService.java', 1, 1);
                INSERT INTO methods (id, name, file, start_line, end_line)
                VALUES ('method:demo.OrderService#approve()@src/main/java/demo/OrderService.java:1', 'approve', 'src/main/java/demo/OrderService.java', 1, 1);
                ",
            )
            .unwrap();
    }

    fn write_fake_jdtls(root: &Path) -> PathBuf {
        let script = root.join("fake-jdtls.py");
        let target = root
            .join("src/main/java/demo/OrderService.java")
            .display()
            .to_string();
        fs::write(
            &script,
            format!(
                r#"#!/usr/bin/env python3
import json
import sys

target_uri = "file://{target}"

def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode("utf-8").strip()
        if not line:
            break
        if line.lower().startswith("content-length:"):
            length = int(line.split(":", 1)[1].strip())
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def write_message(value):
    body = json.dumps(value).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if "id" not in message:
        if method == "exit":
            break
        continue
    if method == "initialize":
        result = {{"capabilities": {{"definitionProvider": True}}}}
    elif method == "shutdown":
        result = None
    elif method == "textDocument/definition":
        result = [{{"uri": target_uri, "range": {{"start": {{"line": 0, "character": 43}}, "end": {{"line": 0, "character": 50}}}}}}]
    else:
        result = None
    write_message({{"jsonrpc": "2.0", "id": message["id"], "result": result}})
"#,
                target = target.replace('\\', "\\\\").replace('"', "\\\"")
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&script).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).unwrap();
        }
        script
    }

    fn count(connection: &Connection, table: &str) -> usize {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        connection
            .query_row(&sql, [], |row| row.get::<_, i64>(0))
            .unwrap() as usize
    }

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "code-parser-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
