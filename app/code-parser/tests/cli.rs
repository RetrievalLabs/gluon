use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
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

#[test]
fn extract_business_rejects_missing_jdtls_with_verbose_error() {
    let root = test_dir("extract-missing-jdtls");
    fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    fs::write(
        root.join("src/main/java/demo/Demo.java"),
        "package demo; class Demo { void run() {} }",
    )
    .unwrap();
    let output_root = test_dir("extract-missing-jdtls-output");

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["extract-business", "--path"])
        .arg(&root)
        .args(["--output-dir"])
        .arg(&output_root)
        .args(["--jdtls-command", "definitely-missing-jdtls-for-gluon-test"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("JDTLS executable not found"));
    assert!(stderr.contains("PATH:"));
    assert!(stderr.contains("--jdtls-command"));
    assert!(stderr.contains("continue: code-parser extract-business"));
    assert!(stderr.contains("--continue"));
    let default_db = output_root
        .join(root.file_name().unwrap())
        .join("business-extraction.db");
    assert!(!default_db.exists());
    assert!(
        default_db
            .with_file_name(".business-extraction.db.extract-business-checkpoint.json")
            .exists()
    );
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(output_root);
}

#[test]
fn extract_business_writes_sqlite_database_and_summary() {
    let root = test_dir("extract-success");
    fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    fs::write(
        root.join("src/main/java/demo/OrderService.java"),
        r#"
        package demo;
        class OrderService {
          @PostMapping("/orders/{id}/approve")
          public void approve(Long id) {
            if (id == null) throw new IllegalArgumentException();
            repository.save(id);
          }
        }
        "#,
    )
    .unwrap();
    let output_root = test_dir("extract-success-output");
    let fake_jdtls = write_fake_jdtls(&root);

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["extract-business", "--path"])
        .arg(&root)
        .args(["--output-dir"])
        .arg(&output_root)
        .args(["--jdtls-command"])
        .arg(&fake_jdtls)
        .output()
        .unwrap();

    assert!(output.status.success());
    let db = output_root
        .join(root.file_name().unwrap())
        .join("business-extraction.db");
    assert!(db.exists());
    assert!(
        !db.with_file_name(".business-extraction.db.extract-business-checkpoint.json")
            .exists()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!("database: {}", db.display())));
    assert!(stdout.contains("modules: 1"));
    assert!(stdout.contains("classes: 1"));
    assert!(stdout.contains("methods: 1"));

    let connection = Connection::open(&db).unwrap();
    let method_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM methods", [], |row| row.get(0))
        .unwrap();
    let module_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM modules", [], |row| row.get(0))
        .unwrap();
    let entry_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM entry_points", [], |row| row.get(0))
        .unwrap();
    assert_eq!(module_count, 1);
    assert_eq!(method_count, 1);
    assert_eq!(entry_count, 1);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(output_root);
}

#[test]
fn extract_business_persists_multi_module_ownership() {
    let root = test_dir("extract-multi-module");
    fs::write(
        root.join("pom.xml"),
        r#"<project><modules><module>api</module><module>service</module></modules></project>"#,
    )
    .unwrap();
    fs::create_dir_all(root.join("api/src/main/java/demo")).unwrap();
    fs::create_dir_all(root.join("service/src/main/java/demo")).unwrap();
    fs::write(root.join("api/pom.xml"), "<project/>").unwrap();
    fs::write(root.join("service/pom.xml"), "<project/>").unwrap();
    fs::write(
        root.join("api/src/main/java/demo/Order.java"),
        "package demo; class Order { Long id() { return 1L; } }",
    )
    .unwrap();
    fs::write(
        root.join("service/src/main/java/demo/OrderService.java"),
        "package demo; class OrderService { void approve() {} }",
    )
    .unwrap();
    let output_root = test_dir("extract-multi-module-output");
    let fake_jdtls = write_fake_jdtls(&root);

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["extract-business", "--path"])
        .arg(&root)
        .args(["--output-dir"])
        .arg(&output_root)
        .args(["--jdtls-command"])
        .arg(&fake_jdtls)
        .output()
        .unwrap();

    assert!(output.status.success());
    let db = output_root
        .join(root.file_name().unwrap())
        .join("business-extraction.db");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("modules: 3"));

    let connection = Connection::open(&db).unwrap();
    let module_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM modules", [], |row| row.get(0))
        .unwrap();
    let service_class_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM classes WHERE module_id = 'module:service'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let api_method_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM methods WHERE module_id = 'module:api'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(module_count, 3);
    assert_eq!(service_class_count, 1);
    assert_eq!(api_method_count, 1);
    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_dir_all(output_root);
}

