mod cli;
mod core;
pub mod languages;
pub mod proto;

pub use cli::run_cli;
pub use core::error::{
    CheckpointError, DatabaseError, FileError, JdtlsError, KgError, KnowledgeBaseError, LlmError,
    ParserError, PathError,
};
pub use languages::business::{
    BuildBusinessKgOptions, BuildBusinessKgSummary, BuildError, BusinessExtractionOptions,
    CharacterizationError, ExtractionSummary, GenerateCharacterizationTestsOptions,
    GenerateCharacterizationTestsSummary, Priority, build_business_kg,
    generate_characterization_tests,
};
pub use languages::java::JavaLanguageParser;
pub use languages::java::build::model::{
    BuildReport, BuildToolInfo, DependencyInfo, Diagnostic, JavaVersionInfo, PluginInfo,
};
pub use languages::java::build::resolver::{CommandOutput, CommandRunner, SystemCommandRunner};
pub use languages::java::build::{
    BuildParseError, BuildSystemParser, parse_build, parse_build_with_runner,
};
pub use languages::java::business::{
    BusinessExtractionError, TestExtractionError, extract_business,
};
pub use languages::java::compatibility::CompatibilityError;
pub use languages::{LanguageParser, ParseOptions};
