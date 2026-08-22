use std::path::Path;

use crate::languages::{LanguageParser, ParseOptions};

pub mod build;
pub mod business;
pub mod compatibility;

pub struct JavaLanguageParser;

impl LanguageParser for JavaLanguageParser {
    type Report = build::model::BuildReport;
    type Error = build::BuildParseError;

    fn language(&self) -> &'static str {
        "java"
    }

    fn parse_project(
        &self,
        path: &Path,
        options: ParseOptions,
        runner: &dyn build::resolver::CommandRunner,
    ) -> Result<Self::Report, Self::Error> {
        build::parse_build_with_runner(path, options.resolve, runner)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::languages::java::build::resolver::{CommandOutput, CommandRunner};

    use super::*;

    struct NoopRunner;

    impl CommandRunner for NoopRunner {
        fn run(&self, _executable: &str, _args: &[&str], _cwd: &Path) -> io::Result<CommandOutput> {
            unreachable!("offline parse must not execute commands");
        }
    }

    #[test]
    fn java_parser_exposes_language_name() {
        let parser = JavaLanguageParser;

        assert_eq!(parser.language(), "java");
    }

    #[test]
    fn java_parser_uses_language_level_parse_contract() {
        let root = std::env::temp_dir().join(format!(
            "code-parser-java-language-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let parser = JavaLanguageParser;

        let report = parser
            .parse_project(&root, ParseOptions { resolve: false }, &NoopRunner)
            .expect("empty project parses");

        assert_eq!(report.project_root, root.display().to_string());
        let _ = fs::remove_dir_all(root);
    }
}
