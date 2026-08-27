use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use regex::Regex;

use crate::languages::java::build::model::{BuildReport, Diagnostic, module_path_for_file};
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

    for scope in gradle_scopes(report) {
        for configuration in configurations_for_scope(report, scope.as_deref()) {
            let task = scoped_task(scope.as_deref(), "dependencies");
            let args = [task.as_str(), "--configuration", configuration.as_str()];
            match runner.run(executable, &args, project_root) {
                Ok(output) if output.status == 0 => {
                    parse_gradle_dependencies(&output.stdout, scope.as_deref(), report)
                }
                Ok(output) => push_command_diagnostic(
                    report,
                    executable,
                    &args,
                    output.status,
                    &output.stderr,
                ),
                Err(error) => push_missing_tool_diagnostic(report, executable, &args, error),
            }
        }

        let task = scoped_task(scope.as_deref(), "buildEnvironment");
        let args = [task.as_str()];
        match runner.run(executable, &args, project_root) {
            Ok(output) if output.status == 0 => {
                parse_gradle_dependencies(&output.stdout, scope.as_deref(), report)
            }
            Ok(output) => {
                push_command_diagnostic(report, executable, &args, output.status, &output.stderr)
            }
            Err(error) => push_missing_tool_diagnostic(report, executable, &args, error),
        }
    }
}

fn parse_gradle_dependencies(stdout: &str, scope: Option<&str>, report: &mut BuildReport) {
    let dependency_regex = Regex::new(
        r"(?m)^(?:\+---|\\---)\s+([A-Za-z0-9_.-]+):([A-Za-z0-9_.-]+)(?::([^\s()]+))?(?:\s+->\s+([^\s()]+))?",
    )
    .expect("valid Gradle dependency regex");
    for captures in dependency_regex.captures_iter(stdout) {
        let group_id = Some(captures[1].to_string());
        let artifact_id = captures[2].to_string();
        let version = captures
            .get(4)
            .or_else(|| captures.get(3))
            .map(|value| value.as_str().to_string());
        for dependency in report.direct_dependencies.iter_mut().filter(|dependency| {
            file_in_scope(dependency.file.as_deref(), scope)
                && dependency.group_id == group_id
                && dependency.artifact_id == artifact_id
        }) {
            dependency.version = version.clone();
        }
    }

    let plugin_regex =
        Regex::new(r"(?m)(?:---|\+---|\\---)\s+([A-Za-z0-9_.-]+):gradle-plugin:([^\s()]+)")
            .expect("valid Gradle plugin regex");
    for captures in plugin_regex.captures_iter(stdout) {
        let id = captures[1].to_string();
        for plugin in report
            .direct_plugins
            .iter_mut()
            .filter(|plugin| file_in_scope(plugin.file.as_deref(), scope) && plugin.id == id)
        {
            plugin.version = Some(captures[2].to_string());
        }
    }
}

fn gradle_scopes(report: &BuildReport) -> Vec<Option<String>> {
    let mut has_root = false;
    let mut modules = BTreeSet::new();
    for file in report
        .direct_dependencies
        .iter()
        .filter_map(|dependency| dependency.file.as_deref())
        .chain(
            report
                .direct_plugins
                .iter()
                .filter_map(|plugin| plugin.file.as_deref()),
        )
    {
        if let Some(module) = module_path_for_file(Some(file)) {
            modules.insert(module);
        } else {
            has_root = true;
        }
    }

    let mut scopes = Vec::new();
    if has_root {
        scopes.push(None);
    }
    scopes.extend(modules.into_iter().map(Some));
    scopes
}

fn configurations_for_scope(report: &BuildReport, scope: Option<&str>) -> Vec<String> {
    let mut configurations = Vec::new();
    let mut seen = HashSet::new();
    for dependency in report
        .direct_dependencies
        .iter()
        .filter(|dependency| file_in_scope(dependency.file.as_deref(), scope))
    {
        let Some(configuration) = dependency.configuration.as_deref() else {
            continue;
        };
        for candidate in dependency_configurations(configuration) {
            if seen.insert(candidate.to_string()) {
                configurations.push(candidate.to_string());
            }
        }
    }
    configurations
}

fn dependency_configurations(configuration: &str) -> Vec<&str> {
    match configuration {
        "api" | "compile" | "compileOnly" | "implementation" => {
            vec!["compileClasspath", "runtimeClasspath"]
        }
        "runtime" | "runtimeOnly" => vec!["runtimeClasspath"],
        "testCompile" | "testCompileOnly" | "testImplementation" => {
            vec!["testCompileClasspath", "testRuntimeClasspath"]
        }
        "testRuntime" | "testRuntimeOnly" => vec!["testRuntimeClasspath"],
        "classpath" => Vec::new(),
        value => vec![value],
    }
}

fn scoped_task(scope: Option<&str>, task: &str) -> String {
    match scope {
        Some(scope) => format!(":{}:{task}", scope.replace('/', ":")),
        None => task.to_string(),
    }
}

fn file_in_scope(file: Option<&str>, scope: Option<&str>) -> bool {
    module_path_for_file(file).as_deref() == scope
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
