use std::path::Path;

use rusqlite::{Connection, params};

use crate::languages::business::model::CodeModel;
use crate::proto::extraction_table;
use crate::proto::gluon::db::v1::{
    CandidateScoreRow, CandidateSignalRow, ClassRow, ContextPacketRow, DiagnosticRow,
    EntryPointRow, EvidenceRangeRow, ExtractionTable, MethodRow, ModuleRow, RelationshipRow,
};
use crate::proto_field;

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
                &format!(
                    "INSERT INTO {} (
                    {}, {}, {}, {}, {}, {}
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    extraction_table(ExtractionTable::Modules),
                    proto_field!(ModuleRow, id),
                    proto_field!(ModuleRow, name),
                    proto_field!(ModuleRow, path),
                    proto_field!(ModuleRow, build_system),
                    proto_field!(ModuleRow, build_file),
                    proto_field!(ModuleRow, parent_id),
                ),
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
                &format!(
                    "INSERT INTO {} (
                    {}, {}, {}, {}, {}, {}, {},
                    {}, {}, {}, {}, {}
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    extraction_table(ExtractionTable::Classes),
                    proto_field!(ClassRow, id),
                    proto_field!(ClassRow, module_id),
                    proto_field!(ClassRow, name),
                    proto_field!(ClassRow, package_name),
                    proto_field!(ClassRow, qualified_name),
                    proto_field!(ClassRow, kind),
                    proto_field!(ClassRow, file),
                    proto_field!(ClassRow, start_line),
                    proto_field!(ClassRow, end_line),
                    proto_field!(ClassRow, superclass),
                    proto_field!(ClassRow, interfaces_json),
                    proto_field!(ClassRow, annotations_json),
                ),
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
                &format!(
                    "INSERT INTO {} (
                    {}, {}, {}, {}, {}, {}, {},
                    {}, {}, {}, {}, {}, {}
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    extraction_table(ExtractionTable::Methods),
                    proto_field!(MethodRow, id),
                    proto_field!(MethodRow, module_id),
                    proto_field!(MethodRow, class_id),
                    proto_field!(MethodRow, name),
                    proto_field!(MethodRow, signature),
                    proto_field!(MethodRow, return_type),
                    proto_field!(MethodRow, parameters_json),
                    proto_field!(MethodRow, annotations_json),
                    proto_field!(MethodRow, file),
                    proto_field!(MethodRow, start_line),
                    proto_field!(MethodRow, end_line),
                    proto_field!(MethodRow, name_line),
                    proto_field!(MethodRow, name_column),
                ),
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
                &format!(
                    "INSERT INTO {} (
                    {}, {}, {}, {}, {}
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    extraction_table(ExtractionTable::Relationships),
                    proto_field!(RelationshipRow, source_id),
                    proto_field!(RelationshipRow, target_id),
                    proto_field!(RelationshipRow, kind),
                    proto_field!(RelationshipRow, confidence),
                    proto_field!(RelationshipRow, source),
                ),
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
                &format!(
                    "INSERT INTO {} (
                    {}, {}, {}, {}, {}, {}, {}
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    extraction_table(ExtractionTable::EntryPoints),
                    proto_field!(EntryPointRow, id),
                    proto_field!(EntryPointRow, method_id),
                    proto_field!(EntryPointRow, kind),
                    proto_field!(EntryPointRow, framework),
                    proto_field!(EntryPointRow, route),
                    proto_field!(EntryPointRow, http_method),
                    proto_field!(EntryPointRow, source),
                ),
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
                &format!(
                    "INSERT INTO {} ({}, {}, {})
                 VALUES (?1, ?2, ?3)",
                    extraction_table(ExtractionTable::CandidateScores),
                    proto_field!(CandidateScoreRow, method_id),
                    proto_field!(CandidateScoreRow, score),
                    proto_field!(CandidateScoreRow, priority),
                ),
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
                &format!(
                    "INSERT INTO {} ({}, {}, {}, {})
                 VALUES (?1, ?2, ?3, ?4)",
                    extraction_table(ExtractionTable::CandidateSignals),
                    proto_field!(CandidateSignalRow, method_id),
                    proto_field!(CandidateSignalRow, name),
                    proto_field!(CandidateSignalRow, count),
                    proto_field!(CandidateSignalRow, weight),
                ),
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
                &format!(
                    "INSERT INTO {} ({}, {}, {}, {}, {})
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                    extraction_table(ExtractionTable::EvidenceRanges),
                    proto_field!(EvidenceRangeRow, method_id),
                    proto_field!(EvidenceRangeRow, file),
                    proto_field!(EvidenceRangeRow, start_line),
                    proto_field!(EvidenceRangeRow, end_line),
                    proto_field!(EvidenceRangeRow, source),
                ),
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
                &format!(
                    "INSERT INTO {} ({}, {})
                 VALUES (?1, ?2)",
                    extraction_table(ExtractionTable::ContextPackets),
                    proto_field!(ContextPacketRow, method_id),
                    proto_field!(ContextPacketRow, summary),
                ),
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
                &format!(
                    "INSERT INTO {} (
                    {}, {}, {}, {}, {}, {}, {}
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    extraction_table(ExtractionTable::Diagnostics),
                    proto_field!(DiagnosticRow, severity),
                    proto_field!(DiagnosticRow, category),
                    proto_field!(DiagnosticRow, message),
                    proto_field!(DiagnosticRow, file),
                    proto_field!(DiagnosticRow, command_json),
                    proto_field!(DiagnosticRow, exit_code),
                    proto_field!(DiagnosticRow, stderr),
                ),
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
        .execute_batch(&format!(
            "
            PRAGMA foreign_keys = ON;

            CREATE TABLE {modules} (
                {module_id} TEXT PRIMARY KEY,
                {module_name} TEXT NOT NULL,
                {module_path} TEXT NOT NULL,
                {module_build_system} TEXT,
                {module_build_file} TEXT,
                {module_parent_id} TEXT
            );

            CREATE TABLE {classes} (
                {class_id} TEXT PRIMARY KEY,
                {class_module_id} TEXT NOT NULL,
                {class_name} TEXT NOT NULL,
                {class_package_name} TEXT,
                {class_qualified_name} TEXT NOT NULL,
                {class_kind} TEXT NOT NULL,
                {class_file} TEXT NOT NULL,
                {class_start_line} INTEGER NOT NULL,
                {class_end_line} INTEGER NOT NULL,
                {class_superclass} TEXT,
                {class_interfaces_json} TEXT NOT NULL,
                {class_annotations_json} TEXT NOT NULL
            );

            CREATE TABLE {methods} (
                {method_id} TEXT PRIMARY KEY,
                {method_module_id} TEXT NOT NULL,
                {method_class_id} TEXT NOT NULL,
                {method_name} TEXT NOT NULL,
                {method_signature} TEXT NOT NULL,
                {method_return_type} TEXT,
                {method_parameters_json} TEXT NOT NULL,
                {method_annotations_json} TEXT NOT NULL,
                {method_file} TEXT NOT NULL,
                {method_start_line} INTEGER NOT NULL,
                {method_end_line} INTEGER NOT NULL,
                {method_name_line} INTEGER NOT NULL,
                {method_name_column} INTEGER NOT NULL
            );

            CREATE TABLE {relationships} (
                {relationship_id} INTEGER PRIMARY KEY AUTOINCREMENT,
                {relationship_source_id} TEXT NOT NULL,
                {relationship_target_id} TEXT NOT NULL,
                {relationship_kind} TEXT NOT NULL,
                {relationship_confidence} REAL NOT NULL,
                {relationship_source} TEXT NOT NULL
            );

            CREATE TABLE {entry_points} (
                {entry_point_id} TEXT PRIMARY KEY,
                {entry_point_method_id} TEXT NOT NULL,
                {entry_point_kind} TEXT NOT NULL,
                {entry_point_framework} TEXT,
                {entry_point_route} TEXT,
                {entry_point_http_method} TEXT,
                {entry_point_source} TEXT NOT NULL
            );

            CREATE TABLE {candidate_scores} (
                {candidate_score_method_id} TEXT PRIMARY KEY,
                {candidate_score_score} INTEGER NOT NULL,
                {candidate_score_priority} TEXT NOT NULL
            );

            CREATE TABLE {candidate_signals} (
                {candidate_signal_id} INTEGER PRIMARY KEY AUTOINCREMENT,
                {candidate_signal_method_id} TEXT NOT NULL,
                {candidate_signal_name} TEXT NOT NULL,
                {candidate_signal_count} INTEGER NOT NULL,
                {candidate_signal_weight} INTEGER NOT NULL
            );

            CREATE TABLE {evidence_ranges} (
                {evidence_range_id} INTEGER PRIMARY KEY AUTOINCREMENT,
                {evidence_range_method_id} TEXT NOT NULL,
                {evidence_range_file} TEXT NOT NULL,
                {evidence_range_start_line} INTEGER NOT NULL,
                {evidence_range_end_line} INTEGER NOT NULL,
                {evidence_range_source} TEXT NOT NULL
            );

            CREATE TABLE {context_packets} (
                {context_packet_method_id} TEXT PRIMARY KEY,
                {context_packet_summary} TEXT NOT NULL
            );

            CREATE TABLE {diagnostics} (
                {diagnostic_id} INTEGER PRIMARY KEY AUTOINCREMENT,
                {diagnostic_severity} TEXT NOT NULL,
                {diagnostic_category} TEXT NOT NULL,
                {diagnostic_message} TEXT NOT NULL,
                {diagnostic_file} TEXT,
                {diagnostic_command_json} TEXT,
                {diagnostic_exit_code} INTEGER,
                {diagnostic_stderr} TEXT
            );
            ",
            modules = extraction_table(ExtractionTable::Modules),
            module_id = proto_field!(ModuleRow, id),
            module_name = proto_field!(ModuleRow, name),
            module_path = proto_field!(ModuleRow, path),
            module_build_system = proto_field!(ModuleRow, build_system),
            module_build_file = proto_field!(ModuleRow, build_file),
            module_parent_id = proto_field!(ModuleRow, parent_id),
            classes = extraction_table(ExtractionTable::Classes),
            class_id = proto_field!(ClassRow, id),
            class_module_id = proto_field!(ClassRow, module_id),
            class_name = proto_field!(ClassRow, name),
            class_package_name = proto_field!(ClassRow, package_name),
            class_qualified_name = proto_field!(ClassRow, qualified_name),
            class_kind = proto_field!(ClassRow, kind),
            class_file = proto_field!(ClassRow, file),
            class_start_line = proto_field!(ClassRow, start_line),
            class_end_line = proto_field!(ClassRow, end_line),
            class_superclass = proto_field!(ClassRow, superclass),
            class_interfaces_json = proto_field!(ClassRow, interfaces_json),
            class_annotations_json = proto_field!(ClassRow, annotations_json),
            methods = extraction_table(ExtractionTable::Methods),
            method_id = proto_field!(MethodRow, id),
            method_module_id = proto_field!(MethodRow, module_id),
            method_class_id = proto_field!(MethodRow, class_id),
            method_name = proto_field!(MethodRow, name),
            method_signature = proto_field!(MethodRow, signature),
            method_return_type = proto_field!(MethodRow, return_type),
            method_parameters_json = proto_field!(MethodRow, parameters_json),
            method_annotations_json = proto_field!(MethodRow, annotations_json),
            method_file = proto_field!(MethodRow, file),
            method_start_line = proto_field!(MethodRow, start_line),
            method_end_line = proto_field!(MethodRow, end_line),
            method_name_line = proto_field!(MethodRow, name_line),
            method_name_column = proto_field!(MethodRow, name_column),
            relationships = extraction_table(ExtractionTable::Relationships),
            relationship_id = proto_field!(RelationshipRow, id),
            relationship_source_id = proto_field!(RelationshipRow, source_id),
            relationship_target_id = proto_field!(RelationshipRow, target_id),
            relationship_kind = proto_field!(RelationshipRow, kind),
            relationship_confidence = proto_field!(RelationshipRow, confidence),
            relationship_source = proto_field!(RelationshipRow, source),
            entry_points = extraction_table(ExtractionTable::EntryPoints),
            entry_point_id = proto_field!(EntryPointRow, id),
            entry_point_method_id = proto_field!(EntryPointRow, method_id),
            entry_point_kind = proto_field!(EntryPointRow, kind),
            entry_point_framework = proto_field!(EntryPointRow, framework),
            entry_point_route = proto_field!(EntryPointRow, route),
            entry_point_http_method = proto_field!(EntryPointRow, http_method),
            entry_point_source = proto_field!(EntryPointRow, source),
            candidate_scores = extraction_table(ExtractionTable::CandidateScores),
            candidate_score_method_id = proto_field!(CandidateScoreRow, method_id),
            candidate_score_score = proto_field!(CandidateScoreRow, score),
            candidate_score_priority = proto_field!(CandidateScoreRow, priority),
            candidate_signals = extraction_table(ExtractionTable::CandidateSignals),
            candidate_signal_id = proto_field!(CandidateSignalRow, id),
            candidate_signal_method_id = proto_field!(CandidateSignalRow, method_id),
            candidate_signal_name = proto_field!(CandidateSignalRow, name),
            candidate_signal_count = proto_field!(CandidateSignalRow, count),
            candidate_signal_weight = proto_field!(CandidateSignalRow, weight),
            evidence_ranges = extraction_table(ExtractionTable::EvidenceRanges),
            evidence_range_id = proto_field!(EvidenceRangeRow, id),
            evidence_range_method_id = proto_field!(EvidenceRangeRow, method_id),
            evidence_range_file = proto_field!(EvidenceRangeRow, file),
            evidence_range_start_line = proto_field!(EvidenceRangeRow, start_line),
            evidence_range_end_line = proto_field!(EvidenceRangeRow, end_line),
            evidence_range_source = proto_field!(EvidenceRangeRow, source),
            context_packets = extraction_table(ExtractionTable::ContextPackets),
            context_packet_method_id = proto_field!(ContextPacketRow, method_id),
            context_packet_summary = proto_field!(ContextPacketRow, summary),
            diagnostics = extraction_table(ExtractionTable::Diagnostics),
            diagnostic_id = proto_field!(DiagnosticRow, id),
            diagnostic_severity = proto_field!(DiagnosticRow, severity),
            diagnostic_category = proto_field!(DiagnosticRow, category),
            diagnostic_message = proto_field!(DiagnosticRow, message),
            diagnostic_file = proto_field!(DiagnosticRow, file),
            diagnostic_command_json = proto_field!(DiagnosticRow, command_json),
            diagnostic_exit_code = proto_field!(DiagnosticRow, exit_code),
            diagnostic_stderr = proto_field!(DiagnosticRow, stderr),
        ))
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
