pub mod gluon {
    pub mod db {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/gluon.db.v1.rs"));
        }
    }
}

#[macro_export]
macro_rules! proto_field {
    ($row:ty, $field:ident) => {{
        let row: $row = ::core::default::Default::default();
        let _ = &row.$field;
        stringify!($field)
    }};
}

use gluon::db::v1::{
    BusinessKgTable, CandidateSignalRow, CharacterizationBehaviorStatus, CharacterizationRunStatus,
    CharacterizationScenarioStatus, ClassRow, EntryPointRow, ExtractionTable,
    LlmExtractionRunStatus, MethodRow, TestAssertionRow, TestCaseRow, TestEntryPointRow,
    TestFixtureRow, TestSuiteRow, TestTargetRow,
};
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, Value};
use std::sync::OnceLock;

const DESCRIPTOR_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/gluon_descriptor.bin"));

const CHARACTERIZATION_ROW_MESSAGES: &[&str] = &[
    "gluon.db.v1.CharacterizationRunRow",
    "gluon.db.v1.CharacterizationBehaviorRow",
    "gluon.db.v1.CharacterizationScenarioRow",
    "gluon.db.v1.CharacterizationInputRow",
    "gluon.db.v1.CharacterizationObservationRow",
    "gluon.db.v1.CharacterizationFileRow",
    "gluon.db.v1.CharacterizationFakeRow",
    "gluon.db.v1.CharacterizationDiagnosticRow",
];

const EXTRACTION_ROW_MESSAGES: &[&str] = &[
    "gluon.db.v1.ModuleRow",
    "gluon.db.v1.ClassRow",
    "gluon.db.v1.MethodRow",
    "gluon.db.v1.RelationshipRow",
    "gluon.db.v1.EntryPointRow",
    "gluon.db.v1.CandidateScoreRow",
    "gluon.db.v1.CandidateSignalRow",
    "gluon.db.v1.EvidenceRangeRow",
    "gluon.db.v1.ContextPacketRow",
    "gluon.db.v1.DiagnosticRow",
    "gluon.db.v1.TestSuiteRow",
    "gluon.db.v1.TestCaseRow",
    "gluon.db.v1.TestTargetRow",
    "gluon.db.v1.TestAssertionRow",
    "gluon.db.v1.TestFixtureRow",
    "gluon.db.v1.TestEntryPointRow",
    "gluon.db.v1.TestDiagnosticRow",
];

pub fn extraction_table(table: ExtractionTable) -> &'static str {
    match table {
        ExtractionTable::Modules => "modules",
        ExtractionTable::Classes => "classes",
        ExtractionTable::Methods => "methods",
        ExtractionTable::Relationships => "relationships",
        ExtractionTable::EntryPoints => "entry_points",
        ExtractionTable::CandidateScores => "candidate_scores",
        ExtractionTable::CandidateSignals => "candidate_signals",
        ExtractionTable::EvidenceRanges => "evidence_ranges",
        ExtractionTable::ContextPackets => "context_packets",
        ExtractionTable::Diagnostics => "diagnostics",
        ExtractionTable::TestSuites => "test_suites",
        ExtractionTable::TestCases => "test_cases",
        ExtractionTable::TestTargets => "test_targets",
        ExtractionTable::TestAssertions => "test_assertions",
        ExtractionTable::TestFixtures => "test_fixtures",
        ExtractionTable::TestEntryPoints => "test_entry_points",
        ExtractionTable::TestDiagnostics => "test_diagnostics",
        ExtractionTable::Unspecified => "unspecified",
    }
}

pub fn business_kg_table(table: BusinessKgTable) -> &'static str {
    match table {
        BusinessKgTable::LlmExtractionRuns => "llm_extraction_runs",
        BusinessKgTable::BusinessNodes => "business_nodes",
        BusinessKgTable::BusinessEdges => "business_edges",
        BusinessKgTable::BusinessEvidence => "business_evidence",
        BusinessKgTable::Unspecified => "unspecified",
    }
}

pub fn characterization_table(table: gluon::db::v1::CharacterizationTable) -> &'static str {
    match table {
        gluon::db::v1::CharacterizationTable::Runs => "characterization_runs",
        gluon::db::v1::CharacterizationTable::Behaviors => "characterization_behaviors",
        gluon::db::v1::CharacterizationTable::Scenarios => "characterization_scenarios",
        gluon::db::v1::CharacterizationTable::Inputs => "characterization_inputs",
        gluon::db::v1::CharacterizationTable::Observations => "characterization_observations",
        gluon::db::v1::CharacterizationTable::Files => "characterization_files",
        gluon::db::v1::CharacterizationTable::Fakes => "characterization_fakes",
        gluon::db::v1::CharacterizationTable::Diagnostics => "characterization_diagnostics",
        gluon::db::v1::CharacterizationTable::Unspecified => "characterization_unspecified",
    }
}

