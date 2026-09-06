use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, TreeCursor};
use walkdir::{DirEntry, WalkDir};

use crate::core::error::{FileError, JdtlsError, ParserError, PathError};
use crate::languages::java::build::model::{BuildReport, DependencyInfo, Diagnostic};
use crate::languages::java::business::jdtls::{
    JdtlsOptions, JdtlsSymbolRequest, resolve_compatibility_symbols,
};
use crate::languages::java::business::modules::{module_id_for_file, modules_from_build_report};

const REPORT_FILE: &str = "model-classification-report.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyModelsOptions {
    pub build_report: PathBuf,
    pub output_dir: PathBuf,
    pub source_path: Option<PathBuf>,
    pub jdtls_command: String,
    pub jdtls_workspace: Option<PathBuf>,
    pub jdtls_max_in_flight: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyModelsSummary {
    pub report_path: String,
    pub module_count: usize,
    pub model_count: usize,
    pub dto_count: usize,
    pub request_body_count: usize,
    pub response_body_count: usize,
    pub repository_count: usize,
    pub entity_count: usize,
    pub table_count: usize,
    pub column_count: usize,
    pub diagnostic_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ClassifyModelsError {
    #[error(transparent)]
    Path(#[from] PathError),

    #[error(transparent)]
    File(#[from] FileError),

    #[error(transparent)]
    Parser(#[from] ParserError),

    #[error(transparent)]
    Jdtls(#[from] JdtlsError),
}

#[derive(Debug, Default, Clone, Deserialize)]
struct SymbolRegistry {
    #[serde(default)]
    #[serde(rename = "schema_version")]
    _schema_version: u32,
    #[serde(flatten)]
    groups: BTreeMap<String, RegistryGroup>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct RegistryGroup {
    #[serde(default)]
    symbols: Vec<String>,
    #[serde(default)]
    symbol_prefixes: Vec<String>,
    #[serde(default)]
    important_symbols: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ClassificationReport {
    project_root: String,
    modules: Vec<ModuleReport>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
struct ModuleReport {
    id: String,
    name: String,
    path: String,
    used_dependencies: Vec<UsedDependency>,
    models: Vec<ModelRow>,
    dtos: Vec<ModelRow>,
    request_bodies: Vec<BodyRow>,
    response_bodies: Vec<BodyRow>,
    repositories: Vec<RepositoryRow>,
    entities: Vec<EntityRow>,
    tables: Vec<TableRow>,
    columns: Vec<ColumnRow>,
    children: Vec<ModuleReport>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct UsedDependency {
    group_id: Option<String>,
    artifact_id: String,
    version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ModelRow {
    qualified_name: String,
    kind: String,
    module_id: String,
    file: String,
    start_line: usize,
    end_line: usize,
    classification: String,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
struct BodyRow {
    type_name: String,
    owner: String,
    method: String,
    module_id: String,
    file: String,
    line: usize,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
struct RepositoryRow {
    qualified_name: String,
    module_id: String,
    file: String,
    start_line: usize,
    end_line: usize,
    entity_type: Option<String>,
    query_methods: Vec<String>,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
struct EntityRow {
    qualified_name: String,
    module_id: String,
    file: String,
    start_line: usize,
    end_line: usize,
    table_name: Option<String>,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
struct TableRow {
    entity: String,
    module_id: String,
    file: String,
    line: usize,
    name: String,
    source: String,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
struct ColumnRow {
    entity: String,
    module_id: String,
    file: String,
    line: usize,
    field_name: String,
    java_type: Option<String>,
    column_name: String,
    source: String,
    evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize)]
struct Evidence {
    source: String,
    symbol: String,
    dependency: Option<UsedDependency>,
}

#[derive(Debug, Default, Clone)]
struct SourceModel {
    classes: Vec<SourceClass>,
    java_files: Vec<String>,
    requests: Vec<JdtlsSymbolRequest>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone)]
struct SourceClass {
    module_id: String,
    name: String,
    qualified_name: String,
    kind: String,
    file: String,
    start_line: usize,
    end_line: usize,
    annotations: Vec<SourceSymbol>,
    superclass: Option<String>,
    interfaces: Vec<String>,
    fields: Vec<SourceField>,
    methods: Vec<SourceMethod>,
}

#[derive(Debug, Clone)]
struct SourceField {
    name: String,
    type_name: Option<String>,
    line: usize,
    annotations: Vec<SourceSymbol>,
}

#[derive(Debug, Clone)]
struct SourceMethod {
    name: String,
    return_type: Option<String>,
    line: usize,
    annotations: Vec<SourceSymbol>,
    parameters: Vec<SourceParameter>,
}

#[derive(Debug, Clone)]
struct SourceParameter {
    type_name: Option<String>,
    line: usize,
    annotations: Vec<SourceSymbol>,
}

#[derive(Debug, Clone)]
struct SourceSymbol {
    raw: String,
    simple: String,
    values: Vec<String>,
    line: usize,
    column: usize,
}

struct ClassificationAccumulator {
    report: BTreeMap<String, ModuleReport>,
    used_dependencies: BTreeMap<String, BTreeSet<UsedDependency>>,
    model_keys: BTreeSet<String>,
    dto_keys: BTreeSet<String>,
    request_keys: BTreeSet<String>,
    response_keys: BTreeSet<String>,
    repository_keys: BTreeSet<String>,
    entity_keys: BTreeSet<String>,
    table_keys: BTreeSet<String>,
    column_keys: BTreeSet<String>,
}

pub fn classify_models(
    options: &ClassifyModelsOptions,
) -> Result<ClassifyModelsSummary, ClassifyModelsError> {
    let build_report = read_build_report(&options.build_report)?;
    let source_path = options
        .source_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(&build_report.project_root));
    if !source_path.exists() {
        return Err(PathError::NotFound(source_path).into());
    }
    let project_root = if source_path.is_file() {
        source_path
            .parent()
            .ok_or_else(|| PathError::NoParent(source_path.clone()))?
            .to_path_buf()
    } else {
        source_path
    };
    let registry = load_symbol_registry()?;
    let modules = modules_from_build_report(&build_report);
    let mut source_model =
        extract_source_model(&project_root, &modules, &registry).map_err(ParserError::Operation)?;
    let workspace = options.jdtls_workspace.clone().unwrap_or_else(|| {
        output_project_dir(&project_root, &options.output_dir).join(".jdtls-workspace")
    });
    let resolved = resolve_compatibility_symbols(
        &project_root,
        &JdtlsOptions {
            command: options.jdtls_command.clone(),
            workspace,
            max_in_flight: options.jdtls_max_in_flight,
        },
        &source_model.java_files,
        &source_model.requests,
    )
    .map_err(JdtlsError::Operation)?;
    let resolved_symbols = resolved
        .into_iter()
        .map(|symbol| ((symbol.file, symbol.line), symbol.values))
        .collect::<BTreeMap<_, _>>();
    apply_jdtls_symbols(&mut source_model, &resolved_symbols);

    let report = build_classification_report(
        &project_root,
        &build_report,
        &modules,
        &registry,
        source_model,
    );
    let summary = summary_for_report(&report, &project_root, &options.output_dir)?;
    let json = serde_json::to_string_pretty(&report).map_err(|error| {
        ParserError::Operation(format!(
            "failed to serialize classification report: {error}"
        ))
    })?;
    fs::create_dir_all(
        Path::new(&summary.report_path)
            .parent()
            .unwrap_or_else(|| Path::new(".")),
    )
    .map_err(|source| FileError::CreateDir {
        path: Path::new(&summary.report_path)
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        source,
    })?;
    fs::write(&summary.report_path, json).map_err(|source| FileError::Write {
        path: PathBuf::from(&summary.report_path),
        source,
    })?;
    Ok(summary)
}

fn read_build_report(path: &Path) -> Result<BuildReport, ClassifyModelsError> {
    let data = fs::read_to_string(path).map_err(|source| FileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut report: BuildReport = serde_json::from_str(&data).map_err(|error| {
        ParserError::Operation(format!(
            "failed to parse build report {}: {error}",
            path.display()
        ))
    })?;
    if report.build_tools.is_empty()
        && report.java_versions.is_empty()
        && report.direct_dependencies.is_empty()
        && report.direct_plugins.is_empty()
    {
        report.rebuild_flat_inventory();
    }
    Ok(report)
}

fn load_symbol_registry() -> Result<SymbolRegistry, ClassifyModelsError> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/java/symbol-registry.yaml");
    let data = fs::read_to_string(&path).map_err(|source| FileError::Read { path, source })?;
    serde_yaml::from_str(&data).map_err(|error| {
        ParserError::Operation(format!("failed to parse symbol registry: {error}")).into()
    })
}

fn extract_source_model(
    project_root: &Path,
    modules: &[crate::languages::business::model::ModuleInfo],
    registry: &SymbolRegistry,
) -> Result<SourceModel, String> {
    let mut source_model = SourceModel::default();
    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                source_model.diagnostics.push(Diagnostic::warning(
                    "classify_models",
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
        let file = relative_path(project_root, entry.path());
        let contents = match fs::read_to_string(entry.path()) {
            Ok(contents) => contents,
            Err(error) => {
                source_model.diagnostics.push(Diagnostic::warning(
                    "classify_models",
                    format!("failed to read {}: {error}", entry.path().display()),
                    Some(file),
                ));
                continue;
            }
        };
        source_model.java_files.push(file.clone());
        parse_java_file(&file, &contents, modules, registry, &mut source_model)?;
    }
    source_model.java_files.sort();
    source_model.java_files.dedup();
    Ok(source_model)
}

fn parse_java_file(
    file: &str,
    contents: &str,
    modules: &[crate::languages::business::model::ModuleInfo],
    registry: &SymbolRegistry,
    source_model: &mut SourceModel,
) -> Result<(), String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .map_err(|error| format!("failed to initialize Java parser: {error}"))?;
    let Some(tree) = parser.parse(contents, None) else {
        return Err(format!("parser returned no syntax tree for {file}"));
    };
    if tree.root_node().has_error() {
        source_model.diagnostics.push(Diagnostic::warning(
            "classify_models",
            format!("failed to parse Java source {file}"),
            Some(file.to_string()),
        ));
        return Ok(());
    }
    let package_name = package_name(tree.root_node(), contents);
    let imports = imports(tree.root_node(), contents);
    let mut cursor = tree.walk();
    collect_classes(
        contents,
        file,
        &module_id_for_file(file, modules),
        package_name.as_deref(),
        &imports,
        registry,
        &mut cursor,
        source_model,
    );
    Ok(())
}

fn collect_classes(
    contents: &str,
    file: &str,
    module_id: &str,
    package_name: Option<&str>,
    imports: &[String],
    registry: &SymbolRegistry,
    cursor: &mut TreeCursor<'_>,
    source_model: &mut SourceModel,
) {
    let node = cursor.node();
    if is_class_node(node.kind()) {
        let class = source_class(
            node,
            contents,
            file,
            module_id,
            package_name,
            imports,
            registry,
            source_model,
        );
        source_model.classes.push(class);
        return;
    }
    if cursor.goto_first_child() {
        loop {
            collect_classes(
                contents,
                file,
                module_id,
                package_name,
                imports,
                registry,
                cursor,
                source_model,
            );
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn source_class(
    node: Node<'_>,
    contents: &str,
    file: &str,
    module_id: &str,
    package_name: Option<&str>,
    imports: &[String],
    registry: &SymbolRegistry,
    source_model: &mut SourceModel,
) -> SourceClass {
    let name = node_text(child_by_field(node, "name"), contents).unwrap_or("Anonymous");
    let qualified_name = match package_name {
        Some(package_name) if !package_name.is_empty() => format!("{package_name}.{name}"),
        _ => name.to_string(),
    };
    let annotations = annotations(node, contents, file, imports, registry, source_model);
    let superclass =
        child_by_field(node, "superclass").and_then(|child| first_type_text(child, contents));
    let interfaces = child_by_field(node, "interfaces")
        .map(|child| type_texts(child, contents))
        .unwrap_or_default();
    let mut fields = Vec::new();
    let mut methods = Vec::new();
    collect_members(
        node,
        contents,
        file,
        imports,
        registry,
        source_model,
        &mut fields,
        &mut methods,
    );
    SourceClass {
        module_id: module_id.to_string(),
        name: name.to_string(),
        qualified_name,
        kind: node.kind().trim_end_matches("_declaration").to_string(),
        file: file.to_string(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        annotations,
        superclass,
        interfaces,
        fields,
        methods,
    }
}

fn collect_members(
    node: Node<'_>,
    contents: &str,
    file: &str,
    imports: &[String],
    registry: &SymbolRegistry,
    source_model: &mut SourceModel,
    fields: &mut Vec<SourceField>,
    methods: &mut Vec<SourceMethod>,
) {
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        let child = cursor.node();
        match child.kind() {
            "field_declaration" => {
                let annotations =
                    annotations(child, contents, file, imports, registry, source_model);
                let type_name = child_by_field(child, "type")
                    .and_then(|type_node| node_text(Some(type_node), contents).map(str::to_string));
                let names = variable_names(child, contents);
                for name in names {
                    fields.push(SourceField {
                        name,
                        type_name: type_name.clone(),
                        line: child.start_position().row + 1,
                        annotations: annotations.clone(),
                    });
                }
            }
            "method_declaration" | "constructor_declaration" => methods.push(method(
                child,
                contents,
                file,
                imports,
                registry,
                source_model,
            )),
            _ => collect_members(
                child,
                contents,
                file,
                imports,
                registry,
                source_model,
                fields,
                methods,
            ),
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    cursor.goto_parent();
}

fn method(
    node: Node<'_>,
    contents: &str,
    file: &str,
    imports: &[String],
    registry: &SymbolRegistry,
    source_model: &mut SourceModel,
) -> SourceMethod {
    let name = node_text(child_by_field(node, "name"), contents)
        .unwrap_or("<anonymous>")
        .to_string();
    let return_type = child_by_field(node, "type")
        .and_then(|type_node| node_text(Some(type_node), contents).map(str::to_string));
    let parameters = child_by_field(node, "parameters")
        .map(|params| parameters(params, contents, file, imports, registry, source_model))
        .unwrap_or_default();
    SourceMethod {
        name,
        return_type,
        line: node.start_position().row + 1,
        annotations: annotations(node, contents, file, imports, registry, source_model),
        parameters,
    }
}

fn parameters(
    node: Node<'_>,
    contents: &str,
    file: &str,
    imports: &[String],
    registry: &SymbolRegistry,
    source_model: &mut SourceModel,
) -> Vec<SourceParameter> {
    let mut params = Vec::new();
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return params;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
            let type_name = child_by_field(child, "type")
                .and_then(|type_node| node_text(Some(type_node), contents).map(str::to_string));
            params.push(SourceParameter {
                type_name,
                line: child.start_position().row + 1,
                annotations: annotations(child, contents, file, imports, registry, source_model),
            });
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    params
}

fn annotations(
    node: Node<'_>,
    contents: &str,
    file: &str,
    imports: &[String],
    registry: &SymbolRegistry,
    source_model: &mut SourceModel,
) -> Vec<SourceSymbol> {
    let mut raw = Vec::new();
    let mut cursor = node.walk();
    collect_annotations(contents, &mut cursor, &mut raw);
    raw.sort();
    raw.dedup();
    raw.into_iter()
        .map(|annotation| {
            let simple = annotation_name(&annotation);
            let values = candidate_values(&simple, imports, registry);
            let symbol = SourceSymbol {
                raw: annotation,
                simple,
                values,
                line: node.start_position().row + 1,
                column: node.start_position().column,
            };
            source_model.requests.push(JdtlsSymbolRequest {
                file: file.to_string(),
                name: symbol.simple.clone(),
                line: symbol.line,
                column: symbol.column,
                values: symbol.values.clone(),
            });
            symbol
        })
        .collect()
}

fn apply_jdtls_symbols(
    source_model: &mut SourceModel,
    resolved: &BTreeMap<(String, usize), Vec<String>>,
) {
    for class in &mut source_model.classes {
        apply_symbols(&mut class.annotations, resolved, &class.file);
        for field in &mut class.fields {
            apply_symbols(&mut field.annotations, resolved, &class.file);
        }
        for method in &mut class.methods {
            apply_symbols(&mut method.annotations, resolved, &class.file);
            for parameter in &mut method.parameters {
                apply_symbols(&mut parameter.annotations, resolved, &class.file);
            }
        }
    }
}

fn apply_symbols(
    symbols: &mut [SourceSymbol],
    resolved: &BTreeMap<(String, usize), Vec<String>>,
    file: &str,
) {
    for symbol in symbols {
        if let Some(values) = resolved.get(&(file.to_string(), symbol.line)) {
            symbol.values.extend(values.clone());
            symbol.values.sort();
            symbol.values.dedup();
        }
    }
}

fn build_classification_report(
    project_root: &Path,
    build_report: &BuildReport,
    modules: &[crate::languages::business::model::ModuleInfo],
    registry: &SymbolRegistry,
    source_model: SourceModel,
) -> ClassificationReport {
    let mut accumulator = ClassificationAccumulator::new(modules);
    let dependencies_by_module = dependencies_by_module(build_report);
    let class_index = source_model
        .classes
        .iter()
        .flat_map(|class| {
            [
                (class.name.clone(), class),
                (class.qualified_name.clone(), class),
            ]
        })
        .collect::<BTreeMap<_, _>>();
    for class in &source_model.classes {
        classify_class(
            class,
            registry,
            &dependencies_by_module,
            &class_index,
            &mut accumulator,
        );
    }
    let mut report = ClassificationReport {
        project_root: project_root.display().to_string(),
        modules: accumulator.into_nested_modules(),
        diagnostics: source_model.diagnostics,
    };
    sort_report(&mut report);
    report
}

fn classify_class(
    class: &SourceClass,
    registry: &SymbolRegistry,
    dependencies_by_module: &BTreeMap<String, Vec<DependencyInfo>>,
    class_index: &BTreeMap<String, &SourceClass>,
    accumulator: &mut ClassificationAccumulator,
) {
    let entity_evidence = evidence_for_symbols(
        &class.annotations,
        "persistence",
        registry,
        class,
        dependencies_by_module,
    );
    let is_entity = has_annotation(class, &["Entity", "Embeddable", "MappedSuperclass"]);
    let repository_evidence = repository_evidence(class, registry, dependencies_by_module);
    let is_repository = !repository_evidence.is_empty();

    if is_entity {
        let table_name = table_name(class);
        accumulator.add_entity(EntityRow {
            qualified_name: class.qualified_name.clone(),
            module_id: class.module_id.clone(),
            file: class.file.clone(),
            start_line: class.start_line,
            end_line: class.end_line,
            table_name: table_name.clone(),
            evidence: entity_evidence.clone(),
        });
        accumulator.add_table(TableRow {
            entity: class.qualified_name.clone(),
            module_id: class.module_id.clone(),
            file: class.file.clone(),
            line: class.start_line,
            name: table_name.unwrap_or_else(|| class.name.clone()),
            source: if has_annotation(class, &["Table"]) {
                "annotation".to_string()
            } else {
                "class_name".to_string()
            },
            evidence: entity_evidence.clone(),
        });
        for field in &class.fields {
            if let Some(column) = column_row(class, field, registry, dependencies_by_module) {
                accumulator.add_column(column);
            }
        }
    }

    if is_repository {
        accumulator.add_repository(RepositoryRow {
            qualified_name: class.qualified_name.clone(),
            module_id: class.module_id.clone(),
            file: class.file.clone(),
            start_line: class.start_line,
            end_line: class.end_line,
            entity_type: repository_entity_type(class),
            query_methods: repository_query_methods(class),
            evidence: repository_evidence,
        });
    }

    for method in &class.methods {
        let method_is_endpoint = is_http_endpoint(class, method);
        for parameter in &method.parameters {
            if has_parameter_annotation(parameter, &["RequestBody"]) {
                if let Some(type_name) = &parameter.type_name {
                    let evidence = evidence_for_symbols(
                        &parameter.annotations,
                        "spring_mvc",
                        registry,
                        class,
                        dependencies_by_module,
                    );
                    accumulator.add_request_body(BodyRow {
                        type_name: type_name.clone(),
                        owner: class.qualified_name.clone(),
                        method: method.name.clone(),
                        module_id: class.module_id.clone(),
                        file: class.file.clone(),
                        line: parameter.line,
                        evidence: evidence.clone(),
                    });
                    accumulator.add_dto(dto_row(
                        type_name,
                        class,
                        class_index,
                        "request_body",
                        evidence,
                    ));
                }
            } else if method_is_endpoint && !is_framework_parameter(parameter) {
                if let Some(type_name) = &parameter.type_name {
                    accumulator.add_request_body(BodyRow {
                        type_name: type_name.clone(),
                        owner: class.qualified_name.clone(),
                        method: method.name.clone(),
                        module_id: class.module_id.clone(),
                        file: class.file.clone(),
                        line: parameter.line,
                        evidence: Vec::new(),
                    });
                    accumulator.add_dto(dto_row(
                        type_name,
                        class,
                        class_index,
                        "request_body",
                        Vec::new(),
                    ));
                }
            }
        }
        if method_is_endpoint {
            if let Some(type_name) = response_type(method) {
                let evidence = evidence_for_symbols(
                    &method.annotations,
                    "spring_mvc",
                    registry,
                    class,
                    dependencies_by_module,
                );
                accumulator.add_response_body(BodyRow {
                    type_name: type_name.clone(),
                    owner: class.qualified_name.clone(),
                    method: method.name.clone(),
                    module_id: class.module_id.clone(),
                    file: class.file.clone(),
                    line: method.line,
                    evidence: evidence.clone(),
                });
                accumulator.add_dto(dto_row(
                    &type_name,
                    class,
                    class_index,
                    "response_body",
                    evidence,
                ));
            }
        }
    }

    if is_entity || is_repository {
        accumulator.add_model(ModelRow {
            qualified_name: class.qualified_name.clone(),
            kind: class.kind.clone(),
            module_id: class.module_id.clone(),
            file: class.file.clone(),
            start_line: class.start_line,
            end_line: class.end_line,
            classification: if is_entity {
                "entity".to_string()
            } else if is_repository {
                "repository".to_string()
            } else {
                "model".to_string()
            },
            evidence: entity_evidence,
        });
    }
}

fn dependencies_by_module(build_report: &BuildReport) -> BTreeMap<String, Vec<DependencyInfo>> {
    let mut result = BTreeMap::new();
    result.insert(
        "module:.".to_string(),
        build_report.parent.direct_dependencies.clone(),
    );
    for module in &build_report.modules {
        let path = if module.path.is_empty() {
            "."
        } else {
            &module.path
        };
        result.insert(format!("module:{path}"), module.direct_dependencies.clone());
    }
    result
}

fn evidence_for_symbols(
    symbols: &[SourceSymbol],
    group_hint: &str,
    registry: &SymbolRegistry,
    class: &SourceClass,
    dependencies_by_module: &BTreeMap<String, Vec<DependencyInfo>>,
) -> Vec<Evidence> {
    let mut evidence = Vec::new();
    for symbol in symbols {
        for value in &symbol.values {
            if registry.matches_group(group_hint, value) || registry.matches_any(value) {
                evidence.push(Evidence {
                    source: "tree_sitter_jdtls".to_string(),
                    symbol: value.clone(),
                    dependency: dependency_for_symbol(value, class, dependencies_by_module),
                });
            }
        }
    }
    dedupe_evidence(evidence)
}

fn repository_evidence(
    class: &SourceClass,
    registry: &SymbolRegistry,
    dependencies_by_module: &BTreeMap<String, Vec<DependencyInfo>>,
) -> Vec<Evidence> {
    let mut evidence = Vec::new();
    for interface in class.interfaces.iter().chain(class.superclass.iter()) {
        if is_repository_type(interface) {
            evidence.push(Evidence {
                source: "tree_sitter".to_string(),
                symbol: interface.clone(),
                dependency: dependency_for_group("spring_data_jpa", class, dependencies_by_module)
                    .or_else(|| dependency_for_group("spring_data", class, dependencies_by_module)),
            });
        }
    }
    if class.methods.iter().any(|method| {
        method
            .annotations
            .iter()
            .any(|annotation| annotation.simple == "Query")
    }) {
        evidence.extend(evidence_for_symbols(
            &class
                .methods
                .iter()
                .flat_map(|method| method.annotations.clone())
                .collect::<Vec<_>>(),
            "spring_data_jpa",
            registry,
            class,
            dependencies_by_module,
        ));
    }
    if class.name.ends_with("Repository")
        && class.fields.iter().any(|field| {
            field.type_name.as_deref().is_some_and(|value| {
                value.contains("EntityManager") || value.contains("Connection")
            })
        })
    {
        evidence.push(Evidence {
            source: "tree_sitter".to_string(),
            symbol: "custom_repository".to_string(),
            dependency: dependency_for_group("jakarta_persistence", class, dependencies_by_module)
                .or_else(|| dependency_for_group("jdbc", class, dependencies_by_module)),
        });
    }
    dedupe_evidence(evidence)
}

fn column_row(
    class: &SourceClass,
    field: &SourceField,
    registry: &SymbolRegistry,
    dependencies_by_module: &BTreeMap<String, Vec<DependencyInfo>>,
) -> Option<ColumnRow> {
    if field.annotations.is_empty() {
        return None;
    }
    let persistence_annotations = [
        "Column",
        "Id",
        "Embedded",
        "EmbeddedId",
        "OneToOne",
        "OneToMany",
        "ManyToOne",
        "ManyToMany",
        "JoinColumn",
    ];
    if !field
        .annotations
        .iter()
        .any(|annotation| persistence_annotations.contains(&annotation.simple.as_str()))
    {
        return None;
    }
    Some(ColumnRow {
        entity: class.qualified_name.clone(),
        module_id: class.module_id.clone(),
        file: class.file.clone(),
        line: field.line,
        field_name: field.name.clone(),
        java_type: field.type_name.clone(),
        column_name: field
            .annotations
            .iter()
            .find_map(|annotation| annotation_name_value(&annotation.raw))
            .unwrap_or_else(|| field.name.clone()),
        source: "annotation".to_string(),
        evidence: evidence_for_symbols(
            &field.annotations,
            "persistence",
            registry,
            class,
            dependencies_by_module,
        ),
    })
}

fn dto_row(
    type_name: &str,
    class: &SourceClass,
    class_index: &BTreeMap<String, &SourceClass>,
    classification: &str,
    evidence: Vec<Evidence>,
) -> ModelRow {
    let type_name = unwrap_generic(type_name);
    let source_class = class_index.get(type_name).copied();
    ModelRow {
        qualified_name: source_class
            .map(|source_class| source_class.qualified_name.clone())
            .unwrap_or_else(|| type_name.to_string()),
        kind: source_class
            .map(|source_class| source_class.kind.clone())
            .unwrap_or_else(|| "class_or_record".to_string()),
        module_id: source_class
            .map(|source_class| source_class.module_id.clone())
            .unwrap_or_else(|| class.module_id.clone()),
        file: source_class
            .map(|source_class| source_class.file.clone())
            .unwrap_or_else(|| class.file.clone()),
        start_line: source_class
            .map(|source_class| source_class.start_line)
            .unwrap_or(class.start_line),
        end_line: source_class
            .map(|source_class| source_class.end_line)
            .unwrap_or(class.end_line),
        classification: classification.to_string(),
        evidence,
    }
}

impl SymbolRegistry {
    fn matches_group(&self, group: &str, value: &str) -> bool {
        self.groups
            .get(group)
            .is_some_and(|registry_group| registry_group.matches(value))
            || (group == "persistence"
                && (self.matches_group("jakarta_persistence", value)
                    || self.matches_group("javax_persistence", value)
                    || self.matches_group("hibernate", value)))
    }

    fn matches_any(&self, value: &str) -> bool {
        self.groups.values().any(|group| group.matches(value))
    }
}

impl RegistryGroup {
    fn matches(&self, value: &str) -> bool {
        self.symbols.iter().any(|symbol| symbol == value)
            || self.important_symbols.iter().any(|symbol| symbol == value)
            || self
                .symbol_prefixes
                .iter()
                .any(|prefix| value.starts_with(prefix))
    }
}

impl ClassificationAccumulator {
    fn new(modules: &[crate::languages::business::model::ModuleInfo]) -> Self {
        let report = modules
            .iter()
            .map(|module| {
                (
                    module.id.clone(),
                    ModuleReport {
                        id: module.id.clone(),
                        name: module.name.clone(),
                        path: module.path.clone(),
                        used_dependencies: Vec::new(),
                        models: Vec::new(),
                        dtos: Vec::new(),
                        request_bodies: Vec::new(),
                        response_bodies: Vec::new(),
                        repositories: Vec::new(),
                        entities: Vec::new(),
                        tables: Vec::new(),
                        columns: Vec::new(),
                        children: Vec::new(),
                    },
                )
            })
            .collect();
        Self {
            report,
            used_dependencies: BTreeMap::new(),
            model_keys: BTreeSet::new(),
            dto_keys: BTreeSet::new(),
            request_keys: BTreeSet::new(),
            response_keys: BTreeSet::new(),
            repository_keys: BTreeSet::new(),
            entity_keys: BTreeSet::new(),
            table_keys: BTreeSet::new(),
            column_keys: BTreeSet::new(),
        }
    }

    fn add_model(&mut self, row: ModelRow) {
        if self.model_keys.insert(row.qualified_name.clone()) {
            self.add_dependencies(&row.module_id, &row.evidence);
            self.report
                .get_mut(&row.module_id)
                .unwrap()
                .models
                .push(row);
        }
    }

    fn add_dto(&mut self, row: ModelRow) {
        if self
            .dto_keys
            .insert(format!("{}\0{}", row.module_id, row.qualified_name))
        {
            self.add_dependencies(&row.module_id, &row.evidence);
            self.add_model(row.clone());
            self.report.get_mut(&row.module_id).unwrap().dtos.push(row);
        }
    }

    fn add_request_body(&mut self, row: BodyRow) {
        if self
            .request_keys
            .insert(format!("{}\0{}\0{}", row.owner, row.method, row.type_name))
        {
            self.add_dependencies(&row.module_id, &row.evidence);
            self.report
                .get_mut(&row.module_id)
                .unwrap()
                .request_bodies
                .push(row);
        }
    }

    fn add_response_body(&mut self, row: BodyRow) {
        if self
            .response_keys
            .insert(format!("{}\0{}\0{}", row.owner, row.method, row.type_name))
        {
            self.add_dependencies(&row.module_id, &row.evidence);
            self.report
                .get_mut(&row.module_id)
                .unwrap()
                .response_bodies
                .push(row);
        }
    }

    fn add_repository(&mut self, row: RepositoryRow) {
        if self.repository_keys.insert(row.qualified_name.clone()) {
            self.add_dependencies(&row.module_id, &row.evidence);
            self.report
                .get_mut(&row.module_id)
                .unwrap()
                .repositories
                .push(row);
        }
    }

    fn add_entity(&mut self, row: EntityRow) {
        if self.entity_keys.insert(row.qualified_name.clone()) {
            self.add_dependencies(&row.module_id, &row.evidence);
            self.report
                .get_mut(&row.module_id)
                .unwrap()
                .entities
                .push(row);
        }
    }

    fn add_table(&mut self, row: TableRow) {
        if self
            .table_keys
            .insert(format!("{}\0{}", row.entity, row.name))
        {
            self.add_dependencies(&row.module_id, &row.evidence);
            self.report
                .get_mut(&row.module_id)
                .unwrap()
                .tables
                .push(row);
        }
    }

    fn add_column(&mut self, row: ColumnRow) {
        if self
            .column_keys
            .insert(format!("{}\0{}", row.entity, row.field_name))
        {
            self.add_dependencies(&row.module_id, &row.evidence);
            self.report
                .get_mut(&row.module_id)
                .unwrap()
                .columns
                .push(row);
        }
    }

    fn add_dependencies(&mut self, module_id: &str, evidence: &[Evidence]) {
        for dependency in evidence
            .iter()
            .filter_map(|evidence| evidence.dependency.clone())
        {
            self.used_dependencies
                .entry(module_id.to_string())
                .or_default()
                .insert(dependency);
        }
    }

    fn into_nested_modules(mut self) -> Vec<ModuleReport> {
        for (module_id, dependencies) in self.used_dependencies {
            if let Some(module) = self.report.get_mut(&module_id) {
                module.used_dependencies = dependencies.into_iter().collect();
            }
        }
        let ids = self.report.keys().cloned().collect::<Vec<_>>();
        let mut children_by_parent: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for id in &ids {
            if id == "module:." {
                continue;
            }
            let parent = parent_id_for_module(id, &ids);
            children_by_parent
                .entry(parent)
                .or_default()
                .push(id.clone());
        }
        build_nested("module:.", &mut self.report, &children_by_parent)
            .map(|root| vec![root])
            .unwrap_or_default()
    }
}

fn build_nested(
    module_id: &str,
    reports: &mut BTreeMap<String, ModuleReport>,
    children_by_parent: &BTreeMap<String, Vec<String>>,
) -> Option<ModuleReport> {
    let mut module = reports.remove(module_id)?;
    if let Some(children) = children_by_parent.get(module_id) {
        for child_id in children {
            if let Some(child) = build_nested(child_id, reports, children_by_parent) {
                module.children.push(child);
            }
        }
    }
    Some(module)
}

fn parent_id_for_module(module_id: &str, ids: &[String]) -> String {
    let path = module_id.trim_start_matches("module:");
    ids.iter()
        .filter(|candidate| candidate.as_str() != module_id)
        .filter(|candidate| {
            let candidate_path = candidate.trim_start_matches("module:");
            candidate_path == "." || path.starts_with(&format!("{candidate_path}/"))
        })
        .max_by_key(|candidate| candidate.trim_start_matches("module:").len())
        .cloned()
        .unwrap_or_else(|| "module:.".to_string())
}

fn sort_report(report: &mut ClassificationReport) {
    for module in &mut report.modules {
        sort_module(module);
    }
}

fn sort_module(module: &mut ModuleReport) {
    module
        .models
        .sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
    module
        .dtos
        .sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
    module
        .request_bodies
        .sort_by(|left, right| left.type_name.cmp(&right.type_name));
    module
        .response_bodies
        .sort_by(|left, right| left.type_name.cmp(&right.type_name));
    module
        .repositories
        .sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
    module
        .entities
        .sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
    module
        .tables
        .sort_by(|left, right| left.name.cmp(&right.name));
    module
        .columns
        .sort_by(|left, right| left.field_name.cmp(&right.field_name));
    for child in &mut module.children {
        sort_module(child);
    }
}

fn summary_for_report(
    report: &ClassificationReport,
    project_root: &Path,
    output_dir: &Path,
) -> Result<ClassifyModelsSummary, ClassifyModelsError> {
    let report_path = output_project_dir(project_root, output_dir).join(REPORT_FILE);
    let mut summary = ClassifyModelsSummary {
        report_path: report_path.display().to_string(),
        module_count: 0,
        model_count: 0,
        dto_count: 0,
        request_body_count: 0,
        response_body_count: 0,
        repository_count: 0,
        entity_count: 0,
        table_count: 0,
        column_count: 0,
        diagnostic_count: report.diagnostics.len(),
    };
    for module in &report.modules {
        count_module(module, &mut summary);
    }
    Ok(summary)
}

fn count_module(module: &ModuleReport, summary: &mut ClassifyModelsSummary) {
    summary.module_count += 1;
    summary.model_count += module.models.len();
    summary.dto_count += module.dtos.len();
    summary.request_body_count += module.request_bodies.len();
    summary.response_body_count += module.response_bodies.len();
    summary.repository_count += module.repositories.len();
    summary.entity_count += module.entities.len();
    summary.table_count += module.tables.len();
    summary.column_count += module.columns.len();
    for child in &module.children {
        count_module(child, summary);
    }
}

fn output_project_dir(project_root: &Path, output_dir: &Path) -> PathBuf {
    output_dir.join(
        project_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("project"),
    )
}

fn dependency_for_symbol(
    symbol: &str,
    class: &SourceClass,
    dependencies_by_module: &BTreeMap<String, Vec<DependencyInfo>>,
) -> Option<UsedDependency> {
    let group = if symbol.starts_with("org.springframework.web")
        || symbol.starts_with("org.springframework.http")
    {
        "spring_mvc"
    } else if symbol.starts_with("org.springframework.data.jpa") {
        "spring_data_jpa"
    } else if symbol.starts_with("org.springframework.data") {
        "spring_data"
    } else if symbol.starts_with("jakarta.persistence") {
        "jakarta_persistence"
    } else if symbol.starts_with("javax.persistence") {
        "javax_persistence"
    } else if symbol.starts_with("org.hibernate") {
        "hibernate"
    } else if symbol.starts_with("jakarta.ws.rs") || symbol.starts_with("javax.ws.rs") {
        "jakarta_rest"
    } else if symbol.starts_with("jakarta.servlet") || symbol.starts_with("javax.servlet") {
        "jakarta_servlet"
    } else if symbol.starts_with("java.sql") || symbol.starts_with("javax.sql") {
        "jdbc"
    } else {
        return None;
    };
    dependency_for_group(group, class, dependencies_by_module)
}

fn dependency_for_group(
    group: &str,
    class: &SourceClass,
    dependencies_by_module: &BTreeMap<String, Vec<DependencyInfo>>,
) -> Option<UsedDependency> {
    dependencies_by_module
        .get(&class.module_id)
        .into_iter()
        .chain(dependencies_by_module.get("module:."))
        .flat_map(|dependencies| dependencies.iter())
        .find(|dependency| dependency_matches_group(dependency, group))
        .map(|dependency| UsedDependency {
            group_id: dependency.group_id.clone(),
            artifact_id: dependency.artifact_id.clone(),
            version: dependency.version.clone(),
        })
}

fn dependency_matches_group(dependency: &DependencyInfo, group: &str) -> bool {
    let group_id = dependency.group_id.as_deref().unwrap_or("");
    let artifact = dependency.artifact_id.as_str();
    match group {
        "spring_mvc" => group_id == "org.springframework" || artifact == "spring-boot-starter-web",
        "spring_data" => group_id == "org.springframework.data",
        "spring_data_jpa" => {
            artifact == "spring-data-jpa" || artifact == "spring-boot-starter-data-jpa"
        }
        "jakarta_persistence" => artifact == "jakarta.persistence-api",
        "javax_persistence" => artifact == "javax.persistence-api",
        "hibernate" => group_id == "org.hibernate" || group_id == "org.hibernate.orm",
        "jakarta_rest" => {
            artifact.ends_with("ws.rs-api")
                || group_id.contains("jersey")
                || group_id.contains("resteasy")
        }
        "jakarta_servlet" => artifact.ends_with("servlet-api"),
        "jdbc" => false,
        _ => false,
    }
}

fn has_annotation(class: &SourceClass, names: &[&str]) -> bool {
    class
        .annotations
        .iter()
        .any(|annotation| names.contains(&annotation.simple.as_str()))
}

fn has_parameter_annotation(parameter: &SourceParameter, names: &[&str]) -> bool {
    parameter
        .annotations
        .iter()
        .any(|annotation| names.contains(&annotation.simple.as_str()))
}

fn is_http_endpoint(class: &SourceClass, method: &SourceMethod) -> bool {
    let class_endpoint = has_annotation(class, &["RestController", "Controller", "Path"]);
    let method_endpoint = method.annotations.iter().any(|annotation| {
        matches!(
            annotation.simple.as_str(),
            "RequestMapping"
                | "GetMapping"
                | "PostMapping"
                | "PutMapping"
                | "PatchMapping"
                | "DeleteMapping"
                | "Path"
                | "GET"
                | "POST"
                | "PUT"
                | "PATCH"
                | "DELETE"
        )
    });
    class_endpoint && method_endpoint
}

fn is_framework_parameter(parameter: &SourceParameter) -> bool {
    parameter.type_name.as_deref().is_some_and(|type_name| {
        type_name.contains("HttpServletRequest")
            || type_name.contains("HttpServletResponse")
            || type_name.contains("HttpExchange")
            || type_name.contains("UriInfo")
            || type_name.contains("HttpHeaders")
    })
}

fn response_type(method: &SourceMethod) -> Option<String> {
    let return_type = method.return_type.as_ref()?;
    if return_type == "void" || return_type == "Void" {
        return None;
    }
    if return_type.contains("ResponseEntity") || return_type.contains("HttpEntity") {
        return generic_argument(return_type).map(str::to_string);
    }
    if return_type.ends_with("Response") {
        return Some(return_type.clone());
    }
    Some(return_type.clone())
}

fn is_repository_type(value: &str) -> bool {
    [
        "Repository",
        "CrudRepository",
        "PagingAndSortingRepository",
        "JpaRepository",
    ]
    .iter()
    .any(|name| value.contains(name))
}

fn repository_entity_type(class: &SourceClass) -> Option<String> {
    class.interfaces.iter().find_map(|interface| {
        if !is_repository_type(interface) {
            return None;
        }
        generic_argument(interface).map(str::to_string)
    })
}

fn repository_query_methods(class: &SourceClass) -> Vec<String> {
    let mut methods = class
        .methods
        .iter()
        .filter(|method| {
            method
                .annotations
                .iter()
                .any(|annotation| annotation.simple == "Query")
                || method.name.starts_with("find")
                || method.name.starts_with("read")
                || method.name.starts_with("get")
                || method.name.starts_with("count")
                || method.name.starts_with("exists")
                || method.name.starts_with("delete")
        })
        .map(|method| method.name.clone())
        .collect::<Vec<_>>();
    methods.sort();
    methods.dedup();
    methods
}

fn table_name(class: &SourceClass) -> Option<String> {
    class
        .annotations
        .iter()
        .find(|annotation| annotation.simple == "Table")
        .and_then(|annotation| annotation_name_value(&annotation.raw))
}

fn annotation_name_value(annotation: &str) -> Option<String> {
    let regex = Regex::new(r#"name\s*=\s*"([^"]+)""#).expect("valid annotation regex");
    regex
        .captures(annotation)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
        .or_else(|| {
            let regex = Regex::new(r#"\(\s*"([^"]+)""#).expect("valid annotation regex");
            regex
                .captures(annotation)
                .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
        })
}

fn dedupe_evidence(evidence: Vec<Evidence>) -> Vec<Evidence> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for item in evidence {
        if seen.insert(format!("{}\0{}", item.source, item.symbol)) {
            result.push(item);
        }
    }
    result
}

fn candidate_values(simple: &str, imports: &[String], registry: &SymbolRegistry) -> Vec<String> {
    let mut values = BTreeSet::new();
    values.insert(simple.to_string());
    for import in imports {
        if import.ends_with(&format!(".{simple}")) {
            values.insert(import.clone());
        } else if import.ends_with(".*") {
            values.insert(format!("{}.{simple}", import.trim_end_matches(".*")));
        }
    }
    for group in registry.groups.values() {
        for symbol in group.symbols.iter().chain(group.important_symbols.iter()) {
            if symbol.rsplit('.').next() == Some(simple) {
                values.insert(symbol.clone());
            }
        }
    }
    values.into_iter().collect()
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

fn imports(root: Node<'_>, contents: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut cursor = root.walk();
    if !cursor.goto_first_child() {
        return imports;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "import_declaration"
            && let Some(text) = node_text(Some(child), contents)
        {
            let imported = text
                .trim()
                .trim_start_matches("import")
                .trim()
                .trim_start_matches("static")
                .trim()
                .trim_end_matches(';')
                .trim();
            imports.push(imported.to_string());
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    imports.sort();
    imports.dedup();
    imports
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

fn variable_names(node: Node<'_>, contents: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    collect_variable_names(contents, &mut cursor, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_variable_names(contents: &str, cursor: &mut TreeCursor<'_>, names: &mut Vec<String>) {
    let node = cursor.node();
    if node.kind() == "variable_declarator" {
        if let Some(name) =
            child_by_field(node, "name").and_then(|name| node_text(Some(name), contents))
        {
            names.push(name.to_string());
        }
        return;
    }
    if cursor.goto_first_child() {
        loop {
            collect_variable_names(contents, cursor, names);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn first_type_text(node: Node<'_>, contents: &str) -> Option<String> {
    type_texts(node, contents).into_iter().next()
}

fn type_texts(node: Node<'_>, contents: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut cursor = node.walk();
    collect_type_texts(contents, &mut cursor, &mut values);
    values.sort();
    values.dedup();
    values
}

fn collect_type_texts(contents: &str, cursor: &mut TreeCursor<'_>, values: &mut Vec<String>) {
    let node = cursor.node();
    if matches!(
        node.kind(),
        "type_identifier" | "scoped_type_identifier" | "generic_type"
    ) {
        if let Some(value) = node_text(Some(node), contents) {
            values.push(value.trim().to_string());
        }
    }
    if cursor.goto_first_child() {
        loop {
            collect_type_texts(contents, cursor, values);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn generic_argument(value: &str) -> Option<&str> {
    let start = value.find('<')?;
    let end = value[start + 1..].find('>')? + start + 1;
    value[start + 1..end].split(',').next().map(str::trim)
}

fn unwrap_generic(value: &str) -> &str {
    generic_argument(value).unwrap_or(value)
}

fn is_class_node(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration"
    )
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

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "code-parser-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn classifies_entity_table_columns_and_repository() {
        let root = test_dir("classify-entity-repository");
        fs::create_dir_all(root.join("service/src/main/java/demo")).unwrap();
        fs::write(
            root.join("service/src/main/java/demo/Order.java"),
            r#"
            package demo;
            import jakarta.persistence.*;
            @Entity
            @Table(name = "orders")
            class Order {
              @Id Long id;
              @Column(name = "order_status") String status;
            }
            "#,
        )
        .unwrap();
        fs::write(
            root.join("service/src/main/java/demo/OrderRepository.java"),
            r#"
            package demo;
            import org.springframework.data.jpa.repository.JpaRepository;
            import org.springframework.data.jpa.repository.Query;
            interface OrderRepository extends JpaRepository<Order, Long> {
              @Query("select o from Order o") Order findReady();
            }
            "#,
        )
        .unwrap();
        let build_report = BuildReport {
            project_root: root.display().to_string(),
            modules: vec![crate::languages::java::build::model::BuildScopeReport {
                name: "service".to_string(),
                path: "service".to_string(),
                direct_dependencies: vec![
                    dependency("jakarta.persistence", "jakarta.persistence-api"),
                    dependency("org.springframework.boot", "spring-boot-starter-data-jpa"),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let registry = load_symbol_registry().unwrap();
        let modules = modules_from_build_report(&build_report);
        let source = extract_source_model(&root, &modules, &registry).unwrap();
        let report = build_classification_report(&root, &build_report, &modules, &registry, source);
        let service = &report.modules[0].children[0];

        assert_eq!(service.entities.len(), 1);
        assert_eq!(service.tables[0].name, "orders");
        assert_eq!(service.columns.len(), 2);
        assert_eq!(service.repositories.len(), 1);
        assert_eq!(service.used_dependencies.len(), 2);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn classifies_request_and_response_dtos() {
        let root = test_dir("classify-dtos");
        fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
        fs::write(
            root.join("src/main/java/demo/OrderController.java"),
            r#"
            package demo;
            import org.springframework.web.bind.annotation.*;
            import org.springframework.http.ResponseEntity;
            @RestController
            class OrderController {
              @PostMapping("/orders")
              ResponseEntity<OrderResponse> create(@RequestBody CreateOrderRequest request) {
                return null;
              }
            }
            record CreateOrderRequest(String name) {}
            record OrderResponse(String id) {}
            "#,
        )
        .unwrap();
        let build_report = BuildReport {
            project_root: root.display().to_string(),
            parent: crate::languages::java::build::model::BuildScopeReport {
                name: "parent".to_string(),
                path: ".".to_string(),
                direct_dependencies: vec![dependency(
                    "org.springframework.boot",
                    "spring-boot-starter-web",
                )],
                ..Default::default()
            },
            ..Default::default()
        };
        let registry = load_symbol_registry().unwrap();
        let modules = modules_from_build_report(&build_report);
        let source = extract_source_model(&root, &modules, &registry).unwrap();
        let report = build_classification_report(&root, &build_report, &modules, &registry, source);
        let root_module = &report.modules[0];

        assert_eq!(root_module.request_bodies.len(), 1);
        assert_eq!(root_module.response_bodies.len(), 1);
        assert_eq!(root_module.dtos.len(), 2);
        assert_eq!(root_module.used_dependencies.len(), 1);
        let _ = fs::remove_dir_all(root);
    }

    fn dependency(group_id: &str, artifact_id: &str) -> DependencyInfo {
        DependencyInfo {
            group_id: Some(group_id.to_string()),
            artifact_id: artifact_id.to_string(),
            version: Some("1.0.0".to_string()),
            configuration: None,
            scope: None,
            file: None,
            source: "test".to_string(),
        }
    }
}
