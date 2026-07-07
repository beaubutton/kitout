//! kitout's output language. One line per step, one glyph per meaning:
//!   ✓ satisfied/converged   + changed   → pending   ⚠ warning   ✗ failure
//! Child-process output is captured by steps and only surfaces on failure
//! (as a dimmed tail) — kitout's own lines are the interface.

use std::time::Duration;

use console::style;

pub fn ok(id: &str, summary: &str, dur: Option<Duration>) {
    println!(
        "{} {}{}{}",
        style("✓").green().bold(),
        style(id).bold(),
        if summary.is_empty() { String::new() } else { format!(" {} {}", style("—").dim(), summary) },
        fmt_dur(dur)
    );
}

pub fn changed(id: &str, summary: &str, dur: Option<Duration>) {
    println!(
        "{} {}{}{}",
        style("+").cyan().bold(),
        style(id).bold(),
        if summary.is_empty() { String::new() } else { format!(" {} {}", style("—").dim(), summary) },
        fmt_dur(dur)
    );
}

pub fn pending(id: &str, what: &str) {
    println!("{} {} {} {}", style("→").yellow().bold(), style(id).bold(), style("—").dim(), what);
}

pub fn warn(msg: &str) {
    eprintln!("{} {}", style("⚠").yellow().bold(), msg);
}

pub fn fail(id: &str, err: &str) {
    eprintln!("{} {} {} {}", style("✗").red().bold(), style(id).bold(), style("—").dim(), err);
}

pub fn detail(msg: &str) {
    println!("    {msg}");
}

pub fn note(msg: &str) {
    eprintln!("{}", style(msg).dim());
}

/// Announce a step whose output streams to the terminal (scripts, prompts).
pub fn stream_banner(id: &str) {
    println!("{}", style(format!("── {id} ──────────────────────")).blue().bold());
}

/// Dimmed tail of captured child output, for failures.
pub fn dump_tail(output: &str, lines: usize) {
    let all: Vec<&str> = output.lines().collect();
    let start = all.len().saturating_sub(lines);
    if start > 0 {
        eprintln!("    {}", style(format!("… ({start} earlier lines hidden)")).dim());
    }
    for line in &all[start..] {
        eprintln!("    {}", style(line).dim());
    }
}

fn fmt_dur(dur: Option<Duration>) -> String {
    match dur {
        Some(d) if d.as_secs() >= 1 => {
            let s = d.as_secs();
            if s >= 60 {
                style(format!("  ({}m{:02}s)", s / 60, s % 60)).dim().to_string()
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
