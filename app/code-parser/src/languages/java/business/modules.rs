use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use regex::Regex;
use walkdir::{DirEntry, WalkDir};

use crate::languages::java::business::model::ModuleInfo;

pub fn discover_modules(project_root: &Path) -> Vec<ModuleInfo> {
    let mut modules = BTreeMap::new();
    let root_name = project_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("root")
        .to_string();
    modules.insert(
        ".".to_string(),
        ModuleDraft {
            name: root_name,
            path: ".".to_string(),
            build_system: build_system(project_root),
            build_file: root_build_file(project_root),
        },
    );

    for entry in WalkDir::new(project_root)
        .into_iter()
        .filter_entry(|entry| !is_ignored_dir(entry))
    {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let file_name = entry.file_name().to_string_lossy();
        let relative_file = relative_path(project_root, entry.path());
        let parent = entry.path().parent().unwrap_or(project_root);

        if file_name == "pom.xml" {
            insert_build_module(project_root, parent, "maven", &relative_file, &mut modules);
            if let Ok(contents) = fs::read_to_string(entry.path()) {
                for module in parse_maven_modules(&contents) {
                    let module_path = parent.join(module);
                    if module_path.exists() {
                        insert_build_module(
                            project_root,
                            &module_path,
                            "maven",
                            &relative_path(project_root, &module_path.join("pom.xml")),
                            &mut modules,
                        );
                    }
                }
            }
        } else if matches!(
            file_name.as_ref(),
            "settings.gradle" | "settings.gradle.kts" | "build.gradle" | "build.gradle.kts"
        ) {
            insert_build_module(project_root, parent, "gradle", &relative_file, &mut modules);
            if file_name.starts_with("settings.")
                && let Ok(contents) = fs::read_to_string(entry.path())
            {
                for module in parse_gradle_modules(&contents) {
                    let module_path = parent.join(module);
                    if module_path.exists() {
                        insert_build_module(
                            project_root,
                            &module_path,
                            "gradle",
                            &relative_path(project_root, &module_path.join("build.gradle")),
                            &mut modules,
                        );
                    }
                }
            }
        }
    }

    let mut result: Vec<_> = modules
        .into_values()
        .map(|module| ModuleInfo {
            id: module_id(&module.path),
            parent_id: None,
            name: module.name,
            path: module.path,
            build_system: module.build_system,
            build_file: module.build_file,
        })
        .collect();
    result.sort_by(|left, right| left.path.cmp(&right.path));
    let ids_by_path: Vec<_> = result
        .iter()
        .map(|module| (module.path.clone(), module.id.clone()))
        .collect();
    for module in &mut result {
        module.parent_id = parent_module_id(&module.path, &ids_by_path);
    }
    result
}

pub fn module_id_for_file(file: &str, modules: &[ModuleInfo]) -> String {
    modules
        .iter()
        .filter(|module| {
            module.path == "."
                || file == module.path
                || file.starts_with(&format!("{}/", module.path))
        })
        .max_by_key(|module| module.path.len())
        .map(|module| module.id.clone())
        .unwrap_or_else(|| "module:.".to_string())
}

fn insert_build_module(
    project_root: &Path,
    module_path: &Path,
    build_system: &str,
    build_file: &str,
    modules: &mut BTreeMap<String, ModuleDraft>,
) {
    let path = normalized_relative_path(project_root, module_path);
    let name = if path == "." {
        project_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("root")
            .to_string()
    } else {
        module_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&path)
            .to_string()
    };
    modules
        .entry(path.clone())
        .and_modify(|module| {
            module.build_system = Some(build_system.to_string());
            module.build_file = Some(build_file.to_string());
        })
        .or_insert(ModuleDraft {
            name,
            path,
            build_system: Some(build_system.to_string()),
            build_file: Some(build_file.to_string()),
        });
}

