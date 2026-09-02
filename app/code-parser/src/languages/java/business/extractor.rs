use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::core::error::{
    CheckpointError, DatabaseError, FileError, JdtlsError, ParserError, PathError,
};
use crate::languages::business::model::{CodeModel, ExtractionSummary};
use crate::languages::business::{
    BusinessDatabasePath, BusinessExtractionOptions, BusinessExtractor,
};
use crate::languages::java::build::model::BuildReport;
use crate::languages::java::business::default_database_path;
use crate::languages::java::business::jdtls::{JdtlsOptions, enrich_with_jdtls};
use crate::languages::java::business::modules::modules_from_build_report;
use crate::languages::java::business::scoring::score_candidates;
use crate::languages::java::business::store::write_database;
use crate::languages::java::business::tree_sitter::extract_structure_with_modules;

pub struct JavaBusinessExtractor;

const CHECKPOINT_VERSION: u32 = 1;

pub type BusinessExtractionResult<T> = Result<T, BusinessExtractionError>;

#[derive(Debug, Error)]
pub enum BusinessExtractionError {
    #[error(transparent)]
    Path(#[from] PathError),

    #[error(transparent)]
    File(#[from] FileError),

    #[error(transparent)]
    Checkpoint(#[from] CheckpointError),

    #[error(transparent)]
    Parser(#[from] ParserError),

    #[error(transparent)]
    Jdtls(#[from] JdtlsError),

    #[error(transparent)]
    Database(#[from] DatabaseError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtractionCheckpoint {
    version: u32,
    project_root: String,
    phase: CheckpointPhase,
    model: CodeModel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CheckpointPhase {
    Structure,
    Jdtls,
    Scored,
}

impl BusinessExtractor for JavaBusinessExtractor {
    type Error = BusinessExtractionError;

    fn language(&self) -> &'static str {
        "java"
    }

    fn extract_business(
        &self,
        options: &BusinessExtractionOptions,
    ) -> BusinessExtractionResult<ExtractionSummary> {
        extract_business(options)
    }
}

impl BusinessDatabasePath for JavaBusinessExtractor {
    fn default_database_path(
        &self,
        project_root: &Path,
        output_dir: &Path,
    ) -> Result<PathBuf, PathError> {
        default_database_path(project_root, output_dir)
    }
}

pub fn extract_business(
    options: &BusinessExtractionOptions,
) -> BusinessExtractionResult<ExtractionSummary> {
    let total_started_at = Instant::now();
    if !options.path.exists() {
        return Err(PathError::NotFound(options.path.clone()).into());
    }
    let project_root = if options.path.is_file() {
        options
            .path
            .parent()
            .ok_or_else(|| PathError::NoParent(options.path.clone()))?
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
    let checkpoint_path = checkpoint_path(&database);

    let mut checkpoint = if options.resume {
        load_checkpoint(&checkpoint_path, &project_root)
            .map_err(|error| CheckpointError::Operation(error))?
    } else {
        remove_checkpoint_if_present(&checkpoint_path)
            .map_err(|error| CheckpointError::Operation(error))?;
        None
    };

    let mut model = if let Some(checkpoint) = checkpoint.take() {
        eprintln!(
            "extract-business resume: loaded checkpoint phase={:?} path={}",
            checkpoint.phase,
            checkpoint_path.display()
        );
        checkpoint.model
    } else {
        eprintln!(
            "extract-business tree-sitter: start path={}",
            project_root.display()
        );
        let phase_started_at = Instant::now();
        let modules = options
            .build_report
            .as_deref()
            .map(load_modules_from_build_report)
            .transpose()?;
        let extraction = extract_structure_with_modules(&project_root, modules)
            .map_err(|error| ParserError::Operation(error))?;
        let model = extraction.model;
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
        save_checkpoint(
            &checkpoint_path,
            &project_root,
            CheckpointPhase::Structure,
            &model,
        )
        .map_err(|error| CheckpointError::Operation(error))?;
        model
    };

    let phase = checkpoint_phase(&checkpoint_path).unwrap_or(CheckpointPhase::Structure);
    if phase == CheckpointPhase::Structure {
        eprintln!(
            "extract-business jdtls: start command={} max_in_flight={}",
            options.jdtls_command, options.jdtls_max_in_flight
        );
        let phase_started_at = Instant::now();
        enrich_with_jdtls(
            &project_root,
            &JdtlsOptions {
                command: options.jdtls_command.clone(),
                workspace,
                max_in_flight: options.jdtls_max_in_flight,
            },
            &mut model,
        )
        .map_err(|error| JdtlsError::Operation(error))?;
        eprintln!(
            "extract-business jdtls: done relationships={} diagnostics={} elapsed_ms={}",
            model.relationships.len(),
            model.diagnostics.len(),
            phase_started_at.elapsed().as_millis()
        );
        save_checkpoint(
            &checkpoint_path,
            &project_root,
            CheckpointPhase::Jdtls,
            &model,
        )
        .map_err(|error| CheckpointError::Operation(error))?;
    } else {
        eprintln!("extract-business jdtls: skipped from checkpoint phase={phase:?}");
    }

    let phase = checkpoint_phase(&checkpoint_path).unwrap_or(CheckpointPhase::Jdtls);
    if phase != CheckpointPhase::Scored {
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
        save_checkpoint(
            &checkpoint_path,
            &project_root,
            CheckpointPhase::Scored,
            &model,
        )
        .map_err(|error| CheckpointError::Operation(error))?;
    } else {
        eprintln!("extract-business scoring: skipped from checkpoint phase={phase:?}");
    }

    let temp_database = temp_database_path(&database);
    eprintln!(
        "extract-business database: start path={} temp={}",
        database.display(),
        temp_database.display()
    );
    let phase_started_at = Instant::now();
    if temp_database.exists() {
        fs::remove_file(&temp_database).map_err(|source| FileError::Remove {
            path: temp_database.clone(),
            source,
        })?;
    }
    write_database(&temp_database, &model).map_err(DatabaseError::Operation)?;
    if database.exists() {
        fs::remove_file(&database).map_err(|source| FileError::Remove {
            path: database.clone(),
            source,
        })?;
    }
    fs::rename(&temp_database, &database).map_err(|source| {
        DatabaseError::Operation(format!(
            "failed to move temporary database {} to {}: {source}",
            temp_database.display(),
            database.display()
        ))
    })?;
    remove_checkpoint_if_present(&checkpoint_path)
        .map_err(|error| CheckpointError::Operation(error))?;
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

fn load_modules_from_build_report(
    path: &Path,
) -> BusinessExtractionResult<Vec<crate::languages::business::model::ModuleInfo>> {
    let data = fs::read_to_string(path).map_err(|source| FileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut build_report: BuildReport = serde_json::from_str(&data).map_err(|error| {
        ParserError::Operation(format!("failed to parse build report: {error}"))
    })?;
    if build_report.build_tools.is_empty()
        && build_report.java_versions.is_empty()
        && build_report.direct_dependencies.is_empty()
        && build_report.direct_plugins.is_empty()
    {
        build_report.rebuild_flat_inventory();
    }
    Ok(modules_from_build_report(&build_report))
}

fn checkpoint_path(database: &Path) -> PathBuf {
    let file_name = database
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("business-extraction.db");
    database.with_file_name(format!(".{file_name}.extract-business-checkpoint.json"))
}

fn save_checkpoint(
    path: &Path,
    project_root: &Path,
    phase: CheckpointPhase,
    model: &CodeModel,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed to create checkpoint directory {}: {error}",
                parent.display()
            )
        })?;
    }
    let checkpoint = ExtractionCheckpoint {
        version: CHECKPOINT_VERSION,
        project_root: project_root.display().to_string(),
        phase,
        model: model.clone(),
    };
    let json = serde_json::to_string(&checkpoint)
        .map_err(|error| format!("failed to serialize extraction checkpoint: {error}"))?;
    fs::write(path, json)
        .map_err(|error| format!("failed to write checkpoint {}: {error}", path.display()))?;
    eprintln!(
        "extract-business checkpoint: saved phase={phase:?} path={}",
        path.display()
    );
    Ok(())
}

fn load_checkpoint(
    path: &Path,
    project_root: &Path,
) -> Result<Option<ExtractionCheckpoint>, String> {
    if !path.exists() {
        eprintln!(
            "extract-business resume: no checkpoint found at {}; starting from beginning",
            path.display()
        );
        return Ok(None);
    }
    let json = fs::read_to_string(path)
        .map_err(|error| format!("failed to read checkpoint {}: {error}", path.display()))?;
    let checkpoint: ExtractionCheckpoint = serde_json::from_str(&json)
        .map_err(|error| format!("failed to parse checkpoint {}: {error}", path.display()))?;
    if checkpoint.version != CHECKPOINT_VERSION {
        return Err(format!(
            "unsupported checkpoint version {} at {}",
            checkpoint.version,
            path.display()
        ));
    }
    if checkpoint.project_root != project_root.display().to_string() {
        return Err(format!(
            "checkpoint project root mismatch.\ncheckpoint: {}\ncurrent: {}",
            checkpoint.project_root,
            project_root.display()
        ));
    }
    Ok(Some(checkpoint))
}

fn checkpoint_phase(path: &Path) -> Option<CheckpointPhase> {
    let json = fs::read_to_string(path).ok()?;
    serde_json::from_str::<ExtractionCheckpoint>(&json)
        .ok()
        .map(|checkpoint| checkpoint.phase)
}

fn remove_checkpoint_if_present(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| format!("failed to remove checkpoint {}: {error}", path.display()))?;
    }
    Ok(())
}
