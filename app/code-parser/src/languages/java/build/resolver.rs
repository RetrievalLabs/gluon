use std::io;
use std::path::Path;
use std::process::Command;

use regex::Regex;

use crate::languages::java::build::maven::parse_pom_contents;
use crate::languages::java::build::model::{BuildReport, DependencyInfo, Diagnostic, PluginInfo};

pub trait CommandRunner {
    fn run(&self, executable: &str, args: &[&str], cwd: &Path) -> io::Result<CommandOutput>;
}

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, executable: &str, args: &[&str], cwd: &Path) -> io::Result<CommandOutput> {
        let output = Command::new(executable)
            .args(args)
            .current_dir(cwd)
            .output()?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub struct BuildResolver<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> BuildResolver<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self { runner }
    }

    pub fn resolve(&self, project_root: &Path, report: &mut BuildReport) {
        if project_root.join("pom.xml").exists() {
            self.resolve_maven(project_root, report);
        }
        if has_gradle_build(project_root) {
            self.resolve_gradle(project_root, report);
        }
    }

    fn resolve_maven(&self, project_root: &Path, report: &mut BuildReport) {
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
        match self.runner.run(executable, &effective_args, project_root) {
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
        match self.runner.run(executable, &dependency_args, project_root) {
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

    fn resolve_gradle(&self, project_root: &Path, report: &mut BuildReport) {
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
            match self.runner.run(executable, &args, project_root) {
                Ok(output) if output.status == 0 => {
                    parse_gradle_dependencies(&output.stdout, report)
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

    report
        .java_versions
        .extend(effective.java_versions.into_iter().map(|mut version| {
            version.source = "maven help:effective-pom".to_string();
            version
        }));
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

fn push_command_diagnostic(
    report: &mut BuildReport,
    executable: &str,
    args: &[&str],
    status: i32,
    stderr: &str,
) {
    let stderr_excerpt = shortest_stderr(stderr);
    report.diagnostics.push(Diagnostic {
        severity: "warning".to_string(),
        category: categorize_stderr(stderr),
        message: format!("{executable} command failed"),
        file: None,
        command: Some(command_vec(executable, args)),
        exit_code: Some(status),
        stderr: stderr_excerpt,
    });
}

fn push_missing_tool_diagnostic(
    report: &mut BuildReport,
    executable: &str,
    args: &[&str],
    error: io::Error,
) {
    report.diagnostics.push(Diagnostic {
        severity: "warning".to_string(),
        category: "missing_tool".to_string(),
        message: format!("failed to run {executable}: {error}"),
        file: None,
        command: Some(command_vec(executable, args)),
        exit_code: None,
        stderr: None,
    });
}

fn command_vec(executable: &str, args: &[&str]) -> Vec<String> {
    std::iter::once(executable.to_string())
        .chain(args.iter().map(|arg| (*arg).to_string()))
        .collect()
}

fn shortest_stderr(stderr: &str) -> Option<String> {
    stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(500).collect())
}

fn categorize_stderr(stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("could not resolve") || lower.contains("authentication") {
        "repository_or_auth_failure".to_string()
    } else if lower.contains("unsupported class file major version")
        || lower.contains("invalid source release")
        || lower.contains("java_home")
    {
        "incompatible_jdk".to_string()
    } else if lower.contains("plugin") && lower.contains("not found") {
        "plugin_resolution_failure".to_string()
    } else {
        "build_resolution_failed".to_string()
    }
}

fn has_gradle_build(project_root: &Path) -> bool {
    [
        "build.gradle",
        "build.gradle.kts",
        "settings.gradle",
        "settings.gradle.kts",
    ]
    .iter()
    .any(|file| project_root.join(file).exists())
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::fs;
    use std::io;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::languages::java::build::parse_build_with_runner;

    use super::*;

    struct MockRunner {
        outputs: RefCell<HashMap<String, CommandOutput>>,
    }

    impl MockRunner {
        fn new(outputs: HashMap<String, CommandOutput>) -> Self {
            Self {
                outputs: RefCell::new(outputs),
            }
        }
    }

    impl CommandRunner for MockRunner {
        fn run(&self, executable: &str, args: &[&str], _cwd: &Path) -> io::Result<CommandOutput> {
            let key = command_vec(executable, args).join(" ");
            self.outputs
                .borrow_mut()
                .remove(&key)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, key))
        }
    }

    #[test]
    fn resolver_adds_maven_dependency_list_results() {
        let root = test_dir("maven-resolve");
        fs::write(
            root.join("pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion></project>",
        )
        .unwrap();
        let mut outputs = HashMap::new();
        outputs.insert(
            "mvn help:effective-pom -DskipTests".to_string(),
            CommandOutput {
                status: 0,
                stdout: "<project><dependencies><dependency><groupId>a</groupId><artifactId>b</artifactId><version>1</version></dependency></dependencies></project>".to_string(),
                stderr: String::new(),
            },
        );
        outputs.insert(
            "mvn dependency:list -DincludeScope=runtime -DoutputAbsoluteArtifactFilename=false -DskipTests"
                .to_string(),
            CommandOutput {
                status: 0,
                stdout: "[INFO]    org.slf4j:slf4j-api:jar:2.0.17:compile".to_string(),
                stderr: String::new(),
            },
        );

        let report = parse_build_with_runner(&root, true, &MockRunner::new(outputs)).unwrap();

        assert!(
            report
                .resolved_dependencies
                .iter()
                .any(|dependency| dependency.artifact_id == "slf4j-api")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolver_keeps_report_when_gradle_fails() {
        let root = test_dir("gradle-fail");
        fs::write(root.join("build.gradle"), "plugins { id 'java' }").unwrap();
        let mut outputs = HashMap::new();
        outputs.insert(
            "gradle dependencies --configuration runtimeClasspath".to_string(),
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "Could not resolve all files".to_string(),
            },
        );
        outputs.insert(
            "gradle buildEnvironment".to_string(),
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "Could not resolve plugin".to_string(),
            },
        );

        let report = parse_build_with_runner(&root, true, &MockRunner::new(outputs)).unwrap();

        assert!(
            report
                .declared_plugins
                .iter()
                .any(|plugin| plugin.id == "java")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.category == "repository_or_auth_failure")
        );
        let _ = fs::remove_dir_all(root);
    }

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "code-parser-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
