use std::fs;
use std::path::Path;

use std::collections::HashSet;
use tree_sitter::{Node, Parser, TreeCursor};
use walkdir::{DirEntry, WalkDir};

use crate::languages::java::build::model::Diagnostic;
use crate::languages::java::business::jdtls::{
    JdtlsOptions, JdtlsSymbolRequest, resolve_compatibility_symbols,
};
use crate::languages::java::compatibility::knowledge_base::ApiRule;
use crate::languages::java::compatibility::model::ApiFinding;

pub fn scan_java_sources_with_jdtls(
    source_path: &Path,
    target_java: u32,
    kb_rules: &[(&str, &[ApiRule])],
    jdtls_options: &JdtlsOptions,
) -> Result<(Vec<ApiFinding>, Vec<Diagnostic>), String> {
    let mut diagnostics = Vec::new();

    if !source_path.exists() {
        diagnostics.push(Diagnostic::warning(
            "source_scan",
            format!("source path does not exist: {}", source_path.display()),
            Some(source_path.display().to_string()),
        ));
        return Ok((Vec::new(), diagnostics));
    }

    let mut candidates = Vec::new();
    let mut java_files = Vec::new();

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
            Ok(contents) => {
                let display_path = relative_path(source_path, entry.path());
                java_files.push(display_path.clone());
                match syntax_candidates(&contents, &display_path) {
                    Ok(file_candidates) => candidates.extend(file_candidates),
                    Err(error) => diagnostics.push(Diagnostic::warning(
                        "source_scan",
                        format!(
                            "failed to parse Java source {}: {error}",
                            entry.path().display()
                        ),
                        Some(entry.path().display().to_string()),
                    )),
                }
            }
            Err(error) => diagnostics.push(Diagnostic::warning(
                "source_scan",
                format!("failed to read {}: {error}", entry.path().display()),
                Some(entry.path().display().to_string()),
            )),
        }
    }

    let requests = candidates
        .iter()
        .map(|candidate| JdtlsSymbolRequest {
            file: candidate.file.clone(),
            name: candidate.name.clone(),
            line: candidate.line,
            column: candidate.column,
            values: candidate.values.clone(),
        })
        .collect::<Vec<_>>();
    let resolved =
        resolve_compatibility_symbols(source_path, jdtls_options, &java_files, &requests)?;
    let mut findings = Vec::new();
    let mut seen = HashSet::new();

    for symbol in resolved {
        scan_resolved_symbol(
            &symbol.file,
            symbol.line,
            &symbol.values,
            target_java,
            kb_rules,
            &mut seen,
            &mut findings,
        );
    }

    Ok((findings, diagnostics))
}

fn scan_resolved_symbol(
    display_path: &str,
    line: usize,
    values: &[String],
    target_java: u32,
    kb_rules: &[(&str, &[ApiRule])],
    seen: &mut HashSet<String>,
    findings: &mut Vec<ApiFinding>,
) {
    for (category, rules) in kb_rules {
        for rule in *rules {
            if let Some(minimum) = rule.applies_when_target_java_at_least {
                if target_java < minimum {
                    continue;
                }
            }
            for matched_text in matched_terms(rule, values) {
                let key = format!(
                    "{}\0{}\0{}\0{}\0{}",
                    rule.id, category, display_path, line, matched_text
                );
                if !seen.insert(key) {
                    continue;
                }
                findings.push(ApiFinding {
                    rule_id: rule.id.clone(),
                    category: (*category).to_string(),
                    severity: rule.severity.clone(),
                    file: display_path.to_string(),
                    line,
                    matched_text,
                    guidance: rule.guidance.clone(),
                    source_ids: rule.source_ids.clone(),
                });
            }
        }
    }
}

#[derive(Debug)]
struct SourceCandidate {
    file: String,
    name: String,
    line: usize,
    column: usize,
    values: Vec<String>,
}

fn syntax_candidates(contents: &str, file: &str) -> Result<Vec<SourceCandidate>, String> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
        .map_err(|error| format!("failed to initialize Java parser: {error}"))?;
    let tree = parser
        .parse(contents, None)
        .ok_or_else(|| "parser returned no syntax tree".to_string())?;
    if tree.root_node().has_error() {
        return Err("syntax tree contains parse errors".to_string());
    }
    let mut candidates = Vec::new();
    let mut cursor = tree.walk();
    collect_syntax_candidates(contents, file, &mut cursor, &mut candidates);
    Ok(candidates)
}

