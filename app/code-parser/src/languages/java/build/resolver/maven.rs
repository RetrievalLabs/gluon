use std::path::Path;

use crate::languages::java::build::maven::parse_pom_contents;
use crate::languages::java::build::model::{BuildReport, DependencyInfo, Diagnostic, PluginInfo};
use crate::languages::java::build::resolver::runner::{
    CommandRunner, is_executable, push_command_diagnostic, push_missing_tool_diagnostic,
};

pub(crate) fn resolve_maven(
    runner: &dyn CommandRunner,
    project_root: &Path,
    report: &mut BuildReport,
) {
    let executable = if project_root.join("mvnw").exists() {
        if !is_executable(&project_root.join("mvnw")) {
            report.diagnostics.push(Diagnostic::warning(
                "wrapper_not_executable",
                "mvnw exists but is not executable; trying system mvn",
                Some("mvnw".to_string()),
            ));
            "mvn"
        } else {
            "./mvnw"
        }
    } else {
        "mvn"
    };

    let effective_args = ["help:effective-pom", "-DskipTests"];
    match runner.run(executable, &effective_args, project_root) {
        Ok(output) if output.status == 0 => parse_effective_pom(&output.stdout, report),
        Ok(output) => push_command_diagnostic(
            report,
            executable,
            &effective_args,
            output.status,
            &output.stderr,
        ),
        Err(error) => push_missing_tool_diagnostic(report, executable, &effective_args, error),
    }
}

fn parse_effective_pom(stdout: &str, report: &mut BuildReport) {
    let Some(start) = stdout.find("<project") else {
        report.diagnostics.push(Diagnostic::warning(
            "unsupported_output",
            "maven help:effective-pom output did not contain a project element",
            None,
        ));
        return;
    };
    let xml = &stdout[start..];
    let mut effective = BuildReport::default();
    parse_pom_contents(xml, "effective-pom", &mut effective);

    for mut version in effective.java_versions {
        version.source = "maven help:effective-pom".to_string();
        report.push_java_version(version);
    }
    enrich_direct_dependency_versions(&effective.direct_dependencies, report);
    enrich_direct_plugin_versions(&effective.direct_plugins, report);
}

fn enrich_direct_dependency_versions(
    effective_dependencies: &[DependencyInfo],
    report: &mut BuildReport,
) {
    for dependency in &mut report.direct_dependencies {
        let Some(effective_dependency) = effective_dependencies
            .iter()
            .find(|candidate| same_dependency(candidate, dependency))
        else {
            continue;
        };
        if effective_dependency.version.is_some() {
            dependency.version = effective_dependency.version.clone();
        }
    }
}

fn same_dependency(left: &DependencyInfo, right: &DependencyInfo) -> bool {
    left.group_id == right.group_id && left.artifact_id == right.artifact_id
}

fn enrich_direct_plugin_versions(effective_plugins: &[PluginInfo], report: &mut BuildReport) {
    for plugin in &mut report.direct_plugins {
        let Some(effective_plugin) = effective_plugins
            .iter()
            .find(|candidate| candidate.id == plugin.id)
        else {
            continue;
        };
        if effective_plugin.version.is_some() {
            plugin.version = effective_plugin.version.clone();
        }
    }
}