fn parse_maven_modules(contents: &str) -> Vec<String> {
    let Some(block) = capture_tag(contents, "modules") else {
        return Vec::new();
    };
    let regex = Regex::new(r"(?s)<module>\s*([^<]+?)\s*</module>").expect("valid module regex");
    regex
        .captures_iter(&block)
        .filter_map(|captures| {
            captures
                .get(1)
                .map(|value| value.as_str().trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn parse_gradle_modules(contents: &str) -> Vec<String> {
    let regex = Regex::new(r#"include\s*\(?\s*([^\n)]+)\)?"#).expect("valid Gradle include regex");
    let mut modules = Vec::new();
    for captures in regex.captures_iter(contents) {
        let Some(raw) = captures.get(1) else {
            continue;
        };
        for quoted in quoted_values(raw.as_str()) {
            let path = quoted.trim_start_matches(':').replace(':', "/");
            if !path.is_empty() {
                modules.push(path);
            }
        }
    }
    modules.sort();
    modules.dedup();
    modules
}

fn quoted_values(value: &str) -> Vec<String> {
    let regex = Regex::new(r#"['"]([^'"]+)['"]"#).expect("valid quoted value regex");
    regex
        .captures_iter(value)
        .filter_map(|captures| captures.get(1).map(|value| value.as_str().to_string()))
        .collect()
}

fn capture_tag(contents: &str, tag: &str) -> Option<String> {
    let regex = Regex::new(&format!(r"(?s)<{tag}>\s*(.*?)\s*</{tag}>")).ok()?;
    regex
        .captures(contents)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().trim().to_string())
}

fn parent_module_id(path: &str, modules: &[(String, String)]) -> Option<String> {
    if path == "." {
        return None;
    }
    modules
        .iter()
        .filter(|(candidate_path, _)| candidate_path != path)
        .filter(|(candidate_path, _)| {
            candidate_path == "." || path.starts_with(&format!("{candidate_path}/"))
        })
        .max_by_key(|(candidate_path, _)| candidate_path.len())
        .map(|(_, id)| id.clone())
}

fn build_system(path: &Path) -> Option<String> {
    if path.join("pom.xml").exists() {
        Some("maven".to_string())
    } else if path.join("settings.gradle").exists()
        || path.join("settings.gradle.kts").exists()
        || path.join("build.gradle").exists()
        || path.join("build.gradle.kts").exists()
    {
        Some("gradle".to_string())
    } else {
        None
    }
}

fn root_build_file(path: &Path) -> Option<String> {
    for file in [
        "pom.xml",
        "settings.gradle",
        "settings.gradle.kts",
        "build.gradle",
        "build.gradle.kts",
    ] {
        if path.join(file).exists() {
            return Some(file.to_string());
        }
    }
    None
}

fn module_id(path: &str) -> String {
    if path == "." {
        "module:.".to_string()
    } else {
        format!("module:{path}")
    }
}

fn normalized_relative_path(root: &Path, path: &Path) -> String {
    let relative = relative_path(root, path);
    if relative.is_empty() {
        ".".to_string()
    } else {
        relative
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

fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry.file_type().is_dir()
        && matches!(
            entry.file_name().to_string_lossy().as_ref(),
            ".git" | "target" | "build" | ".gradle" | ".idea"
        )
}

struct ModuleDraft {
    name: String,
    path: String,
    build_system: Option<String>,
    build_file: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn discovers_maven_modules_and_assigns_parent() {
        let root = test_dir("business-maven-modules");
        fs::write(
            root.join("pom.xml"),
            r#"<project><modules><module>api</module><module>service</module></modules></project>"#,
        )
        .unwrap();
        fs::create_dir_all(root.join("api")).unwrap();
        fs::create_dir_all(root.join("service")).unwrap();
        fs::write(root.join("api/pom.xml"), "<project/>").unwrap();
        fs::write(root.join("service/pom.xml"), "<project/>").unwrap();

        let modules = discover_modules(&root);

        assert!(modules.iter().any(|module| module.id == "module:."));
        assert!(modules.iter().any(|module| module.id == "module:api"));
        assert!(modules.iter().any(|module| module.id == "module:service"
            && module.parent_id.as_deref() == Some("module:.")));
        assert_eq!(
            module_id_for_file("service/src/main/java/demo/Order.java", &modules),
            "module:service"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_gradle_include_modules() {
        let root = test_dir("business-gradle-modules");
        fs::write(
            root.join("settings.gradle"),
            "include ':api', ':service:impl'",
        )
        .unwrap();
        fs::create_dir_all(root.join("api")).unwrap();
        fs::create_dir_all(root.join("service/impl")).unwrap();
        fs::write(root.join("api/build.gradle"), "").unwrap();
        fs::write(root.join("service/impl/build.gradle"), "").unwrap();

        let modules = discover_modules(&root);

        assert!(modules.iter().any(|module| module.id == "module:api"));
        assert!(
            modules
                .iter()
                .any(|module| module.id == "module:service/impl")
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
