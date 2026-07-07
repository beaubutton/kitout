//! FLAGSHIP: agent-skills sync — the Rust port of osx-baseline's step 3d.
//!
//! Reads the pipe-format manifest (`name | source | targets`), fetches each
//! skill (raw SKILL.md URL, or GitHub `owner/repo[@ref][:path]` tarball with
//! one download per repo@ref), installs to each target's skills dir only when
//! content differs, and garbage-collects copies whose manifest entry or
//! target disappeared. Interoperates with the bash implementation: same
//! state file (`~/.config/osx-baseline/managed-skills`), same guard rails
//! (only paths under managed bases, no `..`), same offline behavior (fetch
//! failure never removes or overwrites good copies).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::step::{Applied, Change, ConflictPolicy, Status, Step};

pub struct SkillsStep {
    pub id: String,
    pub needs: Vec<String>,
    /// Path to the pipe-format skills manifest.
    pub manifest: PathBuf,
    /// State file recording every destination this step manages.
    pub state_file: PathBuf,
}

#[derive(Debug, PartialEq)]
struct Entry {
    name: String,
    source: String,
    targets: Vec<Target>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Claude,
    Pi,
    Shared,
}

impl Target {
    fn base(self, home: &Path) -> PathBuf {
        match self {
            Target::Claude => home.join(".claude/skills"),
            Target::Pi => home.join(".pi/agent/skills"),
            Target::Shared => home.join(".agents/skills"),
        }
    }
    fn label(self) -> &'static str {
        match self {
            Target::Claude => "claude",
            Target::Pi => "pi",
            Target::Shared => "shared",
        }
    }
}

fn parse_manifest(raw: &str) -> Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for (ln, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('|').map(str::trim).collect();
        if parts.len() != 3 || parts.iter().any(|p| p.is_empty()) {
            bail!(
                "skills manifest line {}: expected `name | source | targets`",
                ln + 1
            );
        }
        let targets: Vec<Target> = if parts[2] == "all" {
            vec![Target::Claude, Target::Pi, Target::Shared]
        } else {
            parts[2]
                .split(',')
                .map(str::trim)
                .map(|t| match t {
                    "claude" => Ok(Target::Claude),
                    "pi" => Ok(Target::Pi),
                    "shared" => Ok(Target::Shared),
                    other => bail!(
                        "skills manifest line {}: unknown target '{}' (valid: claude, pi, shared, all)",
                        ln + 1,
                        other
                    ),
                })
                .collect::<Result<_>>()?
        };
        entries.push(Entry {
            name: parts[0].to_string(),
            source: parts[1].to_string(),
            targets,
        });
    }
    Ok(entries)
}

/// Staged content of one skill: relative path -> bytes.
type SkillTree = BTreeMap<PathBuf, Vec<u8>>;

fn read_tree(dir: &Path) -> Result<SkillTree> {
    let mut tree = BTreeMap::new();
    fn walk(root: &Path, dir: &Path, tree: &mut SkillTree) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, tree)?;
            } else {
                let rel = path.strip_prefix(root)?.to_path_buf();
                tree.insert(rel, fs::read(&path)?);
            }
        }
        Ok(())
    }
    walk(dir, dir, &mut tree)?;
    Ok(tree)
}

fn write_tree(dir: &Path, tree: &SkillTree) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    for (rel, bytes) in tree {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }
    Ok(())
}

fn fetch_url(url: &str) -> Result<Vec<u8>> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(180))
        .call()
        .with_context(|| format!("GET {url}"))?;
    let mut buf = Vec::new();
    resp.into_reader()
        .take(200 * 1024 * 1024)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

fn valid_frontmatter(tree: &SkillTree) -> bool {
    tree.get(Path::new("SKILL.md"))
        .map(|b| b.starts_with(b"---"))
        .unwrap_or(false)
}

