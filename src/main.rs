mod create;
mod dag;
mod manifest;
mod mcp_serve;
mod step;
mod sudo;
mod ui;
mod steps {
    pub mod block;
    pub mod brewfile;
    pub mod cmd;
    pub mod defaults;
    pub mod file;
    pub mod mcp;
    pub mod merge;
    pub mod script;
    pub mod secret;
    pub mod skills;
}

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use step::{Applied, ConflictPolicy, Status, Step};

#[derive(Parser)]
#[command(
    name = "kitout",
    version,
    about = "The agent-era workstation bootstrapper"
)]
struct Cli {
    /// Path to the manifest
    #[arg(short, long, default_value = "kitout.toml", global = true)]
    manifest: PathBuf,
    /// Machine-readable JSON output (plan, status, validate)
    #[arg(long, global = true)]
    json: bool,
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
    /// Parse and check the manifest without touching the machine
    Validate,
    /// Print the manifest JSON Schema (for editors and agents)
    Schema,
    /// Run as an MCP server over stdio (agent-operable)
    McpServe,
    /// Scaffold a new machine config from a persona template
    CreateConfig {
        /// Directory to create
        dir: PathBuf,
        /// Persona template (see `--type` help / templates/)
        #[arg(long = "type")]
        persona: String,
        /// Also create + push a private GitHub repo (falls back to local-only)
        #[arg(long)]
        github: bool,
    },
    /// Run a single step (and, transitively, its needs)
    Step { id: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    let manifest_path = cli.manifest.canonicalize().unwrap_or(cli.manifest.clone());
    let base = manifest_path
        .parent()
        .context("manifest has no parent directory")?
        .to_path_buf();

    // Validate catches load/build errors itself (to report them structured),
    // so it must run before the eager load below.
    if let Cmd::Validate = cli.command {
        return validate(&manifest_path, &base, json);
    }

    if let Cmd::Schema = cli.command {
        let schema = schemars::schema_for!(manifest::Manifest);
        println!("{}", serde_json::to_string_pretty(&schema)?);
        return Ok(());
    }

    if let Cmd::CreateConfig {
        dir,
        persona,
        github,
    } = &cli.command
    {
        return create::run(dir, persona, *github);
    }

    if let Cmd::McpServe = cli.command {
        return mcp_serve::run();
    }

    let parsed = manifest::load(&manifest_path)?;
    let wants_sudo = parsed.sudo;
    let steps = manifest::build_steps(parsed, &base)?;
    let waves = dag::waves(&steps)?;

    match cli.command {
        Cmd::Plan => plan(&steps, &waves, json),
        Cmd::Status => status(&steps, json),
        Cmd::Validate => unreachable!("handled before manifest load"),
        Cmd::Schema => unreachable!("handled before manifest load"),
        Cmd::CreateConfig { .. } => unreachable!("handled before manifest load"),
        Cmd::McpServe => unreachable!("handled before manifest load"),
        Cmd::Apply { yes, force_replace } => {
            let policy = match (yes, force_replace) {
                (_, true) => ConflictPolicy::ForceReplace,
                (true, false) => ConflictPolicy::KeepLocal,
                (false, false) => ConflictPolicy::Interactive,
            };
            let _sudo = if wants_sudo {
                Some(sudo::setup(policy == ConflictPolicy::Interactive)?)
            } else {
                None
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
    verb: &str,
    f: impl Fn(&dyn Step) -> R + Sync,
    mut print: impl FnMut(usize, usize, R),
) {
    let order: Vec<(usize, usize)> = waves
        .iter()
        .enumerate()
        .flat_map(|(wn, w)| w.iter().map(move |&i| (wn, i)))
        .collect();
    let mp = indicatif::MultiProgress::new();
    ui::set_progress(mp.clone());
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<(usize, R)>();
        for &(_, i) in &order {
            let tx = tx.clone();
            let f = &f;
            let step = &steps[i];
            let pb = ui::spinner(&mp, verb, step.id());
            scope.spawn(move || {
                let r = f(step.as_ref());
                pb.finish_and_clear();
                let _ = tx.send((i, r));
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
    ui::clear_progress();
}

fn plan(steps: &[Box<dyn Step>], waves: &[Vec<usize>], json: bool) -> Result<()> {
    if json {
        return plan_json(steps);
    }
    use console::style;
    ui::note(&format!(
        "planning {} step(s) across {} wave(s)…",
        steps.len(),
        waves.len()
    ));
    let mut pending = 0usize;
    let mut failed: Option<anyhow::Error> = None;
    parallel_ordered(
        steps,
        waves,
        "planning",
        |s| s.plan(),
        |wn, i, result| {
            let wave = style(format!("wave {wn}")).dim();
            match result {
                Ok(changes) if changes.is_empty() => {
                    ui::sync(|| println!("{wave}  {} {}", style("✓").green().bold(), steps[i].id()))
                }
                Ok(changes) => ui::sync(|| {
                    for c in changes {
                        pending += 1;
                        println!(
                            "{wave}  {} {} {} {}",
                            style("→").yellow().bold(),
                            style(steps[i].id()).bold(),
                            style("—").dim(),
                            c.summary
                        );
                        if let Some(d) = c.diff {
                            for line in d.lines() {
                                println!("      {}", style(line).dim());
                            }
                        }
                    }
                }),
                Err(e) => {
                    ui::fail(steps[i].id(), &format!("{e:#}"));
                    failed.get_or_insert(e);
                }
            }
        },
    );
    if let Some(e) = failed {
        return Err(e.context("plan failed for one or more steps"));
    }
    if pending == 0 {
        println!(
            "\n{}",
            console::style("machine matches the manifest — nothing to do").green()
        );
    } else {
        println!(
            "\n{} change(s) pending — run `kitout apply`",
            console::style(pending).yellow().bold()
        );
    }
    Ok(())
}

fn status(steps: &[Box<dyn Step>], json: bool) -> Result<()> {
    if json {
        return status_json(steps);
    }
    let waves = dag::waves(steps)?;
    parallel_ordered(
        steps,
        &waves,
        "checking",
        |s| s.check(),
        |_, i, result| match result {
            Ok(Status::Satisfied) => ui::ok(steps[i].id(), "", None),
            Ok(Status::Pending(why)) => ui::pending(steps[i].id(), &why),
            Err(e) => ui::fail(steps[i].id(), &format!("check failed: {e:#}")),
        },
    );
    Ok(())
}

/// Parse the manifest, build its steps, and topo-sort them — reporting the
/// first error without touching the machine. The agent-facing "does this
/// manifest even load?" check; `--json` makes the verdict machine-readable.
fn validate(path: &Path, base: &Path, json: bool) -> Result<()> {
    let result = (|| -> Result<(usize, usize)> {
        let parsed = manifest::load(path)?;
        let steps = manifest::build_steps(parsed, base)?;
        let waves = dag::waves(&steps)?;
        Ok((steps.len(), waves.len()))
    })();
    if json {
        let obj = match &result {
            Ok((steps, waves)) => serde_json::json!({
                "valid": true, "steps": steps, "waves": waves
            }),
            Err(e) => serde_json::json!({ "valid": false, "error": format!("{e:#}") }),
        };
        println!("{}", serde_json::to_string_pretty(&obj)?);
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let (steps, waves) = result.context("manifest is invalid")?;
    println!("valid — {steps} step(s), {waves} wave(s)");
    Ok(())
}

// ---- machine-readable output (`--json`) ------------------------------------

#[derive(serde::Serialize)]
struct ChangeJson {
    summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<String>,
}

#[derive(serde::Serialize)]
struct PlanStepJson {
    id: String,
    changes: Vec<ChangeJson>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct PlanJson {
    pending: usize,
    steps: Vec<PlanStepJson>,
}

fn plan_json(steps: &[Box<dyn Step>]) -> Result<()> {
    let mut out = PlanJson {
        pending: 0,
        steps: Vec::new(),
    };
    for s in steps {
        match s.plan() {
            Ok(changes) => {
                out.pending += changes.len();
                out.steps.push(PlanStepJson {
                    id: s.id().to_string(),
                    changes: changes
                        .into_iter()
                        .map(|c| ChangeJson {
                            summary: c.summary,
                            diff: c.diff,
                        })
                        .collect(),
                    error: None,
                });
            }
            Err(e) => out.steps.push(PlanStepJson {
                id: s.id().to_string(),
                changes: Vec::new(),
                error: Some(format!("{e:#}")),
            }),
        }
    }
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

#[derive(serde::Serialize)]
struct StatusStepJson {
    id: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

fn status_json(steps: &[Box<dyn Step>]) -> Result<()> {
    let mut out: Vec<StatusStepJson> = Vec::new();
    for s in steps {
        let (status, detail) = match s.check() {
            Ok(Status::Satisfied) => ("satisfied", None),
            Ok(Status::Pending(why)) => ("pending", Some(why)),
            Err(e) => ("error", Some(format!("{e:#}"))),
        };
        out.push(StatusStepJson {
            id: s.id().to_string(),
            status,
            detail,
        });
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({ "steps": out }))?
    );
    Ok(())
}

/// Wave-parallel execution with drain-and-report semantics: a failure stops
/// scheduling later waves, but every step in the current wave runs to
/// completion. Interactive runs stay sequential so prompts never interleave
/// (the locked "prompt serializer" — trivially correct at wave size 1..n by
/// running interactive waves on one thread).
fn apply(steps: &[Box<dyn Step>], waves: &[Vec<usize>], policy: ConflictPolicy) -> Result<()> {
    use std::time::Instant;
    let total: usize = waves.iter().map(|w| w.len()).sum();
    ui::note(&format!(
        "applying {total} step(s) across {} wave(s)…",
        waves.len()
    ));

    let mut failures: Vec<String> = Vec::new();
    // Render one step's outcome the moment it lands.
    let render = |i: usize,
                  dur: std::time::Duration,
                  r: Result<Applied>,
                  failures: &mut Vec<String>|
     -> bool {
        let id = steps[i].id();
        match r {
            Ok(Applied::Unchanged(s)) => ui::ok(id, &s, Some(dur)),
            Ok(Applied::Changed(s)) => ui::changed(id, &s, Some(dur)),
            Ok(Applied::Kept(s)) => ui::warn(&format!("{id} — {s}")),
            Err(e) if steps[i].warn_on_error() => {
                ui::warn(&format!("{id} failed (on-error = warn): {e:#}"));
            }
            Err(e) => {
                ui::fail(id, &format!("{e:#}"));
                failures.push(id.to_string());
                return true;
            }
        }
        false
    };

    'waves: for wave in waves {
        let mut wave_failed = false;
        if policy == ConflictPolicy::Interactive {
            // Sequential (prompts must never interleave), but live: a spinner
            // for the running step — prompts suspend it via ui::sync — and
            // each result renders immediately.
            for &i in wave {
                let streams = steps[i].streams_output();
                let mp = indicatif::MultiProgress::new();
                let pb = if streams {
                    ui::stream_banner(steps[i].id());
                    None
                } else {
                    ui::set_progress(mp.clone());
                    Some(ui::spinner(&mp, "applying", steps[i].id()))
                };
                let t = Instant::now();
                let r = steps[i].apply(policy);
                if let Some(pb) = pb {
                    pb.finish_and_clear();
                    ui::clear_progress();
                }
                wave_failed |= render(i, t.elapsed(), r, &mut failures);
            }
        } else {
            // Parallel: spinner per in-flight step, results rendered as they
            // arrive (not batched until the wave ends).
            let mp = indicatif::MultiProgress::new();
            ui::set_progress(mp.clone());
            std::thread::scope(|scope| {
                let (tx, rx) = std::sync::mpsc::channel();
                for &i in wave {
                    let tx = tx.clone();
                    let pb = ui::spinner(&mp, "applying", steps[i].id());
                    let step = &steps[i];
                    if step.streams_output() {
                        ui::stream_banner(step.id());
                    }
                    scope.spawn(move || {
                        let t = Instant::now();
                        let r = step.apply(policy);
                        pb.finish_and_clear();
                        let _ = tx.send((i, t.elapsed(), r));
                    });
                }
                drop(tx);
                for (i, dur, r) in rx {
                    wave_failed |= render(i, dur, r, &mut failures);
                }
            });
            ui::clear_progress();
        }
        if wave_failed {
            ui::note("stopping: not scheduling later waves (drain-and-report)");
            break 'waves;
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        bail!("failed steps: {}", failures.join(", "))
    }
}
