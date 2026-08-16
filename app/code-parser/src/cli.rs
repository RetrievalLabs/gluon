use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::languages::java::build::parse_build;

#[derive(Debug, PartialEq, Eq)]
struct CliOptions {
    path: PathBuf,
    resolve: bool,
    output_dir: Option<PathBuf>,
}

pub fn run_cli<I, S>(args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    match parse_args(args) {
        Ok(options) => match parse_build(&options.path, options.resolve) {
            Ok(report) => match serde_json::to_string_pretty(&report) {
                Ok(json) => {
                    if let Some(output_dir) = &options.output_dir {
                        match write_report(&options.path, output_dir, &json) {
                            Ok(path) => println!("wrote {}", path.display()),
                            Err(error) => {
                                eprintln!("{error}");
                                return 1;
                            }
                        }
                    } else {
                        println!("{json}");
                    }

                    if has_error_diagnostics(&report) { 1 } else { 0 }
                }
                Err(error) => {
                    eprintln!("failed to serialize report: {error}");
                    1
                }
            },
            Err(error) => {
                eprintln!("{error}");
                2
            }
        },
        Err(error) => {
            eprintln!("{error}");
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
    if command != "parse-build" {
        return Err(format!("unsupported command: {command}"));
    }

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

    Ok(CliOptions {
        path: path.ok_or("missing required --path")?,
        resolve,
        output_dir,
    })
}

fn print_usage() {
    eprintln!(
        "usage: code-parser parse-build --path <project-root> [--resolve] [--format json] [--output-dir <directory>]"
    );
}

fn write_report(project_path: &Path, output_dir: &Path, json: &str) -> Result<PathBuf, String> {
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

    let report_path = project_output_dir.join("build-report.json");
    fs::write(&report_path, json)
        .map_err(|error| format!("failed to write report {}: {error}", report_path.display()))?;
    Ok(report_path)
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

fn has_error_diagnostics(report: &crate::languages::java::build::model::BuildReport) -> bool {
    report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == "error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_required_arguments() {
        let options = parse_args(["code-parser", "parse-build", "--path", ".", "--resolve"])
            .expect("valid arguments");

        assert_eq!(options.path, PathBuf::from("."));
        assert!(options.resolve);
        assert_eq!(options.output_dir, None);
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

        assert_eq!(options.output_dir, Some(PathBuf::from("data")));
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
}
