use std::fs;
use std::path::Path;

use tree_sitter::{Node, Parser, TreeCursor};
use walkdir::{DirEntry, WalkDir};

use crate::languages::java::build::model::Diagnostic;
use crate::languages::java::business::model::{
    ClassInfo, CodeModel, EntryPointInfo, InvocationInfo, MethodInfo, ParameterInfo,
    RelationshipInfo,
};
use crate::languages::java::business::modules::{discover_modules, module_id_for_file};

#[derive(Debug, Clone)]
struct ClassScope {
    id: String,
    qualified_name: String,
}

pub fn extract_structure(project_root: &Path) -> Result<CodeModel, String> {
    if !project_root.exists() {
        return Err(format!("path does not exist: {}", project_root.display()));
    }

    let mut model = CodeModel {
        project_root: project_root.display().to_string(),
        ..CodeModel::default()
    };
    model.modules = discover_modules(project_root);

    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                model.diagnostics.push(Diagnostic::warning(
                    "business_tree_sitter",
                    error.to_string(),
                    None,
                ));
                continue;
            }
        };
        if !entry.file_type().is_file()
            || entry.path().extension().and_then(|ext| ext.to_str()) != Some("java")
        {
            continue;
        }

        match fs::read_to_string(entry.path()) {
            Ok(contents) => parse_file(project_root, entry.path(), &contents, &mut model),
            Err(error) => model.diagnostics.push(Diagnostic::warning(
                "business_tree_sitter",
                format!("failed to read {}: {error}", entry.path().display()),
                Some(entry.path().display().to_string()),
            )),
        }
    }

    model.classes.sort_by(|left, right| left.id.cmp(&right.id));
    model.methods.sort_by(|left, right| left.id.cmp(&right.id));
    model
        .relationships
        .sort_by(|left, right| relationship_key(left).cmp(&relationship_key(right)));
    model
        .entry_points
        .sort_by(|left, right| left.id.cmp(&right.id));
    Ok(model)
}

fn parse_file(project_root: &Path, file: &Path, contents: &str, model: &mut CodeModel) {
    let display_path = relative_path(project_root, file);
    let module_id = module_id_for_file(&display_path, &model.modules);
    let mut parser = Parser::new();
    if let Err(error) = parser.set_language(&tree_sitter_java::LANGUAGE.into()) {
        model.diagnostics.push(Diagnostic::error(
            "business_tree_sitter",
            format!("failed to initialize Java parser: {error}"),
            Some(display_path),
        ));
        return;
    }
    let Some(tree) = parser.parse(contents, None) else {
        model.diagnostics.push(Diagnostic::warning(
            "business_tree_sitter",
            format!("parser returned no syntax tree for {}", file.display()),
            Some(display_path),
        ));
        return;
    };
    if tree.root_node().has_error() {
        model.diagnostics.push(Diagnostic::warning(
            "business_tree_sitter",
            format!("failed to parse Java source {}", file.display()),
            Some(display_path),
        ));
        return;
    }

    let package_name = package_name(tree.root_node(), contents);
    let mut class_stack = Vec::new();
    let mut cursor = tree.walk();
    collect_nodes(
        contents,
        &display_path,
        &module_id,
        package_name.as_deref(),
        &mut cursor,
        &mut class_stack,
        model,
    );
}