pub fn characterization_schema_ddl() -> String {
    sqlite_schema_ddl(CHARACTERIZATION_ROW_MESSAGES)
}

pub fn extraction_schema_ddl() -> String {
    sqlite_schema_ddl(EXTRACTION_ROW_MESSAGES)
}

fn descriptor_pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(DESCRIPTOR_BYTES).expect("generated protobuf descriptor set decodes")
    })
}

fn sqlite_schema_ddl(row_messages: &[&str]) -> String {
    let pool = descriptor_pool();
    let mut statements = vec!["PRAGMA foreign_keys = ON;".to_string()];
    for message_name in row_messages {
        let message = pool
            .get_message_by_name(message_name)
            .unwrap_or_else(|| panic!("protobuf row message exists: {message_name}"));
        statements.push(sqlite_create_table_ddl(pool, &message));
    }
    statements.join("\n\n")
}

fn sqlite_create_table_ddl(pool: &DescriptorPool, message: &MessageDescriptor) -> String {
    let table_name = sqlite_table_name(pool, message);
    let mut definitions = Vec::new();
    let mut foreign_keys = Vec::new();

    for field in message.fields() {
        let column = sqlite_column_options(pool, &field.options()).unwrap_or_else(|| {
            panic!(
                "protobuf field has sqlite_column option: {}.{}",
                message.full_name(),
                field.name()
            )
        });
        let column_name = field.name();
        let sql_type = sqlite_option_string(&column, "sql_type")
            .unwrap_or_else(|| panic!("sqlite_column.sql_type is set for {column_name}"));
        let mut definition = format!("{column_name} {sql_type}");
        if sqlite_option_bool(&column, "primary_key") {
            definition.push_str(" PRIMARY KEY");
        }
        if sqlite_option_bool(&column, "autoincrement") {
            definition.push_str(" AUTOINCREMENT");
        }
        if sqlite_option_bool(&column, "not_null") {
            definition.push_str(" NOT NULL");
        }
        if let Some(default_sql) = sqlite_option_string(&column, "default_sql") {
            if !default_sql.is_empty() {
                definition.push_str(" DEFAULT ");
                definition.push_str(&default_sql);
            }
        }
        if let (Some(references_table), Some(references_column)) = (
            sqlite_option_string(&column, "references_table"),
            sqlite_option_string(&column, "references_column"),
        ) {
            if !references_table.is_empty() && !references_column.is_empty() {
                foreign_keys.push(format!(
                    "FOREIGN KEY ({column_name}) REFERENCES {references_table}({references_column})"
                ));
            }
        }
        definitions.push(definition);
    }

    definitions.extend(foreign_keys);
    format!(
        "CREATE TABLE IF NOT EXISTS {table_name} (\n    {}\n);",
        definitions.join(",\n    ")
    )
}

fn sqlite_table_name(pool: &DescriptorPool, message: &MessageDescriptor) -> String {
    let table_extension = pool
        .get_extension_by_name("gluon.db.v1.sqlite_table")
        .expect("sqlite_table protobuf extension exists");
    let options = message.options();
    let table_value = options.get_extension(&table_extension);
    let Value::Message(table) = table_value.as_ref() else {
        panic!("sqlite_table option is a message for {}", message.full_name());
    };
    sqlite_option_string(table, "name")
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| panic!("sqlite_table.name is set for {}", message.full_name()))
}

fn sqlite_column_options(
    pool: &DescriptorPool,
    options: &DynamicMessage,
) -> Option<DynamicMessage> {
    let column_extension = pool
        .get_extension_by_name("gluon.db.v1.sqlite_column")
        .expect("sqlite_column protobuf extension exists");
    if !options.has_extension(&column_extension) {
        return None;
    }
    let column_value = options.get_extension(&column_extension);
    let Value::Message(column) = column_value.as_ref() else {
        panic!("sqlite_column option is a message");
    };
    Some(column.clone())
}

fn sqlite_option_string(message: &DynamicMessage, field: &str) -> Option<String> {
    message
        .get_field_by_name(field)
        .and_then(|value| value.as_ref().as_str().map(str::to_owned))
}

