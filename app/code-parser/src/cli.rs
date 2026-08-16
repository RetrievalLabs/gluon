use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::languages::java::build::model::BuildReport;
use crate::languages::java::build::parse_build;
use crate::languages::java::compatibility::analyzer::analyze_report_with_options;
use crate::languages::java::compatibility::jdk_tools::{DEFAULT_JDK_ROOT, JdkToolOptions};

#[derive(Debug, PartialEq, Eq)]
enum CliOptions {
    ParseBuild(ParseBuildOptions),
    AnalyzeReport(AnalyzeReportOptions),
}

#[derive(Debug, PartialEq, Eq)]
struct ParseBuildOptions {
    path: PathBuf,
    resolve: bool,
    output_dir: Option<PathBuf>,
}

#[derive(Debug, PartialEq, Eq)]
struct AnalyzeReportOptions {
    report: PathBuf,
    target_java: u32,
    format: String,
    output_dir: Option<PathBuf>,
    source_path: Option<PathBuf>,
    enable_jdk_tools: bool,
    jdk_root: PathBuf,
    classes_paths: Vec<PathBuf>,
}

pub fn run_cli<I, S>(args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    match parse_args(args) {
        Ok(CliOptions::ParseBuild(options)) => match parse_build(&options.path, options.resolve) {
            Ok(report) => match serde_json::to_string_pretty(&report) {
                Ok(json) => {
                    if let Some(output_dir) = &options.output_dir {
                        match write_report(&options.path, output_dir, "build-report.json", &json) {
                            Ok(path) => report_written(&path),
                            Err(error) => {
                                command_failed("parse-build", &error);
                                return 1;
                            }
                        }
                    } else {
                        println!("{json}");
                    }

                    exit_code_for_report("parse-build", &report)
                }
                Err(error) => {
                    command_failed(
                        "parse-build",
                        &format!("failed to serialize report: {error}"),
                    );
                    1
                }
            },
            Err(error) => {
                command_failed("parse-build", &error);
                2
            }
        },
        Ok(CliOptions::AnalyzeReport(options)) => run_analyze_report(options),
        Err(error) => {
            command_failed("arguments", &error);
            print_usage();
            2
        }
    }
}

fn parse_args<I, S>(args: I) -> Result<CliOptions, String>
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    let mut args = args
        .into_iter()
        .map(|arg| arg.into().to_string_lossy().into_owned());
    let _program = args.next();
    let command = args.next().ok_or("missing command")?;
    if command == "--help" || command == "-h" {
        return Err("help requested".to_string());
    }
    match command.as_str() {
        "parse-build" => parse_parse_build_args(args).map(CliOptions::ParseBuild),
        "analyze-report" => parse_analyze_report_args(args).map(CliOptions::AnalyzeReport),
        _ => Err(format!("unsupported command: {command}")),
    }
}

fn parse_parse_build_args(
    mut args: impl Iterator<Item = String>,
) -> Result<ParseBuildOptions, String> {
    let mut path = None;
    let mut resolve = false;
    let mut format = "json".to_string();
    let mut output_dir = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--path" => {
                let value = args.next().ok_or("--path requires a value")?;
                path = Some(PathBuf::from(value));
            }
            "--format" => {
                format = args.next().ok_or("--format requires a value")?;
            }
            "--resolve" => {
                resolve = true;
            }
            "--output-dir" => {
                let value = args.next().ok_or("--output-dir requires a value")?;
                output_dir = Some(PathBuf::from(value));
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            other => return Err(format!("unsupported argument: {other}")),
        }
    }

    if format != "json" {
        return Err(format!("unsupported format: {format}"));
    }

    Ok(ParseBuildOptions {
        path: path.ok_or("missing required --path")?,
        resolve,
        output_dir,
    })
}

