use serde::Serialize;

use crate::languages::java::build::model::Diagnostic;

#[derive(Debug, Serialize)]
pub struct CompatibilityReport {
    pub source_java: Option<String>,
    pub target_java: u32,
    pub dependency_recommendations: Vec<DependencyRecommendation>,
    pub plugin_recommendations: Vec<PluginRecommendation>,
    pub api_findings: Vec<ApiFinding>,
    pub jdk_tool_findings: Vec<JdkToolFinding>,
    pub code_change_recommendations: Vec<CodeChangeRecommendation>,
    pub unknown_dependencies: Vec<UnknownDependency>,
    pub unknown_plugins: Vec<UnknownPlugin>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize)]
pub struct JdkToolFinding {
    pub tool: String,
    pub severity: String,
    pub class_name: Option<String>,
    pub matched_text: String,
    pub source: String,
    pub guidance: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyRecommendation {
    pub id: String,
    pub coordinates: String,
    pub current_version: Option<String>,
    pub recommended_version: Option<String>,
    pub severity: String,
    pub risk: Option<String>,
    pub reason: Option<String>,
    pub edit_strategy: Option<String>,
    pub source_ids: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginRecommendation {
    pub id: String,
    pub plugin: String,
    pub current_version: Option<String>,
    pub recommended_version: Option<String>,
    pub severity: String,
    pub risk: Option<String>,
    pub reason: Option<String>,
    pub edit_strategy: Option<String>,
    pub source_ids: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiFinding {
    pub rule_id: String,
    pub category: String,
    pub severity: String,
    pub file: String,
    pub line: usize,
    pub matched_text: String,
    pub guidance: Option<String>,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeChangeRecommendation {
    pub id: String,
    pub source: String,
    pub reason: String,
    pub guidance: String,
    pub related_findings: Vec<String>,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownDependency {
    pub coordinates: String,
    pub version: Option<String>,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnknownPlugin {
    pub plugin: String,
    pub version: Option<String>,
    pub source: String,
    pub message: String,
}
