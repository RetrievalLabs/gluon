use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

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
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(output_root);
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
