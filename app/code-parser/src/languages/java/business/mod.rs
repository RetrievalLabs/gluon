use std::path::{Path, PathBuf};

pub mod extractor;
pub mod jdtls;
pub mod modules;
pub mod scoring;
pub mod store;
pub mod test_extractor;
pub mod tree_sitter;

pub use crate::languages::business::BusinessExtractionOptions;
pub use crate::languages::business::kg::{
    BuildBusinessKgOptions, BuildBusinessKgSummary, Priority, build_business_kg,
};
pub use crate::languages::business::model::ExtractionSummary;
pub use extractor::{JavaBusinessExtractor, extract_business};
pub use test_extractor::{TestExtractionOptions, TestExtractionSummary, extract_tests};

pub fn default_database_path(project_root: &Path, output_dir: &Path) -> Result<PathBuf, String> {
    let project_name = project_root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(sanitize_path_segment)
        .ok_or_else(|| {
            format!(
                "path has no usable directory name: {}",
                project_root.display()
            )
        })?;
    Ok(output_dir.join(project_name).join("business-extraction.db"))
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
