use std::fs;
use std::path::Path;

use crate::languages::java::build::model::{BuildReport, Diagnostic};

pub(crate) fn read_to_string(path: &Path, report: &mut BuildReport) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(contents) => Some(contents),
        Err(error) => {
            report.diagnostics.push(Diagnostic::error(
                "read_failed",
                format!("failed to read {}: {error}", path.display()),
                Some(path.display().to_string()),
            ));
            None
        }
    }
}

pub(crate) fn relative_path(project_root: &Path, file: &Path) -> String {
    file.strip_prefix(project_root)
        .unwrap_or(file)
        .display()
        .to_string()
}
