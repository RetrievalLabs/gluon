use std::collections::HashSet;
use std::path::Path;

use thiserror::Error;

use crate::core::error::KnowledgeBaseError;
use crate::languages::java::build::model::{
    BuildReport, BuildToolInfo, DependencyInfo, Diagnostic, PluginInfo,
};
use crate::languages::java::business::jdtls::JdtlsOptions;
use crate::languages::java::compatibility::jdk_tools::{JdkToolOptions, run_jdk_tools};
use crate::languages::java::compatibility::knowledge_base::{
    CompatibilityRule, JavaCompatibilityKnowledgeBase, MatchRule, ReplacementRule,
};
use crate::languages::java::compatibility::model::{
    CodeChangeRecommendation, CompatibilityReport, CompatibilityScopeReport,
    DependencyRecommendation, PluginRecommendation, UnknownDependency, UnknownPlugin,
};
use crate::languages::java::compatibility::source_scan::scan_java_sources_with_jdtls;

const UNKNOWN_MESSAGE: &str = "No KB rule; verify via official docs or ask LLM/research agent.";

pub type CompatibilityResult<T> = Result<T, CompatibilityError>;

#[derive(Debug, Error)]
pub enum CompatibilityError {
    #[error("failed to load Java compatibility knowledge base: {0}")]
    KnowledgeBase(#[from] KnowledgeBaseError),
    #[error("Java source analysis failed: {0}")]
    SourceAnalysis(String),
}

pub fn analyze_report(
    build_report: &BuildReport,
    target_java: u32,
    source_path: &Path,
) -> CompatibilityResult<CompatibilityReport> {
    let jdtls_options = JdtlsOptions {
        command: "jdtls".to_string(),
        workspace: source_path.join(".gluon-jdtls-analyze-workspace"),
        max_in_flight: 32,
    };
    analyze_report_with_options(
        build_report,
        target_java,
        source_path,
        &JdkToolOptions::default(),
        &jdtls_options,
    )
}

pub fn analyze_report_with_options(
    build_report: &BuildReport,
    target_java: u32,
    source_path: &Path,
    jdk_tool_options: &JdkToolOptions,
    jdtls_options: &JdtlsOptions,
) -> CompatibilityResult<CompatibilityReport> {
    let kb = JavaCompatibilityKnowledgeBase::load_default()
        .map_err(|error| KnowledgeBaseError::Load(error))?;
    let mut diagnostics = Vec::new();
    let source_java = detect_source_java(build_report);

    let (dependency_recommendations, unknown_dependencies) =
        analyze_dependencies(build_report, target_java, &kb.dependencies);
    let (plugin_recommendations, unknown_plugins) =
        analyze_plugins(build_report, target_java, &kb.plugins);

    let (api_findings, scan_diagnostics) = scan_java_sources_with_jdtls(
        source_path,
        target_java,
        &[
            ("removed_api", &kb.removed_apis),
            ("deprecated_for_removal_api", &kb.deprecated_for_removal),
            ("internal_api", &kb.internal_apis),
            ("reflective_access", &kb.reflective_access),
        ],
        jdtls_options,
    )
    .map_err(CompatibilityError::SourceAnalysis)?;
    diagnostics.extend(scan_diagnostics);

    let (jdk_tool_findings, jdk_tool_diagnostics) = run_jdk_tools(
        source_path,
        source_java.as_deref(),
        target_java,
        jdk_tool_options,
    );
    diagnostics.extend(jdk_tool_diagnostics);

    let code_change_recommendations =
        derive_code_change_recommendations(&api_findings, &kb.replacements, target_java);

    let (parent, modules) = build_compatibility_scopes(
        build_report,
        source_java.clone(),
        &dependency_recommendations,
        &plugin_recommendations,
        &api_findings,
        &jdk_tool_findings,
        &code_change_recommendations,
        &unknown_dependencies,
        &unknown_plugins,
        &diagnostics,
    );

    Ok(CompatibilityReport {
        source_java,
        target_java,
        parent,
        modules,
        dependency_recommendations,
        plugin_recommendations,
        api_findings,
        jdk_tool_findings,
        code_change_recommendations,
        unknown_dependencies,
        unknown_plugins,
        diagnostics,
    })
}

fn analyze_dependencies(
    build_report: &BuildReport,
    target_java: u32,
    rules: &[CompatibilityRule],
) -> (Vec<DependencyRecommendation>, Vec<UnknownDependency>) {
    let mut recommendations = Vec::new();
    let mut unknown = Vec::new();

    for dependency in &build_report.direct_dependencies {
        let matched_rule = rules
            .iter()
            .find(|rule| dependency_matches(&rule.match_rule, dependency));
        match matched_rule {
            Some(rule) => {
                if should_recommend(rule, dependency.version.as_deref(), target_java) {
                    let java = java_compatibility(rule, target_java);
                    recommendations.push(DependencyRecommendation {
                        id: rule.id.clone(),
                        coordinates: coordinates(dependency),
                        current_version: dependency.version.clone(),
                        recommended_version: java.and_then(|java| java.recommended_version.clone()),
                        severity: rule.severity.clone(),
                        risk: rule.risk.clone(),
                        reason: rule.reason.clone(),
                        edit_strategy: rule.edit_strategy.clone(),
                        source_ids: rule.source_ids.clone(),
                        source: dependency
                            .file
                            .clone()
                            .unwrap_or_else(|| dependency.source.clone()),
                    });
                }
            }
            None => unknown.push(UnknownDependency {
                coordinates: coordinates(dependency),
                version: dependency.version.clone(),
                source: dependency.source.clone(),
                message: UNKNOWN_MESSAGE.to_string(),
            }),
        }
    }

    (recommendations, unknown)
}

fn analyze_plugins(
    build_report: &BuildReport,
    target_java: u32,
    rules: &[CompatibilityRule],
) -> (Vec<PluginRecommendation>, Vec<UnknownPlugin>) {
    let mut recommendations = Vec::new();
    let mut unknown = Vec::new();
    let mut matched_plugin_ids = HashSet::new();

    for plugin in &build_report.direct_plugins {
        let matched_rule = rules
            .iter()
            .find(|rule| plugin_matches(&rule.match_rule, plugin));
        match matched_rule {
            Some(rule) => {
                matched_plugin_ids.insert(plugin.id.clone());
                if should_recommend(rule, plugin.version.as_deref(), target_java) {
                    let java = java_compatibility(rule, target_java);
                    recommendations.push(PluginRecommendation {
                        id: rule.id.clone(),
                        plugin: plugin.id.clone(),
                        current_version: plugin.version.clone(),
                        recommended_version: java.and_then(|java| java.recommended_version.clone()),
                        severity: rule.severity.clone(),
                        risk: rule.risk.clone(),
                        reason: rule.reason.clone(),
                        edit_strategy: rule.edit_strategy.clone(),
                        source_ids: rule.source_ids.clone(),
                        source: plugin.file.clone().unwrap_or_else(|| plugin.source.clone()),
                    });
                }
            }
            None => unknown.push(UnknownPlugin {
                plugin: plugin.id.clone(),
                version: plugin.version.clone(),
                source: plugin.file.clone().unwrap_or_else(|| plugin.source.clone()),
                message: UNKNOWN_MESSAGE.to_string(),
            }),
        }
    }

    for tool in &build_report.build_tools {
        for rule in rules
            .iter()
            .filter(|rule| build_tool_matches(&rule.match_rule, tool))
        {
            if should_recommend(rule, tool.version.as_deref(), target_java) {
                let java = java_compatibility(rule, target_java);
                recommendations.push(PluginRecommendation {
                    id: rule.id.clone(),
                    plugin: tool.tool.clone(),
                    current_version: tool.version.clone(),
                    recommended_version: java.and_then(|java| java.recommended_version.clone()),
                    severity: rule.severity.clone(),
                    risk: rule.risk.clone(),
                    reason: rule.reason.clone(),
                    edit_strategy: rule.edit_strategy.clone(),
                    source_ids: rule.source_ids.clone(),
                    source: tool.source.clone(),
                });
            }
        }
    }

    unknown.retain(|plugin| !matched_plugin_ids.contains(&plugin.plugin));
    (recommendations, unknown)
}

fn build_compatibility_scopes(
    build_report: &BuildReport,
    source_java: Option<String>,
    dependency_recommendations: &[DependencyRecommendation],
    plugin_recommendations: &[PluginRecommendation],
    api_findings: &[crate::languages::java::compatibility::model::ApiFinding],
    jdk_tool_findings: &[crate::languages::java::compatibility::model::JdkToolFinding],
    code_change_recommendations: &[CodeChangeRecommendation],
    unknown_dependencies: &[UnknownDependency],
    unknown_plugins: &[UnknownPlugin],
    diagnostics: &[Diagnostic],
) -> (CompatibilityScopeReport, Vec<CompatibilityScopeReport>) {
    let mut parent = CompatibilityScopeReport {
        name: "parent".to_string(),
        path: ".".to_string(),
        source_java: source_java.clone(),
        ..CompatibilityScopeReport::default()
    };
    let mut modules: Vec<CompatibilityScopeReport> = build_report
        .modules
        .iter()
        .map(|module| CompatibilityScopeReport {
            name: module.name.clone(),
            path: module.path.clone(),
            source_java: detect_source_java_for_module(module).or_else(|| source_java.clone()),
            ..CompatibilityScopeReport::default()
        })
        .collect();

    for item in dependency_recommendations {
        scope_for_source(&item.source, &mut parent, &mut modules)
            .dependency_recommendations
            .push(item.clone());
    }
    for item in plugin_recommendations {
        scope_for_source(&item.source, &mut parent, &mut modules)
            .plugin_recommendations
            .push(item.clone());
    }
    for item in api_findings {
        scope_for_source(&item.file, &mut parent, &mut modules)
            .api_findings
            .push(item.clone());
    }
    for item in jdk_tool_findings {
        scope_for_source(&item.source, &mut parent, &mut modules)
            .jdk_tool_findings
            .push(item.clone());
    }
    for item in code_change_recommendations {
        let source = item
            .related_findings
            .first()
            .and_then(|finding| finding.split(':').next())
            .unwrap_or(&item.source);
        scope_for_source(source, &mut parent, &mut modules)
            .code_change_recommendations
            .push(item.clone());
    }
    for item in unknown_dependencies {
        scope_for_source(&item.source, &mut parent, &mut modules)
            .unknown_dependencies
            .push(item.clone());
    }
    for item in unknown_plugins {
        scope_for_source(&item.source, &mut parent, &mut modules)
            .unknown_plugins
            .push(item.clone());
    }
    for item in diagnostics {
        scope_for_source(
            item.file.as_deref().unwrap_or(""),
            &mut parent,
            &mut modules,
        )
        .diagnostics
        .push(item.clone());
    }

    modules.sort_by(|left, right| left.path.cmp(&right.path));
    (parent, modules)
}

fn scope_for_source<'a>(
    source: &str,
    parent: &'a mut CompatibilityScopeReport,
    modules: &'a mut Vec<CompatibilityScopeReport>,
) -> &'a mut CompatibilityScopeReport {
    let normalized = source.replace('\\', "/");
    if let Some(index) = modules.iter().position(|module| {
        normalized == module.path || normalized.starts_with(&format!("{}/", module.path))
    }) {
        return &mut modules[index];
    }

    parent
}