fn collect_nodes(
    contents: &str,
    file: &str,
    module_id: &str,
    package_name: Option<&str>,
    cursor: &mut TreeCursor<'_>,
    class_stack: &mut Vec<ClassScope>,
    model: &mut CodeModel,
) {
    let node = cursor.node();
    if is_class_node(node.kind()) {
        let class = class_info(
            node,
            contents,
            file,
            module_id,
            package_name,
            class_stack
                .last()
                .map(|scope| scope.qualified_name.as_str()),
        );
        for interface in &class.interfaces {
            model.relationships.push(RelationshipInfo {
                source_id: class.id.clone(),
                target_id: format!("type:{interface}"),
                kind: "IMPLEMENTS".to_string(),
                confidence: 0.60,
                source: "tree_sitter".to_string(),
            });
        }
        if let Some(superclass) = &class.superclass {
            model.relationships.push(RelationshipInfo {
                source_id: class.id.clone(),
                target_id: format!("type:{superclass}"),
                kind: "EXTENDS".to_string(),
                confidence: 0.60,
                source: "tree_sitter".to_string(),
            });
        }
        class_stack.push(ClassScope {
            id: class.id.clone(),
            qualified_name: class.qualified_name.clone(),
        });
        model.classes.push(class);
    } else if is_method_node(node.kind()) {
        if let Some(class_scope) = class_stack.last() {
            let method = method_info(
                node,
                contents,
                file,
                &class_scope.id,
                &class_scope.qualified_name,
            );
            let method = MethodInfo {
                module_id: module_id.to_string(),
                ..method
            };
            collect_entry_points(&method, model);
            collect_invocations(node, contents, &method, model);
            model.methods.push(method);
        }
    }

    if cursor.goto_first_child() {
        loop {
            collect_nodes(
                contents,
                file,
                module_id,
                package_name,
                cursor,
                class_stack,
                model,
            );
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }

    if is_class_node(node.kind()) {
        class_stack.pop();
    }
}

fn class_info(
    node: Node<'_>,
    contents: &str,
    file: &str,
    module_id: &str,
    package_name: Option<&str>,
    parent_qualified_name: Option<&str>,
) -> ClassInfo {
    let name = node_text(child_by_field(node, "name"), contents).unwrap_or("Anonymous");
    let qualified_name = match (package_name, parent_qualified_name) {
        (_, Some(parent)) => format!("{parent}.{name}"),
        (Some(package_name), None) if !package_name.is_empty() => format!("{package_name}.{name}"),
        _ => name.to_string(),
    };
    let id = format!(
        "class:{qualified_name}@{}:{}",
        file,
        node.start_position().row + 1
    );
    let superclass = child_by_field(node, "superclass")
        .and_then(|child| first_type_text(child, contents))
        .map(|value| value.to_string());
    let interfaces = child_by_field(node, "interfaces")
        .map(|child| type_texts(child, contents))
        .unwrap_or_default();

    ClassInfo {
        id,
        module_id: module_id.to_string(),
        name: name.to_string(),
        package_name: package_name.map(str::to_string),
        qualified_name,
        kind: node.kind().trim_end_matches("_declaration").to_string(),
        file: file.to_string(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        superclass,
        interfaces,
        annotations: annotations(node, contents),
    }
}

fn method_info(
    node: Node<'_>,
    contents: &str,
    file: &str,
    class_id: &str,
    class_qualified_name: &str,
) -> MethodInfo {
    let name_node = child_by_field(node, "name");
    let name = node_text(name_node, contents).unwrap_or("<anonymous>");
    let return_type = child_by_field(node, "type")
        .and_then(|child| node_text(Some(child), contents).map(str::to_string));
    let parameters = child_by_field(node, "parameters")
        .map(|params| parameters(params, contents))
        .unwrap_or_default();
    let signature = format!(
        "{}({})",
        name,
        parameters
            .iter()
            .map(|parameter| parameter.type_name.as_deref().unwrap_or("_"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let body_text = child_by_field(node, "body")
        .and_then(|body| node_text(Some(body), contents).map(str::to_string))
        .unwrap_or_default();
    let id = format!(
        "method:{class_qualified_name}#{signature}@{}:{}",
        file,
        node.start_position().row + 1
    );
    let name_position = name_node
        .map(|name| name.start_position())
        .unwrap_or_else(|| node.start_position());

    MethodInfo {
        id,
        module_id: String::new(),
        class_id: class_id.to_string(),
        name: name.to_string(),
        signature,
        return_type,
        parameters,
        annotations: annotations(node, contents),
        file: file.to_string(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
        name_line: name_position.row + 1,
        name_column: name_position.column,
        body_text,
    }
}

fn parameters(node: Node<'_>, contents: &str) -> Vec<ParameterInfo> {
    let mut params = Vec::new();
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return params;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
            let name = node_text(child_by_field(child, "name"), contents)
                .unwrap_or("_")
                .to_string();
            let type_name = child_by_field(child, "type")
                .and_then(|type_node| node_text(Some(type_node), contents).map(str::to_string));
            params.push(ParameterInfo { name, type_name });
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    params
}

fn collect_invocations(node: Node<'_>, contents: &str, method: &MethodInfo, model: &mut CodeModel) {
    let mut cursor = node.walk();
    collect_invocation_nodes(contents, method, &mut cursor, model);
}

fn collect_invocation_nodes(
    contents: &str,
    method: &MethodInfo,
    cursor: &mut TreeCursor<'_>,
    model: &mut CodeModel,
) {
    let node = cursor.node();
    if node.kind() == "method_invocation" {
        let name_node = child_by_field(node, "name").unwrap_or(node);
        let name = node_text(Some(name_node), contents)
            .unwrap_or("")
            .to_string();
        if !name.is_empty() {
            let position = name_node.start_position();
            model.invocations.push(InvocationInfo {
                caller_method_id: method.id.clone(),
                file: method.file.clone(),
                name: name.clone(),
                line: position.row + 1,
                column: position.column,
            });
            model.relationships.push(RelationshipInfo {
                source_id: method.id.clone(),
                target_id: format!("unresolved-call:{name}"),
                kind: "CALLS".to_string(),
                confidence: 0.25,
                source: "tree_sitter".to_string(),
            });
        }
    }
    if cursor.goto_first_child() {
        loop {
            collect_invocation_nodes(contents, method, cursor, model);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn collect_entry_points(method: &MethodInfo, model: &mut CodeModel) {
    if method.name == "main"
        && method.return_type.as_deref() == Some("void")
        && method.parameters.len() == 1
    {
        model.entry_points.push(EntryPointInfo {
            id: format!("entry:{}:main", method.id),
            method_id: method.id.clone(),
            kind: "Main".to_string(),
            framework: None,
            route: None,
            http_method: None,
            source: "tree_sitter".to_string(),
        });
    }

    for annotation in &method.annotations {
        let normalized = annotation_name(annotation);
        let http_method = match normalized.as_str() {
            "GetMapping" => Some("GET"),
            "PostMapping" => Some("POST"),
            "PutMapping" => Some("PUT"),
            "PatchMapping" => Some("PATCH"),
            "DeleteMapping" => Some("DELETE"),
            "RequestMapping" => Some("REQUEST"),
            _ => None,
        };
        if let Some(http_method) = http_method {
            model.entry_points.push(EntryPointInfo {
                id: format!("entry:{}:{normalized}", method.id),
                method_id: method.id.clone(),
                kind: "Http".to_string(),
                framework: Some("Spring MVC".to_string()),
                route: annotation_route(annotation),
                http_method: Some(http_method.to_string()),
                source: "heuristic".to_string(),
            });
        } else if matches!(
            normalized.as_str(),
            "Scheduled" | "KafkaListener" | "JmsListener"
        ) {
            model.entry_points.push(EntryPointInfo {
                id: format!("entry:{}:{normalized}", method.id),
                method_id: method.id.clone(),
                kind: if normalized == "Scheduled" {
                    "Scheduled".to_string()
                } else {
                    "Message".to_string()
                },
                framework: framework_for_annotation(&normalized),
                route: annotation_route(annotation),
                http_method: None,
                source: "heuristic".to_string(),
            });
        }
    }
}

fn package_name(root: Node<'_>, contents: &str) -> Option<String> {
    let mut cursor = root.walk();
    if !cursor.goto_first_child() {
        return None;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "package_declaration" {
            return child
                .child_by_field_name("name")
                .and_then(|name| node_text(Some(name), contents).map(str::to_string));
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    None
}

fn annotations(node: Node<'_>, contents: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut cursor = node.walk();
    collect_annotations(contents, &mut cursor, &mut values);
    values.sort();
    values.dedup();
    values
}

fn collect_annotations(contents: &str, cursor: &mut TreeCursor<'_>, values: &mut Vec<String>) {
    let node = cursor.node();
    if matches!(node.kind(), "annotation" | "marker_annotation") {
        if let Some(value) = node_text(Some(node), contents) {
            values.push(value.trim().to_string());
        }
        return;
    }
    if cursor.goto_first_child() {
        loop {
            collect_annotations(contents, cursor, values);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn annotation_name(annotation: &str) -> String {
    let before_args = annotation
        .trim()
        .trim_start_matches('@')
        .split_once('(')
        .map(|(name, _)| name)
        .unwrap_or_else(|| annotation.trim().trim_start_matches('@'));
    before_args.rsplit('.').next().unwrap_or("").to_string()
}

fn annotation_route(annotation: &str) -> Option<String> {
    let start = annotation.find('"')?;
    let rest = &annotation[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn framework_for_annotation(annotation: &str) -> Option<String> {
    match annotation {
        "KafkaListener" => Some("Kafka".to_string()),
        "JmsListener" => Some("JMS".to_string()),
        "Scheduled" => Some("Spring Scheduler".to_string()),
        _ => None,
    }
}

fn is_class_node(kind: &str) -> bool {
    matches!(
        kind,
        "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration"
    )
}

fn is_method_node(kind: &str) -> bool {
    matches!(kind, "method_declaration" | "constructor_declaration")
}

fn child_by_field<'a>(node: Node<'a>, field: &str) -> Option<Node<'a>> {
    node.child_by_field_name(field)
}

fn node_text<'a>(node: Option<Node<'a>>, contents: &'a str) -> Option<&'a str> {
    node.and_then(|node| node.utf8_text(contents.as_bytes()).ok())
}

fn first_type_text(node: Node<'_>, contents: &str) -> Option<String> {
    let mut values = type_texts(node, contents);
    if values.is_empty() {
        None
    } else {
        Some(values.remove(0))
    }
}

fn type_texts(node: Node<'_>, contents: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut cursor = node.walk();
    collect_type_texts(contents, &mut cursor, &mut values);
    values.sort();
    values.dedup();
    values
}

fn collect_type_texts(contents: &str, cursor: &mut TreeCursor<'_>, values: &mut Vec<String>) {
    let node = cursor.node();
    if matches!(
        node.kind(),
        "type_identifier" | "scoped_type_identifier" | "generic_type"
    ) {
        if let Some(value) = node_text(Some(node), contents) {
            values.push(value.trim().to_string());
        }
    }
    if cursor.goto_first_child() {
        loop {
            collect_type_texts(contents, cursor, values);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

fn relative_path(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn relationship_key(relationship: &RelationshipInfo) -> String {
    format!(
        "{}\0{}\0{}\0{}",
        relationship.source_id, relationship.target_id, relationship.kind, relationship.source
    )
}

fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && matches!(
            entry.file_name().to_string_lossy().as_ref(),
            ".git" | "target" | "build" | ".gradle" | ".idea"
        )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn extracts_classes_methods_entry_points_and_calls() {
        let root = test_dir("business-structure");
        fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
        fs::write(
            root.join("src/main/java/demo/OrderController.java"),
            r#"
            package demo;

            class OrderController {
              @PostMapping("/orders/{id}/approve")
              public void approve(Long id) {
                service.approve(id);
              }
            }
            "#,
        )
        .unwrap();

        let model = extract_structure(&root).unwrap();

        assert_eq!(model.classes.len(), 1);
        assert_eq!(model.modules.len(), 1);
        assert_eq!(model.classes[0].module_id, "module:.");
        assert_eq!(model.methods.len(), 1);
        assert_eq!(model.methods[0].module_id, "module:.");
        assert_eq!(model.entry_points[0].kind, "Http");
        assert_eq!(model.entry_points[0].http_method.as_deref(), Some("POST"));
        assert!(
            model
                .relationships
                .iter()
                .any(|relationship| relationship.target_id == "unresolved-call:approve")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn assigns_classes_and_methods_to_declared_modules() {
        let root = test_dir("business-structure-modules");
        fs::write(
            root.join("pom.xml"),
            r#"<project><modules><module>api</module><module>service</module></modules></project>"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("api/src/main/java/demo")).unwrap();
        fs::create_dir_all(root.join("service/src/main/java/demo")).unwrap();
        fs::write(root.join("api/pom.xml"), "<project/>").unwrap();
        fs::write(root.join("service/pom.xml"), "<project/>").unwrap();
        fs::write(
            root.join("api/src/main/java/demo/Order.java"),
            "package demo; class Order { Long id() { return 1L; } }",
        )
        .unwrap();
        fs::write(
            root.join("service/src/main/java/demo/OrderService.java"),
            "package demo; class OrderService { void approve() {} }",
        )
        .unwrap();

        let model = extract_structure(&root).unwrap();

        assert!(model.modules.iter().any(|module| module.id == "module:api"));
        assert!(
            model
                .classes
                .iter()
                .any(|class| class.file.starts_with("api/") && class.module_id == "module:api")
        );
        assert!(model.methods.iter().any(|method| {
            method.file.starts_with("service/") && method.module_id == "module:service"
        }));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_java_emits_warning() {
        let root = test_dir("business-malformed");
        fs::create_dir_all(root.join("src/main/java/demo")).unwrap();
        fs::write(
            root.join("src/main/java/demo/Broken.java"),
            "package demo; class Broken {",
        )
        .unwrap();

        let model = extract_structure(&root).unwrap();

        assert!(model.classes.is_empty());
        assert_eq!(model.diagnostics.len(), 1);
        assert_eq!(model.diagnostics[0].severity, "warning");
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
