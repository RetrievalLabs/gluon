use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::languages::business::{
    BuildBusinessKgOptions, BusinessExtractionOptions, GenerateCharacterizationTestsOptions,
    Priority, build_business_kg, generate_characterization_tests,
};
use crate::languages::java::build::model::BuildReport;
use crate::languages::java::build::parse_build;
use crate::languages::java::business::{TestExtractionOptions, extract_business, extract_tests};
use crate::languages::java::compatibility::analyzer::analyze_report_with_options;
use crate::languages::java::compatibility::jdk_tools::{DEFAULT_JDK_ROOT, JdkToolOptions};

#[derive(Debug, PartialEq, Eq)]
enum CliOptions {
    ParseBuild(ParseBuildOptions),
    AnalyzeReport(AnalyzeReportOptions),
    ExtractBusiness(ExtractBusinessOptions),
    ExtractTests(ExtractTestsOptions),
    BuildBusinessKg(BuildBusinessKgCliOptions),
    GenerateCharacterizationTests(GenerateCharacterizationTestsCliOptions),
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

#[derive(Debug, PartialEq, Eq)]
struct ExtractBusinessOptions {
    path: PathBuf,
    output_dir: PathBuf,
    database: Option<PathBuf>,
    jdtls_command: String,
    jdtls_workspace: Option<PathBuf>,
    jdtls_max_in_flight: usize,
    jdtls_deep: bool,
    resume: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct ExtractTestsOptions {
    path: PathBuf,
    database: PathBuf,
    jdtls_command: String,
    jdtls_workspace: Option<PathBuf>,
    jdtls_max_in_flight: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct BuildBusinessKgCliOptions {
    database: PathBuf,
    output: Option<PathBuf>,
    source_path: PathBuf,
    min_priority: Priority,
    max_methods: Option<usize>,
    force: bool,
    resume: bool,
    max_failures: Option<usize>,
}

#[derive(Debug, PartialEq, Eq)]
struct GenerateCharacterizationTestsCliOptions {
    business_database: PathBuf,
    kg_database: PathBuf,
    source_path: PathBuf,
    output_dir: PathBuf,
    max_behaviors: Option<usize>,
    node_kind: Option<String>,
    force: bool,
    resume: bool,
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
        Ok(CliOptions::ExtractBusiness(options)) => run_extract_business(options),
        Ok(CliOptions::ExtractTests(options)) => run_extract_tests(options),
        Ok(CliOptions::BuildBusinessKg(options)) => run_build_business_kg(options),
        Ok(CliOptions::GenerateCharacterizationTests(options)) => {
            run_generate_characterization_tests(options)
        }
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
        "extract-business" => parse_extract_business_args(args).map(CliOptions::ExtractBusiness),
        "extract-tests" => parse_extract_tests_args(args).map(CliOptions::ExtractTests),
        "build-business-kg" => parse_build_business_kg_args(args).map(CliOptions::BuildBusinessKg),
        "generate-characterization-tests" => parse_generate_characterization_tests_args(args)
            .map(CliOptions::GenerateCharacterizationTests),
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

fn parse_extract_business_args(
    mut args: impl Iterator<Item = String>,
) -> Result<ExtractBusinessOptions, String> {
    let mut path = None;
    let mut output_dir = None;
    let mut database = None;
    let mut jdtls_command = "jdtls".to_string();
    let mut jdtls_workspace = None;
    let mut jdtls_max_in_flight = 32;
    let mut jdtls_deep = false;
    let mut resume = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--path" => {
                let value = args.next().ok_or("--path requires a value")?;
                path = Some(PathBuf::from(value));
            }
            "--output-dir" => {
                let value = args.next().ok_or("--output-dir requires a value")?;
                output_dir = Some(PathBuf::from(value));
            }
            "--database" => {
                let value = args.next().ok_or("--database requires a value")?;
                database = Some(PathBuf::from(value));
            }
            "--jdtls-command" => {
                jdtls_command = args.next().ok_or("--jdtls-command requires a value")?;
            }
            "--jdtls-workspace" => {
                let value = args.next().ok_or("--jdtls-workspace requires a value")?;
                jdtls_workspace = Some(PathBuf::from(value));
            }
            "--jdtls-max-in-flight" => {
                let value = args
                    .next()
                    .ok_or("--jdtls-max-in-flight requires a value")?;
                jdtls_max_in_flight = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --jdtls-max-in-flight: {value}"))?;
                if jdtls_max_in_flight == 0 {
                    return Err("--jdtls-max-in-flight must be greater than 0".to_string());
                }
            }
            "--jdtls-deep" => {
                jdtls_deep = true;
            }
            "--continue" => {
                resume = true;
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            other => return Err(format!("unsupported argument: {other}")),
        }
    }

    Ok(ExtractBusinessOptions {
        path: path.ok_or("missing required --path")?,
        output_dir: output_dir.ok_or("missing required --output-dir")?,
        database,
        jdtls_command,
        jdtls_workspace,
        jdtls_max_in_flight,
        jdtls_deep,
        resume,
    })
}

fn parse_extract_tests_args(
    mut args: impl Iterator<Item = String>,
) -> Result<ExtractTestsOptions, String> {
    let mut path = None;
    let mut database = None;
    let mut jdtls_command = "jdtls".to_string();
    let mut jdtls_workspace = None;
    let mut jdtls_max_in_flight = 32;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--path" => {
                let value = args.next().ok_or("--path requires a value")?;
                path = Some(PathBuf::from(value));
            }
            "--database" => {
                let value = args.next().ok_or("--database requires a value")?;
                database = Some(PathBuf::from(value));
            }
            "--jdtls-command" => {
                jdtls_command = args.next().ok_or("--jdtls-command requires a value")?;
            }
            "--jdtls-workspace" => {
                let value = args.next().ok_or("--jdtls-workspace requires a value")?;
                jdtls_workspace = Some(PathBuf::from(value));
            }
            "--jdtls-max-in-flight" => {
                let value = args
                    .next()
                    .ok_or("--jdtls-max-in-flight requires a value")?;
                jdtls_max_in_flight = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --jdtls-max-in-flight: {value}"))?;
                if jdtls_max_in_flight == 0 {
                    return Err("--jdtls-max-in-flight must be greater than 0".to_string());
                }
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            other => return Err(format!("unsupported argument: {other}")),
        }
    }

