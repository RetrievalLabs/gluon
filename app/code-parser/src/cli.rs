use std::ffi::OsString;
use std::path::PathBuf;

use crate::languages::java::build::parse_build;

#[derive(Debug, PartialEq, Eq)]
struct CliOptions {
    path: PathBuf,
    resolve: bool,
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
                    println!("{json}");
                    if report
                        .diagnostics
                        .iter()
                        .any(|diagnostic| diagnostic.severity == "error")
                    {
                        1
                    } else {
                        0
                    }
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
    })
}

fn print_usage() {
    eprintln!("usage: code-parser parse-build --path <project-root> [--resolve] [--format json]");
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
