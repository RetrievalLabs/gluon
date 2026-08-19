use serde::{Deserialize, Serialize};

use crate::languages::java::build::model::Diagnostic;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CodeModel {
    pub project_root: String,
    pub modules: Vec<ModuleInfo>,
    pub classes: Vec<ClassInfo>,
    pub methods: Vec<MethodInfo>,
    pub invocations: Vec<InvocationInfo>,
    pub relationships: Vec<RelationshipInfo>,
    pub entry_points: Vec<EntryPointInfo>,
    pub candidate_scores: Vec<CandidateScore>,
    pub candidate_signals: Vec<CandidateSignal>,
    pub evidence_ranges: Vec<EvidenceRange>,
    pub context_packets: Vec<ContextPacket>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub id: String,
    pub name: String,
    pub path: String,
    pub build_system: Option<String>,
    pub build_file: Option<String>,
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassInfo {
    pub id: String,
    pub module_id: String,
    pub name: String,
    pub package_name: Option<String>,
    pub qualified_name: String,
    pub kind: String,
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub superclass: Option<String>,
    pub interfaces: Vec<String>,
    pub annotations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MethodInfo {
    pub id: String,
    pub module_id: String,
    pub class_id: String,
    pub name: String,
    pub signature: String,
    pub return_type: Option<String>,
    pub parameters: Vec<ParameterInfo>,
    pub annotations: Vec<String>,
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub name_line: usize,
    pub name_column: usize,
    pub body_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInfo {
    pub name: String,
    pub type_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationInfo {
    pub caller_method_id: String,
    pub file: String,
    pub name: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationshipInfo {
    pub source_id: String,
    pub target_id: String,
    pub kind: String,
    pub confidence: f64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryPointInfo {
    pub id: String,
    pub method_id: String,
    pub kind: String,
    pub framework: Option<String>,
    pub route: Option<String>,
    pub http_method: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateScore {
    pub method_id: String,
    pub score: i64,
    pub priority: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSignal {
    pub method_id: String,
    pub name: String,
    pub count: i64,
    pub weight: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRange {
    pub method_id: String,
    pub file: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPacket {
    pub method_id: String,
    pub summary: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionSummary {
    pub database_path: String,
    pub module_count: usize,
    pub class_count: usize,
    pub method_count: usize,
    pub relationship_count: usize,
    pub high_priority_candidates: usize,
    pub medium_priority_candidates: usize,
    pub low_priority_candidates: usize,
    pub diagnostic_count: usize,
}
