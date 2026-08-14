use std::collections::HashMap;
use std::path::Path;

use regex::Regex;

use crate::core::fs::{read_to_string, relative_path};
use crate::languages::java::build::BuildSystemParser;
use crate::languages::java::build::model::{
    BuildReport, BuildToolInfo, DependencyInfo, JavaVersionInfo, PluginInfo,
};

pub struct GradleBuildParser;
pub struct GradleWrapperParser;
pub struct GradleVersionCatalogParser;

impl BuildSystemParser for GradleBuildParser {
    fn build_system(&self) -> &'static str {
        "gradle"
    }

    fn parse_file(&self, project_root: &Path, file: &Path, report: &mut BuildReport) {
        let Some(contents) = read_to_string(file, report) else {
            return;
        };
        let relative = relative_path(project_root, file);
        parse_gradle_build_contents(&contents, &relative, report);
    }
}

impl BuildSystemParser for GradleWrapperParser {
    fn build_system(&self) -> &'static str {
        "gradle-wrapper"
    }

    fn parse_file(&self, project_root: &Path, file: &Path, report: &mut BuildReport) {
        let Some(contents) = read_to_string(file, report) else {
            return;
        };
        let relative = relative_path(project_root, file);
        parse_gradle_wrapper_contents(&contents, &relative, report);
    }
}

impl BuildSystemParser for GradleVersionCatalogParser {
    fn build_system(&self) -> &'static str {
        "gradle-version-catalog"
    }

    fn parse_file(&self, project_root: &Path, file: &Path, report: &mut BuildReport) {
        let Some(contents) = read_to_string(file, report) else {
            return;
        };
        let relative = relative_path(project_root, file);
        parse_version_catalog_contents(&contents, &relative, report);
    }
}

pub fn parse_gradle_build_contents(contents: &str, file: &str, report: &mut BuildReport) {
    parse_java_versions(contents, file, report);
    parse_plugins(contents, file, report);
    parse_dependencies(contents, file, report);
}

pub fn parse_gradle_wrapper_contents(contents: &str, file: &str, report: &mut BuildReport) {
    let regex =
        Regex::new(r"gradle-([0-9][A-Za-z0-9_.-]*)-(?:bin|all)\.zip").expect("valid wrapper regex");
    if let Some(captures) = regex.captures(contents) {
        report.build_tools.push(BuildToolInfo {
            tool: "gradle".to_string(),
            version: Some(captures[1].to_string()),
            file: Some(file.to_string()),
            source: "gradle-wrapper.properties".to_string(),
        });
    }
}

pub fn parse_version_catalog_contents(contents: &str, file: &str, report: &mut BuildReport) {
    let mut versions = HashMap::new();
    let mut section = "";

    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(&['[', ']'][..]);
            continue;
        }

        match section {
            "versions" => {
                if let Some((key, value)) = parse_assignment(line) {
                    versions.insert(key.to_string(), value.to_string());
                }
            }
            "libraries" => parse_catalog_library(line, file, &versions, report),
            "plugins" => parse_catalog_plugin(line, file, &versions, report),
            _ => {}
        }
    }
}

fn push_catalog_dependency(
    alias: &str,
    coordinate: &str,
    version_override: Option<&str>,
    file: &str,
    report: &mut BuildReport,
) {
    let parts: Vec<_> = coordinate.split(':').collect();
    if parts.len() < 2 {
        return;
    }
    report.declared_dependencies.push(DependencyInfo {
        group_id: Some(parts[0].to_string()),
        artifact_id: parts[1].to_string(),
        version: version_override
            .map(ToString::to_string)
            .or_else(|| parts.get(2).map(|value| (*value).to_string())),
        configuration: Some(alias.to_string()),
        scope: None,
        file: Some(file.to_string()),
        source: "gradle/libs.versions.toml".to_string(),
    });
}

fn parse_catalog_library(
    line: &str,
    file: &str,
    versions: &HashMap<String, String>,
    report: &mut BuildReport,
) {
    let Some((alias, declaration)) = line.split_once('=') else {
        return;
    };
    let alias = alias.trim();
    let declaration = declaration.trim();

    if declaration.starts_with('"')
        && let Some(coordinate) = quoted_value(declaration)
    {
        push_catalog_dependency(alias, coordinate, None, file, report);
        return;
    }

    let module = capture_assignment_value(declaration, "module").or_else(|| {
        let group = capture_assignment_value(declaration, "group")?;
        let name = capture_assignment_value(declaration, "name")?;
        Some(format!("{group}:{name}"))
    });
    let version = capture_version(declaration, versions);

    if let Some(module) = module {
        push_catalog_dependency(alias, &module, version.as_deref(), file, report);
    }
}

fn parse_catalog_plugin(
    line: &str,
    file: &str,
    versions: &HashMap<String, String>,
    report: &mut BuildReport,
) {
    let Some((_alias, declaration)) = line.split_once('=') else {
        return;
    };
    let Some(id) = capture_assignment_value(declaration, "id") else {
        return;
    };
    let version = capture_version(declaration, versions);
    report.declared_plugins.push(PluginInfo {
        id,
        version,
        file: Some(file.to_string()),
        source: "gradle/libs.versions.toml".to_string(),
    });
}

fn parse_assignment(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    quoted_value(value).map(|value| (key.trim(), value))
}

