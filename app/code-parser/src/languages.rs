use std::path::Path;

use crate::languages::java::build::resolver::CommandRunner;

pub mod business;
pub mod java;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParseOptions {
    pub resolve: bool,
}

pub trait LanguageParser {
    type Report;

    fn language(&self) -> &'static str;

    fn parse_project(
        &self,
        path: &Path,
        options: ParseOptions,
        runner: &dyn CommandRunner,
    ) -> Result<Self::Report, String>;
}