fn detect_source_java_for_module(
    module: &crate::languages::java::build::model::BuildScopeReport,
) -> Option<String> {
    module
        .java_versions
        .iter()
        .find(|version| matches!(version.kind.as_str(), "release" | "source" | "target"))
        .or_else(|| module.java_versions.first())
        .map(|version| version.version.clone())
}

fn derive_code_change_recommendations(
    findings: &[crate::languages::java::compatibility::model::ApiFinding],
    replacements: &[ReplacementRule],
    target_java: u32,
) -> Vec<CodeChangeRecommendation> {
    let mut recommendations = Vec::new();
    for replacement in replacements {
        if let Some(minimum) = replacement.applies_when_target_java_at_least {
            if target_java < minimum {
                continue;
            }
        }
        let related: Vec<String> = findings
            .iter()
            .filter(|finding| {
                replacement.from_symbols.iter().any(|symbol| {
                    finding.matched_text.starts_with(symbol)
                        || symbol.starts_with(&finding.matched_text)
                })
            })
            .map(|finding| format!("{}:{}:{}", finding.file, finding.line, finding.rule_id))
            .collect();
        if related.is_empty() {
            continue;
        }
        recommendations.push(CodeChangeRecommendation {
            id: replacement.id.clone(),
            source: "replacements".to_string(),
            reason: replacement
                .migration_kind
                .clone()
                .unwrap_or_else(|| replacement.id.clone()),
            guidance: replacement.note.clone().unwrap_or_else(|| {
                let targets = replacement.to_symbols.join(", ");
                if targets.is_empty() {
                    "Review replacement rule guidance.".to_string()
                } else {
                    format!("Replace with {targets}.")
                }
            }),
            related_findings: related,
            source_ids: replacement.source_ids.clone(),
        });
    }
    recommendations
}

