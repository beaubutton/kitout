# Examples

Self-contained kitout manifests. Every one is safe to `plan` (read-only); read
the header comment in each `kitout.toml` before you `apply`.

| Example | Shows | Notes |
|---------|-------|-------|
| [`demo/`](demo/) | `file`, `script` | The minimal smoke test (`make demo`). Writes only under `/tmp` — nothing on your real machine changes. |
| [`dotfiles/`](dotfiles/) | `file`, `block-in-file`, `toml-merge`, `defaults` | A personal setup: gitconfig, a managed `~/.zshrc` region, seeded Starship config, macOS prefs. |
| [`agent-workstation/`](agent-workstation/) | `brewfile`, `command-if-missing`, `secret`, `skills`, `mcp-server` (stdio + HTTP) | kitout's flagship — coding-agent infrastructure. Needs Homebrew and the Claude Code CLI installed. |

Try one (read-only):

```bash
kitout -m examples/dotfiles/kitout.toml plan
```

The full field reference for every step type is in
[../docs/manifest.md](../docs/manifest.md).
