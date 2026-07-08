//! `create-config`: scaffold a new machine config from an embedded persona
//! template, git-init it, and (optionally) create + push a GitHub repo. The
//! templates ship inside the binary, so this works offline anywhere.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use include_dir::{include_dir, Dir};

static TEMPLATES: Dir = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// The persona names available to `--type`.
pub fn personas() -> Vec<&'static str> {
    let mut names: Vec<&str> = TEMPLATES
        .dirs()
        .filter_map(|d| d.path().file_name().and_then(|n| n.to_str()))
        .collect();
    names.sort_unstable();
    names
}

pub fn run(dir: &Path, persona: &str, github: bool) -> Result<()> {
    let tpl = TEMPLATES.get_dir(persona).ok_or_else(|| {
        anyhow!(
            "unknown template '{persona}'. Available: {}",
            personas().join(", ")
        )
    })?;
    if dir.exists() {
        bail!("{} already exists — pick a new path", dir.display());
    }
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let n = extract(tpl, persona, dir)?;
    println!(
        "scaffolded {n} file(s) from the '{persona}' template into {}",
        dir.display()
    );

    // Local git repo — always. A converged machine is reproducible from it.
    git(dir, &["init", "-q"])?;
    git(dir, &["add", "-A"])?;
    git(
        dir,
        &[
            "commit",
            "-q",
            "-m",
            &format!("{persona} workstation baseline"),
        ],
    )
    .context("git commit failed (is user.name/user.email configured?)")?;
    println!("initialized a git repo with the baseline committed");

    if github {
        host_on_github(dir)?;
    }

    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("config");
    println!();
    println!("next:");
    println!("  cd {}", dir.display());
    println!("  # trim the sub-categories you don't want in Brewfile + skills/manifest");
    println!("  kitout plan            # review, then `kitout apply`");
    if !github {
        println!("  gh repo create {name} --private --source=. --push   # to host it");
    }
    Ok(())
}

/// Extract every file in `d` (stripping the `persona/` prefix) into `dest`.
fn extract(d: &Dir, persona: &str, dest: &Path) -> Result<usize> {
    let mut count = 0;
    for f in d.files() {
        let rel = f.path().strip_prefix(persona).unwrap_or_else(|_| f.path());
        let out = dest.join(rel);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&out, f.contents()).with_context(|| format!("writing {}", out.display()))?;
        count += 1;
    }
    for sub in d.dirs() {
        count += extract(sub, persona, dest)?;
    }
    Ok(count)
}

/// Create a private GitHub repo and push — but degrade gracefully to
/// local-only when `gh` is missing or unauthenticated (the common case on a
/// brand-new machine).
fn host_on_github(dir: &Path) -> Result<()> {
    if !tool_ok(Command::new("gh").arg("--version")) {
        println!("gh not installed — kept the local repo only. `brew install gh`, then `gh repo create --source=. --push`.");
        return Ok(());
    }
    if !tool_ok(Command::new("gh").args(["auth", "status"])) {
        println!("gh not authenticated — kept the local repo only. Run `gh auth login`, then `gh repo create --source=. --push`.");
        return Ok(());
    }
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("config");
    let ok = Command::new("gh")
        .current_dir(dir)
        .args(["repo", "create", name, "--private", "--source=.", "--push"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        println!("created and pushed private GitHub repo '{name}'");
    } else {
        println!("gh repo create failed — the local repo is intact; push it manually.");
    }
    Ok(())
}

fn git(dir: &Path, args: &[&str]) -> Result<()> {
    let ok = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .with_context(|| format!("running git {}", args.join(" ")))?
        .success();
    if !ok {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

fn tool_ok(cmd: &mut Command) -> bool {
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