    Ok(ExtractTestsOptions {
        path: path.ok_or("missing required --path")?,
        database: database.ok_or("missing required --database")?,
        jdtls_command,
        jdtls_workspace,
        jdtls_max_in_flight,
    })
}

fn parse_build_business_kg_args(
    mut args: impl Iterator<Item = String>,
) -> Result<BuildBusinessKgCliOptions, String> {
    let mut database = None;
    let mut output = None;
    let mut source_path = None;
    let mut min_priority = Priority::High;
    let mut max_methods = None;
    let mut force = false;
    let mut resume = false;
    let mut max_failures = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--database" => {
                let value = args.next().ok_or("--database requires a value")?;
                database = Some(PathBuf::from(value));
            }
            "--output" => {
                let value = args.next().ok_or("--output requires a value")?;
                output = Some(PathBuf::from(value));
            }
            "--source-path" => {
                let value = args.next().ok_or("--source-path requires a value")?;
                source_path = Some(PathBuf::from(value));
            }
            "--min-priority" => {
                let value = args.next().ok_or("--min-priority requires a value")?;
                min_priority = Priority::parse(&value)?;
            }
            "--max-methods" => {
                let value = args.next().ok_or("--max-methods requires a value")?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --max-methods: {value}"))?;
                if parsed == 0 {
                    return Err("--max-methods must be greater than 0".to_string());
                }
                max_methods = Some(parsed);
            }
            "--force" => {
                force = true;
            }
            "--continue" => {
                resume = true;
            }
            "--max-failures" => {
                let value = args.next().ok_or("--max-failures requires a value")?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --max-failures: {value}"))?;
                if parsed == 0 {
                    return Err("--max-failures must be greater than 0".to_string());
                }
                max_failures = Some(parsed);
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            other => return Err(format!("unsupported argument: {other}")),
        }
    }
    if force && resume {
        return Err("--force and --continue cannot be used together".to_string());
    }

    Ok(BuildBusinessKgCliOptions {
        database: database.ok_or("missing required --database")?,
        output,
        source_path: source_path.ok_or("missing required --source-path")?,
        min_priority,
        max_methods,
        force,
        resume,
        max_failures,
    })
}

