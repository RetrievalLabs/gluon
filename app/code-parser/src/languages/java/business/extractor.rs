use std::fs;
use std::path::{Path, PathBuf};

use crate::languages::java::business::default_database_path;
use crate::languages::java::business::jdtls::{JdtlsOptions, enrich_with_jdtls};
use crate::languages::java::business::model::{CodeModel, ExtractionSummary};
use crate::languages::java::business::scoring::score_candidates;
use crate::languages::java::business::store::write_database;
use crate::languages::java::business::tree_sitter::extract_structure;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusinessExtractionOptions {
    pub path: PathBuf,
    pub output_dir: PathBuf,
    pub database: Option<PathBuf>,
    pub jdtls_command: String,
    pub jdtls_workspace: Option<PathBuf>,
    pub jdtls_max_in_flight: usize,
    pub jdtls_deep: bool,
}

pub fn extract_business(options: &BusinessExtractionOptions) -> Result<ExtractionSummary, String> {
    if !options.path.exists() {
        return Err(format!("path does not exist: {}", options.path.display()));
    }
    let project_root = if options.path.is_file() {
        options
            .path
            .parent()
            .ok_or_else(|| format!("path has no parent: {}", options.path.display()))?
            .to_path_buf()
    } else {
        options.path.clone()
    };
    let database = options
        .database
        .clone()
        .map(Ok)
        .unwrap_or_else(|| default_database_path(&project_root, &options.output_dir))?;
    let workspace = options.jdtls_workspace.clone().unwrap_or_else(|| {
        database
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".jdtls-workspace")
    });

    let mut model = extract_structure(&project_root)?;
    enrich_with_jdtls(
        &project_root,
        &JdtlsOptions {
            command: options.jdtls_command.clone(),
            workspace,
            max_in_flight: options.jdtls_max_in_flight,
            deep_enrichment: options.jdtls_deep,
        },
        &mut model,
    )?;
    deduplicate_relationships(&mut model);
    score_candidates(&mut model);

    let temp_database = temp_database_path(&database);
    if temp_database.exists() {
        fs::remove_file(&temp_database).map_err(|error| {
            format!(
                "failed to remove stale temporary database {}: {error}",
                temp_database.display()
            )
        })?;
    }
    write_database(&temp_database, &model)?;
    if database.exists() {
        fs::remove_file(&database).map_err(|error| {
            format!(
                "failed to replace existing database {}: {error}",
                database.display()
            )
        })?;
    }
    fs::rename(&temp_database, &database).map_err(|error| {
        format!(
            "failed to move temporary database {} to {}: {error}",
            temp_database.display(),
            database.display()
        )
    })?;

    Ok(summary(&database, &model))
}

fn summary(database: &Path, model: &CodeModel) -> ExtractionSummary {
    ExtractionSummary {
        database_path: database.display().to_string(),
        module_count: model.modules.len(),
        class_count: model.classes.len(),
        method_count: model.methods.len(),
        relationship_count: model.relationships.len(),
        high_priority_candidates: model
            .candidate_scores
            .iter()
            .filter(|score| score.priority == "high")
            .count(),
        medium_priority_candidates: model
            .candidate_scores
            .iter()
            .filter(|score| score.priority == "medium")
            .count(),
        low_priority_candidates: model
            .candidate_scores
            .iter()
            .filter(|score| score.priority == "low")
            .count(),
        diagnostic_count: model.diagnostics.len(),
    }
}

fn deduplicate_relationships(model: &mut CodeModel) {
    model
        .relationships
        .sort_by(|left, right| relationship_key(left).cmp(&relationship_key(right)));
    model
        .relationships
        .dedup_by(|left, right| relationship_key(left) == relationship_key(right));
}

fn relationship_key(
    relationship: &crate::languages::java::business::model::RelationshipInfo,
) -> String {
    format!(
        "{}\0{}\0{}\0{}",
        relationship.source_id, relationship.target_id, relationship.kind, relationship.source
    )
}

fn temp_database_path(database: &Path) -> PathBuf {
    let file_name = database
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("business-extraction.db");
    database.with_file_name(format!(".{file_name}.tmp"))
}