fn parse_analyze_report_args(
    mut args: impl Iterator<Item = String>,
) -> Result<AnalyzeReportOptions, String> {
    let mut report = None;
    let mut target_java = None;
    let mut format = "json".to_string();
    let mut output_dir = None;
    let mut source_path = None;
    let mut enable_jdk_tools = false;
    let mut jdk_root = default_jdk_root();
    let mut classes_paths = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--report" => {
                let value = args.next().ok_or("--report requires a value")?;
                report = Some(PathBuf::from(value));
            }
            "--target-java" => {
                let value = args.next().ok_or("--target-java requires a value")?;
                target_java = Some(
                    value
                        .parse::<u32>()
                        .map_err(|_| format!("invalid --target-java: {value}"))?,
                );
            }
            "--format" => {
                format = args.next().ok_or("--format requires a value")?;
            }
            "--output-dir" => {
                let value = args.next().ok_or("--output-dir requires a value")?;
                output_dir = Some(PathBuf::from(value));
            }
            "--source-path" => {
                let value = args.next().ok_or("--source-path requires a value")?;
                source_path = Some(PathBuf::from(value));
            }
            "--enable-jdk-tools" => {
                enable_jdk_tools = true;
            }
            "--jdk-root" => {
                let value = args.next().ok_or("--jdk-root requires a value")?;
                jdk_root = PathBuf::from(value);
            }
            "--classes-path" => {
                let value = args.next().ok_or("--classes-path requires a value")?;
                classes_paths.push(PathBuf::from(value));
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            other => return Err(format!("unsupported argument: {other}")),
        }
    }

    if format != "json" {
        return Err(format!("unsupported format: {format}"));
    }

    Ok(AnalyzeReportOptions {
        report: report.ok_or("missing required --report")?,
        target_java: target_java.ok_or("missing required --target-java")?,
        format,
        output_dir,
        source_path,
        enable_jdk_tools,
        jdk_root,
        classes_paths,
    })
}

fn run_analyze_report(options: AnalyzeReportOptions) -> i32 {
    let build_report = match read_build_report(&options.report) {
        Ok(report) => report,
        Err(error) => {
            command_failed("analyze-report", &error);
            return 2;
        }
    };
    let source_path = options
        .source_path
        .clone()
        .unwrap_or_else(|| PathBuf::from(&build_report.project_root));

    let jdk_tool_options = JdkToolOptions {
        enabled: options.enable_jdk_tools,
        jdk_root: options.jdk_root,
        classes_paths: options.classes_paths,
    };

    match analyze_report_with_options(
        &build_report,
        options.target_java,
        &source_path,
        &jdk_tool_options,
    ) {
        Ok(report) => match serde_json::to_string_pretty(&report) {
            Ok(json) => {
                if let Some(output_dir) = &options.output_dir {
                    match write_report(&source_path, output_dir, "compatibility-report.json", &json)
                    {
                        Ok(path) => report_written(&path),
                        Err(error) => {
                            command_failed("analyze-report", &error);
                            return 1;
                        }
                    }
                } else {
                    println!("{json}");
                }

                exit_code_for_report("analyze-report", &report)
            }
            Err(error) => {
                command_failed(
                    "analyze-report",
                    &format!("failed to serialize compatibility report: {error}"),
                );
                1
            }
        },
        Err(error) => {
            command_failed("analyze-report", &error);
            2
        }
    }
}

fn print_usage() {
    eprintln!(
        "usage: code-parser parse-build --path <project-root> [--resolve] [--format json] [--output-dir <directory>]\n       code-parser analyze-report --report <build-report.json> --target-java <version> [--format json] [--output-dir <directory>] [--source-path <project-root>] [--enable-jdk-tools] [--jdk-root <directory>] [--classes-path <directory>]"
    );
}

