use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

#[test]
fn parse_build_outputs_json_for_project_root() {
    let root = test_dir("cli-success");
    fs::write(
        root.join("pom.xml"),
        r#"
        <project>
          <modelVersion>4.0.0</modelVersion>
          <properties><java.version>17</java.version></properties>
          <dependencies>
            <dependency>
              <groupId>org.example</groupId>
              <artifactId>demo</artifactId>
              <version>1.2.3</version>
            </dependency>
          </dependencies>
        </project>
        "#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["parse-build", "--path"])
        .arg(&root)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"artifact_id\": \"demo\""));
    assert!(stdout.contains("\"version\": \"17\""));
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn parse_build_rejects_missing_path() {
    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["parse-build", "--path", "/definitely/missing/gluon/project"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("path does not exist"));
}

#[test]
fn parse_build_writes_report_to_output_dir() {
    let root = test_dir("cli-output-project");
    let output_root = test_dir("cli-output-root");
    fs::write(
        root.join("pom.xml"),
        r#"
        <project>
          <modelVersion>4.0.0</modelVersion>
          <properties><java.version>21</java.version></properties>
        </project>
        "#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["parse-build", "--path"])
        .arg(&root)
        .args(["--output-dir"])
        .arg(&output_root)
        .output()
        .unwrap();

    assert!(output.status.success());
    let report_path = output_root
        .join(root.file_name().unwrap())
        .join("build-report.json");
    let report = fs::read_to_string(&report_path).unwrap();
    assert!(report.contains("\"version\": \"21\""));
    assert!(String::from_utf8_lossy(&output.stdout).contains(&report_path.display().to_string()));
    assert!(String::from_utf8_lossy(&output.stderr).contains(&format!(
        "JSON report written to: {}",
        report_path.display()
    )));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(output_root);
}

#[test]
fn analyze_report_outputs_valid_json() {
    let root = test_dir("analyze-project");
    fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    fs::write(
        root.join("src/main/java/demo/Demo.java"),
        "package demo; import javax.xml.bind.JAXBContext; class Demo {}",
    )
    .unwrap();
    let report_path = write_build_report(
        &root,
        r#""resolved_dependencies":[{"group_id":"org.ow2.asm","artifact_id":"asm","version":"9.7","configuration":null,"scope":null,"file":null,"source":"maven:resolved"}]"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["analyze-report", "--report"])
        .arg(&report_path)
        .args(["--target-java", "25"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["target_java"], 25);
    assert_eq!(value["dependency_recommendations"][0]["id"], "asm-java25");
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn analyze_report_writes_report_to_output_dir() {
    let root = test_dir("analyze-output-project");
    let output_root = test_dir("analyze-output-root");
    let report_path = write_build_report(&root, r#""resolved_dependencies":[]"#);

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["analyze-report", "--report"])
        .arg(&report_path)
        .args(["--target-java", "25", "--output-dir"])
        .arg(&output_root)
        .output()
        .unwrap();

    assert!(output.status.success());
    let compatibility_report = output_root
        .join(root.file_name().unwrap())
        .join("compatibility-report.json");
    let value: Value =
        serde_json::from_str(&fs::read_to_string(&compatibility_report).unwrap()).unwrap();
    assert_eq!(value["target_java"], 25);
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(&compatibility_report.display().to_string())
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains(&format!(
        "JSON report written to: {}",
        compatibility_report.display()
    )));
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(output_root);
}

#[test]
fn analyze_report_rejects_missing_report() {
    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args([
            "analyze-report",
            "--report",
            "/definitely/missing/gluon/build-report.json",
            "--target-java",
            "25",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("code-parser analyze-report failed"));
    assert!(stderr.contains("failed to read report"));
}

#[test]
fn analyze_report_rejects_invalid_json() {
    let root = test_dir("analyze-invalid");
    let report_path = root.join("build-report.json");
    fs::write(&report_path, "{not json").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["analyze-report", "--report"])
        .arg(&report_path)
        .args(["--target-java", "25"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("failed to parse report"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn analyze_report_warns_for_missing_source_path_and_keeps_dependency_analysis() {
    let root = test_dir("analyze-missing-source");
    let report_path = write_build_report(
        &root,
        r#""resolved_dependencies":[{"group_id":"org.example","artifact_id":"demo","version":"1.0.0","configuration":null,"scope":null,"file":null,"source":"maven:resolved"}]"#,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["analyze-report", "--report"])
        .arg(&report_path)
        .args([
            "--target-java",
            "25",
            "--source-path",
            "/definitely/missing/gluon/source",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["diagnostics"][0]["severity"], "warning");
    assert_eq!(
        value["unknown_dependencies"][0]["coordinates"],
        "org.example:demo"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn analyze_report_jdk_tools_missing_root_emits_warnings() {
    let root = test_dir("analyze-jdk-tools");
    fs::create_dir_all(root.join("target/classes")).unwrap();
    let report_path = write_build_report(&root, r#""resolved_dependencies":[]"#);
    let missing_jdk_root = root.join("missing-jdks");

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["analyze-report", "--report"])
        .arg(&report_path)
        .args(["--target-java", "25", "--enable-jdk-tools", "--jdk-root"])
        .arg(&missing_jdk_root)
        .args(["--classes-path"])
        .arg(root.join("target/classes"))
        .output()
        .unwrap();

    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| {
                diagnostic["category"] == "jdk_tools"
                    && diagnostic["message"]
                        .as_str()
                        .unwrap()
                        .contains("jdeps not found")
            })
    );
    assert_eq!(value["jdk_tool_findings"].as_array().unwrap().len(), 0);
    let _ = fs::remove_dir_all(root);
}

fn write_build_report(root: &PathBuf, dependency_fragment: &str) -> PathBuf {
    let report_path = root.join("build-report.json");
    fs::write(
        &report_path,
        format!(
            r#"{{
              "project_root":"{}",
              "build_tools":[],
              "java_versions":[{{"version":"17","kind":"release","file":"pom.xml","source":"maven:property"}}],
              "declared_dependencies":[],
              {},
              "declared_plugins":[],
              "resolved_plugins":[],
              "diagnostics":[]
            }}"#,
            root.display(),
            dependency_fragment
        ),
    )
    .unwrap();
    report_path
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
