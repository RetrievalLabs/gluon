use std::collections::BTreeSet;
use std::path::Path;

use crate::languages::java::build::maven::parse_pom_contents;
use crate::languages::java::build::model::{
    BuildReport, DependencyInfo, Diagnostic, PluginInfo, module_path_for_file,
};
use crate::languages::java::build::resolver::runner::{
    CommandRunner, is_executable, push_command_diagnostic, push_missing_tool_diagnostic,
};

pub(crate) fn resolve_maven(
    runner: &dyn CommandRunner,
    project_root: &Path,
    report: &mut BuildReport,
) {
    let executable = if project_root.join("mvnw").exists() {
        let wrapper = project_root.join("mvnw");
        if !is_executable(&wrapper) {
            report.diagnostics.push(Diagnostic::warning(
                "wrapper_not_executable",
                "mvnw exists but is not executable; trying system mvn",
                Some("mvnw".to_string()),
            ));
            "mvn".to_string()
        } else {
            wrapper.display().to_string()
        }
    } else {
        "mvn".to_string()
    };

    for scope in maven_scopes(report) {
        let effective_args = ["help:effective-pom", "-DskipTests"];
        let cwd = scope
            .as_deref()
            .map(|scope| project_root.join(scope))
            .unwrap_or_else(|| project_root.to_path_buf());
        match runner.run(&executable, &effective_args, &cwd) {
            Ok(output) if output.status == 0 => {
                parse_effective_pom(&output.stdout, scope.as_deref(), report)
            }
            Ok(output) => push_command_diagnostic(
                report,
                &executable,
                &effective_args,
                output.status,
                &output.stderr,
            ),
            Err(error) => push_missing_tool_diagnostic(report, &executable, &effective_args, error),
        }
    }
}

fn parse_effective_pom(stdout: &str, scope: Option<&str>, report: &mut BuildReport) {
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
    enrich_direct_dependency_versions(&effective.direct_dependencies, scope, report);
    enrich_direct_plugin_versions(&effective.direct_plugins, scope, report);
}

fn enrich_direct_dependency_versions(
    effective_dependencies: &[DependencyInfo],
    scope: Option<&str>,
    report: &mut BuildReport,
) {
    for dependency in &mut report.direct_dependencies {
        let Some(effective_dependency) = effective_dependencies.iter().find(|candidate| {
            file_in_scope(dependency.file.as_deref(), scope)
                && same_dependency(candidate, dependency)
        }) else {
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

fn enrich_direct_plugin_versions(
    effective_plugins: &[PluginInfo],
    scope: Option<&str>,
    report: &mut BuildReport,
) {
    for plugin in &mut report.direct_plugins {
        let Some(effective_plugin) = effective_plugins.iter().find(|candidate| {
            file_in_scope(plugin.file.as_deref(), scope) && candidate.id == plugin.id
        }) else {
            continue;
        };
        if effective_plugin.version.is_some() {
            plugin.version = effective_plugin.version.clone();
        }
    }
}

fn maven_scopes(report: &BuildReport) -> Vec<Option<String>> {
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
        .filter(|file| file.ends_with("pom.xml"))
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

fn file_in_scope(file: Option<&str>, scope: Option<&str>) -> bool {
    module_path_for_file(file).as_deref() == scope
}