fn detect_source_java(build_report: &BuildReport) -> Option<String> {
    build_report
        .java_versions
        .iter()
        .find(|version| matches!(version.kind.as_str(), "release" | "source" | "target"))
        .or_else(|| build_report.java_versions.first())
        .map(|version| version.version.clone())
}

fn dependency_key(dependency: &DependencyInfo) -> String {
    format!(
        "{}:{}",
        dependency.group_id.as_deref().unwrap_or(""),
        dependency.artifact_id
    )
}

fn coordinates(dependency: &DependencyInfo) -> String {
    dependency_key(dependency)
}

fn dependency_matches(rule: &MatchRule, dependency: &DependencyInfo) -> bool {
    string_match(
        rule.group_id.as_deref(),
        rule.group_id_pattern.as_deref(),
        dependency.group_id.as_deref().unwrap_or(""),
    ) && string_match(
        rule.artifact_id.as_deref(),
        rule.artifact_id_pattern.as_deref(),
        &dependency.artifact_id,
    )
}

fn plugin_matches(rule: &MatchRule, plugin: &PluginInfo) -> bool {
    if rule.plugin_id.is_none()
        && rule.plugin_id_pattern.is_none()
        && rule.artifact_id.is_none()
        && rule.artifact_id_pattern.is_none()
    {
        return false;
    }
    let plugin_id_match = string_match(
        rule.plugin_id.as_deref().or(rule.artifact_id.as_deref()),
        rule.plugin_id_pattern
            .as_deref()
            .or(rule.artifact_id_pattern.as_deref()),
        &plugin.id,
    );
    plugin_id_match
}