fn parse_generate_characterization_tests_args(
    mut args: impl Iterator<Item = String>,
) -> Result<GenerateCharacterizationTestsCliOptions, String> {
    let mut business_database = None;
    let mut kg_database = None;
    let mut source_path = None;
    let mut output_dir = None;
    let mut max_behaviors = None;
    let mut node_kind = None;
    let mut force = false;
    let mut resume = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--business-database" => {
                let value = args.next().ok_or("--business-database requires a value")?;
                business_database = Some(PathBuf::from(value));
            }
            "--kg-database" => {
                let value = args.next().ok_or("--kg-database requires a value")?;
                kg_database = Some(PathBuf::from(value));
            }
            "--source-path" => {
                let value = args.next().ok_or("--source-path requires a value")?;
                source_path = Some(PathBuf::from(value));
            }
            "--output-dir" => {
                let value = args.next().ok_or("--output-dir requires a value")?;
                output_dir = Some(PathBuf::from(value));
            }
            "--max-behaviors" => {
                let value = args.next().ok_or("--max-behaviors requires a value")?;
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| format!("invalid --max-behaviors: {value}"))?;
                if parsed == 0 {
                    return Err("--max-behaviors must be greater than 0".to_string());
                }
                max_behaviors = Some(parsed);
            }
            "--node-kind" => {
                node_kind = Some(args.next().ok_or("--node-kind requires a value")?);
            }
            "--force" => {
                force = true;
            }
            "--continue" => {
                resume = true;
            }
            "--help" | "-h" => return Err("help requested".to_string()),
            other => return Err(format!("unsupported argument: {other}")),
        }
    }

    if force && resume {
        return Err("--force and --continue cannot be used together".to_string());
    }

    Ok(GenerateCharacterizationTestsCliOptions {
        business_database: business_database.ok_or("missing required --business-database")?,
        kg_database: kg_database.ok_or("missing required --kg-database")?,
        source_path: source_path.ok_or("missing required --source-path")?,
        output_dir: output_dir.ok_or("missing required --output-dir")?,
        max_behaviors,
        node_kind,
        force,
        resume,
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

fn run_extract_business(options: ExtractBusinessOptions) -> i32 {
    let continue_command = extract_business_continue_command(&options);
    let extraction_options = BusinessExtractionOptions {
        path: options.path,
        output_dir: options.output_dir,
        database: options.database,
        jdtls_command: options.jdtls_command,
        jdtls_workspace: options.jdtls_workspace,
        jdtls_max_in_flight: options.jdtls_max_in_flight,
        jdtls_deep: options.jdtls_deep,
        resume: options.resume,
    };

    match extract_business(&extraction_options) {
        Ok(summary) => {
            println!("database: {}", summary.database_path);
            println!("modules: {}", summary.module_count);
            println!("classes: {}", summary.class_count);
            println!("methods: {}", summary.method_count);
            println!("relationships: {}", summary.relationship_count);
            println!(
                "candidates: high={} medium={} low={}",
                summary.high_priority_candidates,
                summary.medium_priority_candidates,
                summary.low_priority_candidates
            );
            println!("diagnostics: {}", summary.diagnostic_count);
            0
        }
        Err(error) => {
            command_failed("extract-business", &error);
            eprintln!("  continue: {continue_command}");
            if is_usage_or_jdtls_startup_error(&error) {
                2
            } else {
                1
            }
        }
    }
}

fn extract_business_continue_command(options: &ExtractBusinessOptions) -> String {
    let mut parts = vec![
        "code-parser".to_string(),
        "extract-business".to_string(),
        "--path".to_string(),
        shell_arg(&options.path),
        "--output-dir".to_string(),
        shell_arg(&options.output_dir),
    ];
    if let Some(database) = &options.database {
        parts.push("--database".to_string());
        parts.push(shell_arg(database));
    }
    parts.push("--jdtls-command".to_string());
    parts.push(shell_string(&options.jdtls_command));
    if let Some(workspace) = &options.jdtls_workspace {
        parts.push("--jdtls-workspace".to_string());
        parts.push(shell_arg(workspace));
    }
    parts.push("--jdtls-max-in-flight".to_string());
    parts.push(options.jdtls_max_in_flight.to_string());
    if options.jdtls_deep {
        parts.push("--jdtls-deep".to_string());
    }
    if !options.resume {
        parts.push("--continue".to_string());
    }
    parts.join(" ")
}

fn shell_arg(path: &Path) -> String {
    shell_string(&path.display().to_string())
}

fn shell_string(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '-' | '_' | ':' | '='))
    {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn run_extract_tests(options: ExtractTestsOptions) -> i32 {
    let extraction_options = TestExtractionOptions {
        path: options.path,
        database: options.database,
        jdtls_command: options.jdtls_command,
        jdtls_workspace: options.jdtls_workspace,
        jdtls_max_in_flight: options.jdtls_max_in_flight,
    };

    match extract_tests(&extraction_options) {
        Ok(summary) => {
            println!("database: {}", summary.database_path);
            println!("test_suites: {}", summary.suites);
            println!("test_cases: {}", summary.cases);
            println!("test_targets: {}", summary.targets);
            println!("test_assertions: {}", summary.assertions);
            println!("test_fixtures: {}", summary.fixtures);
            println!("test_entry_points: {}", summary.entry_points);
            println!("diagnostics: {}", summary.diagnostics);
            0
        }
        Err(error) => {
            command_failed("extract-tests", &error);
            if is_usage_or_jdtls_startup_error(&error) {
                2
            } else {
                1
            }
        }
    }
}

fn run_build_business_kg(options: BuildBusinessKgCliOptions) -> i32 {
    let kg_options = BuildBusinessKgOptions {
        database: options.database,
        output: options.output,
        source_path: options.source_path,
        min_priority: options.min_priority,
        max_methods: options.max_methods,
        force: options.force,
        resume: options.resume,
        max_failures: options.max_failures,
    };

    match build_business_kg(&kg_options) {
        Ok(summary) => {
            println!("build-business-kg select:");
            println!("  candidates={}", summary.candidates);
            println!("  high_priority={}", summary.high_priority_candidates);
            println!("  selected={}", summary.selected);
            println!("build-business-kg llm:");
            println!(
                "  {}/{} complete",
                summary.methods_processed, summary.selected
            );
            println!("  tool_calls={}", summary.tool_calls);
            println!("  input_tokens={}", summary.input_tokens);
            println!("  output_tokens={}", summary.output_tokens);
            println!(
                "  cache_creation_input_tokens={}",
                summary.cache_creation_input_tokens
            );
            println!(
                "  cache_read_input_tokens={}",
                summary.cache_read_input_tokens
            );
            println!("  total_tokens={}", summary.total_tokens);
            println!("  failed={}", summary.failed);
            println!("build-business-kg database:");
            println!("  path={}", summary.output_path);
            println!("  nodes={}", summary.nodes);
            println!("  edges={}", summary.edges);
            println!("  evidence={}", summary.evidence);
            if summary.failed == 0 { 0 } else { 1 }
        }
        Err(error) => {
            command_failed("build-business-kg", &error);
            if error.contains("missing ANTHROPIC_API_KEY")
                || error.contains("invalid extraction DB")
                || error.contains("does not exist")
            {
                2
            } else {
                1
            }
        }
    }
}

fn run_generate_characterization_tests(options: GenerateCharacterizationTestsCliOptions) -> i32 {
    let characterization_options = GenerateCharacterizationTestsOptions {
        business_database: options.business_database,
        kg_database: options.kg_database,
        source_path: options.source_path,
        output_dir: options.output_dir,
        max_behaviors: options.max_behaviors,
        node_kind: options.node_kind,
        force: options.force,
        resume: options.resume,
    };

    match generate_characterization_tests(&characterization_options) {
        Ok(summary) => {
            println!("generate-characterization-tests select:");
            println!("  selected_behaviors={}", summary.selected_behaviors);
            println!("  persisted_behaviors={}", summary.persisted_behaviors);
            println!("  skipped_behaviors={}", summary.skipped_behaviors);
            println!("generate-characterization-tests database:");
            println!("  path={}", summary.output_path);
            println!("  diagnostics={}", summary.diagnostics);
            if summary.diagnostics == 0 { 0 } else { 1 }
        }
        Err(error) => {
            command_failed("generate-characterization-tests", &error);
            if error.contains("does not exist")
                || error.contains("invalid business database")
                || error.contains("invalid KG database")
                || error.contains("unsupported --node-kind")
            {
                2
            } else {
                1
            }
        }
    }
}

fn print_usage() {
    eprintln!(
        "usage: code-parser parse-build --path <project-root> [--resolve] [--format json] [--output-dir <directory>]\n       code-parser analyze-report --report <build-report.json> --target-java <version> [--format json] [--output-dir <directory>] [--source-path <project-root>] [--enable-jdk-tools] [--jdk-root <directory>] [--classes-path <directory>]\n       code-parser extract-business --path <project-root> --output-dir <directory> [--database <path>] [--jdtls-command <command>] [--jdtls-workspace <directory>] [--jdtls-max-in-flight <count>] [--jdtls-deep] [--continue]\n       code-parser extract-tests --path <project-root> --database <business-extraction.db> [--jdtls-command <command>] [--jdtls-workspace <directory>] [--jdtls-max-in-flight <count>]\n       code-parser build-business-kg --database <business-extraction.db> --source-path <project-root> [--output <business-kg.db>] [--min-priority high|medium|low] [--max-methods <count>] [--max-failures <count>] [--continue] [--force]\n       code-parser generate-characterization-tests --business-database <business-extraction.db> --kg-database <business-kg.db> --source-path <legacy-project-root> --output-dir <gluon-output-dir> [--max-behaviors <count>] [--node-kind BusinessRule|Workflow|Invariant|StateTransition|SideEffect] [--continue] [--force]"
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
    eprintln!("code-parser {command} failed");
    eprintln!("  error: {error}");
}

fn is_usage_or_jdtls_startup_error(error: &str) -> bool {
    error.contains("path does not exist")
        || error.contains("JDTLS executable not found")
        || error.contains("failed to start JDTLS")
        || error.contains("JDTLS initialize request failed")
        || error.contains("JDTLS initialized notification failed")
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
    for (index, diagnostic) in errors.iter().take(5).enumerate() {
        print_diagnostic(index + 1, diagnostic);
    }
    if errors.len() > 5 {
        eprintln!(
            "- {} more error diagnostic(s) in JSON report",
            errors.len() - 5
        );
    }

    1
}

fn print_diagnostic(index: usize, diagnostic: &crate::languages::java::build::model::Diagnostic) {
    eprintln!(
        "- diagnostic {index}: [{}] {}",
        diagnostic.category, diagnostic.message
    );
    eprintln!("  severity: {}", diagnostic.severity);
    if let Some(file) = &diagnostic.file {
        eprintln!("  file: {file}");
    }
    if let Some(command) = &diagnostic.command {
        eprintln!("  command: {}", command.join(" "));
    }
    if let Some(exit_code) = diagnostic.exit_code {
        eprintln!("  exit_code: {exit_code}");
    }
    if let Some(stderr) = &diagnostic.stderr {
        eprintln!("  stderr: {stderr}");
    }
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

    #[test]
    fn parses_extract_business_arguments() {
        let options = parse_args([
            "code-parser",
            "extract-business",
            "--path",
            "project",
            "--output-dir",
            "data",
            "--database",
            "business.db",
            "--jdtls-command",
            "/bin/jdtls",
            "--jdtls-workspace",
            "workspace",
            "--jdtls-max-in-flight",
            "24",
            "--jdtls-deep",
            "--continue",
        ])
        .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::ExtractBusiness(ExtractBusinessOptions {
                path: PathBuf::from("project"),
                output_dir: PathBuf::from("data"),
                database: Some(PathBuf::from("business.db")),
                jdtls_command: "/bin/jdtls".to_string(),
                jdtls_workspace: Some(PathBuf::from("workspace")),
                jdtls_max_in_flight: 24,
                jdtls_deep: true,
                resume: true,
            })
        );
    }

    #[test]
    fn extract_business_requires_output_dir() {
        let error = parse_args(["code-parser", "extract-business", "--path", "project"])
            .expect_err("missing output dir");

        assert!(error.contains("missing required --output-dir"));
    }

    #[test]
    fn parses_extract_tests_arguments() {
        let options = parse_args([
            "code-parser",
            "extract-tests",
            "--path",
            "project",
            "--database",
            "business-extraction.db",
            "--jdtls-command",
            "/bin/jdtls",
            "--jdtls-workspace",
            "workspace",
            "--jdtls-max-in-flight",
            "16",
        ])
        .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::ExtractTests(ExtractTestsOptions {
                path: PathBuf::from("project"),
                database: PathBuf::from("business-extraction.db"),
                jdtls_command: "/bin/jdtls".to_string(),
                jdtls_workspace: Some(PathBuf::from("workspace")),
                jdtls_max_in_flight: 16,
            })
        );
    }

    #[test]
    fn extract_tests_requires_database() {
        let error = parse_args(["code-parser", "extract-tests", "--path", "project"])
            .expect_err("missing database");

        assert!(error.contains("missing required --database"));
    }

    #[test]
    fn parses_build_business_kg_arguments() {
        let options = parse_args([
            "code-parser",
            "build-business-kg",
            "--database",
            "business-extraction.db",
            "--output",
            "business-kg.db",
            "--source-path",
            "project",
            "--min-priority",
            "medium",
            "--max-methods",
            "5",
            "--max-failures",
            "3",
            "--force",
        ])
        .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::BuildBusinessKg(BuildBusinessKgCliOptions {
                database: PathBuf::from("business-extraction.db"),
                output: Some(PathBuf::from("business-kg.db")),
                source_path: PathBuf::from("project"),
                min_priority: Priority::Medium,
                max_methods: Some(5),
                force: true,
                resume: false,
                max_failures: Some(3),
            })
        );
    }

    #[test]
    fn parses_build_business_kg_continue_argument() {
        let options = parse_args([
            "code-parser",
            "build-business-kg",
            "--database",
            "business-extraction.db",
            "--source-path",
            "project",
            "--continue",
        ])
        .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::BuildBusinessKg(BuildBusinessKgCliOptions {
                database: PathBuf::from("business-extraction.db"),
                output: None,
                source_path: PathBuf::from("project"),
                min_priority: Priority::High,
                max_methods: None,
                force: false,
                resume: true,
                max_failures: None,
            })
        );
    }

    #[test]
    fn build_business_kg_rejects_invalid_priority() {
        let error = parse_args([
            "code-parser",
            "build-business-kg",
            "--database",
            "business-extraction.db",
            "--source-path",
            "project",
            "--min-priority",
            "urgent",
        ])
        .expect_err("invalid priority");

        assert!(error.contains("invalid --min-priority"));
    }

    #[test]
    fn build_business_kg_requires_database() {
        let error = parse_args([
            "code-parser",
            "build-business-kg",
            "--source-path",
            "project",
        ])
        .expect_err("missing database");

        assert!(error.contains("missing required --database"));
    }

    #[test]
    fn parses_generate_characterization_tests_arguments() {
        let options = parse_args([
            "code-parser",
            "generate-characterization-tests",
            "--business-database",
            "business-extraction.db",
            "--kg-database",
            "business-kg.db",
            "--source-path",
            "project",
            "--output-dir",
            "data",
            "--max-behaviors",
            "4",
            "--node-kind",
            "Workflow",
            "--continue",
        ])
        .expect("valid arguments");

        assert_eq!(
            options,
            CliOptions::GenerateCharacterizationTests(GenerateCharacterizationTestsCliOptions {
                business_database: PathBuf::from("business-extraction.db"),
                kg_database: PathBuf::from("business-kg.db"),
                source_path: PathBuf::from("project"),
                output_dir: PathBuf::from("data"),
                max_behaviors: Some(4),
                node_kind: Some("Workflow".to_string()),
                force: false,
                resume: true,
            })
        );
    }

    #[test]
    fn generate_characterization_tests_rejects_force_continue_conflict() {
        let error = parse_args([
            "code-parser",
            "generate-characterization-tests",
            "--business-database",
            "business-extraction.db",
            "--kg-database",
            "business-kg.db",
            "--source-path",
            "project",
            "--output-dir",
            "data",
            "--force",
            "--continue",
        ])
        .expect_err("conflicting resume options");

        assert!(error.contains("--force and --continue cannot be used together"));
    }
}
