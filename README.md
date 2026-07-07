# kitout

**The agent-era workstation bootstrapper.** Declarative machine setup as a
DAG of typed, idempotent steps — with first-class support for the things no
other tool manages: coding-agent **skills** (pinned, reviewed, garbage-
collected across Claude Code, codex, opencode, gemini, and pi) and **MCP
server registration**.

> Status: **v0.1.x — early, but real.** kitout fully manages its author's
> machines. Ten step types, strict CI (fmt, clippy, tests) on every release
> build, shipped on Homebrew, crates.io, and GitHub Releases. Early adopters
> welcome — macOS only until the Linux fast-follow lands, and manifest-schema
> changes may break before 1.0 (they'll be `feat!:` commits and major-noted
> in releases).

## Install

```bash
brew install beaubutton/tap/kitout   # macOS, prebuilt binary
cargo install kitout                 # anywhere with a Rust toolchain
```

## The idea

```toml
[[step]]
type = "file"
source = "config/ghostty"
target = "~/.config/ghostty/config"

[[step]]
type = "script"
id = "dev-certs"
path = "steps/trust-dev-certs.sh"
on-error = "warn"

[[step]]
type = "mcp-server"          # coming: the reason this tool exists
agent = "claude"
name = "kubernetes"
command = ["mcp-server-kubernetes"]
env = { ALLOW_ONLY_NON_DESTRUCTIVE_TOOLS = "true" }
needs = ["dev-certs"]
```

```
kitout plan      # every change apply would make — read-only, topo-ordered
kitout apply     # converge, interactively (diffs + prompts, serialized)
kitout apply -y  # unattended: local edits are KEPT and warned about
kitout apply --force-replace   # unattended: manifest wins (servers/CI)
```

## Design principles (locked by design review)

- **Local edits are sacred.** Unattended runs never destroy a locally
  modified file; converge-to-manifest is an explicit flag, not a default.
- **DAG execution** with drain-and-report failures: a failed step stops
  scheduling, in-flight steps finish, everything gets a status line.
- **Steps are typed and idempotent** (`check` / `plan` / `apply`); scripts
  are the escape hatch, not the model.
- **Agent infrastructure is first-class**, not a shell hack.

## Platforms

macOS today. Linux is the fast-follow (the step/provider traits are shaped
for it). Windows is out of scope.

## AI transparency

kitout is built in collaboration with Claude (Anthropic's Claude Code). The
majority of the code is AI-written under human direction: the architecture
was locked through a recorded design review, and every change is
human-reviewed, tested, and approved before it lands. Attribution lives here
at the repository level rather than in per-commit trailers. If something is
broken, the human is accountable — file an issue.

## License

MIT OR Apache-2.0, at your option.