fn build_tool_matches(rule: &MatchRule, tool: &BuildToolInfo) -> bool {
    rule.tool.as_deref() == Some(tool.tool.as_str())
}

fn string_match(exact: Option<&str>, pattern: Option<&str>, value: &str) -> bool {
    match (exact, pattern) {
        (Some(exact), _) => exact == value,
        (None, Some(pattern)) => wildcard_string_match(pattern, value),
        (None, None) => true,
    }
}

fn wildcard_string_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(suffix), _) => value.ends_with(suffix),
        (_, Some(prefix)) => value.starts_with(prefix),
        _ => pattern == value,
    }
}

fn should_recommend(
    rule: &CompatibilityRule,
    current_version: Option<&str>,
    target_java: u32,
) -> bool {
    let Some(java) = java_compatibility(rule, target_java) else {
        return false;
    };
    let Some(min_version) = java.min_version.as_deref() else {
        return true;
    };
    if is_manual_version(min_version)
        || java
            .recommended_version
            .as_deref()
            .is_some_and(is_manual_version)
    {
        return true;
    }
    let Some(current_version) = current_version else {
        return true;
    };
    version_is_below(current_version, min_version)
}

fn java_compatibility(
    rule: &CompatibilityRule,
    target_java: u32,
) -> Option<&crate::languages::java::compatibility::knowledge_base::JavaCompatibility> {
    rule.compatibility.java.get(&target_java.to_string())
}