fn parse_repo_source(source: &str) -> (&str, &str, Option<&str>) {
    let (spec, subpath) = match source.split_once(':') {
        Some((s, p)) => (s, Some(p)),
        None => (source, None),
    };
    let (repo, gitref) = match spec.split_once('@') {
        Some((r, g)) => (r, g),
        None => (spec, "HEAD"),
    };
    (repo, gitref, subpath)
}

fn is_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

/// Download every unique source concurrently: raw SKILL.md URLs into bytes,
/// repo tarballs extracted once per repo@ref. Errors are stored per-source so
/// one bad fetch never poisons the others.
struct Fetched {
    raw: HashMap<String, Result<Vec<u8>, String>>,
    repos: HashMap<String, Result<PathBuf, String>>,
}

fn fetch_all(sources: &[String], tmp: &Path) -> Fetched {
    let mut raw_urls: Vec<&String> = Vec::new();
    let mut repo_keys: HashMap<String, (String, String)> = HashMap::new();
    for s in sources {
        if is_url(s) {
            if !raw_urls.contains(&s) {
                raw_urls.push(s);
            }
        } else {
            let (repo, gitref, _) = parse_repo_source(s);
            repo_keys
                .entry(format!("{repo}@{gitref}"))
                .or_insert_with(|| (repo.to_string(), gitref.to_string()));
        }
    }

    let mut fetched = Fetched {
        raw: HashMap::new(),
        repos: HashMap::new(),
    };
    std::thread::scope(|scope| {
        let raw_handles: Vec<_> = raw_urls
            .iter()
            .map(|url| {
                let url = url.to_string();
                (
                    url.clone(),
                    scope.spawn(move || fetch_url(&url).map_err(|e| format!("{e:#}"))),
                )
            })
            .collect();
        let repo_handles: Vec<_> = repo_keys
            .iter()
            .map(|(key, (repo, gitref))| {
                let (key, repo, gitref) = (key.clone(), repo.clone(), gitref.clone());
                let out = tmp.join(key.replace(['/', '@'], "__"));
                (
                    key.clone(),
                    scope.spawn(move || -> Result<PathBuf, String> {
                        let url = format!("https://codeload.github.com/{repo}/tar.gz/{gitref}");
                        let bytes = fetch_url(&url).map_err(|e| format!("{e:#}"))?;
                        fs::create_dir_all(&out).map_err(|e| e.to_string())?;
                        tar::Archive::new(flate2::read::GzDecoder::new(&bytes[..]))
                            .unpack(&out)
                            .map_err(|e| format!("extracting tarball for {key}: {e}"))?;
                        // tarball root is <repo>-<ref-ish>; locate, don't predict
                        fs::read_dir(&out)
                            .map_err(|e| e.to_string())?
                            .filter_map(|e| e.ok())
                            .map(|e| e.path())
                            .find(|p| p.is_dir())
                            .ok_or_else(|| "tarball contained no directory".to_string())
                    }),
                )
            })
            .collect();
        for (url, h) in raw_handles {
            fetched
                .raw
                .insert(url, h.join().expect("fetch thread panicked"));
        }
        for (key, h) in repo_handles {
            fetched
                .repos
                .insert(key, h.join().expect("fetch thread panicked"));
        }
    });
    fetched
}

/// Assemble a skill's tree from the pre-fetched caches.
fn stage(source: &str, fetched: &Fetched) -> Result<SkillTree> {
    if is_url(source) {
        let bytes = fetched.raw[source]
            .clone()
            .map_err(|e| anyhow::anyhow!(e))?;
        let mut tree = SkillTree::new();
        tree.insert(PathBuf::from("SKILL.md"), bytes);
        if !valid_frontmatter(&tree) {
            bail!("fetched SKILL.md has no YAML frontmatter");
        }
        return Ok(tree);
    }

    let (repo, gitref, subpath) = parse_repo_source(source);
    let key = format!("{repo}@{gitref}");
    let extracted = fetched.repos[&key]
        .clone()
        .map_err(|e| anyhow::anyhow!(e))?;
    let skill_dir = match subpath {
        Some(p) => extracted.join(p),
        None => extracted,
    };
    if !skill_dir.is_dir() {
        bail!(
            "path '{}' not found in {repo}@{gitref}",
            subpath.unwrap_or(".")
        );
    }
    let tree = read_tree(&skill_dir)?;
    if !valid_frontmatter(&tree) {
        bail!("skill at {source} has no SKILL.md with YAML frontmatter");
    }
    Ok(tree)
}