fn sqlite_option_bool(message: &DynamicMessage, field: &str) -> bool {
    message
        .get_field_by_name(field)
        .and_then(|value| value.as_ref().as_bool())
        .unwrap_or(false)
}

pub fn characterization_business_fixture_ddl() -> String {
    format!(
        r#"
        {schema}
        INSERT INTO {modules} ({module_id}, {module_name}, {module_path})
        VALUES ('module:demo', 'demo', '.');
        INSERT INTO {classes} ({class_id}, {class_module_id}, {class_name}, {class_package_name}, {class_qualified_name}, {class_kind}, {class_file}, {class_start_line}, {class_end_line}, {class_interfaces_json}, {class_annotations_json})
        VALUES ('class:OrderService', 'module:demo', 'OrderService', 'demo', 'demo.OrderService', 'class', 'src/main/java/demo/OrderService.java', 1, 20, '[]', '[]');
        INSERT INTO {methods} ({method_id}, {method_module_id}, {method_class_id}, {method_name}, {method_signature}, {method_parameters_json}, {method_annotations_json}, {method_file}, {method_start_line}, {method_end_line}, {method_name_line}, {method_name_column})
        VALUES ('method:OrderService#approve', 'module:demo', 'class:OrderService', 'approve', 'approve()', '[]', '[]', 'src/main/java/demo/OrderService.java', 3, 5, 3, 12);
        INSERT INTO {entry_points} ({entry_point_id}, {entry_point_method_id}, {entry_point_kind}, {entry_point_framework}, {entry_point_route}, {entry_point_http_method}, {entry_point_source})
        VALUES ('entry:approve', 'method:OrderService#approve', 'http', 'spring', '/orders/{{id}}/approve', 'POST', 'tree_sitter');
        INSERT INTO {candidate_signals} ({candidate_signal_method_id}, {candidate_signal_name}, {candidate_signal_count}, {candidate_signal_weight})
        VALUES ('method:OrderService#approve', 'business_terms', 2, 3);
        INSERT INTO {test_suites} ({test_suite_id}, {test_suite_class_name}, {test_suite_qualified_name}, {test_suite_test_kind}, {test_suite_file}, {test_suite_start_line}, {test_suite_end_line}, {test_suite_annotations_json})
        VALUES ('suite:OrderServiceTest', 'OrderServiceTest', 'demo.OrderServiceTest', 'unit', 'src/test/java/demo/OrderServiceTest.java', 1, 40, '[]');
        INSERT INTO {test_cases} ({test_case_id}, {test_case_suite_id}, {test_case_name}, {test_case_test_kind}, {test_case_file}, {test_case_start_line}, {test_case_end_line}, {test_case_annotations_json}, {test_case_body_text})
        VALUES ('case:approvesOrder', 'suite:OrderServiceTest', 'approvesOrder', 'unit', 'src/test/java/demo/OrderServiceTest.java', 10, 20, '[]', 'assertEquals(...)');
        INSERT INTO {test_targets} ({test_target_test_case_id}, {test_target_target_kind}, {test_target_target_id}, {test_target_relationship}, {test_target_confidence}, {test_target_source})
        VALUES ('case:approvesOrder', 'method', 'method:OrderService#approve', 'calls', 1.0, 'static');
        INSERT INTO {test_assertions} ({test_assertion_test_case_id}, {test_assertion_kind}, {test_assertion_expression}, {test_assertion_file}, {test_assertion_line})
        VALUES ('case:approvesOrder', 'equals', 'assertEquals', 'src/test/java/demo/OrderServiceTest.java', 18);
        INSERT INTO {test_fixtures} ({test_fixture_test_case_id}, {test_fixture_kind}, {test_fixture_name}, {test_fixture_details_json}, {test_fixture_file}, {test_fixture_line})
        VALUES ('case:approvesOrder', 'mock', 'repository', '{{}}', 'src/test/java/demo/OrderServiceTest.java', 12);
        INSERT INTO {test_entry_points} ({test_entry_point_test_case_id}, {test_entry_point_kind}, {test_entry_point_framework}, {test_entry_point_route}, {test_entry_point_http_method}, {test_entry_point_source})
        VALUES ('case:approvesOrder', 'http', 'mockmvc', '/orders/{{id}}/approve', 'POST', 'test');
        "#,
        schema = extraction_schema_ddl(),
        modules = extraction_table(ExtractionTable::Modules),
        module_id = proto_field!(gluon::db::v1::ModuleRow, id),
        module_name = proto_field!(gluon::db::v1::ModuleRow, name),
        module_path = proto_field!(gluon::db::v1::ModuleRow, path),
        methods = extraction_table(ExtractionTable::Methods),
        method_id = proto_field!(MethodRow, id),
        method_module_id = proto_field!(MethodRow, module_id),
        method_class_id = proto_field!(MethodRow, class_id),
        method_name = proto_field!(MethodRow, name),
        method_signature = proto_field!(MethodRow, signature),
        method_parameters_json = proto_field!(MethodRow, parameters_json),
        method_annotations_json = proto_field!(MethodRow, annotations_json),
        method_file = proto_field!(MethodRow, file),
        method_start_line = proto_field!(MethodRow, start_line),
        method_end_line = proto_field!(MethodRow, end_line),
        method_name_line = proto_field!(MethodRow, name_line),
        method_name_column = proto_field!(MethodRow, name_column),
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
        class_interfaces_json = proto_field!(ClassRow, interfaces_json),
        class_annotations_json = proto_field!(ClassRow, annotations_json),
        entry_points = extraction_table(ExtractionTable::EntryPoints),
        entry_point_id = proto_field!(EntryPointRow, id),
        entry_point_method_id = proto_field!(EntryPointRow, method_id),
        entry_point_kind = proto_field!(EntryPointRow, kind),
        entry_point_framework = proto_field!(EntryPointRow, framework),
        entry_point_route = proto_field!(EntryPointRow, route),
        entry_point_http_method = proto_field!(EntryPointRow, http_method),
        entry_point_source = proto_field!(EntryPointRow, source),
        candidate_signals = extraction_table(ExtractionTable::CandidateSignals),
        candidate_signal_method_id = proto_field!(CandidateSignalRow, method_id),
        candidate_signal_name = proto_field!(CandidateSignalRow, name),
        candidate_signal_count = proto_field!(CandidateSignalRow, count),
        candidate_signal_weight = proto_field!(CandidateSignalRow, weight),
        test_suites = extraction_table(ExtractionTable::TestSuites),
        test_suite_id = proto_field!(TestSuiteRow, id),
        test_suite_class_name = proto_field!(TestSuiteRow, class_name),
        test_suite_qualified_name = proto_field!(TestSuiteRow, qualified_name),
        test_suite_test_kind = proto_field!(TestSuiteRow, test_kind),
        test_suite_file = proto_field!(TestSuiteRow, file),
        test_suite_start_line = proto_field!(TestSuiteRow, start_line),
        test_suite_end_line = proto_field!(TestSuiteRow, end_line),
        test_suite_annotations_json = proto_field!(TestSuiteRow, annotations_json),
        test_cases = extraction_table(ExtractionTable::TestCases),
        test_case_id = proto_field!(TestCaseRow, id),
        test_case_suite_id = proto_field!(TestCaseRow, suite_id),
        test_case_name = proto_field!(TestCaseRow, name),
        test_case_test_kind = proto_field!(TestCaseRow, test_kind),
        test_case_file = proto_field!(TestCaseRow, file),
        test_case_start_line = proto_field!(TestCaseRow, start_line),
        test_case_end_line = proto_field!(TestCaseRow, end_line),
        test_case_annotations_json = proto_field!(TestCaseRow, annotations_json),
        test_case_body_text = proto_field!(TestCaseRow, body_text),
        test_targets = extraction_table(ExtractionTable::TestTargets),
        test_target_test_case_id = proto_field!(TestTargetRow, test_case_id),
        test_target_target_kind = proto_field!(TestTargetRow, target_kind),
        test_target_target_id = proto_field!(TestTargetRow, target_id),
        test_target_relationship = proto_field!(TestTargetRow, relationship),
        test_target_confidence = proto_field!(TestTargetRow, confidence),
        test_target_source = proto_field!(TestTargetRow, source),
        test_assertions = extraction_table(ExtractionTable::TestAssertions),
        test_assertion_test_case_id = proto_field!(TestAssertionRow, test_case_id),
        test_assertion_kind = proto_field!(TestAssertionRow, assertion_kind),
        test_assertion_expression = proto_field!(TestAssertionRow, expression),
        test_assertion_file = proto_field!(TestAssertionRow, file),
        test_assertion_line = proto_field!(TestAssertionRow, line),
        test_fixtures = extraction_table(ExtractionTable::TestFixtures),
        test_fixture_test_case_id = proto_field!(TestFixtureRow, test_case_id),
        test_fixture_kind = proto_field!(TestFixtureRow, fixture_kind),
        test_fixture_name = proto_field!(TestFixtureRow, name),
        test_fixture_details_json = proto_field!(TestFixtureRow, details_json),
        test_fixture_file = proto_field!(TestFixtureRow, file),
        test_fixture_line = proto_field!(TestFixtureRow, line),
        test_entry_points = extraction_table(ExtractionTable::TestEntryPoints),
        test_entry_point_test_case_id = proto_field!(TestEntryPointRow, test_case_id),
        test_entry_point_kind = proto_field!(TestEntryPointRow, kind),
        test_entry_point_framework = proto_field!(TestEntryPointRow, framework),
        test_entry_point_route = proto_field!(TestEntryPointRow, route),
        test_entry_point_http_method = proto_field!(TestEntryPointRow, http_method),
        test_entry_point_source = proto_field!(TestEntryPointRow, source),
    )
}

