use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use walkdir::{DirEntry, WalkDir};

use crate::languages::java::build::model::Diagnostic;
use crate::languages::java::compatibility::model::JdkToolFinding;

pub const DEFAULT_JDK_ROOT: &str = "/opt/jdks";

#[derive(Debug, Clone)]
pub struct JdkToolOptions {
    pub enabled: bool,
    pub jdk_root: PathBuf,
    pub classes_paths: Vec<PathBuf>,
}

impl Default for JdkToolOptions {
    fn default() -> Self {
        Self {
            enabled: false,
            jdk_root: std::env::var_os("GLUON_JDK_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_JDK_ROOT)),
            classes_paths: Vec::new(),
        }
    }
}

pub fn run_jdk_tools(
    project_root: &Path,
    source_java: Option<&str>,
    target_java: u32,
    options: &JdkToolOptions,
) -> (Vec<JdkToolFinding>, Vec<Diagnostic>) {
    let mut findings = Vec::new();
    let mut diagnostics = Vec::new();

    if !options.enabled {
        return (findings, diagnostics);
    }

    let target_jdk = options.jdk_root.join(format!("jdk{target_java}"));
    let jdeps = target_jdk.join("bin").join(executable_name("jdeps"));
    let jdeprscan = target_jdk.join("bin").join(executable_name("jdeprscan"));
    if !jdeps.exists() {
        diagnostics.push(Diagnostic::warning(
            "jdk_tools",
            format!("jdeps not found: {}", jdeps.display()),
            Some(jdeps.display().to_string()),
        ));
    }
    if !jdeprscan.exists() {
        diagnostics.push(Diagnostic::warning(
            "jdk_tools",
            format!("jdeprscan not found: {}", jdeprscan.display()),
            Some(jdeprscan.display().to_string()),
        ));
    }

    if options.classes_paths.is_empty() {
        match source_java.and_then(parse_java_major) {
            Some(version) => {
                let compile_jdk = options.jdk_root.join(format!("jdk{version}"));
                diagnostics.extend(compile_project(project_root, &compile_jdk));
            }
            None => diagnostics.push(Diagnostic::warning(
                "jdk_tools",
                "source Java version is unknown; skipping compile before JDK tool scan",
                None,
            )),
        }
    }

    let classes_paths = if options.classes_paths.is_empty() {
        discover_classes_paths(project_root)
    } else {
        options.classes_paths.clone()
    };
    if classes_paths.is_empty() {
        diagnostics.push(Diagnostic::warning(
            "jdk_tools",
            format!(
                "no compiled class directories found under {}; skipping jdeps and jdeprscan",
                project_root.display()
            ),
            Some(project_root.display().to_string()),
        ));
        return (findings, diagnostics);
    }

    if jdeps.exists() {
        match run_jdeps(&jdeps, &classes_paths) {
            Ok(output) => findings.extend(parse_jdeps_output(&output)),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }
    if jdeprscan.exists() {
        match run_jdeprscan(&jdeprscan, target_java, &classes_paths) {
            Ok(output) => findings.extend(parse_jdeprscan_output(&output)),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    (findings, diagnostics)
}

fn compile_project(project_root: &Path, jdk_home: &Path) -> Vec<Diagnostic> {
    if !jdk_home.exists() {
        return vec![Diagnostic::warning(
            "jdk_tools",
            format!("compile JDK not found: {}", jdk_home.display()),
            Some(jdk_home.display().to_string()),
        )];
    }

    if project_root.join("pom.xml").exists() {
        let executable = if project_root.join("mvnw").exists() {
            project_root.join("mvnw")
        } else {
            PathBuf::from("mvn")
        };
        return run_compile_command(
            project_root,
            jdk_home,
            executable,
            ["-DskipTests", "test-compile"],
        );
    }

    if project_root.join("build.gradle").exists()
        || project_root.join("build.gradle.kts").exists()
        || project_root.join("settings.gradle").exists()
        || project_root.join("settings.gradle.kts").exists()
    {
        let executable = if project_root.join("gradlew").exists() {
            project_root.join("gradlew")
        } else {
            PathBuf::from("gradle")
        };
        return run_compile_command(project_root, jdk_home, executable, ["testClasses"]);
    }

    vec![Diagnostic::warning(
        "jdk_tools",
        format!(
            "no Maven or Gradle build file found under {}; skipping compile before JDK tool scan",
            project_root.display()
        ),
        Some(project_root.display().to_string()),
    )]
}

fn run_compile_command<I, S>(
    project_root: &Path,
    jdk_home: &Path,
    executable: PathBuf,
    args: I,
) -> Vec<Diagnostic>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<_> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect();
    let output = Command::new(&executable)
        .args(&args)
        .current_dir(project_root)
        .env("JAVA_HOME", jdk_home)
        .env("PATH", path_with_jdk_bin(jdk_home))
        .output();

    match output {
        Ok(output) if output.status.success() => Vec::new(),
        Ok(output) => vec![Diagnostic {
            severity: "warning".to_string(),
            category: "jdk_tools".to_string(),
            message: format!(
                "compile command failed before JDK tool scan: {} {}",
                executable.display(),
                args.iter()
                    .map(|arg| arg.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            file: Some(project_root.display().to_string()),
            command: Some(
                std::iter::once(executable.display().to_string())
                    .chain(args.iter().map(|arg| arg.to_string_lossy().to_string()))
                    .collect(),
            ),
            exit_code: output.status.code(),
            stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
        }],
        Err(error) => vec![Diagnostic::warning(
            "jdk_tools",
            format!(
                "failed to run compile command {}: {error}",
                executable.display()
            ),
            Some(project_root.display().to_string()),
        )],
    }
}

fn discover_classes_paths(project_root: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        let path = entry.path();
        if matches_class_dir(project_root, path) {
            paths.push(path.to_path_buf());
        }
    }
    paths.sort();
    paths.dedup();
    paths
}

fn matches_class_dir(project_root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(project_root).unwrap_or(path);
    let normalized = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    normalized.ends_with("target/classes")
        || normalized.ends_with("target/test-classes")
        || normalized.ends_with("build/classes/java/main")
        || normalized.ends_with("build/classes/java/test")
}

fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && matches!(
            entry.file_name().to_string_lossy().as_ref(),
            ".git" | ".gradle" | ".idea" | ".mvn" | "node_modules"
        )
}

fn run_jdeps(jdeps: &Path, classes_paths: &[PathBuf]) -> Result<String, Diagnostic> {
    let output = Command::new(jdeps)
        .arg("--jdk-internals")
        .args(classes_paths)
        .output()
        .map_err(|error| {
            Diagnostic::warning(
                "jdk_tools",
                format!("failed to run {}: {error}", jdeps.display()),
                Some(jdeps.display().to_string()),
            )
        })?;
    if !output.status.success() {
        return Err(command_failure_diagnostic(
            "jdeps",
            jdeps,
            &["--jdk-internals"],
            output,
        ));
    }
    Ok(combined_output(output.stdout, output.stderr))
}

fn run_jdeprscan(
    jdeprscan: &Path,
    target_java: u32,
    classes_paths: &[PathBuf],
) -> Result<String, Diagnostic> {
    let release = target_java.to_string();
    let output = Command::new(jdeprscan)
        .args(["--release", &release, "--for-removal"])
        .args(classes_paths)
        .output()
        .map_err(|error| {
            Diagnostic::warning(
                "jdk_tools",
                format!("failed to run {}: {error}", jdeprscan.display()),
                Some(jdeprscan.display().to_string()),
            )
        })?;
    if !output.status.success() {
        return Err(command_failure_diagnostic(
            "jdeprscan",
            jdeprscan,
            &["--release", &release, "--for-removal"],
            output,
        ));
    }
    Ok(combined_output(output.stdout, output.stderr))
}

pub fn parse_jdeps_output(output: &str) -> Vec<JdkToolFinding> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || !line.contains("JDK internal API") {
                return None;
            }
            let mut parts = line.split("->");
            let class_name = parts.next()?.trim();
            let target = parts
                .next()
                .unwrap_or("")
                .split("JDK internal API")
                .next()
                .unwrap_or("")
                .trim();
            Some(JdkToolFinding {
                tool: "jdeps".to_string(),
                severity: "warning".to_string(),
                class_name: Some(class_name.to_string()),
                matched_text: target.to_string(),
                source: line.to_string(),
                guidance: Some(
                    "Replace JDK internal API usage with supported Java SE APIs or maintained libraries."
                        .to_string(),
                ),
            })
        })
        .collect()
}

