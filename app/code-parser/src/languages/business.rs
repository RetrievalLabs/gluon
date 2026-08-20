use std::path::Path;

pub mod characterization;
pub mod kg;
pub mod model;

pub use characterization::{
    GenerateCharacterizationTestsOptions, GenerateCharacterizationTestsSummary,
    generate_characterization_tests,
};
pub use kg::{BuildBusinessKgOptions, BuildBusinessKgSummary, Priority, build_business_kg};
pub use model::ExtractionSummary;

pub trait BusinessExtractor {
    fn language(&self) -> &'static str;

    fn extract_business(
        &self,
        options: &BusinessExtractionOptions,
    ) -> Result<ExtractionSummary, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BusinessExtractionOptions {
    pub path: std::path::PathBuf,
    pub output_dir: std::path::PathBuf,
    pub database: Option<std::path::PathBuf>,
    pub jdtls_command: String,
    pub jdtls_workspace: Option<std::path::PathBuf>,
    pub jdtls_max_in_flight: usize,
    pub resume: bool,
}

pub trait BusinessDatabasePath {
    fn default_database_path(
        &self,
        project_root: &Path,
        output_dir: &Path,
    ) -> Result<std::path::PathBuf, String>;
}