fn collect_syntax_candidates(
    contents: &str,
    file: &str,
    cursor: &mut TreeCursor<'_>,
    candidates: &mut Vec<SourceCandidate>,
) {
    let node = cursor.node();
    collect_node_candidates(contents, file, node, candidates);

    if cursor.goto_first_child() {
        loop {
            collect_syntax_candidates(contents, file, cursor, candidates);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn collect_node_candidates(
    contents: &str,
    file: &str,
    node: Node<'_>,
    candidates: &mut Vec<SourceCandidate>,
) {
    if !node.is_named() || is_ignored_syntax_node(node.kind()) {
        return;
    }

    let raw = node.utf8_text(contents.as_bytes()).unwrap_or("");
    let mut values = match node.kind() {
        "import_declaration" => import_candidates(raw),
        "scoped_identifier"
        | "field_access"
        | "method_invocation"
        | "object_creation_expression"
        | "annotation"
        | "marker_annotation" => expression_candidates(raw),
        _ => Vec::new(),
    };

    if values.is_empty() {
        return;
    }
    values.sort();
    values.dedup();
    let target = definition_target_node(node).unwrap_or(node);
    candidates.push(SourceCandidate {
        file: file.to_string(),
        name: candidate_name(&values),
        line: node.start_position().row + 1,
        column: target.start_position().column,
        values,
    });
}

fn definition_target_node<'tree>(node: Node<'tree>) -> Option<Node<'tree>> {
    node.child_by_field_name("name")
        .or_else(|| node.child_by_field_name("type"))
        .or_else(|| node.named_child(0))
}

fn candidate_name(values: &[String]) -> String {
    values.first().cloned().unwrap_or_default()
}

fn import_candidates(raw: &str) -> Vec<String> {
    let imported = raw
        .trim()
        .trim_start_matches("import")
        .trim()
        .trim_start_matches("static")
        .trim()
        .trim_end_matches(';')
        .trim();
    expression_candidates(imported)
}

fn expression_candidates(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut candidates = vec![trimmed.to_string(), compact_whitespace(trimmed)];
    if let Some(method_name) = method_invocation_name(trimmed) {
        candidates.push(method_name);
    }
    for literal in class_for_name_literals(trimmed) {
        candidates.push(literal);
    }
    candidates
}

fn method_invocation_name(value: &str) -> Option<String> {
    let before_args = value.split_once('(')?.0.trim();
    let method = before_args.rsplit('.').next()?.trim();
    if method.is_empty() {
        return None;
    }
    let args = value.split_once('(')?.1.rsplit_once(')')?.0;
    Some(format!("{}({})", method, compact_whitespace(args)))
}

fn class_for_name_literals(value: &str) -> Vec<String> {
    let compact = compact_whitespace(value);
    let Some(arguments) = compact.strip_prefix("Class.forName(") else {
        return Vec::new();
    };
    let Some(literal) = arguments.strip_prefix('"') else {
        return Vec::new();
    };
    let Some((class_name, _)) = literal.split_once('"') else {
        return Vec::new();
    };
    vec![
        class_name.to_string(),
        format!("Class.forName(\"{class_name}"),
    ]
}

fn compact_whitespace(value: &str) -> String {
    value.split_whitespace().collect()
}

fn is_ignored_syntax_node(kind: &str) -> bool {
    matches!(
        kind,
        "comment" | "line_comment" | "block_comment" | "string_literal" | "character_literal"
    )
}

fn matched_terms(rule: &ApiRule, candidates: &[String]) -> Vec<String> {
    if rule.except_symbol_prefixes.iter().any(|prefix| {
        candidates
            .iter()
            .any(|candidate| candidate.contains(prefix))
    }) {
        return Vec::new();
    }

    let mut terms = Vec::new();
    for symbol in &rule.symbols {
        if candidates
            .iter()
            .any(|candidate| candidate == symbol || candidate.starts_with(&format!("{symbol}.")))
        {
            terms.push(symbol.clone());
        }
    }
    for prefix in &rule.symbol_prefixes {
        if candidates.iter().any(|candidate| {
            candidate.starts_with(prefix) || candidate.contains(&format!(".{prefix}"))
        }) {
            terms.push(prefix.clone());
        }
    }
    for pattern in &rule.patterns {
        if candidates
            .iter()
            .any(|candidate| candidate.contains(pattern))
        {
            terms.push(pattern.clone());
        }
    }
    for pattern in &rule.symbol_patterns {
        if candidates
            .iter()
            .any(|candidate| wildcard_match(pattern, candidate))
        {
            terms.push(pattern.clone());
        }
    }
    terms.sort();
    terms.dedup();
    terms
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

    #[test]
    fn resolved_symbols_match_java_api_patterns() {
        let kb = JavaCompatibilityKnowledgeBase::load_default().unwrap();
        let mut findings = Vec::new();
        let mut seen = HashSet::new();
        scan_resolved_symbol(
            "src/main/java/demo/Demo.java",
            3,
            &[
                "javax.xml.bind.JAXBContext".to_string(),
                "sun.misc.BASE64Encoder".to_string(),
                "setAccessible(true)".to_string(),
            ],
            25,
            &[
                ("removed_api", &kb.removed_apis),
                ("deprecated_for_removal_api", &kb.deprecated_for_removal),
                ("internal_api", &kb.internal_apis),
                ("reflective_access", &kb.reflective_access),
            ],
            &mut seen,
            &mut findings,
        );

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
    }

    #[test]
    fn syntax_candidates_ignore_comments_and_non_reflective_strings() {
        let root = test_dir("source-scan-comments");
        fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
        let contents = r#"
            package demo;
            class Demo {
              // import javax.xml.bind.JAXBContext;
              String text = "sun.misc.BASE64Encoder";
            }
            "#;
        fs::write(root.join("src/main/java/demo/Demo.java"), contents).unwrap();
        let candidates = syntax_candidates(contents, "src/main/java/demo/Demo.java").unwrap();

        assert!(candidates.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn resolved_symbols_detect_reflective_class_for_name_literal() {
        let kb = JavaCompatibilityKnowledgeBase::load_default().unwrap();
        let mut findings = Vec::new();
        let mut seen = HashSet::new();
        scan_resolved_symbol(
            "src/main/java/demo/Demo.java",
            4,
            &["Class.forName(\"sun.misc.Unsafe".to_string()],
            25,
            &[("reflective_access", &kb.reflective_access)],
            &mut seen,
            &mut findings,
        );

        assert!(
            findings
                .iter()
                .any(|finding| finding.matched_text == "Class.forName(\"sun.")
        );
    }

    #[test]
    fn malformed_java_returns_parse_error_without_line_scan_fallback() {
        let error = syntax_candidates(
            r#"
            package demo;
            import javax.xml.bind.JAXBContext
            class Demo {
            "#,
            "src/main/java/demo/Demo.java",
        )
        .expect_err("parse error");

        assert!(error.contains("syntax tree contains parse errors"));
    }
}
