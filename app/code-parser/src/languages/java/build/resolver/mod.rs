use std::path::Path;

use crate::languages::java::build::model::BuildReport;

mod gradle;
mod maven;
mod runner;

pub use runner::{CommandOutput, CommandRunner, SystemCommandRunner};

pub struct BuildResolver<'a> {
    runner: &'a dyn CommandRunner,
}

impl<'a> BuildResolver<'a> {
    pub fn new(runner: &'a dyn CommandRunner) -> Self {
        Self { runner }
    }

    pub fn resolve(&self, project_root: &Path, report: &mut BuildReport) {
        if project_root.join("pom.xml").exists() {
            maven::resolve_maven(self.runner, project_root, report);
        }
        if gradle::has_gradle_build(project_root) {
            gradle::resolve_gradle(self.runner, project_root, report);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::languages::java::build::parse_build_with_runner;

    use super::*;

    struct MockRunner {
        outputs: RefCell<HashMap<String, CommandOutput>>,
    }

    impl MockRunner {
        fn new(outputs: HashMap<String, CommandOutput>) -> Self {
            Self {
                outputs: RefCell::new(outputs),
            }
        }
    }

    impl CommandRunner for MockRunner {
        fn run(&self, executable: &str, args: &[&str], _cwd: &Path) -> io::Result<CommandOutput> {
            let key = runner::command_vec(executable, args).join(" ");
            self.outputs
                .borrow_mut()
                .remove(&key)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, key))
        }
    }

    #[test]
    fn resolver_adds_maven_dependency_list_results() {
        let root = test_dir("maven-resolve");
        fs::write(
            root.join("pom.xml"),
            "<project><modelVersion>4.0.0</modelVersion></project>",
        )
        .unwrap();
        let mut outputs = HashMap::new();
        outputs.insert(
            "mvn help:effective-pom -DskipTests".to_string(),
            CommandOutput {
                status: 0,
                stdout: "<project><dependencies><dependency><groupId>a</groupId><artifactId>b</artifactId><version>1</version></dependency></dependencies></project>".to_string(),
                stderr: String::new(),
            },
        );
        outputs.insert(
            "mvn dependency:list -DincludeScope=runtime -DoutputAbsoluteArtifactFilename=false -DskipTests"
                .to_string(),
            CommandOutput {
                status: 0,
                stdout: "[INFO]    org.slf4j:slf4j-api:jar:2.0.17:compile".to_string(),
                stderr: String::new(),
            },
        );

        let report = parse_build_with_runner(&root, true, &MockRunner::new(outputs)).unwrap();

        assert!(
            report
                .resolved_dependencies
                .iter()
                .any(|dependency| dependency.artifact_id == "slf4j-api")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolver_keeps_report_when_gradle_fails() {
        let root = test_dir("gradle-fail");
        fs::write(root.join("build.gradle"), "plugins { id 'java' }").unwrap();
        let mut outputs = HashMap::new();
        outputs.insert(
            "gradle dependencies --configuration runtimeClasspath".to_string(),
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "Could not resolve all files".to_string(),
            },
        );
        outputs.insert(
            "gradle buildEnvironment".to_string(),
            CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "Could not resolve plugin".to_string(),
            },
        );

        let report = parse_build_with_runner(&root, true, &MockRunner::new(outputs)).unwrap();

        assert!(
            report
                .declared_plugins
                .iter()
                .any(|plugin| plugin.id == "java")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.category == "repository_or_auth_failure")
        );
        let _ = fs::remove_dir_all(root);
    }

    fn test_dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "code-parser-{name}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
}
