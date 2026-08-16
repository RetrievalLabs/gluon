use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::languages::java::build::model::{
    BuildReport, BuildToolInfo, DependencyInfo, PluginInfo,
};
use crate::languages::java::compatibility::knowledge_base::{
    CompatibilityRule, JavaCompatibilityKnowledgeBase, MatchRule, ReplacementRule,
};
use crate::languages::java::compatibility::model::{
    CodeChangeRecommendation, CompatibilityReport, DependencyRecommendation, PluginRecommendation,
    UnknownDependency, UnknownPlugin,
};
use crate::languages::java::compatibility::source_scan::scan_java_sources;

const UNKNOWN_MESSAGE: &str = "No KB rule; verify via official docs or ask LLM/research agent.";

pub fn analyze_report(
    build_report: &BuildReport,
    target_java: u32,
    source_path: &Path,
) -> Result<CompatibilityReport, String> {
    let kb = JavaCompatibilityKnowledgeBase::load_default()?;
    let mut diagnostics = Vec::new();
    let source_java = detect_source_java(build_report);

    let (dependency_recommendations, unknown_dependencies) =
        analyze_dependencies(build_report, target_java, &kb.dependencies);
    let (plugin_recommendations, unknown_plugins) =
        analyze_plugins(build_report, target_java, &kb.plugins);

    let (api_findings, scan_diagnostics) = scan_java_sources(
        source_path,
        target_java,
        &[
            ("removed_api", &kb.removed_apis),
            ("deprecated_for_removal_api", &kb.deprecated_for_removal),
            ("internal_api", &kb.internal_apis),
            ("reflective_access", &kb.reflective_access),
        ],
    );
    diagnostics.extend(scan_diagnostics);

    let code_change_recommendations =
        derive_code_change_recommendations(&api_findings, &kb.replacements, target_java)
            .into_iter()
            .chain(
                kb.migration_steps
                    .iter()
                    .map(|step| CodeChangeRecommendation {
                        id: step.id.clone(),
                        source: "incremental_migration".to_string(),
                        reason: step.action.clone(),
                        guidance: step.guidance.clone().unwrap_or_else(|| step.action.clone()),
                        related_findings: Vec::new(),
                        source_ids: Vec::new(),
                    }),
            )
            .collect();

    Ok(CompatibilityReport {
        source_java,
        target_java,
        dependency_recommendations,
        plugin_recommendations,
        api_findings,
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
    let declared_sources = dependency_sources(&build_report.declared_dependencies);
    let inventory = if build_report.resolved_dependencies.is_empty() {
        &build_report.declared_dependencies
    } else {
        &build_report.resolved_dependencies
    };
    let mut recommendations = Vec::new();
    let mut unknown = Vec::new();

    for dependency in inventory {
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
                        source: declared_sources
                            .get(&dependency_key(dependency))
                            .cloned()
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
    let inventory = if build_report.resolved_plugins.is_empty() {
        &build_report.declared_plugins
    } else {
        &build_report.resolved_plugins
    };
    let mut recommendations = Vec::new();
    let mut unknown = Vec::new();
    let mut matched_plugin_ids = HashSet::new();

    for plugin in inventory {
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
                        source: plugin.source.clone(),
                    });
                }
            }
            None => unknown.push(UnknownPlugin {
                plugin: plugin.id.clone(),
                version: plugin.version.clone(),
                source: plugin.source.clone(),
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

fn dependency_sources(dependencies: &[DependencyInfo]) -> HashMap<String, String> {
    dependencies
        .iter()
        .map(|dependency| (dependency_key(dependency), dependency.source.clone()))
        .collect()
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
            resolved_dependencies: vec![DependencyInfo {
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
            resolved_dependencies: vec![DependencyInfo {
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
            resolved_plugins: vec![PluginInfo {
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
            resolved_plugins: vec![PluginInfo {
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
