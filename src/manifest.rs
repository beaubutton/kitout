use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use std::collections::BTreeMap;

use crate::step::Step;
use crate::steps::{
    block::BlockInFileStep,
    brewfile::BrewfileStep,
    cmd::CommandIfMissingStep,
    defaults::{DefaultsStep, DefaultsWrite},
    file::FileStep,
    mcp::McpServerStep,
    merge::{JsonMergeStep, MergeMode, TomlMergeStep},
    script::ScriptStep,
    secret::SecretStep,
    skills::SkillsStep,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// When true, `apply` collects the sudo password once (Keychain-backed
    /// SUDO_ASKPASS for every child process) before running steps.
    #[serde(default)]
    pub sudo: bool,
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
    Skills(SkillsDef),
    McpServer(McpDef),
    CommandIfMissing(CmdDef),
    Brewfile(BrewfileDef),
    Secret(SecretDef),
    JsonMerge(MergeDef),
    TomlMerge(MergeDef),
    BlockInFile(BlockDef),
    Defaults(DefaultsDef),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SecretDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Keychain service name.
    pub service: String,
    /// Prompt text shown when stashing interactively.
    pub prompt: String,
    /// Keychain account; defaults to $USER.
    pub account: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MergeDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Target file; `~` expanded.
    pub target: String,
    #[serde(default)]
    pub mode: MergeMode,
    /// Keys to merge (a TOML table).
    pub value: toml::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct BlockDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Target file; `~` expanded.
    pub target: String,
    /// Marker name embedded in the begin/end comment lines.
    pub marker: String,
    pub block: String,
    #[serde(default = "default_comment_prefix")]
    pub comment_prefix: String,
}

fn default_comment_prefix() -> String {
    "#".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DefaultsDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub kill: Vec<String>,
    pub write: Vec<DefaultsWriteDef>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DefaultsWriteDef {
    pub domain: String,
    pub key: String,
    pub value: toml::Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SkillsDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Pipe-format skills manifest, relative to this manifest's directory.
    pub manifest: String,
    /// State file for GC; `~` expanded. Set this to osx-baseline's
    /// `~/.config/osx-baseline/managed-skills` to interoperate with Setup.sh.
    #[serde(default = "default_skills_state")]
    pub state_file: String,
}

fn default_skills_state() -> String {
    "~/.config/kitout/managed-skills".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct McpDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    pub name: String,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CmdDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Binary name probed on PATH.
    pub probe: String,
    /// Installer argv, spawned directly (no shell).
    pub install: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct BrewfileDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Brewfile path, relative to this manifest's directory.
    pub path: String,
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
    /// Optional convergence probe (argv; exit 0 = satisfied, script skipped).
    pub check: Option<Vec<String>>,
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
                    check: d.check,
                }));
            }
            StepDef::Skills(d) => {
                let id = d.id.clone().unwrap_or_else(|| "skills".into());
                let state_file = PathBuf::from(shellexpand::tilde(&d.state_file).into_owned());
                steps.push(Box::new(SkillsStep {
                    id,
                    needs: d.needs,
                    manifest: base.join(&d.manifest),
                    state_file,
                }));
            }
            StepDef::McpServer(d) => {
                let id = d.id.clone().unwrap_or_else(|| format!("mcp:{}", d.name));
                steps.push(Box::new(McpServerStep {
                    id,
                    needs: d.needs,
                    name: d.name,
                    command: d.command,
                    env: d.env,
                    url: d.url,
                    headers: d.headers,
                }));
            }
            StepDef::CommandIfMissing(d) => {
                let id = d.id.clone().unwrap_or_else(|| format!("cmd:{}", d.probe));
                steps.push(Box::new(CommandIfMissingStep {
                    id,
                    needs: d.needs,
                    probe: d.probe,
                    install: d.install,
                }));
            }
            StepDef::Brewfile(d) => {
                let id = d.id.clone().unwrap_or_else(|| "brewfile".into());
                steps.push(Box::new(BrewfileStep {
                    id,
                    needs: d.needs,
                    path: base.join(&d.path),
                }));
            }
            StepDef::Secret(d) => {
                let id =
                    d.id.clone()
                        .unwrap_or_else(|| format!("secret:{}", d.service));
                let account = d
                    .account
                    .or_else(|| std::env::var("USER").ok())
                    .context("secret step: no account and $USER unset")?;
                steps.push(Box::new(SecretStep {
                    id,
                    needs: d.needs,
                    service: d.service,
                    account,
                    prompt: d.prompt,
                }));
            }
            StepDef::JsonMerge(d) => {
                let id =
                    d.id.clone()
                        .unwrap_or_else(|| format!("json-merge:{}", d.target));
                steps.push(Box::new(JsonMergeStep {
                    id,
                    needs: d.needs,
                    target: PathBuf::from(shellexpand::tilde(&d.target).into_owned()),
                    mode: d.mode,
                    value: d.value,
                }));
            }
            StepDef::TomlMerge(d) => {
                let id =
                    d.id.clone()
                        .unwrap_or_else(|| format!("toml-merge:{}", d.target));
                steps.push(Box::new(TomlMergeStep {
                    id,
                    needs: d.needs,
                    target: PathBuf::from(shellexpand::tilde(&d.target).into_owned()),
                    mode: d.mode,
                    value: d.value,
                }));
            }
            StepDef::BlockInFile(d) => {
                let id =
                    d.id.clone()
                        .unwrap_or_else(|| format!("block:{}", d.marker));
                steps.push(Box::new(BlockInFileStep {
                    id,
                    needs: d.needs,
                    target: PathBuf::from(shellexpand::tilde(&d.target).into_owned()),
                    marker: d.marker,
                    block: d.block,
                    comment_prefix: d.comment_prefix,
                }));
            }
            StepDef::Defaults(d) => {
                let id = d.id.clone().unwrap_or_else(|| "defaults".into());
                steps.push(Box::new(DefaultsStep {
                    id,
                    needs: d.needs,
                    writes: d
                        .write
                        .into_iter()
                        .map(|w| DefaultsWrite {
                            domain: w.domain,
                            key: w.key,
                            value: w.value,
                        })
                        .collect(),
                    kill: d.kill,
                }));
            }
        }
    }
    Ok(steps)
}