pub fn characterization_run_status(status: CharacterizationRunStatus) -> &'static str {
    match status {
        CharacterizationRunStatus::Running => "running",
        CharacterizationRunStatus::Completed => "completed",
        CharacterizationRunStatus::Failed => "failed",
        CharacterizationRunStatus::PartialFailure => "partial_failure",
        CharacterizationRunStatus::Unspecified => "unspecified",
    }
}

pub fn characterization_behavior_status(status: CharacterizationBehaviorStatus) -> &'static str {
    match status {
        CharacterizationBehaviorStatus::Selected => "selected",
        CharacterizationBehaviorStatus::Skipped => "skipped",
        CharacterizationBehaviorStatus::Unspecified => "unspecified",
    }
}

pub fn characterization_scenario_status(status: CharacterizationScenarioStatus) -> &'static str {
    match status {
        CharacterizationScenarioStatus::GeneratedScaffold => "generated_scaffold",
        CharacterizationScenarioStatus::Implementing => "implementing",
        CharacterizationScenarioStatus::Observing => "observing",
        CharacterizationScenarioStatus::Accepted => "accepted",
        CharacterizationScenarioStatus::Committed => "committed",
        CharacterizationScenarioStatus::Skipped => "skipped",
        CharacterizationScenarioStatus::Failed => "failed",
        CharacterizationScenarioStatus::Unspecified => "unspecified",
    }
}

