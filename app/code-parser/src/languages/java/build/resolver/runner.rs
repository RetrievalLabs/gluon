use std::io;
use std::path::Path;
use std::process::Command;

use crate::languages::java::build::model::{BuildReport, Diagnostic};

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

pub(crate) fn push_command_diagnostic(
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

pub(crate) fn push_missing_tool_diagnostic(
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

pub(crate) fn command_vec(executable: &str, args: &[&str]) -> Vec<String> {
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

#[cfg(unix)]
pub(crate) fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub(crate) fn is_executable(path: &Path) -> bool {
    path.exists()
}