fn is_manual_version(version: &str) -> bool {
    matches!(
        version,
        "TBD" | "latest-supported" | "unknown" | "do-not-upgrade-blindly"
    )
}

fn version_is_below(current: &str, minimum: &str) -> bool {
    let current = version_parts(current);
    let minimum = version_parts(minimum);
    if current.is_empty() || minimum.is_empty() {
        return true;
    }
    current < minimum
}

fn version_parts(version: &str) -> Vec<u32> {
    version
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dependency_below_minimum_emits_recommendation() {
        let report = BuildReport {
            direct_dependencies: vec![DependencyInfo {
                group_id: Some("org.ow2.asm".to_string()),
                artifact_id: "asm".to_string(),
                version: Some("9.7".to_string()),
                configuration: None,
                scope: None,
                file: None,
                source: "maven:resolved".to_string(),
            }],
            ..BuildReport::default()
        };
        let kb = JavaCompatibilityKnowledgeBase::load_default().unwrap();
        let (recommendations, unknown) = analyze_dependencies(&report, 25, &kb.dependencies);

        assert!(unknown.is_empty());
        assert!(recommendations.iter().any(|item| item.id == "asm-java25"));
    }

    #[test]
    fn unknown_dependency_is_reported() {
        let report = BuildReport {
            direct_dependencies: vec![DependencyInfo {
                group_id: Some("org.example".to_string()),
                artifact_id: "demo".to_string(),
                version: Some("1.0.0".to_string()),
                configuration: None,
                scope: None,
                file: None,
                source: "maven:resolved".to_string(),
            }],
            ..BuildReport::default()
        };
        let kb = JavaCompatibilityKnowledgeBase::load_default().unwrap();
        let (_, unknown) = analyze_dependencies(&report, 25, &kb.dependencies);

        assert_eq!(unknown.len(), 1);
        assert_eq!(unknown[0].coordinates, "org.example:demo");
    }

    #[test]
    fn known_plugin_below_minimum_emits_recommendation() {
        let report = BuildReport {
            direct_plugins: vec![PluginInfo {
                id: "maven-compiler-plugin".to_string(),
                version: Some("3.14.0".to_string()),
                file: None,
                source: "maven:resolved".to_string(),
            }],
            ..BuildReport::default()
        };
        let kb = JavaCompatibilityKnowledgeBase::load_default().unwrap();
        let (recommendations, _) = analyze_plugins(&report, 25, &kb.plugins);

        assert!(
            recommendations
                .iter()
                .any(|item| item.id == "maven-compiler-plugin-java25")
        );
    }

    #[test]
    fn plugin_rule_does_not_match_unrelated_plugin() {
        let report = BuildReport {
            direct_plugins: vec![PluginInfo {
                id: "org.graalvm.buildtools:native-maven-plugin".to_string(),
                version: Some("0.10.3".to_string()),
                file: None,
                source: "maven:resolved".to_string(),
            }],
            ..BuildReport::default()
        };
        let kb = JavaCompatibilityKnowledgeBase::load_default().unwrap();
        let (recommendations, unknown) = analyze_plugins(&report, 25, &kb.plugins);

        assert!(recommendations.is_empty());
        assert_eq!(unknown.len(), 1);
    }
}