#[test]
fn extract_tests_appends_test_tables_to_business_database() {
    let root = test_dir("extract-tests");
    fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
    fs::create_dir_all(root.join("src/integrationTest/java/demo")).unwrap();
    fs::write(
        root.join("src/main/java/demo/OrderService.java"),
        "package demo; public class OrderService { public void approve() {} }\n",
    )
    .unwrap();
    fs::write(
        root.join("src/integrationTest/java/demo/OrderServiceIT.java"),
        r#"package demo;
import org.junit.jupiter.api.Test;
import org.springframework.boot.test.context.SpringBootTest;
@SpringBootTest
class OrderServiceIT {
  @Test
  void approvesOrder() {
    OrderService service = new OrderService();
    service.approve();
    assertEquals("ok", "ok");
  }
}
"#,
    )
    .unwrap();
    let db = root.join("business-extraction.db");
    write_test_target_business_db(&db);
    let fake_jdtls = write_definition_fake_jdtls(&root);

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .args(["extract-tests", "--path"])
        .arg(&root)
        .args(["--database"])
        .arg(&db)
        .args(["--jdtls-command"])
        .arg(&fake_jdtls)
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("test_suites: 1"));
    assert!(stdout.contains("test_cases: 1"));
    assert!(stdout.contains("test_assertions: 1"));
    let connection = Connection::open(&db).unwrap();
    let suite_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM test_suites", [], |row| row.get(0))
        .unwrap();
    let case_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM test_cases", [], |row| row.get(0))
        .unwrap();
    let target_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM test_targets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(suite_count, 1);
    assert_eq!(case_count, 1);
    assert!(target_count >= 1);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn build_business_kg_rejects_missing_api_key() {
    let root = test_dir("build-kg-missing-key");
    fs::write(
        root.join("OrderService.java"),
        "class OrderService {\n  void approve() {\n    if (status != null) return;\n  }\n}\n",
    )
    .unwrap();
    let extraction_db = root.join("business-extraction.db");
    write_business_extraction_db(&extraction_db, "OrderService.java");

    let output = Command::new(env!("CARGO_BIN_EXE_code-parser"))
        .env_remove("ANTHROPIC_API_KEY")
        .args(["build-business-kg", "--database"])
        .arg(&extraction_db)
        .args(["--source-path"])
        .arg(&root)
        .args(["--max-methods", "1"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing ANTHROPIC_API_KEY"));
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

fn write_fake_jdtls(root: &PathBuf) -> PathBuf {
    let script = root.join("fake-jdtls.py");
    fs::write(
        &script,
        r#"#!/usr/bin/env python3
import json
import sys

def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode("utf-8").strip()
        if not line:
            break
        if line.lower().startswith("content-length:"):
            length = int(line.split(":", 1)[1].strip())
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def write_message(value):
    body = json.dumps(value).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if "id" not in message:
        if method == "exit":
            break
        continue
    if method == "initialize":
        result = {"capabilities": {"definitionProvider": True, "referencesProvider": True, "documentSymbolProvider": True}}
    elif method == "shutdown":
        result = None
    elif method in ("textDocument/documentSymbol", "textDocument/definition", "textDocument/references", "textDocument/implementation"):
        result = []
    else:
        result = None
    write_message({"jsonrpc": "2.0", "id": message["id"], "result": result})
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }
    script
}

fn write_business_extraction_db(path: &PathBuf, file: &str) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE classes (
                id TEXT PRIMARY KEY,
                qualified_name TEXT NOT NULL
            );
            CREATE TABLE methods (
                id TEXT PRIMARY KEY,
                class_id TEXT NOT NULL,
                name TEXT NOT NULL,
                signature TEXT NOT NULL,
                file TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL
            );
            CREATE TABLE candidate_scores (
                method_id TEXT PRIMARY KEY,
                score INTEGER NOT NULL,
                priority TEXT NOT NULL
            );
            ",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO classes (id, qualified_name) VALUES ('class:OrderService', 'OrderService')",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO methods (
                id, class_id, name, signature, file, start_line, end_line
             ) VALUES (
                'method:OrderService#approve', 'class:OrderService', 'approve', 'approve()', ?1, 2, 4
             )",
            [file],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO candidate_scores (method_id, score, priority)
             VALUES ('method:OrderService#approve', 10, 'high')",
            [],
        )
        .unwrap();
}

fn write_test_target_business_db(path: &PathBuf) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "
            CREATE TABLE classes (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                qualified_name TEXT NOT NULL,
                file TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL
            );
            CREATE TABLE methods (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                file TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL
            );
            INSERT INTO classes (id, name, qualified_name, file, start_line, end_line)
            VALUES ('class:demo.OrderService@src/main/java/demo/OrderService.java:1', 'OrderService', 'demo.OrderService', 'src/main/java/demo/OrderService.java', 1, 1);
            INSERT INTO methods (id, name, file, start_line, end_line)
            VALUES ('method:demo.OrderService#approve()@src/main/java/demo/OrderService.java:1', 'approve', 'src/main/java/demo/OrderService.java', 1, 1);
            ",
        )
        .unwrap();
}

fn write_definition_fake_jdtls(root: &PathBuf) -> PathBuf {
    let script = root.join("fake-definition-jdtls.py");
    let target = root
        .join("src/main/java/demo/OrderService.java")
        .display()
        .to_string();
    fs::write(
        &script,
        format!(
            r#"#!/usr/bin/env python3
import json
import sys

target_uri = "file://{target}"

def read_message():
    length = None
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.decode("utf-8").strip()
        if not line:
            break
        if line.lower().startswith("content-length:"):
            length = int(line.split(":", 1)[1].strip())
    if length is None:
        return None
    return json.loads(sys.stdin.buffer.read(length).decode("utf-8"))

def write_message(value):
    body = json.dumps(value).encode("utf-8")
    sys.stdout.buffer.write(b"Content-Length: " + str(len(body)).encode("ascii") + b"\r\n\r\n" + body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if "id" not in message:
        if method == "exit":
            break
        continue
    if method == "initialize":
        result = {{"capabilities": {{"definitionProvider": True}}}}
    elif method == "shutdown":
        result = None
    elif method == "textDocument/definition":
        result = [{{"uri": target_uri, "range": {{"start": {{"line": 0, "character": 43}}, "end": {{"line": 0, "character": 50}}}}}}]
    else:
        result = None
    write_message({{"jsonrpc": "2.0", "id": message["id"], "result": result}})
"#,
            target = target.replace('\\', "\\\\").replace('"', "\\\"")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();
    }
    script
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
