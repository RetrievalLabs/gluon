use std::path::Path;

use rusqlite::{Connection, params};

use crate::languages::business::model::CodeModel;

pub fn write_database(path: &Path, model: &CodeModel) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create database directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let mut connection = Connection::open(path)
        .map_err(|error| format!("failed to open database {}: {error}", path.display()))?;
    create_schema(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("failed to start database transaction: {error}"))?;

    for module in &model.modules {
        transaction
            .execute(
                "INSERT INTO modules (
                    id, name, path, build_system, build_file, parent_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    module.id,
                    module.name,
                    module.path,
                    module.build_system,
                    module.build_file,
                    module.parent_id,
                ],
            )
            .map_err(|error| format!("failed to insert module {}: {error}", module.id))?;
    }

    for class in &model.classes {
        transaction
            .execute(
                "INSERT INTO classes (
                    id, module_id, name, package_name, qualified_name, kind, file,
                    start_line, end_line, superclass, interfaces_json, annotations_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    class.id,
                    class.module_id,
                    class.name,
                    class.package_name,
                    class.qualified_name,
                    class.kind,
                    class.file,
                    class.start_line as i64,
                    class.end_line as i64,
                    class.superclass,
                    json(&class.interfaces)?,
                    json(&class.annotations)?,
                ],
            )
            .map_err(|error| format!("failed to insert class {}: {error}", class.id))?;
    }

    for method in &model.methods {
        transaction
            .execute(
                "INSERT INTO methods (
                    id, module_id, class_id, name, signature, return_type, parameters_json,
                    annotations_json, file, start_line, end_line, name_line, name_column
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    method.id,
                    method.module_id,
                    method.class_id,
                    method.name,
                    method.signature,
                    method.return_type,
                    json(&method.parameters)?,
                    json(&method.annotations)?,
                    method.file,
                    method.start_line as i64,
                    method.end_line as i64,
                    method.name_line as i64,
                    method.name_column as i64,
                ],
            )
            .map_err(|error| format!("failed to insert method {}: {error}", method.id))?;
    }

    for relationship in &model.relationships {
        transaction
            .execute(
                "INSERT INTO relationships (
                    source_id, target_id, kind, confidence, source
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    relationship.source_id,
                    relationship.target_id,
                    relationship.kind,
                    relationship.confidence,
                    relationship.source,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert relationship {} -> {}: {error}",
                    relationship.source_id, relationship.target_id
                )
            })?;
    }

    for entry_point in &model.entry_points {
        transaction
            .execute(
                "INSERT INTO entry_points (
                    id, method_id, kind, framework, route, http_method, source
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    entry_point.id,
                    entry_point.method_id,
                    entry_point.kind,
                    entry_point.framework,
                    entry_point.route,
                    entry_point.http_method,
                    entry_point.source,
                ],
            )
            .map_err(|error| format!("failed to insert entry point {}: {error}", entry_point.id))?;
    }

    for score in &model.candidate_scores {
        transaction
            .execute(
                "INSERT INTO candidate_scores (method_id, score, priority)
                 VALUES (?1, ?2, ?3)",
                params![score.method_id, score.score, score.priority],
            )
            .map_err(|error| {
                format!(
                    "failed to insert candidate score for {}: {error}",
                    score.method_id
                )
            })?;
    }

    for signal in &model.candidate_signals {
        transaction
            .execute(
                "INSERT INTO candidate_signals (method_id, name, count, weight)
                 VALUES (?1, ?2, ?3, ?4)",
                params![signal.method_id, signal.name, signal.count, signal.weight],
            )
            .map_err(|error| {
                format!(
                    "failed to insert candidate signal {} for {}: {error}",
                    signal.name, signal.method_id
                )
            })?;
    }

    for evidence in &model.evidence_ranges {
        transaction
            .execute(
                "INSERT INTO evidence_ranges (method_id, file, start_line, end_line, source)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    evidence.method_id,
                    evidence.file,
                    evidence.start_line as i64,
                    evidence.end_line as i64,
                    evidence.source,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert evidence range for {}: {error}",
                    evidence.method_id
                )
            })?;
    }

    for packet in &model.context_packets {
        transaction
            .execute(
                "INSERT INTO context_packets (method_id, summary)
                 VALUES (?1, ?2)",
                params![packet.method_id, packet.summary],
            )
            .map_err(|error| {
                format!(
                    "failed to insert context packet for {}: {error}",
                    packet.method_id
                )
            })?;
    }

    for diagnostic in &model.diagnostics {
        transaction
            .execute(
                "INSERT INTO diagnostics (
                    severity, category, message, file, command_json, exit_code, stderr
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    diagnostic.severity,
                    diagnostic.category,
                    diagnostic.message,
                    diagnostic.file,
                    json(&diagnostic.command)?,
                    diagnostic.exit_code,
                    diagnostic.stderr,
                ],
            )
            .map_err(|error| {
                format!(
                    "failed to insert diagnostic {}: {error}",
                    diagnostic.category
                )
            })?;
    }

    transaction
        .commit()
        .map_err(|error| format!("failed to commit database transaction: {error}"))?;
    Ok(())
}

