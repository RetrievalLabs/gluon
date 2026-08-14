use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct BuildReport {
    pub project_root: String,
    pub build_tools: Vec<BuildToolInfo>,
    pub java_versions: Vec<JavaVersionInfo>,
    pub declared_dependencies: Vec<DependencyInfo>,
    pub resolved_dependencies: Vec<DependencyInfo>,
    pub declared_plugins: Vec<PluginInfo>,
    pub resolved_plugins: Vec<PluginInfo>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildToolInfo {
    pub tool: String,
    pub version: Option<String>,
    pub file: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JavaVersionInfo {
    pub version: String,
    pub kind: String,
    pub file: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependencyInfo {
    pub group_id: Option<String>,
    pub artifact_id: String,
    pub version: Option<String>,
    pub configuration: Option<String>,
    pub scope: Option<String>,
    pub file: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub version: Option<String>,
    pub file: Option<String>,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
