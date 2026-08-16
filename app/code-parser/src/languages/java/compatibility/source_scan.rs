use std::fs;
use std::path::Path;

use walkdir::{DirEntry, WalkDir};

use crate::languages::java::build::model::Diagnostic;
use crate::languages::java::compatibility::knowledge_base::ApiRule;
use crate::languages::java::compatibility::model::ApiFinding;

pub fn scan_java_sources(
    source_path: &Path,
    target_java: u32,
    kb_rules: &[(&str, &[ApiRule])],
) -> (Vec<ApiFinding>, Vec<Diagnostic>) {
    let mut findings = Vec::new();
    let mut diagnostics = Vec::new();

    if !source_path.exists() {
        diagnostics.push(Diagnostic::warning(
            "source_scan",
            format!("source path does not exist: {}", source_path.display()),
            Some(source_path.display().to_string()),
        ));
        return (findings, diagnostics);
    }

    for entry in WalkDir::new(source_path)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(Diagnostic::warning("source_scan", error.to_string(), None));
                continue;
            }
        };
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|ext| ext.to_str()) != Some("java")
        {
            continue;
        }
        match fs::read_to_string(entry.path()) {
            Ok(contents) => scan_file(
                source_path,
                entry.path(),
                &contents,
                target_java,
                kb_rules,
                &mut findings,
            ),
            Err(error) => diagnostics.push(Diagnostic::warning(
                "source_scan",
                format!("failed to read {}: {error}", entry.path().display()),
                Some(entry.path().display().to_string()),
            )),
        }
    }

    (findings, diagnostics)
}

fn scan_file(
    source_root: &Path,
    file: &Path,
    contents: &str,
    target_java: u32,
    kb_rules: &[(&str, &[ApiRule])],
    findings: &mut Vec<ApiFinding>,
) {
    let display_path = relative_path(source_root, file);
    for (line_index, line) in contents.lines().enumerate() {
        for (category, rules) in kb_rules {
            for rule in *rules {
                if let Some(minimum) = rule.applies_when_target_java_at_least {
                    if target_java < minimum {
                        continue;
                    }
                }
                for matched_text in matched_terms(rule, line) {
                    findings.push(ApiFinding {
                        rule_id: rule.id.clone(),
                        category: (*category).to_string(),
                        severity: rule.severity.clone(),
                        file: display_path.clone(),
                        line: line_index + 1,
                        matched_text,
                        guidance: rule.guidance.clone(),
                        source_ids: rule.source_ids.clone(),
                    });
                }
            }
        }
    }
}

fn matched_terms(rule: &ApiRule, line: &str) -> Vec<String> {
    if rule
        .except_symbol_prefixes
        .iter()
        .any(|prefix| line.contains(prefix))
    {
        return Vec::new();
    }

    let mut terms = Vec::new();
    for symbol in &rule.symbols {
        if line.contains(symbol)
            || line.contains(&call_form(symbol))
            || line.contains(&constructor_form(symbol))
        {
            terms.push(symbol.clone());
        }
    }
    for prefix in &rule.symbol_prefixes {
        if line.contains(prefix) {
            terms.push(prefix.clone());
        }
    }
    for pattern in &rule.patterns {
        if line.contains(pattern) {
            terms.push(pattern.clone());
        }
    }
    for pattern in &rule.symbol_patterns {
        if wildcard_match(pattern, line) {
            terms.push(pattern.clone());
        }
    }
    terms
}

fn call_form(symbol: &str) -> String {
    symbol
        .trim_end_matches("()")
        .rsplit('.')
        .next()
        .unwrap_or(symbol)
        .to_string()
}

fn constructor_form(symbol: &str) -> String {
    symbol
        .rsplit('.')
        .next()
        .unwrap_or(symbol)
        .replace("<init>", "new")
}

fn wildcard_match(pattern: &str, line: &str) -> bool {
    let required = pattern.replace('*', "");
    !required.is_empty() && line.contains(&required)
}

fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && matches!(
            entry.file_name().to_string_lossy().as_ref(),
            ".git" | "target" | "build" | ".gradle" | ".idea"
        )
}

fn relative_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::languages::java::compatibility::knowledge_base::JavaCompatibilityKnowledgeBase;

    use super::*;

    #[test]
    fn detects_java_api_patterns() {
        let root = test_dir("source-scan");
        fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
        fs::write(
            root.join("src/main/java/demo/Demo.java"),
            r#"
            package demo;
            import javax.xml.bind.JAXBContext;
            import sun.misc.BASE64Encoder;
            class Demo {
              void x() throws Exception {
                Demo.class.getDeclaredField("x").setAccessible(true);
              }
            }
            "#,
        )
        .unwrap();
        let kb = JavaCompatibilityKnowledgeBase::load_default().unwrap();
        let (findings, diagnostics) = scan_java_sources(
            &root,
            25,
            &[
                ("removed_api", &kb.removed_apis),
                ("deprecated_for_removal_api", &kb.deprecated_for_removal),
                ("internal_api", &kb.internal_apis),
                ("reflective_access", &kb.reflective_access),
            ],
        );

        assert!(diagnostics.is_empty());
        assert!(
            findings
                .iter()
                .any(|finding| finding.matched_text == "javax.xml.bind")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.matched_text == "sun.misc.BASE64Encoder")
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.matched_text == "setAccessible(true)")
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