pub fn llm_extraction_run_status(status: LlmExtractionRunStatus) -> &'static str {
    match status {
        LlmExtractionRunStatus::Running => "running",
        LlmExtractionRunStatus::Completed => "completed",
        LlmExtractionRunStatus::Failed => "failed",
        LlmExtractionRunStatus::PartialFailure => "partial_failure",
        LlmExtractionRunStatus::Unspecified => "unspecified",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        business_kg_table, characterization_run_status, characterization_schema_ddl,
        characterization_table, extraction_table,
        gluon::db::v1::{
            BusinessKgTable, BusinessNodeRow, CharacterizationRunStatus,
            CharacterizationScenarioKind, CharacterizationScenarioRow,
            CharacterizationScenarioStatus, CharacterizationTable, ExtractionTable,
            LlmExtractionRunStatus, MethodRow,
        },
        llm_extraction_run_status,
    };

    #[test]
    fn generated_database_contract_clients_are_available() {
        let scenario = CharacterizationScenarioRow {
            id: "scenario:one".to_string(),
            behavior_id: "behavior:one".to_string(),
            name: "Approve".to_string(),
            scenario_kind: CharacterizationScenarioKind::HappyPath.into(),
            status: CharacterizationScenarioStatus::GeneratedScaffold.into(),
            ..Default::default()
        };
        let method = MethodRow {
            id: "method:one".to_string(),
            name: "approve".to_string(),
            ..Default::default()
        };
        let node = BusinessNodeRow {
            id: "node:one".to_string(),
            name: "Approve".to_string(),
            ..Default::default()
        };

        assert_eq!(scenario.id, "scenario:one");
        assert_eq!(method.id, "method:one");
        assert_eq!(node.id, "node:one");
        assert_eq!(
            characterization_run_status(CharacterizationRunStatus::PartialFailure),
            "partial_failure"
        );
        assert_eq!(
            llm_extraction_run_status(LlmExtractionRunStatus::Completed),
            "completed"
        );
        assert_eq!(
            business_kg_table(BusinessKgTable::BusinessNodes),
            "business_nodes"
        );
        assert_eq!(
            characterization_table(CharacterizationTable::Scenarios),
            "characterization_scenarios"
        );
        assert!(characterization_schema_ddl().contains("characterization_scenarios"));
        assert_eq!(extraction_table(ExtractionTable::Methods), "methods");
    }
}