fn default_jdk_root() -> PathBuf {
    std::env::var_os("GLUON_JDK_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_JDK_ROOT))
}

fn read_build_report(path: &Path) -> Result<BuildReport, String> {
    let data = fs::read_to_string(path)
        .map_err(|error| format!("failed to read report {}: {error}", path.display()))?;
    serde_json::from_str(&data)
        .map_err(|error| format!("failed to parse report {}: {error}", path.display()))
}

fn write_report(
    project_path: &Path,
    output_dir: &Path,
    file_name: &str,
    json: &str,
) -> Result<PathBuf, String> {
    let project_name = project_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(sanitize_path_segment)
        .ok_or_else(|| {
            format!(
                "path has no usable directory name: {}",
                project_path.display()
            )
        })?;
    let project_output_dir = output_dir.join(project_name);
    fs::create_dir_all(&project_output_dir).map_err(|error| {
        format!(
            "failed to create output directory {}: {error}",
            project_output_dir.display()
        )
    })?;

    let report_path = project_output_dir.join(file_name);
    fs::write(&report_path, json)
        .map_err(|error| format!("failed to write report {}: {error}", report_path.display()))?;
    Ok(report_path)
}

fn report_written(path: &Path) {
    println!("wrote {}", path.display());
    eprintln!("JSON report written to: {}", path.display());
}

fn command_failed(command: &str, error: &str) {
    eprintln!("code-parser {command} failed: {error}");
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

trait ReportDiagnostics {
    fn diagnostics(&self) -> &[crate::languages::java::build::model::Diagnostic];
}

impl ReportDiagnostics for crate::languages::java::build::model::BuildReport {
    fn diagnostics(&self) -> &[crate::languages::java::build::model::Diagnostic] {
        &self.diagnostics
    }
}

impl ReportDiagnostics for crate::languages::java::compatibility::model::CompatibilityReport {
    fn diagnostics(&self) -> &[crate::languages::java::build::model::Diagnostic] {
        &self.diagnostics
    }
}

fn exit_code_for_report(command: &str, report: &impl ReportDiagnostics) -> i32 {
    let errors: Vec<_> = report
        .diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.severity == "error")
        .collect();

    if errors.is_empty() {
        return 0;
    }

    eprintln!(
        "code-parser {command} completed with {} error diagnostic(s)",
        errors.len()
    );
    for diagnostic in errors.iter().take(5) {
        eprintln!("- [{}] {}", diagnostic.category, diagnostic.message);
        if let Some(command) = &diagnostic.command {
            eprintln!("  command: {}", command.join(" "));
        }
    }
    if errors.len() > 5 {
        eprintln!(
            "- {} more error diagnostic(s) in JSON report",
            errors.len() - 5
        );
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_required_arguments() {
        let options = parse_args(["code-parser", "parse-build", "--path", ".", "--resolve"])
            .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::ParseBuild(ParseBuildOptions {
                path: PathBuf::from("."),
                resolve: true,
                output_dir: None,
            })
        );
    }

    #[test]
    fn parses_output_dir() {
        let options = parse_args([
            "code-parser",
            "parse-build",
            "--path",
            ".",
            "--output-dir",
            "data",
        ])
        .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::ParseBuild(ParseBuildOptions {
                path: PathBuf::from("."),
                resolve: false,
                output_dir: Some(PathBuf::from("data")),
            })
        );
    }

    #[test]
    fn rejects_unknown_format() {
        let error = parse_args([
            "code-parser",
            "parse-build",
            "--path",
            ".",
            "--format",
            "yaml",
        ])
        .expect_err("unsupported format");

        assert!(error.contains("unsupported format"));
    }

    #[test]
    fn parses_analyze_report_arguments() {
        let options = parse_args([
            "code-parser",
            "analyze-report",
            "--report",
            "build-report.json",
            "--target-java",
            "25",
            "--source-path",
            "project",
            "--output-dir",
            "data",
        ])
        .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::AnalyzeReport(AnalyzeReportOptions {
                report: PathBuf::from("build-report.json"),
                target_java: 25,
                format: "json".to_string(),
                output_dir: Some(PathBuf::from("data")),
                source_path: Some(PathBuf::from("project")),
                enable_jdk_tools: false,
                jdk_root: default_jdk_root(),
                classes_paths: Vec::new(),
            })
        );
    }

    #[test]
    fn parses_analyze_report_jdk_tool_arguments() {
        let options = parse_args([
            "code-parser",
            "analyze-report",
            "--report",
            "build-report.json",
            "--target-java",
            "25",
            "--enable-jdk-tools",
            "--jdk-root",
            "/vm/jdks",
            "--classes-path",
            "target/classes",
            "--classes-path",
            "target/test-classes",
        ])
        .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::AnalyzeReport(AnalyzeReportOptions {
                report: PathBuf::from("build-report.json"),
                target_java: 25,
                format: "json".to_string(),
                output_dir: None,
                source_path: None,
                enable_jdk_tools: true,
                jdk_root: PathBuf::from("/vm/jdks"),
                classes_paths: vec![
                    PathBuf::from("target/classes"),
                    PathBuf::from("target/test-classes"),
                ],
            })
        );
    }
}
