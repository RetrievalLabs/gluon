use std::path::{Path, PathBuf};

use thiserror::Error;
use walkdir::{DirEntry, WalkDir};

use crate::core::error::{FileError, PathError};
use crate::languages::java::build::gradle::{
    GradleBuildParser, GradleVersionCatalogParser, GradleWrapperParser,
};
use crate::languages::java::build::maven::MavenPomParser;
use crate::languages::java::build::model::BuildReport;
use crate::languages::java::build::resolver::{BuildResolver, CommandRunner, SystemCommandRunner};

pub mod gradle;
pub mod maven;
pub mod model;
pub mod resolver;

pub trait BuildSystemParser {
    fn build_system(&self) -> &'static str;

    fn parse_file(&self, project_root: &Path, file: &Path, report: &mut BuildReport);
}

pub type BuildParseResult<T> = Result<T, BuildParseError>;

#[derive(Debug, Error)]
pub enum BuildParseError {
    #[error(transparent)]
    Path(#[from] PathError),

    #[error(transparent)]
    File(#[from] FileError),
}

pub fn parse_build(path: &Path, resolve: bool) -> BuildParseResult<BuildReport> {
    parse_build_with_runner(path, resolve, &SystemCommandRunner)
}

pub fn parse_build_with_runner(
    path: &Path,
    resolve: bool,
    runner: &dyn CommandRunner,
) -> BuildParseResult<BuildReport> {
    if !path.exists() {
        return Err(PathError::NotFound(path.to_path_buf()).into());
    }

    let project_root = if path.is_file() {
        path.parent()
            .ok_or_else(|| PathError::NoParent(path.to_path_buf()))?
            .to_path_buf()
    } else {
        path.to_path_buf()
    };

    let mut report = BuildReport {
        project_root: project_root.display().to_string(),
        ..BuildReport::default()
    };

    let files = discover_build_files(path)?;
    let maven = MavenPomParser;
    let gradle = GradleBuildParser;
    let wrapper = GradleWrapperParser;
    let catalog = GradleVersionCatalogParser;

    for file in files {
        match file.file_name().and_then(|name| name.to_str()) {
            Some("pom.xml") => maven.parse_file(&project_root, &file, &mut report),
            Some("gradle-wrapper.properties") => {
                wrapper.parse_file(&project_root, &file, &mut report);
            }
            Some("libs.versions.toml") => {
                catalog.parse_file(&project_root, &file, &mut report);
            }
            Some(
                "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts",
            ) => {
                gradle.parse_file(&project_root, &file, &mut report);
            }
            _ => {}
        }
    }

    if resolve {
        BuildResolver::new(runner).resolve(&project_root, &mut report);
    }

    report.rebuild_scopes();
    Ok(report)
}

fn discover_build_files(path: &Path) -> BuildParseResult<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(path)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
    {
        let entry = entry.map_err(|error| FileError::Walk(error.to_string()))?;
        if !entry.file_type().is_file() {
            continue;
        }

        let file_name = entry.file_name().to_string_lossy();
        if matches!(
            file_name.as_ref(),
            "pom.xml"
                | "build.gradle"
                | "build.gradle.kts"
                | "settings.gradle"
                | "settings.gradle.kts"
                | "gradle-wrapper.properties"
                | "libs.versions.toml"
        ) {
            files.push(entry.path().to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}

fn is_ignored_dir(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return false;
    }

    matches!(
        entry.file_name().to_string_lossy().as_ref(),
        ".git" | ".gradle" | "build" | "target" | ".idea"
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("code-parser-{name}-{nanos}"))
    }

    #[test]
    fn discovers_build_files_deterministically() {
        let root = test_dir("discover");
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("build")).unwrap();
        fs::write(root.join("pom.xml"), "").unwrap();
        fs::write(root.join("a").join("build.gradle"), "").unwrap();
        fs::write(root.join("build").join("pom.xml"), "").unwrap();

        let files = discover_build_files(&root).unwrap();
        let relative: Vec<_> = files
            .iter()
            .map(|file| crate::core::fs::relative_path(&root, file))
            .collect();

        assert_eq!(relative, vec!["a/build.gradle", "pom.xml"]);
        let _ = fs::remove_dir_all(root);
    }
}
