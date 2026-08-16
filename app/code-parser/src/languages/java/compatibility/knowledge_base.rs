use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug)]
pub struct JavaCompatibilityKnowledgeBase {
    pub dependencies: Vec<CompatibilityRule>,
    pub plugins: Vec<CompatibilityRule>,
    pub removed_apis: Vec<ApiRule>,
    pub deprecated_for_removal: Vec<ApiRule>,
    pub internal_apis: Vec<ApiRule>,
    pub reflective_access: Vec<ApiRule>,
    pub replacements: Vec<ReplacementRule>,
    pub migration_steps: Vec<MigrationStep>,
}

impl JavaCompatibilityKnowledgeBase {
    pub fn load_default() -> Result<Self, String> {
        Self::load_from_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/java"))
    }

    pub fn load_from_dir(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let dependency: DependencyCompatibilityFile =
            load_yaml(path.join("dependency_compatibility.yaml"))?;
        let plugin: PluginCompatibilityFile = load_yaml(path.join("plugin_compatibility.yaml"))?;
        let removed: RemovedApisFile = load_yaml(path.join("removed_apis.yaml"))?;
        let deprecated: DeprecatedForRemovalFile =
            load_yaml(path.join("deprecated_for_removal.yaml"))?;
        let internal: InternalApisFile = load_yaml(path.join("internal_apis.yaml"))?;
        let replacements: ReplacementsFile = load_yaml(path.join("replacements.yaml"))?;
        let migration: IncrementalMigrationFile =
            load_yaml(path.join("incremental_migration.yaml"))?;

        Ok(Self {
            dependencies: dependency.dependencies,
            plugins: plugin.plugins,
            removed_apis: removed.removed_apis,
            deprecated_for_removal: deprecated.deprecated_for_removal,
            internal_apis: internal.internal_apis,
            reflective_access: internal.reflective_access,
            replacements: replacements.replacements,
            migration_steps: migration.migration_strategy.recommended_steps,
        })
    }
}

fn load_yaml<T: for<'de> Deserialize<'de>>(path: PathBuf) -> Result<T, String> {
    let data = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_yaml::from_str(&data)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

#[derive(Debug, Deserialize)]
struct DependencyCompatibilityFile {
    #[serde(default)]
    dependencies: Vec<CompatibilityRule>,
}

#[derive(Debug, Deserialize)]
struct PluginCompatibilityFile {
    #[serde(default)]
    plugins: Vec<CompatibilityRule>,
}

#[derive(Debug, Deserialize)]
struct RemovedApisFile {
    #[serde(default)]
    removed_apis: Vec<ApiRule>,
}

#[derive(Debug, Deserialize)]
struct DeprecatedForRemovalFile {
    #[serde(default)]
    deprecated_for_removal: Vec<ApiRule>,
}

#[derive(Debug, Deserialize)]
struct InternalApisFile {
    #[serde(default)]
    internal_apis: Vec<ApiRule>,
    #[serde(default)]
    reflective_access: Vec<ApiRule>,
}

#[derive(Debug, Deserialize)]
struct ReplacementsFile {
    #[serde(default)]
    replacements: Vec<ReplacementRule>,
}

#[derive(Debug, Deserialize)]
struct IncrementalMigrationFile {
    #[serde(default)]
    migration_strategy: MigrationStrategy,
}

#[derive(Debug, Default, Deserialize)]
struct MigrationStrategy {
    #[serde(default)]
    recommended_steps: Vec<MigrationStep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompatibilityRule {
    pub id: String,
    #[serde(default)]
    #[serde(rename = "match")]
    pub match_rule: MatchRule,
    #[serde(default)]
    pub risk: Option<String>,
    #[serde(default = "default_warning")]
    pub severity: String,
    #[serde(default)]
    pub compatibility: Compatibility,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub edit_strategy: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MatchRule {
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub group_id_pattern: Option<String>,
    #[serde(default)]
    pub artifact_id: Option<String>,
    #[serde(default)]
    pub artifact_id_pattern: Option<String>,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub plugin_id_pattern: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Compatibility {
    #[serde(default)]
    pub java: HashMap<String, JavaCompatibility>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct JavaCompatibility {
    #[serde(default)]
    pub min_version: Option<String>,
    #[serde(default)]
    pub recommended_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiRule {
    pub id: String,
    #[serde(default = "default_warning")]
    pub severity: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default)]
    pub symbol_prefixes: Vec<String>,
    #[serde(default)]
    pub except_symbol_prefixes: Vec<String>,
    #[serde(default)]
    pub symbol_patterns: Vec<String>,
    #[serde(default)]
    pub patterns: Vec<String>,
    #[serde(default)]
    pub guidance: Option<String>,
    #[serde(default)]
    pub applies_when_target_java_at_least: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReplacementRule {
    pub id: String,
    #[serde(default)]
    pub applies_when_target_java_at_least: Option<u32>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub from_symbols: Vec<String>,
    #[serde(default)]
    pub to_symbols: Vec<String>,
    #[serde(default)]
    pub migration_kind: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MigrationStep {
    pub id: String,
    pub action: String,
    #[serde(default)]
    pub guidance: Option<String>,
}

fn default_warning() -> String {
    "warning".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_default_java_knowledge_base() {
        let kb = JavaCompatibilityKnowledgeBase::load_default().expect("KB loads");

        assert!(!kb.dependencies.is_empty());
        assert!(!kb.plugins.is_empty());
        assert!(!kb.removed_apis.is_empty());
        assert!(!kb.deprecated_for_removal.is_empty());
        assert!(!kb.internal_apis.is_empty());
        assert!(!kb.reflective_access.is_empty());
        assert!(!kb.replacements.is_empty());
        assert!(!kb.migration_steps.is_empty());
    }
}
