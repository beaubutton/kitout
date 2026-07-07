//! kitout's output language. One line per step, one glyph per meaning:
//!   ✓ satisfied/converged   + changed   → pending   ⚠ warning   ✗ failure
//! Child-process output is captured by steps and only surfaces on failure
//! (as a dimmed tail) — kitout's own lines are the interface.

use std::sync::Mutex;
use std::time::Duration;

use console::style;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// The spinner frames from osx-baseline's original bash `spin()` helper,
/// living on in Rust.
const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

static MP: Mutex<Option<MultiProgress>> = Mutex::new(None);

/// Install a MultiProgress so every ui print serializes with spinner redraws.
pub fn set_progress(mp: MultiProgress) {
    *MP.lock().unwrap() = Some(mp);
}

pub fn clear_progress() {
    *MP.lock().unwrap() = None;
}

/// Run `f` with spinner drawing suspended (no-op when no spinners active).
fn with_progress<R>(f: impl FnOnce() -> R) -> R {
    let guard = MP.lock().unwrap();
    match &*guard {
        Some(mp) => mp.suspend(f),
        None => f(),
    }
}

/// One animated line for an in-flight step: `⠹ applying skills… (3s)`.
/// Finish-and-clear it when the step lands; its result line replaces it.
pub fn spinner(mp: &MultiProgress, verb: &str, id: &str) -> ProgressBar {
    let pb = mp.add(ProgressBar::new_spinner());
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan.bold} {msg} {elapsed:.dim}")
            .expect("valid template")
            .tick_strings(FRAMES),
    );
    pb.set_message(format!("{verb} {}…", style(id).bold()));
    pb.enable_steady_tick(Duration::from_millis(90));
    pb
}

/// Run `f` with spinner drawing suspended — use around any direct printing
/// done while spinners may be active.
pub fn sync<R>(f: impl FnOnce() -> R) -> R {
    with_progress(f)
}

pub fn ok(id: &str, summary: &str, dur: Option<Duration>) {
    with_progress(|| {
        println!(
            "{} {}{}{}",
            style("✓").green().bold(),
            style(id).bold(),
            if summary.is_empty() {
                String::new()
            } else {
                format!(" {} {}", style("—").dim(), summary)
            },
            fmt_dur(dur)
        )
    });
}

pub fn changed(id: &str, summary: &str, dur: Option<Duration>) {
    with_progress(|| {
        println!(
            "{} {}{}{}",
            style("+").cyan().bold(),
            style(id).bold(),
            if summary.is_empty() {
                String::new()
            } else {
                format!(" {} {}", style("—").dim(), summary)
            },
            fmt_dur(dur)
        )
    });
}

pub fn pending(id: &str, what: &str) {
    with_progress(|| {
        println!(
            "{} {} {} {}",
            style("→").yellow().bold(),
            style(id).bold(),
            style("—").dim(),
            what
        )
    });
}

pub fn warn(msg: &str) {
    with_progress(|| eprintln!("{} {}", style("⚠").yellow().bold(), msg));
}

pub fn fail(id: &str, err: &str) {
    with_progress(|| {
        eprintln!(
            "{} {} {} {}",
            style("✗").red().bold(),
            style(id).bold(),
            style("—").dim(),
            err
        )
    });
}

pub fn detail(msg: &str) {
    with_progress(|| println!("    {msg}"));
}

pub fn note(msg: &str) {
    with_progress(|| eprintln!("{}", style(msg).dim()));
}

/// Announce a step whose output streams to the terminal (scripts, prompts).
pub fn stream_banner(id: &str) {
    with_progress(|| {
        println!(
            "{}",
            style(format!("── {id} ──────────────────────"))
                .blue()
                .bold()
        )
    });
}

/// Dimmed tail of captured child output, for failures.
pub fn dump_tail(output: &str, lines: usize) {
    with_progress(|| {
        let all: Vec<&str> = output.lines().collect();
        let start = all.len().saturating_sub(lines);
        if start > 0 {
            eprintln!(
                "    {}",
                style(format!("… ({start} earlier lines hidden)")).dim()
            );
        }
        for line in &all[start..] {
            eprintln!("    {}", style(line).dim());
        }
    });
}

fn fmt_dur(dur: Option<Duration>) -> String {
    match dur {
        Some(d) if d.as_secs() >= 1 => {
            let s = d.as_secs();
            if s >= 60 {
                style(format!("  ({}m{:02}s)", s / 60, s % 60))
                    .dim()
                    .to_string()
            } else {
                style(format!("  ({s}s)")).dim().to_string()
            }
        }
        _ => String::new(),
    }
}

/// Run a command with captured output; returns (success, combined out+err).
pub fn run_captured(cmd: &mut std::process::Command) -> anyhow::Result<(bool, String)> {
    let out = cmd.output()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok((out.status.success(), text))
}
