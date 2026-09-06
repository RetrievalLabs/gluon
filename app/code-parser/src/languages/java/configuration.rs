use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use walkdir::{DirEntry, WalkDir};

use crate::core::error::{FileError, ParserError, PathError};
use crate::languages::business::model::ModuleInfo;
use crate::languages::java::build::model::{BuildReport, DependencyInfo, Diagnostic};
use crate::languages::java::business::modules::{module_id_for_file, modules_from_build_report};

const REPORT_FILE: &str = "configuration-classification-report.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyConfigsOptions {
    pub build_report: PathBuf,
    pub output_dir: PathBuf,
    pub source_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifyConfigsSummary {
    pub report_path: String,
    pub module_count: usize,
    pub configuration_file_count: usize,
    pub property_count: usize,
    pub linked_dependency_count: usize,
    pub diagnostic_count: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ClassifyConfigsError {
    #[error(transparent)]
    Path(#[from] PathError),

    #[error(transparent)]
    File(#[from] FileError),

    #[error(transparent)]
    Parser(#[from] ParserError),
}

#[derive(Debug, Default, Clone, Deserialize)]
struct ConfigurationRules {
    #[serde(rename = "schema_version")]
    _schema_version: u32,
    #[serde(default)]
    ignored_file_names: Vec<String>,
    #[serde(default)]
    ignored_path_prefixes: Vec<String>,
    #[serde(default)]
    secret_key_patterns: Vec<String>,
    #[serde(default)]
    rules: Vec<ConfigRule>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct ConfigRule {
    id: String,
    #[serde(rename = "type")]
    config_type: String,
    #[serde(default)]
    framework: Option<String>,
    #[serde(default)]
    file_names: Vec<String>,
    #[serde(default)]
    file_name_patterns: Vec<String>,
    #[serde(default)]
    path_patterns: Vec<String>,
    #[serde(default)]
    xml_roots: Vec<String>,
    #[serde(default)]
    xml_namespaces: Vec<String>,
    #[serde(default)]
    property_prefixes: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    dependency_hints: Vec<DependencyHint>,
}

#[derive(Debug, Default, Clone, Deserialize)]
struct DependencyHint {
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    group_id_pattern: Option<String>,
    #[serde(default)]
    artifact_id: Option<String>,
    #[serde(default)]
    artifact_id_pattern: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ConfigurationReport {
    project_root: String,
    modules: Vec<ModuleReport>,
    diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
struct ModuleReport {
    id: String,
    name: String,
    path: String,
    configuration_files: Vec<ConfigurationFile>,
    used_dependencies: Vec<UsedDependency>,
    children: Vec<ModuleReport>,
}

#[derive(Debug, Clone, Serialize)]
struct ConfigurationFile {
    path: String,
    #[serde(rename = "type")]
    config_type: String,
    format: String,
    framework: Option<String>,
    profile: Option<String>,
    scope: String,
    categories: Vec<String>,
    properties: Vec<ConfigProperty>,
    linked_dependencies: Vec<UsedDependency>,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ConfigProperty {
    key: String,
    value_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reference: Option<String>,
    sensitivity: String,
    profile: Option<String>,
    scope: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct UsedDependency {
    group_id: Option<String>,
    artifact_id: String,
    version: Option<String>,
}

#[derive(Debug, Clone)]
struct CandidateFile {
    path: String,
    file_name: String,
    format: String,
    scope: String,
    profile: Option<String>,
    contents: String,
    properties: Vec<ConfigProperty>,
    xml_root: Option<String>,
}

struct ConfigurationAccumulator {
    reports: BTreeMap<String, ModuleReport>,
    dependencies: BTreeMap<String, BTreeSet<UsedDependency>>,
}

pub fn classify_configs(
    options: &ClassifyConfigsOptions,
) -> Result<ClassifyConfigsSummary, ClassifyConfigsError> {
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

    let rules = load_configuration_rules()?;
    let modules = modules_from_build_report(&build_report);
    let report = build_configuration_report(&project_root, &build_report, &modules, &rules)?;
    let summary = summary_for_report(&report, &project_root, &options.output_dir);
    let json = serde_json::to_string_pretty(&report).map_err(|error| {
        ParserError::Operation(format!(
            "failed to serialize configuration classification report: {error}"
        ))
    })?;
    let report_path = PathBuf::from(&summary.report_path);
    fs::create_dir_all(report_path.parent().unwrap_or_else(|| Path::new("."))).map_err(
        |source| FileError::CreateDir {
            path: report_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
            source,
        },
    )?;
    fs::write(&report_path, json).map_err(|source| FileError::Write {
        path: report_path,
        source,
    })?;
    Ok(summary)
}

fn read_build_report(path: &Path) -> Result<BuildReport, ClassifyConfigsError> {
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

fn load_configuration_rules() -> Result<ConfigurationRules, ClassifyConfigsError> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data/java/configuration_classification.yaml");
    let data = fs::read_to_string(&path).map_err(|source| FileError::Read {
        path: path.clone(),
        source,
    })?;
    serde_yaml::from_str(&data).map_err(|error| {
        ParserError::Operation(format!(
            "failed to parse configuration classification rules: {error}"
        ))
        .into()
    })
}

fn build_configuration_report(
    project_root: &Path,
    build_report: &BuildReport,
    modules: &[ModuleInfo],
    rules: &ConfigurationRules,
) -> Result<ConfigurationReport, ClassifyConfigsError> {
    let dependencies_by_module = dependencies_by_module(build_report);
    let mut accumulator = ConfigurationAccumulator::new(modules);
    let mut diagnostics = Vec::new();

    for candidate in discover_candidates(project_root, rules, &mut diagnostics) {
        let matched_rules = matching_rules(&candidate, rules);
        if matched_rules.is_empty() {
            continue;
        }
        let module_id = module_id_for_file(&candidate.path, modules);
        let file = configuration_file(
            &candidate,
            &module_id,
            &matched_rules,
            &dependencies_by_module,
        );
        accumulator.add_file(&module_id, file);
    }

    let mut report = ConfigurationReport {
        project_root: project_root.display().to_string(),
        modules: accumulator.into_nested_modules(),
        diagnostics,
    };
    sort_report(&mut report);
    Ok(report)
}

fn discover_candidates(
    project_root: &Path,
    rules: &ConfigurationRules,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<CandidateFile> {
    let mut candidates = Vec::new();
    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry, rules))
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(Diagnostic::warning(
                    "classify_configs",
                    error.to_string(),
                    None,
                ));
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let path = relative_path(project_root, entry.path());
        let file_name = entry.file_name().to_string_lossy().to_string();
        if rules
            .ignored_file_names
            .iter()
            .any(|ignored| ignored == &file_name)
        {
            continue;
        }
        if is_ignored_path(&path, rules) {
            continue;
        }
        let Some(format) = format_for_file(&file_name) else {
            continue;
        };
        let contents = match fs::read_to_string(entry.path()) {
            Ok(contents) => contents,
            Err(error) => {
                diagnostics.push(Diagnostic::warning(
                    "classify_configs",
                    format!("failed to read {}: {error}", entry.path().display()),
                    Some(path),
                ));
                continue;
            }
        };
        let scope = scope_for_path(&path);
        let profile = profile_for_file(&file_name).or_else(|| profile_for_properties(&contents));
        let xml_root = if format == "xml" {
            xml_root(&contents)
        } else {
            None
        };
        let properties =
            extract_properties(&contents, &format, profile.clone(), scope.clone(), rules);
        candidates.push(CandidateFile {
            path,
            file_name,
            format,
            scope,
            profile,
            contents,
            properties,
            xml_root,
        });
    }
    candidates
}

fn matching_rules<'a>(
    candidate: &CandidateFile,
    rules: &'a ConfigurationRules,
) -> Vec<&'a ConfigRule> {
    rules
        .rules
        .iter()
        .filter(|rule| rule_matches(candidate, rule))
        .collect()
}

fn rule_matches(candidate: &CandidateFile, rule: &ConfigRule) -> bool {
    if rule
        .file_names
        .iter()
        .any(|name| name == &candidate.file_name)
    {
        return true;
    }
    if matches_any_regex(&rule.file_name_patterns, &candidate.file_name)
        || matches_any_regex(&rule.path_patterns, &candidate.path)
    {
        return true;
    }
    if let Some(root) = &candidate.xml_root
        && rule
            .xml_roots
            .iter()
            .any(|candidate_root| candidate_root == root)
    {
        return true;
    }
    if !rule.xml_namespaces.is_empty()
        && rule
            .xml_namespaces
            .iter()
            .any(|namespace| candidate.contents.contains(namespace))
    {
        return true;
    }
    candidate
        .properties
        .iter()
        .any(|property| key_matches_prefixes(&property.key, &rule.property_prefixes))
}

fn configuration_file(
    candidate: &CandidateFile,
    module_id: &str,
    rules: &[&ConfigRule],
    dependencies_by_module: &BTreeMap<String, Vec<DependencyInfo>>,
) -> ConfigurationFile {
    let mut categories = BTreeSet::new();
    let mut evidence = BTreeSet::new();
    let mut linked_dependencies = BTreeSet::new();
    let config_type = rules
        .first()
        .map(|rule| rule.config_type.clone())
        .unwrap_or_else(|| "UNKNOWN".to_string());
    let framework = rules.iter().find_map(|rule| rule.framework.clone());

    for rule in rules {
        for category in &rule.categories {
            categories.insert(category.clone());
        }
        evidence.insert(rule.id.clone());
        add_match_evidence(candidate, rule, &mut evidence);
        for dependency in
            dependencies_for_hints(module_id, &rule.dependency_hints, dependencies_by_module)
        {
            linked_dependencies.insert(dependency);
        }
    }

    ConfigurationFile {
        path: candidate.path.clone(),
        config_type,
        format: candidate.format.clone(),
        framework,
        profile: candidate.profile.clone(),
        scope: candidate.scope.clone(),
        categories: categories.into_iter().collect(),
        properties: candidate.properties.clone(),
        linked_dependencies: linked_dependencies.into_iter().collect(),
        evidence: evidence.into_iter().collect(),
    }
}

fn add_match_evidence(
    candidate: &CandidateFile,
    rule: &ConfigRule,
    evidence: &mut BTreeSet<String>,
) {
    if rule
        .file_names
        .iter()
        .any(|name| name == &candidate.file_name)
    {
        evidence.insert("filename".to_string());
    }
    if matches_any_regex(&rule.file_name_patterns, &candidate.file_name) {
        evidence.insert("filename_pattern".to_string());
    }
    if matches_any_regex(&rule.path_patterns, &candidate.path) {
        evidence.insert("path_pattern".to_string());
    }
    if candidate.xml_root.as_ref().is_some_and(|root| {
        rule.xml_roots
            .iter()
            .any(|candidate_root| candidate_root == root)
    }) {
        evidence.insert("xml_root".to_string());
    }
    if !rule.xml_namespaces.is_empty()
        && rule
            .xml_namespaces
            .iter()
            .any(|namespace| candidate.contents.contains(namespace))
    {
        evidence.insert("xml_namespace".to_string());
    }
    if candidate
        .properties
        .iter()
        .any(|property| key_matches_prefixes(&property.key, &rule.property_prefixes))
    {
        evidence.insert("property_prefix".to_string());
    }
}

fn extract_properties(
    contents: &str,
    format: &str,
    profile: Option<String>,
    scope: String,
    rules: &ConfigurationRules,
) -> Vec<ConfigProperty> {
    let mut properties = match format {
        "properties" | "env" => parse_properties(contents),
        "yaml" => parse_yaml_properties(contents),
        "json" => parse_json_properties(contents),
        _ => Vec::new(),
    };
    for property in &mut properties {
        property.profile = profile.clone();
        property.scope = scope.clone();
        property.sensitivity = sensitivity_for_key(&property.key, rules);
    }
    properties.sort_by(|left, right| left.key.cmp(&right.key));
    properties.dedup_by(|left, right| left.key == right.key);
    properties
}

fn parse_properties(contents: &str) -> Vec<ConfigProperty> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                return None;
            }
            let (key, value) = line.split_once('=').or_else(|| line.split_once(':'))?;
            Some(config_property(key.trim(), value.trim()))
        })
        .collect()
}

fn parse_yaml_properties(contents: &str) -> Vec<ConfigProperty> {
    let Ok(value) = serde_yaml::from_str::<YamlValue>(contents) else {
        return Vec::new();
    };
    let mut properties = Vec::new();
    flatten_yaml(None, &value, &mut properties);
    properties
}

fn flatten_yaml(prefix: Option<String>, value: &YamlValue, properties: &mut Vec<ConfigProperty>) {
    match value {
        YamlValue::Mapping(mapping) => {
            for (key, value) in mapping {
                let Some(key) = scalar_key(key) else {
                    continue;
                };
                let next = match &prefix {
                    Some(prefix) => format!("{prefix}.{key}"),
                    None => key,
                };
                flatten_yaml(Some(next), value, properties);
            }
        }
        YamlValue::Sequence(_) => {
            if let Some(key) = prefix {
                properties.push(config_property(&key, "[]"));
            }
        }
        _ => {
            if let Some(key) = prefix {
                properties.push(config_property(&key, &scalar_value(value)));
            }
        }
    }
}

fn parse_json_properties(contents: &str) -> Vec<ConfigProperty> {
    let Ok(value) = serde_json::from_str::<JsonValue>(contents) else {
        return Vec::new();
    };
    let mut properties = Vec::new();
    flatten_json(None, &value, &mut properties);
    properties
}

fn flatten_json(prefix: Option<String>, value: &JsonValue, properties: &mut Vec<ConfigProperty>) {
    match value {
        JsonValue::Object(map) => {
            for (key, value) in map {
                let next = match &prefix {
                    Some(prefix) => format!("{prefix}.{key}"),
                    None => key.clone(),
                };
                flatten_json(Some(next), value, properties);
            }
        }
        JsonValue::Array(_) => {
            if let Some(key) = prefix {
                properties.push(config_property(&key, "[]"));
            }
        }
        _ => {
            if let Some(key) = prefix {
                properties.push(config_property(&key, &json_scalar_value(value)));
            }
        }
    }
}

fn config_property(key: &str, value: &str) -> ConfigProperty {
    let (value_kind, reference) = value_kind(value);
    ConfigProperty {
        key: key.to_string(),
        value_kind,
        reference,
        sensitivity: "normal".to_string(),
        profile: None,
        scope: "main".to_string(),
    }
}

fn value_kind(value: &str) -> (String, Option<String>) {
    let trimmed = value.trim();
    let placeholder = Regex::new(r"\$\{([^}:]+)(?::[^}]*)?\}").expect("valid placeholder regex");
    if let Some(captures) = placeholder.captures(trimmed) {
        let reference = captures.get(1).map(|value| value.as_str().to_string());
        let kind = if reference.as_deref().is_some_and(is_environment_name) {
            "ENVIRONMENT_REFERENCE"
        } else {
            "PROPERTY_REFERENCE"
        };
        return (kind.to_string(), reference);
    }
    if trimmed.starts_with("java:") || trimmed.starts_with("jdbc/") || trimmed.starts_with("jms/") {
        return ("JNDI_REFERENCE".to_string(), Some(trimmed.to_string()));
    }
    if trimmed.starts_with("http://")
        || trimmed.starts_with("https://")
        || trimmed.starts_with("jdbc:")
        || trimmed.starts_with("r2dbc:")
    {
        return ("URL".to_string(), None);
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return ("FILE_PATH".to_string(), None);
    }
    ("LITERAL".to_string(), None)
}

fn is_environment_name(value: &str) -> bool {
    value.chars().any(|ch| ch == '_')
        || value
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
}

fn sensitivity_for_key(key: &str, rules: &ConfigurationRules) -> String {
    let key = key.to_ascii_lowercase();
    if rules
        .secret_key_patterns
        .iter()
        .any(|pattern| key.contains(&pattern.to_ascii_lowercase()))
    {
        "secret".to_string()
    } else {
        "normal".to_string()
    }
}

fn scalar_key(value: &YamlValue) -> Option<String> {
    match value {
        YamlValue::String(value) => Some(value.clone()),
        YamlValue::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn scalar_value(value: &YamlValue) -> String {
    match value {
        YamlValue::String(value) => value.clone(),
        YamlValue::Number(value) => value.to_string(),
        YamlValue::Bool(value) => value.to_string(),
        YamlValue::Null => String::new(),
        _ => String::new(),
    }
}

fn json_scalar_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        JsonValue::Number(value) => value.to_string(),
        JsonValue::Bool(value) => value.to_string(),
        JsonValue::Null => String::new(),
        _ => String::new(),
    }
}

fn profile_for_file(file_name: &str) -> Option<String> {
    let regex = Regex::new(r"^(?:application|bootstrap)-([A-Za-z0-9_.-]+)\.(?:properties|ya?ml)$")
        .expect("valid profile regex");
    regex
        .captures(file_name)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

fn profile_for_properties(contents: &str) -> Option<String> {
    let regex = Regex::new(r"(?m)^\s*(?:spring\.profiles\.active|spring\.profiles\.default|spring\.config\.activate\.on-profile)\s*[:=]\s*([A-Za-z0-9_.-]+)")
        .expect("valid Spring profile regex");
    regex
        .captures(contents)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

fn scope_for_path(path: &str) -> String {
    if path.contains("/src/test/") || path.starts_with("src/test/") {
        "test".to_string()
    } else {
        "main".to_string()
    }
}

fn xml_root(contents: &str) -> Option<String> {
    let regex = Regex::new(r"<\s*([A-Za-z_][A-Za-z0-9_.:-]*)").expect("valid XML root regex");
    regex
        .captures(contents)
        .and_then(|captures| captures.get(1))
        .map(|value| {
            value
                .as_str()
                .rsplit(':')
                .next()
                .unwrap_or(value.as_str())
                .to_string()
        })
}

fn format_for_file(file_name: &str) -> Option<String> {
    if file_name == ".env" || file_name.starts_with(".env.") {
        return Some("env".to_string());
    }
    let extension = Path::new(file_name)
        .extension()
        .and_then(|ext| ext.to_str())?;
    match extension {
        "properties" => Some("properties".to_string()),
        "yml" | "yaml" => Some("yaml".to_string()),
        "xml" | "wsdl" | "xsd" => Some("xml".to_string()),
        "json" => Some("json".to_string()),
        "graphql" | "graphqls" => Some("graphql".to_string()),
        "proto" => Some("proto".to_string()),
        "avsc" | "avdl" | "avpr" => Some("avro".to_string()),
        "sql" => Some("sql".to_string()),
        _ => None,
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

fn dependencies_for_hints(
    module_id: &str,
    hints: &[DependencyHint],
    dependencies_by_module: &BTreeMap<String, Vec<DependencyInfo>>,
) -> Vec<UsedDependency> {
    dependencies_by_module
        .get(module_id)
        .into_iter()
        .chain(dependencies_by_module.get("module:."))
        .flat_map(|dependencies| dependencies.iter())
        .filter(|dependency| {
            hints
                .iter()
                .any(|hint| dependency_matches_hint(dependency, hint))
        })
        .map(|dependency| UsedDependency {
            group_id: dependency.group_id.clone(),
            artifact_id: dependency.artifact_id.clone(),
            version: dependency.version.clone(),
        })
        .collect()
}

fn dependency_matches_hint(dependency: &DependencyInfo, hint: &DependencyHint) -> bool {
    if let Some(group_id) = &hint.group_id
        && dependency.group_id.as_deref() == Some(group_id)
    {
        return true;
    }
    if let Some(artifact_id) = &hint.artifact_id
        && dependency.artifact_id == *artifact_id
    {
        return true;
    }
    if let Some(pattern) = &hint.group_id_pattern
        && dependency
            .group_id
            .as_deref()
            .is_some_and(|group_id| regex_matches(pattern, group_id))
    {
        return true;
    }
    if let Some(pattern) = &hint.artifact_id_pattern
        && regex_matches(pattern, &dependency.artifact_id)
    {
        return true;
    }
    false
}

impl ConfigurationAccumulator {
    fn new(modules: &[ModuleInfo]) -> Self {
        let reports = modules
            .iter()
            .map(|module| {
                (
                    module.id.clone(),
                    ModuleReport {
                        id: module.id.clone(),
                        name: module.name.clone(),
                        path: module.path.clone(),
                        configuration_files: Vec::new(),
                        used_dependencies: Vec::new(),
                        children: Vec::new(),
                    },
                )
            })
            .collect();
        Self {
            reports,
            dependencies: BTreeMap::new(),
        }
    }

    fn add_file(&mut self, module_id: &str, file: ConfigurationFile) {
        for dependency in &file.linked_dependencies {
            self.dependencies
                .entry(module_id.to_string())
                .or_default()
                .insert(dependency.clone());
        }
        if let Some(module) = self.reports.get_mut(module_id) {
            module.configuration_files.push(file);
        }
    }

    fn into_nested_modules(mut self) -> Vec<ModuleReport> {
        for (module_id, dependencies) in self.dependencies {
            if let Some(module) = self.reports.get_mut(&module_id) {
                module.used_dependencies = dependencies.into_iter().collect();
            }
        }
        let ids = self.reports.keys().cloned().collect::<Vec<_>>();
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
        build_nested("module:.", &mut self.reports, &children_by_parent)
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

fn sort_report(report: &mut ConfigurationReport) {
    for module in &mut report.modules {
        sort_module(module);
    }
}

fn sort_module(module: &mut ModuleReport) {
    module
        .configuration_files
        .sort_by(|left, right| left.path.cmp(&right.path));
    module
        .used_dependencies
        .sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    for file in &mut module.configuration_files {
        file.linked_dependencies
            .sort_by(|left, right| left.artifact_id.cmp(&right.artifact_id));
    }
    for child in &mut module.children {
        sort_module(child);
    }
}

fn summary_for_report(
    report: &ConfigurationReport,
    project_root: &Path,
    output_dir: &Path,
) -> ClassifyConfigsSummary {
    let report_path = output_project_dir(project_root, output_dir).join(REPORT_FILE);
    let mut summary = ClassifyConfigsSummary {
        report_path: report_path.display().to_string(),
        module_count: 0,
        configuration_file_count: 0,
        property_count: 0,
        linked_dependency_count: 0,
        diagnostic_count: report.diagnostics.len(),
    };
    for module in &report.modules {
        count_module(module, &mut summary);
    }
    summary
}

fn count_module(module: &ModuleReport, summary: &mut ClassifyConfigsSummary) {
    summary.module_count += 1;
    summary.configuration_file_count += module.configuration_files.len();
    summary.linked_dependency_count += module.used_dependencies.len();
    for file in &module.configuration_files {
        summary.property_count += file.properties.len();
    }
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

fn key_matches_prefixes(key: &str, prefixes: &[String]) -> bool {
    prefixes.iter().any(|prefix| {
        if prefix.ends_with('.') {
            key.starts_with(prefix)
        } else {
            key == prefix || key.starts_with(&format!("{prefix}."))
        }
    })
}

fn matches_any_regex(patterns: &[String], value: &str) -> bool {
    patterns.iter().any(|pattern| regex_matches(pattern, value))
}

fn regex_matches(pattern: &str, value: &str) -> bool {
    Regex::new(pattern)
        .map(|regex| regex.is_match(value))
        .unwrap_or(false)
}

fn is_ignored_dir(entry: &DirEntry, rules: &ConfigurationRules) -> bool {
    entry.file_type().is_dir() && is_ignored_path(&relative_dir_path(entry), rules)
}

fn relative_dir_path(entry: &DirEntry) -> String {
    format!("{}/", entry.file_name().to_string_lossy())
}

fn is_ignored_path(path: &str, rules: &ConfigurationRules) -> bool {
    rules
        .ignored_path_prefixes
        .iter()
        .any(|prefix| path == prefix.trim_end_matches('/') || path.starts_with(prefix))
}

fn relative_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "code-parser-config-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn loads_default_configuration_rules() {
        let rules = load_configuration_rules().expect("rules load");

        assert!(!rules.rules.is_empty());
        assert!(
            rules
                .ignored_file_names
                .iter()
                .any(|file| file == "pom.xml")
        );
    }

    #[test]
    fn parses_yaml_properties_and_redacts_secret_keys() {
        let rules = load_configuration_rules().expect("rules load");
        let properties = extract_properties(
            "spring:\n  datasource:\n    password: ${DB_PASSWORD}\n",
            "yaml",
            Some("prod".to_string()),
            "main".to_string(),
            &rules,
        );

        assert_eq!(properties[0].key, "spring.datasource.password");
        assert_eq!(properties[0].value_kind, "ENVIRONMENT_REFERENCE");
        assert_eq!(properties[0].reference.as_deref(), Some("DB_PASSWORD"));
        assert_eq!(properties[0].sensitivity, "secret");
    }

    #[test]
    fn classifies_module_configuration_and_ignores_build_files() {
        let root = test_dir("module");
        fs::create_dir_all(root.join("service/src/main/resources")).unwrap();
        fs::write(root.join("service/pom.xml"), "<project/>").unwrap();
        fs::write(root.join("service/Dockerfile"), "FROM eclipse-temurin").unwrap();
        fs::write(
            root.join("service/src/main/resources/application-prod.yml"),
            "spring:\n  datasource:\n    url: jdbc:postgresql://localhost/app\n    password: ${DB_PASSWORD}\n",
        )
        .unwrap();
        let build_report = BuildReport {
            project_root: root.display().to_string(),
            modules: vec![crate::languages::java::build::model::BuildScopeReport {
                name: "service".to_string(),
                path: "service".to_string(),
                direct_dependencies: vec![DependencyInfo {
                    group_id: Some("org.springframework.boot".to_string()),
                    artifact_id: "spring-boot-starter-data-jpa".to_string(),
                    version: Some("3.3.0".to_string()),
                    configuration: None,
                    scope: None,
                    file: Some("service/pom.xml".to_string()),
                    source: "test".to_string(),
                }],
                ..Default::default()
            }],
            diagnostics: Vec::new(),
            ..Default::default()
        };
        let rules = load_configuration_rules().expect("rules load");
        let modules = modules_from_build_report(&build_report);
        let report =
            build_configuration_report(&root, &build_report, &modules, &rules).expect("report");
        let service = &report.modules[0].children[0];

        assert_eq!(service.configuration_files.len(), 1);
        assert_eq!(
            service.configuration_files[0].path,
            "service/src/main/resources/application-prod.yml"
        );
        assert_eq!(
            service.configuration_files[0].profile.as_deref(),
            Some("prod")
        );
        assert_eq!(service.used_dependencies.len(), 1);
        let _ = fs::remove_dir_all(root);
    }
}