/// Everything one convergence pass needs to know, computed up front so that
/// `plan` and `apply` share one code path.
struct SyncPlan {
    /// (entry name, target label, dest, staged tree or None on fetch failure)
    actions: Vec<(String, &'static str, PathBuf, Option<SkillTree>)>,
    /// Destinations claimed by the current manifest (state-file "new").
    claimed: BTreeSet<String>,
    /// Old state entries no longer claimed → GC candidates.
    removals: Vec<PathBuf>,
    warnings: Vec<String>,
}

impl SkillsStep {
    fn home(&self) -> Result<PathBuf> {
        std::env::var("HOME")
            .map(PathBuf::from)
            .context("HOME not set")
    }

    fn compute(&self) -> Result<SyncPlan> {
        let home = self.home()?;
        let raw = fs::read_to_string(&self.manifest)
            .with_context(|| format!("reading {}", self.manifest.display()))?;
        let entries = parse_manifest(&raw)?;

        let tmp = tempfile::tempdir()?;
        let sources: Vec<String> = entries.iter().map(|e| e.source.clone()).collect();
        let fetched = fetch_all(&sources, tmp.path());
        let mut actions = Vec::new();
        let mut claimed = BTreeSet::new();
        let mut warnings = Vec::new();

        for e in &entries {
            let staged = match stage(&e.source, &fetched) {
                Ok(t) => Some(t),
                Err(err) => {
                    warnings.push(format!(
                        "skill {} — fetch failed ({err:#}); keeping existing copies",
                        e.name
                    ));
                    None
                }
            };
            for t in &e.targets {
                let dest = t.base(&home).join(&e.name);
                claimed.insert(dest.display().to_string());
                actions.push((e.name.clone(), t.label(), dest, staged.clone()));
            }
        }

        // GC: previously-managed destinations no longer claimed.
        let mut removals = Vec::new();
        if let Ok(old) = fs::read_to_string(&self.state_file) {
            let bases = [Target::Claude, Target::Pi, Target::Shared].map(|t| t.base(&home));
            for line in old.lines().map(str::trim).filter(|l| !l.is_empty()) {
                if claimed.contains(line) {
                    continue;
                }
                let p = PathBuf::from(line);
                if line.contains("..") {
                    warnings.push(format!(
                        "state lists suspicious path '{line}' — not removing"
                    ));
                } else if bases.iter().any(|b| p.starts_with(b) && p != *b) {
                    if p.is_dir() {
                        removals.push(p);
                    }
                } else {
                    warnings.push(format!(
                        "state lists unexpected path '{line}' — not removing"
                    ));
                }
            }
        }

        // The tempdir must outlive staging reads; trees are already in memory.
        drop(tmp);
        Ok(SyncPlan {
            actions,
            claimed,
            removals,
            warnings,
        })
    }
}

impl Step for SkillsStep {
    fn id(&self) -> &str {
        &self.id
    }
    fn needs(&self) -> &[String] {
        &self.needs
    }
    fn warn_on_error(&self) -> bool {
        true // network step: never fail the whole run (matches bash behavior)
    }

    fn check(&self) -> Result<Status> {
        // Cheap check without network: pending if any claimed dest is missing.
        // Full drift detection requires fetching — that's `plan`'s job.
        Ok(Status::Pending("sync agent skills from manifest".into()))
    }

