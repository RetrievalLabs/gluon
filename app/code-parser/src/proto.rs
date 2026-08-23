pub mod gluon {
    pub mod db {
        pub mod v1 {
            include!(concat!(env!("OUT_DIR"), "/gluon.db.v1.rs"));
        }
    }
}

use gluon::db::v1::{
    BusinessKgTable, CharacterizationBehaviorStatus, CharacterizationRunStatus,
    CharacterizationScenarioStatus, ExtractionTable, LlmExtractionRunStatus,
};

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
        business_kg_table, characterization_run_status, extraction_table,
        gluon::db::v1::{
            BusinessKgTable, BusinessNodeRow, CharacterizationRunStatus,
            CharacterizationScenarioKind, CharacterizationScenarioRow,
            CharacterizationScenarioStatus, ExtractionTable, LlmExtractionRunStatus, MethodRow,
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
        assert_eq!(extraction_table(ExtractionTable::Methods), "methods");
    }
}