fn create_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE modules (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                build_system TEXT,
                build_file TEXT,
                parent_id TEXT
            );

            CREATE TABLE classes (
                id TEXT PRIMARY KEY,
                module_id TEXT NOT NULL,
                name TEXT NOT NULL,
                package_name TEXT,
                qualified_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                file TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                superclass TEXT,
                interfaces_json TEXT NOT NULL,
                annotations_json TEXT NOT NULL
            );

            CREATE TABLE methods (
                id TEXT PRIMARY KEY,
                module_id TEXT NOT NULL,
                class_id TEXT NOT NULL,
                name TEXT NOT NULL,
                signature TEXT NOT NULL,
                return_type TEXT,
                parameters_json TEXT NOT NULL,
                annotations_json TEXT NOT NULL,
                file TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                name_line INTEGER NOT NULL,
                name_column INTEGER NOT NULL
            );

            CREATE TABLE relationships (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                confidence REAL NOT NULL,
                source TEXT NOT NULL
            );

            CREATE TABLE entry_points (
                id TEXT PRIMARY KEY,
                method_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                framework TEXT,
                route TEXT,
                http_method TEXT,
                source TEXT NOT NULL
            );

            CREATE TABLE candidate_scores (
                method_id TEXT PRIMARY KEY,
                score INTEGER NOT NULL,
                priority TEXT NOT NULL
            );

            CREATE TABLE candidate_signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                method_id TEXT NOT NULL,
                name TEXT NOT NULL,
                count INTEGER NOT NULL,
                weight INTEGER NOT NULL
            );

            CREATE TABLE evidence_ranges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                method_id TEXT NOT NULL,
                file TEXT NOT NULL,
                start_line INTEGER NOT NULL,
                end_line INTEGER NOT NULL,
                source TEXT NOT NULL
            );

            CREATE TABLE context_packets (
                method_id TEXT PRIMARY KEY,
                summary TEXT NOT NULL
            );

            CREATE TABLE diagnostics (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                severity TEXT NOT NULL,
                category TEXT NOT NULL,
                message TEXT NOT NULL,
                file TEXT,
                command_json TEXT,
                exit_code INTEGER,
                stderr TEXT
            );
            ",
        )
        .map_err(|error| format!("failed to create database schema: {error}"))
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, String> {
    serde_json::to_string(value)
        .map_err(|error| format!("failed to serialize database JSON: {error}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use crate::languages::business::model::{ClassInfo, MethodInfo, ModuleInfo};

    use super::*;

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

    #[test]
    fn writes_required_tables_and_rows() {
        let root = test_dir("business-store");
        let db = root.join("business-extraction.db");
        let model = CodeModel {
            modules: vec![ModuleInfo {
                id: "module:.".to_string(),
                name: "business-store".to_string(),
                path: ".".to_string(),
                build_system: None,
                build_file: None,
                parent_id: None,
            }],
            classes: vec![ClassInfo {
                id: "class:demo.OrderService".to_string(),
                module_id: "module:.".to_string(),
                name: "OrderService".to_string(),
                package_name: Some("demo".to_string()),
                qualified_name: "demo.OrderService".to_string(),
                kind: "class".to_string(),
                file: "OrderService.java".to_string(),
                start_line: 1,
                end_line: 4,
                superclass: None,
                interfaces: Vec::new(),
                annotations: Vec::new(),
            }],
            methods: vec![MethodInfo {
                id: "method:demo.OrderService#approve()@2".to_string(),
                module_id: "module:.".to_string(),
                class_id: "class:demo.OrderService".to_string(),
                name: "approve".to_string(),
                signature: "approve()".to_string(),
                return_type: Some("void".to_string()),
                parameters: Vec::new(),
                annotations: Vec::new(),
                file: "OrderService.java".to_string(),
                start_line: 2,
                end_line: 3,
                name_line: 2,
                name_column: 7,
                body_text: "{}".to_string(),
            }],
            ..CodeModel::default()
        };

        write_database(&db, &model).unwrap();

        let connection = Connection::open(&db).unwrap();
        let class_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM classes", [], |row| row.get(0))
            .unwrap();
        let module_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM modules", [], |row| row.get(0))
            .unwrap();
        let method_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM methods", [], |row| row.get(0))
            .unwrap();
        assert_eq!(module_count, 1);
        assert_eq!(class_count, 1);
        assert_eq!(method_count, 1);
        let _ = fs::remove_dir_all(root);
    }
}
