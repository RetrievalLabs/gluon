use std::path::Path;

use regex::Regex;

use crate::languages::java::build::model::{BuildReport, DependencyInfo, Diagnostic, PluginInfo};
use crate::languages::java::build::resolver::runner::{
    CommandRunner, is_executable, push_command_diagnostic, push_missing_tool_diagnostic,
};

pub(crate) fn resolve_gradle(
    runner: &dyn CommandRunner,
    project_root: &Path,
    report: &mut BuildReport,
) {
    let executable = if project_root.join("gradlew").exists() {
        if !is_executable(&project_root.join("gradlew")) {
            report.diagnostics.push(Diagnostic::warning(
                "wrapper_not_executable",
                "gradlew exists but is not executable; trying system gradle",
                Some("gradlew".to_string()),
            ));
            "gradle"
        } else {
            "./gradlew"
        }
    } else {
        "gradle"
    };

    for args in [
        ["dependencies", "--configuration", "runtimeClasspath"],
        ["buildEnvironment", "", ""],
    ] {
        let args: Vec<_> = args.into_iter().filter(|arg| !arg.is_empty()).collect();
        match runner.run(executable, &args, project_root) {
            Ok(output) if output.status == 0 => parse_gradle_dependencies(&output.stdout, report),
            Ok(output) => {
                push_command_diagnostic(report, executable, &args, output.status, &output.stderr)
            }
            Err(error) => push_missing_tool_diagnostic(report, executable, &args, error),
        }
    }
}

fn parse_gradle_dependencies(stdout: &str, report: &mut BuildReport) {
    let dependency_regex = Regex::new(
        r"(?m)(?:---|\+---|\\---)\s+([A-Za-z0-9_.-]+):([A-Za-z0-9_.-]+):([^\s()]+)(?:\s+->\s+([^\s()]+))?",
    )
    .expect("valid Gradle dependency regex");
    for captures in dependency_regex.captures_iter(stdout) {
        report.resolved_dependencies.push(DependencyInfo {
            group_id: Some(captures[1].to_string()),
            artifact_id: captures[2].to_string(),
            version: captures
                .get(4)
                .or_else(|| captures.get(3))
                .map(|value| value.as_str().to_string()),
            configuration: None,
            scope: None,
            file: None,
            source: "gradle dependencies".to_string(),
        });
    }

    let plugin_regex =
        Regex::new(r"(?m)(?:---|\+---|\\---)\s+([A-Za-z0-9_.-]+):gradle-plugin:([^\s()]+)")
            .expect("valid Gradle plugin regex");
    for captures in plugin_regex.captures_iter(stdout) {
        report.resolved_plugins.push(PluginInfo {
            id: captures[1].to_string(),
            version: Some(captures[2].to_string()),
            file: None,
            source: "gradle buildEnvironment".to_string(),
        });
    }
}

pub(crate) fn has_gradle_build(project_root: &Path) -> bool {
    [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .any(|file| project_root.join(file).exists())
}
