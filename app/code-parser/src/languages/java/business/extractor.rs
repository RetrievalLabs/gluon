use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::languages::business::model::{CodeModel, ExtractionSummary};
use crate::languages::business::{
    BusinessDatabasePath, BusinessExtractionOptions, BusinessExtractor,
};
use crate::languages::java::business::default_database_path;
use crate::languages::java::business::jdtls::{JdtlsOptions, enrich_with_jdtls};
use crate::languages::java::business::scoring::score_candidates;
use crate::languages::java::business::store::write_database;
use crate::languages::java::business::tree_sitter::extract_structure_with_stats;

pub struct JavaBusinessExtractor;

impl BusinessExtractor for JavaBusinessExtractor {
    fn language(&self) -> &'static str {
        "java"
    }

    fn extract_business(
        &self,
        options: &BusinessExtractionOptions,
    ) -> Result<ExtractionSummary, String> {
        extract_business(options)
    }
}

impl BusinessDatabasePath for JavaBusinessExtractor {
    fn default_database_path(
        &self,
        project_root: &Path,
        output_dir: &Path,
    ) -> Result<PathBuf, String> {
        default_database_path(project_root, output_dir)
    }
}

pub fn extract_business(options: &BusinessExtractionOptions) -> Result<ExtractionSummary, String> {
    let total_started_at = Instant::now();
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

    eprintln!(
        "extract-business tree-sitter: start path={}",
        project_root.display()
    );
    let phase_started_at = Instant::now();
    let extraction = extract_structure_with_stats(&project_root)?;
    let mut model = extraction.model;
    eprintln!(
        "extract-business tree-sitter: done java_seen={} parsed={} skipped_path={} skipped_generated={} modules={} classes={} methods={} invocations={} elapsed_ms={}",
        extraction.stats.java_files_seen,
        extraction.stats.java_files_parsed,
        extraction.stats.skipped_test_or_generated_path,
        extraction.stats.skipped_generated_content,
        model.modules.len(),
        model.classes.len(),
        model.methods.len(),
        model.invocations.len(),
        phase_started_at.elapsed().as_millis()
    );

    eprintln!(
        "extract-business jdtls: start command={} max_in_flight={} deep={}",
        options.jdtls_command, options.jdtls_max_in_flight, options.jdtls_deep
    );
    let phase_started_at = Instant::now();
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
    eprintln!(
        "extract-business jdtls: done relationships={} diagnostics={} elapsed_ms={}",
        model.relationships.len(),
        model.diagnostics.len(),
        phase_started_at.elapsed().as_millis()
    );

    eprintln!("extract-business relationships: deduplicate start");
    let phase_started_at = Instant::now();
    let relationships_before = model.relationships.len();
    deduplicate_relationships(&mut model);
    eprintln!(
        "extract-business relationships: deduplicate done before={} after={} elapsed_ms={}",
        relationships_before,
        model.relationships.len(),
        phase_started_at.elapsed().as_millis()
    );

    eprintln!(
        "extract-business scoring: start methods={}",
        model.methods.len()
    );
    let phase_started_at = Instant::now();
    score_candidates(&mut model);
    eprintln!(
        "extract-business scoring: done candidates={} signals={} elapsed_ms={}",
        model.candidate_scores.len(),
        model.candidate_signals.len(),
        phase_started_at.elapsed().as_millis()
    );

    let temp_database = temp_database_path(&database);
    eprintln!(
        "extract-business database: start path={} temp={}",
        database.display(),
        temp_database.display()
    );
    let phase_started_at = Instant::now();
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
    eprintln!(
        "extract-business database: done path={} elapsed_ms={}",
        database.display(),
        phase_started_at.elapsed().as_millis()
    );
    eprintln!(
        "extract-business done: total_elapsed_ms={}",
        total_started_at.elapsed().as_millis()
    );

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

fn relationship_key(relationship: &crate::languages::business::model::RelationshipInfo) -> String {
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
