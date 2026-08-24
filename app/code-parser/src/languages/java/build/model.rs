use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct BuildReport {
    pub project_root: String,
    #[serde(default)]
    pub parent: BuildScopeReport,
    #[serde(default)]
    pub modules: Vec<BuildScopeReport>,
    #[serde(default, skip_serializing)]
    pub build_tools: Vec<BuildToolInfo>,
    #[serde(default, skip_serializing)]
    pub java_versions: Vec<JavaVersionInfo>,
    #[serde(default, skip_serializing)]
    pub declared_dependencies: Vec<DependencyInfo>,
    #[serde(default, skip_serializing)]
    pub resolved_dependencies: Vec<DependencyInfo>,
    #[serde(default, skip_serializing)]
    pub declared_plugins: Vec<PluginInfo>,
    #[serde(default, skip_serializing)]
    pub resolved_plugins: Vec<PluginInfo>,
    pub diagnostics: Vec<Diagnostic>,
}

impl BuildReport {
    pub fn push_java_version(&mut self, version: JavaVersionInfo) {
        if self
            .java_versions
            .iter()
            .any(|existing| existing.version == version.version)
        {
            return;
        }
        self.java_versions.push(version);
    }

    pub fn rebuild_scopes(&mut self) {
        let mut parent = BuildScopeReport::parent();
        let mut modules: Vec<BuildScopeReport> = Vec::new();

        for item in &self.build_tools {
            scope_for_file(item.file.as_deref(), &mut parent, &mut modules)
                .build_tools
                .push(item.clone());
        }
        for item in &self.java_versions {
            scope_for_file(Some(&item.file), &mut parent, &mut modules)
                .java_versions
                .push(item.clone());
        }
        for item in &self.declared_dependencies {
            scope_for_file(item.file.as_deref(), &mut parent, &mut modules)
                .declared_dependencies
                .push(item.clone());
        }
        for item in &self.resolved_dependencies {
            scope_for_file(item.file.as_deref(), &mut parent, &mut modules)
                .resolved_dependencies
                .push(item.clone());
        }
        for item in &self.declared_plugins {
            scope_for_file(item.file.as_deref(), &mut parent, &mut modules)
                .declared_plugins
                .push(item.clone());
        }
        for item in &self.resolved_plugins {
            scope_for_file(item.file.as_deref(), &mut parent, &mut modules)
                .resolved_plugins
                .push(item.clone());
        }

        modules.sort_by(|left, right| left.path.cmp(&right.path));
        self.parent = parent;
        self.modules = modules;
    }

    pub fn rebuild_flat_inventory(&mut self) {
        self.build_tools.clear();
        self.java_versions.clear();
        self.declared_dependencies.clear();
        self.resolved_dependencies.clear();
        self.declared_plugins.clear();
        self.resolved_plugins.clear();

        for scope in std::iter::once(&self.parent).chain(self.modules.iter()) {
            self.build_tools.extend(scope.build_tools.clone());
            self.java_versions.extend(scope.java_versions.clone());
            self.declared_dependencies
                .extend(scope.declared_dependencies.clone());
            self.resolved_dependencies
                .extend(scope.resolved_dependencies.clone());
            self.declared_plugins.extend(scope.declared_plugins.clone());
            self.resolved_plugins.extend(scope.resolved_plugins.clone());
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct BuildScopeReport {
    pub name: String,
    pub path: String,
    pub build_tools: Vec<BuildToolInfo>,
    pub java_versions: Vec<JavaVersionInfo>,
    pub declared_dependencies: Vec<DependencyInfo>,
    pub resolved_dependencies: Vec<DependencyInfo>,
    pub declared_plugins: Vec<PluginInfo>,
    pub resolved_plugins: Vec<PluginInfo>,
}

impl BuildScopeReport {
    fn parent() -> Self {
        Self {
            name: "parent".to_string(),
            path: ".".to_string(),
            ..Self::default()
        }
    }
}

fn scope_for_file<'a>(
    file: Option<&str>,
    parent: &'a mut BuildScopeReport,
    modules: &'a mut Vec<BuildScopeReport>,
) -> &'a mut BuildScopeReport {
    let Some(module_path) = module_path_for_file(file) else {
        return parent;
    };
    if let Some(index) = modules.iter().position(|module| module.path == module_path) {
        return &mut modules[index];
    }
    modules.push(BuildScopeReport {
        name: module_path
            .rsplit('/')
            .next()
            .unwrap_or(&module_path)
            .to_string(),
        path: module_path,
        ..BuildScopeReport::default()
    });
    modules.last_mut().expect("module was just pushed")
}

pub fn module_path_for_file(file: Option<&str>) -> Option<String> {
    let file = file?.replace('\\', "/");
    if !file.contains('/') {
        return None;
    }
    if file.starts_with("gradle/") {
        return None;
    }
    if let Some((module, _)) = file.split_once("/gradle/") {
        return Some(module.to_string());
    }
    let module = file.rsplit_once('/').map(|(module, _)| module)?;
    if module.is_empty() {
        None
    } else {
        Some(module.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BuildToolInfo {
    pub tool: String,
    pub version: Option<String>,
    pub file: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct JavaVersionInfo {
    pub version: String,
    pub kind: String,
    pub file: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DependencyInfo {
    pub group_id: Option<String>,
    pub artifact_id: String,
    pub version: Option<String>,
    pub configuration: Option<String>,
    pub scope: Option<String>,
    pub file: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub version: Option<String>,
    pub file: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Diagnostic {
    pub severity: String,
    pub category: String,
    pub message: String,
    pub file: Option<String>,
    pub command: Option<Vec<String>>,
    pub exit_code: Option<i32>,
    pub stderr: Option<String>,
}

impl Diagnostic {
    pub fn error(category: &str, message: impl Into<String>, file: Option<String>) -> Self {
        Self {
            severity: "error".to_string(),
            category: category.to_string(),
            message: message.into(),
            file,
            command: None,
            exit_code: None,
            stderr: None,
        }
    }

    pub fn warning(category: &str, message: impl Into<String>, file: Option<String>) -> Self {
        Self {
            severity: "warning".to_string(),
            category: category.to_string(),
            message: message.into(),
            file,
            command: None,
            exit_code: None,
            stderr: None,
        }
    }
}
