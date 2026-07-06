use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::step::Step;
use crate::steps::{file::FileStep, script::ScriptStep};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    #[serde(default, rename = "step")]
    pub steps: Vec<StepDef>,
}

/// One `[[step]]` table. Tagged by `type`; unknown keys are hard errors so
/// typos surface at parse time, not as silently-ignored config.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StepDef {
    File(FileDef),
    Script(ScriptDef),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct FileDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Source path, relative to the manifest file's directory.
    pub source: String,
    /// Target path; `~` is expanded.
    pub target: String,
    #[serde(default)]
    pub on_conflict: OnConflict,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnConflict {
    #[default]
    PromptDiff,
    Replace,
    Keep,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ScriptDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Executable path, relative to the manifest file's directory.
    pub path: String,
    #[serde(default)]
    pub on_error: OnError,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnError {
    #[default]
    Fail,
    Warn,
}

pub fn load(path: &Path) -> Result<Manifest> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading manifest {}", path.display()))?;
    let manifest: Manifest =
        toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    Ok(manifest)
}

/// Turn parsed defs into executable steps. `base` is the manifest's directory;
/// all relative source/script paths resolve against it.
pub fn build_steps(manifest: Manifest, base: &Path) -> Result<Vec<Box<dyn Step>>> {
    let mut steps: Vec<Box<dyn Step>> = Vec::new();
    for def in manifest.steps {
        match def {
            StepDef::File(d) => {
                let target = PathBuf::from(shellexpand::tilde(&d.target).into_owned());
                let id = d.id.clone().unwrap_or_else(|| format!("file:{}", d.target));
                steps.push(Box::new(FileStep {
                    id,
                    needs: d.needs,
                    source: base.join(&d.source),
                    target,
                    on_conflict: d.on_conflict,
                }));
            }
            StepDef::Script(d) => {
                let id = d.id.clone().unwrap_or_else(|| format!("script:{}", d.path));
                steps.push(Box::new(ScriptStep {
                    id,
                    needs: d.needs,
                    path: base.join(&d.path),
                    on_error: d.on_error,
                }));
            }
        }
    }
    Ok(steps)
}
