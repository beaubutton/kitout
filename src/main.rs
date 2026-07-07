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

/// Run a read-only computation for every step in parallel, printing results
/// progressively in deterministic wave order (a slow early step delays later
/// lines, but fast steps before it appear immediately).
fn parallel_ordered<R: Send>(
    steps: &[Box<dyn Step>],
    waves: &[Vec<usize>],
    f: impl Fn(&dyn Step) -> R + Sync,
    mut print: impl FnMut(usize, usize, R),
) {
    let order: Vec<(usize, usize)> = waves
        .iter()
        .enumerate()
        .flat_map(|(wn, w)| w.iter().map(move |&i| (wn, i)))
        .collect();
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<(usize, R)>();
        for &(_, i) in &order {
            let tx = tx.clone();
            let f = &f;
            let step = &steps[i];
            scope.spawn(move || {
                let _ = tx.send((i, f(step.as_ref())));
            });
        }
        drop(tx);
        let mut buf: std::collections::HashMap<usize, R> = std::collections::HashMap::new();
        for &(wn, i) in &order {
            let r = loop {
                if let Some(r) = buf.remove(&i) {
                    break r;
                }
                match rx.recv() {
                    Ok((j, r)) if j == i => break r,
                    Ok((j, r)) => {
                        buf.insert(j, r);
                    }
                    Err(_) => return,
                }
            };
            print(wn, i, r);
        }
    });
}

fn plan(steps: &[Box<dyn Step>], waves: &[Vec<usize>]) -> Result<()> {
    eprintln!("planning {} step(s) across {} wave(s)…", steps.len(), waves.len());
    let mut pending = 0usize;
    let mut failed: Option<anyhow::Error> = None;
    parallel_ordered(steps, waves, |s| s.plan(), |wn, i, result| match result {
        Ok(changes) if changes.is_empty() => println!("wave {wn}  ✓ {}", steps[i].id()),
        Ok(changes) => {
            for c in changes {
                pending += 1;
                println!("wave {wn}  → {}: {}", steps[i].id(), c.summary);
                if let Some(d) = c.diff {
                    for line in d.lines() {
                        println!("      {line}");
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("wave {wn}  ✗ {}: {e:#}", steps[i].id());
            failed.get_or_insert(e);
        }
    });
    if let Some(e) = failed {
        return Err(e.context("plan failed for one or more steps"));
    }
    println!("\n{pending} change(s) pending");
    Ok(())
}

fn status(steps: &[Box<dyn Step>]) -> Result<()> {
    let waves = dag::waves(steps)?;
    parallel_ordered(steps, &waves, |s| s.check(), |_, i, result| match result {
        Ok(Status::Satisfied) => println!("✓ {}", steps[i].id()),
        Ok(Status::Pending(why)) => println!("→ {} ({why})", steps[i].id()),
        Err(e) => eprintln!("✗ {} (check failed: {e:#})", steps[i].id()),
    });
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
