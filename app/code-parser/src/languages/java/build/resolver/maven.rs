use std::path::Path;

use regex::Regex;

use crate::languages::java::build::maven::parse_pom_contents;
use crate::languages::java::build::model::{BuildReport, DependencyInfo, Diagnostic};
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

    let dependency_args = [
        "dependency:list",
        "-DincludeScope=runtime",
        "-DoutputAbsoluteArtifactFilename=false",
        "-DskipTests",
    ];
    match runner.run(executable, &dependency_args, project_root) {
        Ok(output) if output.status == 0 => parse_maven_dependency_list(&output.stdout, report),
        Ok(output) => push_command_diagnostic(
            report,
            executable,
            &dependency_args,
            output.status,
            &output.stderr,
        ),
        Err(error) => push_missing_tool_diagnostic(report, executable, &dependency_args, error),
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
    report
        .resolved_dependencies
        .extend(
            effective
                .declared_dependencies
                .into_iter()
                .map(|mut dependency| {
                    dependency.source = "maven help:effective-pom".to_string();
                    dependency.file = None;
                    dependency
                }),
        );
    report
        .resolved_plugins
        .extend(effective.declared_plugins.into_iter().map(|mut plugin| {
            plugin.source = "maven help:effective-pom".to_string();
            plugin.file = None;
            plugin
        }));
}

fn parse_maven_dependency_list(stdout: &str, report: &mut BuildReport) {
    let regex = Regex::new(
        r"(?m)^\[INFO\]\s+([A-Za-z0-9_.-]+):([A-Za-z0-9_.-]+):[^:\s]+:(?:[^:\s]+:)?([^:\s]+):([A-Za-z0-9_.-]+)",
    )
    .expect("valid Maven dependency list regex");
    for captures in regex.captures_iter(stdout) {
        report.resolved_dependencies.push(DependencyInfo {
            group_id: Some(captures[1].to_string()),
            artifact_id: captures[2].to_string(),
            version: Some(captures[3].to_string()),
            configuration: None,
            scope: Some(captures[4].to_string()),
            file: None,
            source: "maven dependency:list".to_string(),
        });
    }
}
