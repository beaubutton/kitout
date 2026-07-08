//! `mcp-serve`: expose kitout as an MCP server over stdio, so any MCP-capable
//! agent can drive configs as first-class tools. Newline-delimited JSON-RPC
//! 2.0. Read-only by default — `apply` returns a plan unless `confirm:true`.
//!
//! Each tool shells out to this same binary with `--json` and captures its
//! stdout, which keeps the JSON-RPC stream on our stdout uncorrupted and
//! reuses the exact code paths the CLI is tested against.

use std::io::{BufRead, Write};
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

pub fn run() -> Result<()> {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = req.get("id").cloned();
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let resp = match method {
            "initialize" => Some(json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "kitout", "version": env!("CARGO_PKG_VERSION") }
                }
            })),
            "tools/list" => Some(json!({
                "jsonrpc": "2.0", "id": id, "result": { "tools": tool_defs() }
            })),
            "tools/call" => Some(handle_call(id, &req)),
            "ping" => Some(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
            // notifications (no id) get no response; unknown requests get an error
            _ if id.is_some() => Some(json!({
                "jsonrpc": "2.0", "id": id,
                "error": { "code": -32601, "message": format!("method not found: {method}") }
            })),
            _ => None,
        };
        if let Some(r) = resp {
            writeln!(out, "{}", serde_json::to_string(&r)?)?;
            out.flush()?;
        }
    }
    Ok(())
}

fn dir_arg() -> Value {
    json!({
        "type": "string",
        "description": "The config directory (the folder containing kitout.toml)."
    })
}

fn tool_defs() -> Value {
    json!([
        {
            "name": "personas",
            "description": "List the persona templates available to create_config.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "schema",
            "description": "The kitout manifest JSON Schema — the shape of a valid kitout.toml.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "validate",
            "description": "Parse and check a config without touching the machine. Returns {valid, steps, waves} or {valid:false, error}.",
            "inputSchema": { "type": "object", "properties": { "dir": dir_arg() }, "required": ["dir"] }
        },
        {
            "name": "plan",
            "description": "Read-only: every change apply would make, per step, with diffs.",
            "inputSchema": { "type": "object", "properties": { "dir": dir_arg() }, "required": ["dir"] }
        },
        {
            "name": "status",
            "description": "Read-only: per-step convergence state of the machine against the config.",
            "inputSchema": { "type": "object", "properties": { "dir": dir_arg() }, "required": ["dir"] }
        },
        {
            "name": "create_config",
            "description": "Scaffold a new machine config from a persona template, git-init it, and optionally host it on GitHub.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir": { "type": "string", "description": "Directory to create." },
                    "type": { "type": "string", "description": "Persona template (call personas for the list)." },
                    "github": { "type": "boolean", "description": "Also create + push a private GitHub repo (falls back to local-only)." }
                },
                "required": ["dir", "type"]
            }
        },
        {
            "name": "apply",
            "description": "Converge the machine on the config. GATED: without confirm:true this returns the plan only. With confirm:true it actually applies (keeping local edits).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dir": dir_arg(),
                    "confirm": { "type": "boolean", "description": "Must be true to actually converge; otherwise a plan is returned." }
                },
                "required": ["dir"]
            }
        }
    ])
}

fn handle_call(id: Option<Value>, req: &Value) -> Value {
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    match dispatch(name, &args) {
        Ok(text) => json!({
            "jsonrpc": "2.0", "id": id,
            "result": { "content": [{ "type": "text", "text": text }], "isError": false }
        }),
        Err(e) => json!({
            "jsonrpc": "2.0", "id": id,
            "result": { "content": [{ "type": "text", "text": format!("error: {e:#}") }], "isError": true }
        }),
    }
}

fn dispatch(name: &str, args: &Value) -> Result<String> {
    let exe = std::env::current_exe().context("locating kitout binary")?;
    let dir = args.get("dir").and_then(Value::as_str);
    match name {
        "personas" => Ok(crate::create::personas().join("\n")),
        "schema" => run_kitout(&exe, &["schema"]),
        "validate" => run_kitout(&exe, &["-m", &manifest_of(dir)?, "validate", "--json"]),
        "plan" => run_kitout(&exe, &["-m", &manifest_of(dir)?, "plan", "--json"]),
        "status" => run_kitout(&exe, &["-m", &manifest_of(dir)?, "status", "--json"]),
        "create_config" => {
            let d = dir.context("create_config needs 'dir'")?;
            let persona = args
                .get("type")
                .and_then(Value::as_str)
                .context("create_config needs 'type'")?;
            let mut a = vec!["create-config", d, "--type", persona];
            if args.get("github").and_then(Value::as_bool).unwrap_or(false) {
                a.push("--github");
            }
            run_kitout(&exe, &a)
        }
        "apply" => {
            let manifest = manifest_of(dir)?;
            if args
                .get("confirm")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                run_kitout(&exe, &["-m", &manifest, "apply", "-y"])
            } else {
                let plan = run_kitout(&exe, &["-m", &manifest, "plan", "--json"])?;
                Ok(format!(
                    "apply is gated — this is a PLAN only. Re-call apply with confirm:true to converge.\n\n{plan}"
                ))
            }
        }
        _ => bail!("unknown tool '{name}'"),
    }
}

fn manifest_of(dir: Option<&str>) -> Result<String> {
    let d = dir.context("this tool needs 'dir' (the config directory)")?;
    Ok(format!("{}/kitout.toml", d.trim_end_matches('/')))
}

fn run_kitout(exe: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new(exe)
        .args(args)
        .output()
        .with_context(|| format!("running kitout {}", args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        return Ok(stdout);
    }
    // validate --json prints its verdict to stdout even on a non-zero exit;
    // include stderr for the human-formatted commands.
    let stderr = String::from_utf8_lossy(&out.stderr);
    Ok(if stdout.trim().is_empty() {
        stderr.into_owned()
    } else {
        format!("{stdout}\n{stderr}")
    })
}
