use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

use std::collections::BTreeMap;

use crate::step::Step;
use crate::steps::{
    block::BlockInFileStep,
    brewfile::{BrewStep, BrewfileStep},
    cmd::CommandIfMissingStep,
    defaults::{DefaultsStep, DefaultsWrite},
    file::FileStep,
    mcp::McpServerStep,
    merge::{JsonMergeStep, MergeMode, TomlMergeStep},
    script::ScriptStep,
    secret::SecretStep,
    skills::SkillsStep,
};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Base manifests to inherit. Their steps merge in first (left-to-right,
    /// recursively, each file once), then this file's steps. Accepts a bare
    /// string or a list. Paths resolve relative to this file and must stay
    /// within its directory tree; keep extended files as siblings — every
    /// step's source/script/Brewfile path resolves against the *root*
    /// manifest's directory.
    #[serde(default, deserialize_with = "de_extends")]
    pub extends: Vec<String>,
    /// When true, `apply` collects the sudo password once (Keychain-backed
    /// SUDO_ASKPASS for every child process) before running steps.
    #[serde(default)]
    pub sudo: bool,
    #[serde(default, rename = "step")]
    pub steps: Vec<StepDef>,
}

/// `extends` accepts a bare string or a list of strings.
fn de_extends<'de, D>(d: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => vec![s],
        OneOrMany::Many(v) => v,
    })
}

/// One `[[step]]` table. Tagged by `type`; unknown keys are hard errors so
/// typos surface at parse time, not as silently-ignored config.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum StepDef {
    File(FileDef),
    Script(ScriptDef),
    Skills(SkillsDef),
    McpServer(McpDef),
    CommandIfMissing(CmdDef),
    Brewfile(BrewfileDef),
    Brew(BrewDef),
    Secret(SecretDef),
    JsonMerge(MergeDef),
    TomlMerge(MergeDef),
    BlockInFile(BlockDef),
    Defaults(DefaultsDef),
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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
    #[schemars(with = "serde_json::Value")]
    pub value: toml::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DefaultsDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    #[serde(default)]
    pub kill: Vec<String>,
    pub write: Vec<DefaultsWriteDef>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DefaultsWriteDef {
    pub domain: String,
    pub key: String,
    #[schemars(with = "serde_json::Value")]
    pub value: toml::Value,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SkillsDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Pipe-format skills manifest, relative to this manifest's directory.
    pub manifest: String,
    /// State file recording which skills kitout installed (drives GC of
    /// removed manifest entries); `~` expanded.
    #[serde(default = "default_skills_state")]
    pub state_file: String,
}