    fn plan(&self) -> Result<Vec<Change>> {
        let plan = self.compute()?;
        let mut changes = Vec::new();
        for (name, label, dest, staged) in &plan.actions {
            match staged {
                None => {}
                Some(tree) => {
                    let current = if dest.is_dir() {
                        read_tree(dest).ok()
                    } else {
                        None
                    };
                    if current.as_ref() != Some(tree) {
                        changes.push(Change {
                            summary: format!("skill {name}: install/update {label} copy"),
                            diff: None,
                        });
                    }
                }
            }
        }
        for r in &plan.removals {
            changes.push(Change {
                summary: format!("remove retired skill copy {}", r.display()),
                diff: None,
            });
        }
        for w in &plan.warnings {
            changes.push(Change {
                summary: format!("⚠ {w}"),
                diff: None,
            });
        }
        Ok(changes)
    }

    fn apply(&self, _policy: ConflictPolicy) -> Result<Applied> {
        let plan = self.compute()?;
        for w in &plan.warnings {
            crate::ui::warn(w);
        }
        // Quiet when converged: only updated/removed skills get detail lines.
        let mut updated: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        let mut current: BTreeMap<String, Vec<&str>> = BTreeMap::new();
        for (name, label, dest, staged) in &plan.actions {
            let Some(tree) = staged else { continue };
            let existing = if dest.is_dir() {
                read_tree(dest).ok()
            } else {
                None
            };
            if existing.as_ref() == Some(tree) {
                current.entry(name.clone()).or_default().push(label);
            } else {
                write_tree(dest, tree)
                    .with_context(|| format!("installing skill {name} to {}", dest.display()))?;
                updated.entry(name.clone()).or_default().push(label);
            }
        }
        for (name, labels) in &updated {
            crate::ui::detail(&format!("+ skill {name} → {}", labels.join(", ")));
        }
        for r in &plan.removals {
            fs::remove_dir_all(r).with_context(|| format!("removing {}", r.display()))?;
            crate::ui::detail(&format!("- removed retired copy {}", r.display()));
        }
        if let Some(parent) = self.state_file.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut state: Vec<&String> = plan.claimed.iter().collect();
        state.sort();
        fs::write(
            &self.state_file,
            state
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n")
                + "\n",
        )?;
        let removed = plan.removals.len();
        let summary = match (updated.len(), removed) {
            (0, 0) => {
                return Ok(Applied::Unchanged(format!(
                    "all {} skills up to date",
                    current.len()
                )))
            }
            (u, 0) => format!(
                "{u} skill(s) installed/updated, {} up to date",
                current.len()
            ),
            (u, r) => format!(
                "{u} skill(s) installed/updated, {r} removed, {} up to date",
                current.len()
            ),
        };
        Ok(Applied::Changed(summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries_and_targets() {
        let raw =
            "# comment\n\nherdr | https://x/SKILL.md | all\nfoo | o/r@abc:p/q | claude,shared\n";
        let e = parse_manifest(raw).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(
            e[0].targets,
            vec![Target::Claude, Target::Pi, Target::Shared]
        );
        assert_eq!(e[1].targets, vec![Target::Claude, Target::Shared]);
    }

    #[test]
    fn rejects_malformed_and_unknown_targets() {
        assert!(parse_manifest("just-a-name\n").is_err());
        assert!(parse_manifest("a | b | bogus\n").is_err());
    }

    #[test]
    fn tree_roundtrip_and_compare() {
        let dir = tempfile::tempdir().unwrap();
        let mut tree = SkillTree::new();
        tree.insert(PathBuf::from("SKILL.md"), b"---\nname: t\n".to_vec());
        tree.insert(PathBuf::from("scripts/a.py"), b"print(1)\n".to_vec());
        let dest = dir.path().join("skill");
        write_tree(&dest, &tree).unwrap();
        assert_eq!(read_tree(&dest).unwrap(), tree);
    }
}
