mod dag;
mod manifest;
mod step;
mod steps {
    pub mod brewfile;
    pub mod cmd;
    pub mod file;
    pub mod mcp;
    pub mod script;
    pub mod skills;
}

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use step::{ConflictPolicy, Status, Step};

#[derive(Parser)]
#[command(name = "kitout", version, about = "The agent-era workstation bootstrapper")]
struct Cli {
    /// Path to the manifest
    #[arg(short, long, default_value = "kitout.toml", global = true)]
    manifest: PathBuf,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show every change apply would make (read-only), in execution order
    Plan,
    /// Converge the machine on the manifest
    Apply {
        /// Non-interactive: conflicts keep local edits and warn
        #[arg(short = 'y', long)]
        yes: bool,
        /// Non-interactive: the manifest wins conflicts (servers/CI)
        #[arg(long)]
        force_replace: bool,
    },
    /// Per-step convergence status (read-only)
    Status,
    /// Run a single step (and, transitively, its needs)
    Step { id: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let manifest_path = cli.manifest.canonicalize().unwrap_or(cli.manifest.clone());
    let base = manifest_path
        .parent()
        .context("manifest has no parent directory")?
        .to_path_buf();
    let parsed = manifest::load(&manifest_path)?;
    let steps = manifest::build_steps(parsed, &base)?;
    let waves = dag::waves(&steps)?;

    match cli.command {
        Cmd::Plan => plan(&steps, &waves),
        Cmd::Status => status(&steps),
        Cmd::Apply { yes, force_replace } => {
            let policy = match (yes, force_replace) {
                (_, true) => ConflictPolicy::ForceReplace,
                (true, false) => ConflictPolicy::KeepLocal,
                (false, false) => ConflictPolicy::Interactive,
            };
            apply(&steps, &waves, policy)
        }
        Cmd::Step { id } => {
            let Some(_) = steps.iter().find(|s| s.id() == id) else {
                bail!("no step with id '{id}'");
            };
            let chain = needs_chain(&steps, &id)?;
            let filtered_waves: Vec<Vec<usize>> = waves
                .iter()
                .map(|w| w.iter().copied().filter(|i| chain.contains(i)).collect())
                .filter(|w: &Vec<usize>| !w.is_empty())
                .collect();
            apply(&steps, &filtered_waves, ConflictPolicy::Interactive)
        }
    }
}

/// Indices of a step and its transitive needs.
fn needs_chain(steps: &[Box<dyn Step>], id: &str) -> Result<std::collections::HashSet<usize>> {
    let index: std::collections::HashMap<&str, usize> =
        steps.iter().enumerate().map(|(i, s)| (s.id(), i)).collect();
    let mut wanted = std::collections::HashSet::new();
    let mut stack = vec![index[id]];
    while let Some(i) = stack.pop() {
        if wanted.insert(i) {
            for n in steps[i].needs() {
                stack.push(index[n.as_str()]);
            }
        }
    }
    Ok(wanted)
}

fn plan(steps: &[Box<dyn Step>], waves: &[Vec<usize>]) -> Result<()> {
    let mut pending = 0usize;
    for (wn, wave) in waves.iter().enumerate() {
        for &i in wave {
            let s = &steps[i];
            let changes = s.plan()?;
            if changes.is_empty() {
                println!("wave {wn}  ✓ {}", s.id());
                continue;
            }
            for c in changes {
                pending += 1;
                println!("wave {wn}  → {}: {}", s.id(), c.summary);
                if let Some(d) = c.diff {
                    for line in d.lines() {
                        println!("      {line}");
                    }
                }
            }
        }
    }
    println!("\n{pending} change(s) pending");
    Ok(())
}

fn status(steps: &[Box<dyn Step>]) -> Result<()> {
    for s in steps {
        match s.check()? {
            Status::Satisfied => println!("✓ {}", s.id()),
            Status::Pending(why) => println!("→ {} ({why})", s.id()),
        }
    }
    Ok(())
}

/// Wave-parallel execution with drain-and-report semantics: a failure stops
/// scheduling later waves, but every step in the current wave runs to
/// completion. Interactive runs stay sequential so prompts never interleave
/// (the locked "prompt serializer" — trivially correct at wave size 1..n by
/// running interactive waves on one thread).
fn apply(steps: &[Box<dyn Step>], waves: &[Vec<usize>], policy: ConflictPolicy) -> Result<()> {
    let mut failures: Vec<String> = Vec::new();
    'waves: for wave in waves {
        let results: Vec<(usize, Result<()>)> = if policy == ConflictPolicy::Interactive {
            wave.iter().map(|&i| (i, steps[i].apply(policy))).collect()
        } else {
            std::thread::scope(|scope| {
                let handles: Vec<_> = wave
                    .iter()
                    .map(|&i| (i, scope.spawn(move || steps[i].apply(policy))))
                    .collect();
                handles
                    .into_iter()
                    .map(|(i, h)| (i, h.join().expect("step thread panicked")))
                    .collect()
            })
        };

        let mut wave_failed = false;
        for (i, r) in results {
            match r {
                Ok(()) => println!("✓ {}", steps[i].id()),
                Err(e) if steps[i].warn_on_error() => {
                    eprintln!("⚠ {} failed (on-error = warn): {e:#}", steps[i].id());
                }
                Err(e) => {
                    eprintln!("✗ {} failed: {e:#}", steps[i].id());
                    failures.push(steps[i].id().to_string());
                    wave_failed = true;
                }
            }
        }
        if wave_failed {
            eprintln!("stopping: not scheduling later waves (drain-and-report)");
            break 'waves;
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!("failed steps: {}", failures.join(", "))
    }
}