pub fn parse_jdeprscan_output(output: &str) -> Vec<JdkToolFinding> {
    output
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || !line.contains("forRemoval=true") {
                return None;
            }
            Some(JdkToolFinding {
                tool: "jdeprscan".to_string(),
                severity: "warning".to_string(),
                class_name: deprecated_class_name(line),
                matched_text: line.to_string(),
                source: line.to_string(),
                guidance: Some(
                    "Replace deprecated-for-removal API usage before moving to target Java."
                        .to_string(),
                ),
            })
        })
        .collect()
}

fn deprecated_class_name(line: &str) -> Option<String> {
    line.split_whitespace()
        .find(|part| part.contains('/') || part.contains('.'))
        .map(|part| part.trim_end_matches(':').replace('/', "."))
}

fn command_failure_diagnostic(
    tool: &str,
    executable: &Path,
    args: &[&str],
    output: std::process::Output,
) -> Diagnostic {
    Diagnostic {
        severity: "warning".to_string(),
        category: "jdk_tools".to_string(),
        message: format!("{tool} command failed"),
        file: Some(executable.display().to_string()),
        command: Some(
            std::iter::once(executable.display().to_string())
                .chain(args.iter().map(|arg| (*arg).to_string()))
                .collect(),
        ),
        exit_code: output.status.code(),
        stderr: Some(String::from_utf8_lossy(&output.stderr).to_string()),
    }
}

