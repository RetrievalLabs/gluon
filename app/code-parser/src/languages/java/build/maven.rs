use std::collections::HashMap;
use std::path::Path;

use regex::Regex;

use crate::core::fs::{read_to_string, relative_path};
use crate::languages::java::build::BuildSystemParser;
use crate::languages::java::build::model::{
    BuildReport, BuildToolInfo, DependencyInfo, Diagnostic, JavaVersionInfo, PluginInfo,
};

pub struct MavenPomParser;

impl BuildSystemParser for MavenPomParser {
    fn build_system(&self) -> &'static str {
        "maven"
    }

    fn parse_file(&self, project_root: &Path, file: &Path, report: &mut BuildReport) {
        let Some(contents) = read_to_string(file, report) else {
            return;
        };
        let relative = relative_path(project_root, file);
        parse_pom_contents(&contents, &relative, report);
    }
}

pub fn parse_pom_contents(contents: &str, file: &str, report: &mut BuildReport) {
    if !contents.contains("<project") {
        report.diagnostics.push(Diagnostic::warning(
            "malformed_build_file",
            "pom.xml does not contain a project element",
            Some(file.to_string()),
        ));
        return;
    }

    let properties = parse_properties(contents);
    if let Some(model_version) = capture_tag(contents, "modelVersion") {
        report.build_tools.push(BuildToolInfo {
            tool: "maven-model".to_string(),
            version: Some(resolve_property(&model_version, &properties)),
            file: Some(file.to_string()),
            source: "pom.xml".to_string(),
        });
    }

    for key in [
        "maven.compiler.release",
        "maven.compiler.source",
        "maven.compiler.target",
        "java.version",
    ] {
        if let Some(version) = properties.get(key) {
            report.java_versions.push(JavaVersionInfo {
                version: resolve_property(version, &properties),
                kind: key.to_string(),
                file: file.to_string(),
                source: "pom.xml properties".to_string(),
            });
        }
    }

    for block in capture_blocks(contents, "dependency") {
        let Some(artifact_id) = capture_tag(&block, "artifactId") else {
            continue;
        };
        report.declared_dependencies.push(DependencyInfo {
            group_id: capture_tag(&block, "groupId")
                .map(|value| resolve_property(&value, &properties)),
            artifact_id: resolve_property(&artifact_id, &properties),
            version: capture_tag(&block, "version")
                .map(|value| resolve_property(&value, &properties)),
            configuration: None,
            scope: capture_tag(&block, "scope").map(|value| resolve_property(&value, &properties)),
            file: Some(file.to_string()),
            source: "pom.xml".to_string(),
        });
    }

    for block in capture_blocks(contents, "plugin") {
        let Some(artifact_id) = capture_tag(&block, "artifactId") else {
            continue;
        };
        let id = match capture_tag(&block, "groupId") {
            Some(group_id) => format!(
                "{}:{}",
                resolve_property(&group_id, &properties),
                resolve_property(&artifact_id, &properties)
            ),
            None => resolve_property(&artifact_id, &properties),
        };
        let version =
            capture_tag(&block, "version").map(|value| resolve_property(&value, &properties));
        report.declared_plugins.push(PluginInfo {
            id,
            version,
            file: Some(file.to_string()),
            source: "pom.xml".to_string(),
        });

        if artifact_id == "maven-compiler-plugin" {
            capture_compiler_configuration(&block, file, &properties, report);
        }
    }
}

fn capture_compiler_configuration(
    plugin_block: &str,
    file: &str,
    properties: &HashMap<String, String>,
    report: &mut BuildReport,
) {
    for key in ["release", "source", "target"] {
        if let Some(version) = capture_tag(plugin_block, key) {
            report.java_versions.push(JavaVersionInfo {
                version: resolve_property(&version, properties),
                kind: format!("maven.compiler.{key}"),
                file: file.to_string(),
                source: "maven-compiler-plugin".to_string(),
            });
        }
    }
}

pub(crate) fn parse_properties(contents: &str) -> HashMap<String, String> {
    let mut properties = HashMap::new();
    let Some(properties_block) = capture_tag(contents, "properties") else {
        return properties;
    };

    let property_regex = Regex::new(r"(?s)<([A-Za-z0-9_.-]+)>\s*([^<]+?)\s*</[A-Za-z0-9_.-]+>")
        .expect("valid property regex");
    for captures in property_regex.captures_iter(&properties_block) {
        let key = captures[1].trim().to_string();
        let value = captures[2].trim().to_string();
        properties.insert(key, value);
    }

    properties
}

pub(crate) fn capture_tag(contents: &str, tag: &str) -> Option<String> {
    let regex = Regex::new(&format!(r"(?s)<{tag}>\s*(.*?)\s*</{tag}>")).ok()?;
    regex
        .captures(contents)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().trim().to_string())
}

fn capture_blocks(contents: &str, tag: &str) -> Vec<String> {
    let regex = Regex::new(&format!(r"(?s)<{tag}>\s*(.*?)\s*</{tag}>")).expect("valid block regex");
    regex
        .captures_iter(contents)
        .filter_map(|captures| captures.get(1).map(|value| value.as_str().to_string()))
        .collect()
}

pub(crate) fn resolve_property(value: &str, properties: &HashMap<String, String>) -> String {
    let property_ref =
        Regex::new(r"\$\{([A-Za-z0-9_.-]+)\}").expect("valid property reference regex");
    property_ref
        .replace_all(value, |captures: &regex::Captures<'_>| {
            properties
                .get(&captures[1])
                .cloned()
                .unwrap_or_else(|| captures[0].to_string())
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_maven_dependencies_plugins_and_java_versions() {
        let pom = r#"
            <project>
              <modelVersion>4.0.0</modelVersion>
              <properties>
                <java.version>17</java.version>
                <spring.version>6.2.0</spring.version>
              </properties>
              <dependencies>
                <dependency>
                  <groupId>org.springframework</groupId>
                  <artifactId>spring-core</artifactId>
                  <version>${spring.version}</version>
                  <scope>compile</scope>
                </dependency>
              </dependencies>
              <build>
                <plugins>
                  <plugin>
                    <groupId>org.apache.maven.plugins</groupId>
                    <artifactId>maven-compiler-plugin</artifactId>
                    <version>3.13.0</version>
                    <configuration><release>21</release></configuration>
                  </plugin>
                </plugins>
              </build>
            </project>
        "#;
        let mut report = BuildReport::default();

        parse_pom_contents(pom, "pom.xml", &mut report);

        assert_eq!(report.build_tools[0].version.as_deref(), Some("4.0.0"));
        assert_eq!(report.declared_dependencies[0].artifact_id, "spring-core");
        assert_eq!(
            report.declared_dependencies[0].version.as_deref(),
            Some("6.2.0")
        );
        assert_eq!(
            report.declared_plugins[0].id,
            "org.apache.maven.plugins:maven-compiler-plugin"
        );
        assert!(
            report
                .java_versions
                .iter()
                .any(|version| version.version == "17")
        );
        assert!(
            report
                .java_versions
                .iter()
                .any(|version| version.version == "21")
        );
    }
}