fn default_skills_state() -> String {
    "~/.config/kitout/managed-skills".into()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct BrewfileDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Brewfile path, relative to this manifest's directory.
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct BrewDef {
    pub id: Option<String>,
    #[serde(default)]
    pub needs: Vec<String>,
    /// Homebrew taps to add (`user/repo`).
    #[serde(default)]
    pub taps: Vec<String>,
    /// Formulae to install (`brew "..."`).
    #[serde(default)]
    pub formulae: Vec<String>,
    /// Casks to install (`cask "..."`).
    #[serde(default)]
    pub casks: Vec<String>,
    /// VS Code extensions to install (`vscode "..."`).
    #[serde(default)]
    pub vscode: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OnConflict {
    #[default]
    PromptDiff,
    Replace,
    Keep,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OnError {
    #[default]
    Fail,
    Warn,
}

pub fn load(path: &Path) -> Result<Manifest> {
    let root_dir = path
        .parent()
        .and_then(|p| p.canonicalize().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let mut steps = Vec::new();
    let mut sudo = false;
    let mut done = std::collections::HashSet::new();
    let mut stack = Vec::new();
    merge_into(
        path, &root_dir, &mut steps, &mut sudo, &mut done, &mut stack,
    )?;
    Ok(Manifest {
        extends: Vec::new(),
        sudo,
        steps,
    })
}

/// Recursively merge `path` and everything it `extends` into `steps`/`sudo`.
/// Extends are resolved left-to-right, depth-first (so a base's steps precede
/// the file's own), deduped by canonical path (a file reached twice — e.g. a
/// diamond — is merged once), and cycles are a hard error. Extended files must
/// stay within `root_dir` (the root manifest's tree).
fn merge_into(
    path: &Path,
    root_dir: &Path,
    steps: &mut Vec<StepDef>,
    sudo: &mut bool,
    done: &mut std::collections::HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
) -> Result<()> {
    let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if done.contains(&canon) {
        return Ok(());
    }
    if stack.contains(&canon) {
        bail!("extends cycle detected at {}", path.display());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading manifest {}", path.display()))?;
    let m: Manifest =
        toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
    stack.push(canon.clone());
    *sudo |= m.sudo;
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    for ext in &m.extends {
        let ext_path = dir.join(ext).canonicalize().with_context(|| {
            format!(
                "extends target '{}' (from {}) not found",
                ext,
                path.display()
            )
        })?;
        if !ext_path.starts_with(root_dir) {
            bail!(
                "extends target '{}' escapes the manifest directory {}",
                ext,
                root_dir.display()
            );
        }
        merge_into(&ext_path, root_dir, steps, sudo, done, stack)?;
    }
    steps.extend(m.steps);
    stack.pop();
    done.insert(canon);
    Ok(())
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
            StepDef::Brew(d) => {
                // Default id from the first package so multiple inline `brew`
                // steps (e.g. across `extends` fragments) don't collide on a
                // bare "brew" and trip the duplicate-id guard.
                let id = d.id.clone().unwrap_or_else(|| {
                    match d
                        .formulae
                        .first()
                        .or_else(|| d.casks.first())
                        .or_else(|| d.taps.first())
                    {
                        Some(first) => format!("brew:{first}"),
                        None => "brew".into(),
                    }
                });
                let content = BrewStep::render(&d.taps, &d.formulae, &d.casks, &d.vscode);
                steps.push(Box::new(BrewStep {
                    id,
                    needs: d.needs,
                    content,
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
    // Ids must be unique across the manifest and everything it extends — a
    // collision is a mistake (append-only merge, no override).
    let mut seen = std::collections::HashSet::new();
    for s in &steps {
        if !seen.insert(s.id().to_string()) {
            bail!(
                "duplicate step id '{}' — ids must be unique across a manifest and everything it extends",
                s.id()
            );
        }
    }
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), body).unwrap();
    }
    fn ids(m: Manifest, base: &Path) -> Vec<String> {
        build_steps(m, base)
            .unwrap()
            .iter()
            .map(|s| s.id().to_string())
            .collect()
    }

    #[test]
    fn extends_merges_dedups_and_orders() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        w(
            p,
            "base.toml",
            "[[step]]\ntype = \"defaults\"\nid = \"base\"\nwrite = []\n",
        );
        w(
            p,
            "mid.toml",
            "extends = \"base.toml\"\n[[step]]\ntype = \"defaults\"\nid = \"mid\"\nwrite = []\n",
        );
        // root -> base and root -> mid -> base: base is a diamond, merged once, first.
        w(
            p,
            "root.toml",
            "extends = [\"base.toml\", \"mid.toml\"]\n[[step]]\ntype = \"defaults\"\nid = \"root\"\nwrite = []\n",
        );
        assert_eq!(
            ids(load(&p.join("root.toml")).unwrap(), p),
            vec!["base", "mid", "root"]
        );
    }

    #[test]
    fn extends_accepts_bare_string() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        w(
            p,
            "base.toml",
            "[[step]]\ntype = \"defaults\"\nid = \"b\"\nwrite = []\n",
        );
        w(p, "c.toml", "extends = \"base.toml\"\n");
        assert_eq!(ids(load(&p.join("c.toml")).unwrap(), p), vec!["b"]);
    }

    #[test]
    fn extends_cycle_is_error() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        w(p, "a.toml", "extends = \"b.toml\"\n");
        w(p, "b.toml", "extends = \"a.toml\"\n");
        assert!(load(&p.join("a.toml")).is_err());
    }

    #[test]
    fn duplicate_step_id_is_error() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        w(
            p,
            "base.toml",
            "[[step]]\ntype = \"defaults\"\nid = \"dup\"\nwrite = []\n",
        );
        w(
            p,
            "child.toml",
            "extends = \"base.toml\"\n[[step]]\ntype = \"defaults\"\nid = \"dup\"\nwrite = []\n",
        );
        assert!(build_steps(load(&p.join("child.toml")).unwrap(), p).is_err());
    }
}