fn combined_output(stdout: Vec<u8>, stderr: Vec<u8>) -> String {
    let mut output = String::from_utf8_lossy(&stdout).to_string();
    output.push_str(&String::from_utf8_lossy(&stderr));
    output
}

fn parse_java_major(version: &str) -> Option<u32> {
    let version = version.trim();
    if let Some(rest) = version.strip_prefix("1.") {
        return rest.split('.').next()?.parse().ok();
    }
    version
        .split(|ch: char| !ch.is_ascii_digit())
        .find(|part| !part.is_empty())?
        .parse()
        .ok()
}

fn path_with_jdk_bin(jdk_home: &Path) -> std::ffi::OsString {
    let mut paths = vec![jdk_home.join("bin")];
    if let Some(existing) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&existing));
    }
    std::env::join_paths(paths).unwrap_or_default()
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn parses_jdeps_internal_api_output() {
        let findings =
            parse_jdeps_output("demo.App -> sun.misc.Unsafe JDK internal API (jdk.unsupported)\n");

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].tool, "jdeps");
        assert_eq!(findings[0].class_name.as_deref(), Some("demo.App"));
        assert_eq!(findings[0].matched_text, "sun.misc.Unsafe");
    }

    #[test]
    fn parses_jdeprscan_for_removal_output() {
        let findings = parse_jdeprscan_output(
            "class demo/App uses deprecated method java/lang/System::setSecurityManager(Ljava/lang/SecurityManager;)V (forRemoval=true)\n",
        );

        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].tool, "jdeprscan");
        assert_eq!(findings[0].class_name.as_deref(), Some("demo.App"));
    }

    #[test]
    fn discovers_multimodule_class_directories() {
        let root = test_dir("jdk-tools-discover");
        fs::create_dir_all(root.join("app/build/classes/java/main")).unwrap();
        fs::create_dir_all(root.join("lib/build/classes/java/test")).unwrap();
        fs::create_dir_all(root.join("service/target/classes")).unwrap();
        fs::create_dir_all(root.join(".gradle/ignored/build/classes/java/main")).unwrap();

        let paths = discover_classes_paths(&root);
        let relative: Vec<_> = paths
            .iter()
            .map(|path| {
                path.strip_prefix(&root)
                    .unwrap()
                    .components()
                    .map(|component| component.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .collect();

        assert_eq!(
            relative,
            vec![
                "app/build/classes/java/main",
                "lib/build/classes/java/test",
                "service/target/classes"
            ]
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