fn capture_assignment_value(declaration: &str, key: &str) -> Option<String> {
    let regex = Regex::new(&format!(r#"{key}\s*=\s*"([^"]+)""#)).ok()?;
    regex
        .captures(declaration)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

fn capture_version(declaration: &str, versions: &HashMap<String, String>) -> Option<String> {
    capture_assignment_value(declaration, "version\\.ref")
        .or_else(|| capture_assignment_value(declaration, "version_ref"))
        .or_else(|| capture_assignment_value(declaration, "ref"))
        .and_then(|reference| versions.get(&reference).cloned())
        .or_else(|| capture_assignment_value(declaration, "version"))
}

fn quoted_value(value: &str) -> Option<&str> {
    let start = value.find('"')?;
    let end = value[start + 1..].find('"')?;
    Some(&value[start + 1..start + 1 + end])
}

fn parse_java_versions(contents: &str, file: &str, report: &mut BuildReport) {
    let patterns = [
        (
            "sourceCompatibility",
            r#"sourceCompatibility\s*=?\s*['"]?([0-9][0-9._]*)['"]?"#,
        ),
        (
            "targetCompatibility",
            r#"targetCompatibility\s*=?\s*['"]?([0-9][0-9._]*)['"]?"#,
        ),
        (
            "java.toolchain.languageVersion",
            r#"languageVersion\s*=?\s*JavaLanguageVersion\.of\((\d+)\)"#,
        ),
        ("java.version.constant", r#"JavaVersion\.VERSION_(\d+)"#),
    ];

    for (kind, pattern) in patterns {
        let regex = Regex::new(pattern).expect("valid Java version regex");
        for captures in regex.captures_iter(contents) {
            report.java_versions.push(JavaVersionInfo {
                version: captures[1].replace('_', "."),
                kind: kind.to_string(),
                file: file.to_string(),
                source: "gradle build file".to_string(),
            });
        }
    }
}

fn parse_plugins(contents: &str, file: &str, report: &mut BuildReport) {
    let regex = Regex::new(
        r#"id\s*\(?\s*['"]([^'"]+)['"]\s*\)?(?:\s+version\s*\(?\s*['"]([^'"]+)['"]\s*\)?)?"#,
    )
    .expect("valid plugin regex");
    for captures in regex.captures_iter(contents) {
        report.declared_plugins.push(PluginInfo {
            id: captures[1].to_string(),
            version: captures.get(2).map(|value| value.as_str().to_string()),
            file: Some(file.to_string()),
            source: "gradle build file".to_string(),
        });
    }
}

fn parse_dependencies(contents: &str, file: &str, report: &mut BuildReport) {
    let regex = Regex::new(
        r#"(?m)^\s*([A-Za-z][A-Za-z0-9_]*)\s*\(?\s*['"]([^:'"]+):([^:'"]+):([^'"]+)['"]\s*\)?"#,
    )
    .expect("valid dependency regex");
    for captures in regex.captures_iter(contents) {
        report.declared_dependencies.push(DependencyInfo {
            group_id: Some(captures[2].to_string()),
            artifact_id: captures[3].to_string(),
            version: Some(captures[4].trim_end_matches(')').to_string()),
            configuration: Some(captures[1].to_string()),
            scope: None,
            file: Some(file.to_string()),
            source: "gradle build file".to_string(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_gradle_wrapper_version() {
        let mut report = BuildReport::default();
        parse_gradle_wrapper_contents(
            "distributionUrl=https\\://services.gradle.org/distributions/gradle-9.7.0-bin.zip",
            "gradle/wrapper/gradle-wrapper.properties",
            &mut report,
        );

        assert_eq!(report.build_tools[0].version.as_deref(), Some("9.7.0"));
    }

    #[test]
    fn extracts_gradle_build_data() {
        let mut report = BuildReport::default();
        parse_gradle_build_contents(
            r#"
                plugins {
                    id("java")
                    id("org.springframework.boot") version "3.5.0"
                }
                java {
                    toolchain {
                        languageVersion = JavaLanguageVersion.of(21)
                    }
                }
                dependencies {
                    implementation("org.slf4j:slf4j-api:2.0.17")
                    testImplementation 'junit:junit:4.13.2'
                }
            "#,
            "build.gradle.kts",
            &mut report,
        );

        assert!(
            report
                .java_versions
                .iter()
                .any(|version| version.version == "21")
        );
        assert!(
            report
                .declared_plugins
                .iter()
                .any(|plugin| plugin.id == "org.springframework.boot")
        );
        assert!(
            report
                .declared_dependencies
                .iter()
                .any(|dependency| dependency.artifact_id == "slf4j-api")
        );
    }

    #[test]
    fn extracts_version_catalog_libraries_and_plugins() {
        let mut report = BuildReport::default();
        parse_version_catalog_contents(
            r#"
                [versions]
                spring = "6.2.0"

                [libraries]
                spring-core = { module = "org.springframework:spring-core", version.ref = "spring" }

                [plugins]
                boot = { id = "org.springframework.boot", version = "3.5.0" }
            "#,
            "gradle/libs.versions.toml",
            &mut report,
        );

        assert_eq!(
            report.declared_dependencies[0].version.as_deref(),
            Some("6.2.0")
        );
        assert_eq!(report.declared_plugins[0].id, "org.springframework.boot");
    }
}
