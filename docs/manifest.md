# Manifest reference

A kitout manifest is a single TOML file (`kitout.toml` by default; override
with `-m/--manifest`). It has an optional top-level section and an array of
`[[step]]` tables. Every step is tagged by `type`.

```toml
# Top-level
sudo = true          # collect the sudo password once, up front (see below)

[[step]]
type = "file"
source = "config/ghostty"
target = "~/.config/ghostty/config"
```

Unknown keys are **hard errors** — a typo'd field fails at parse time rather
than being silently ignored. Validate a manifest without touching the machine
with `kitout plan`.

## Top-level

| Key       | Type            | Default | Meaning |
|-----------|-----------------|---------|---------|
| `extends` | string/string[] | `[]`    | Base manifests to inherit — see below. |
| `sudo`    | bool            | `false` | When true, `apply` prompts for the sudo password once and stashes it in the login Keychain, exposing it to every child process via `SUDO_ASKPASS` for the rest of the run. Needed because `brew` runs `sudo --reset-timestamp`, so a cached timestamp alone doesn't survive. Scripts opt in with `sudo -A`. The Keychain item is deleted when the run ends. `sudo` OR-merges across `extends`. |
| `step`    | array           | `[]`    | The steps, written as repeated `[[step]]` tables. |

## Inheritance (`extends`)

One shared baseline, many machines — without duplicating it. Split the config
into single-purpose files and compose them:

```toml
# base.toml       → the baseline (every machine)
# game-dev.toml   → just the game-dev steps
# ai.toml         → just the ai steps

# this machine's kitout.toml:
extends = ["base.toml", "game-dev.toml", "ai.toml"]
[[step]]                       # plus anything machine-specific
...
```

`kitout -m <file>` loads the file, merges everything it `extends` (left-to-right,
**recursively**, each file merged **once** even via a diamond), then appends the
file's own steps. Which config a machine runs is just which file you point `-m`
at — no flags, no profiles, no state.

Rules: **step ids must be unique** across a manifest and everything it extends
(a collision is an error — merge is append-only, no override). `extends` cycles
are an error. Paths resolve relative to the file that declares them and must
stay within its directory tree. **Keep extended files as siblings** — every
step's `source` / `path` / Brewfile resolves against the *root* manifest's
directory. Omit `extends` entirely and a manifest behaves exactly as before.

## Fields common to every step

| Field   | Type       | Default        | Meaning |
|---------|------------|----------------|---------|
| `type`  | string     | *(required)*   | Selects the step type (the values in this doc). |
| `id`    | string     | *(auto)*       | Stable identifier, used by `needs` and by `kitout step <id>`. Auto-derived when omitted — see each type for its default shape (e.g. `file:<target>`, `mcp:<name>`). |
| `needs` | string[]   | `[]`           | Ids this step depends on. kitout builds a DAG and runs independent steps in parallel waves; a step runs only after everything in `needs` has succeeded. |

Two more scheduling notes that aren't fields:

- **Resource serialization** — steps that write the same target file are run
  in manifest order, never concurrently, even if the DAG would allow it. You
  don't declare this; kitout infers it from the target.
- **Failure = drain and report** — when a step fails, in-flight steps finish
  but no *new* waves are scheduled, and the failures are reported together at
  the end. Use `on-error = "warn"` (script steps) to downgrade a non-fatal
  step so it can't halt the run.

---

## `file`

Copy a source file from the repo to a target path, diffing on conflict.

| Field         | Type   | Default       | Meaning |
|---------------|--------|---------------|---------|
| `source`      | string | *(required)*  | Path relative to the manifest's directory. |
| `target`      | string | *(required)*  | Destination; `~` is expanded. |
| `on-conflict` | enum   | `prompt-diff` | What to do when the target exists and differs: `prompt-diff` shows a unified diff and asks; `replace` overwrites; `keep` leaves the local file. Apply-wide flags override this: `apply -y` keeps local edits and warns, `apply --force-replace` makes the manifest win. |

Auto id: `file:<target>`.

```toml
[[step]]
type = "file"
source = "config/ghostty"
target = "~/.config/ghostty/config"
on-conflict = "prompt-diff"
```

Local edits are sacred: kitout never clobbers a hand-edited target
unattended — the default is to show the diff and ask.

---

## `script`

Run an idempotent script — the escape hatch for what no typed step covers.

| Field      | Type     | Default | Meaning |
|------------|----------|---------|---------|
| `path`     | string   | *(required)* | Executable, relative to the manifest's directory. Spawned directly (no shell wrapper). |
| `check`    | string[] | *(none)* | Optional convergence probe as argv. Exit `0` means already-satisfied and the script is **skipped**. Without a `check`, the script runs every apply and must be idempotent on its own. |
| `on-error` | enum     | `fail`  | `fail` halts the run (drain-and-report); `warn` reports a warning and lets the run continue. Use `warn` for steps that can legitimately fail unattended (e.g. GUI-gated actions). |

Auto id: `script:<path>`.

```toml
[[step]]
type = "script"
id = "starship"
path = "steps/starship.sh"
check = ["sh", "-c", "test -f ~/.config/starship.toml"]
on-error = "warn"
```

Script conventions: `set -e`, self-contained, idempotent, plain `echo` output
(kitout owns the presentation), `sudo -A` for privilege. A cheap `check` probe
keeps `plan`/`status` fast and honest.

---

## `skills`

Sync agent skills across coding agents from a pinned manifest, with garbage
collection of removed entries.

| Field        | Type   | Default                          | Meaning |
|--------------|--------|----------------------------------|---------|
| `manifest`   | string | *(required)*                     | Pipe-format skills manifest (`name \| source \| targets`), relative to the manifest's directory. |
| `state-file` | string | `~/.config/kitout/managed-skills`| Records what kitout installed so removed entries can be garbage-collected; `~` expanded. |

Auto id: `skills`.

Each manifest line pins a skill and its targets:

```
frontend-design | owner/repo@<sha>:path/to/skill | claude,shared
grill-me        | https://example.com/SKILL.md   | all
```

`source` is either a raw `SKILL.md` URL or a GitHub `owner/repo[@ref][:path]`
(fetched as one tarball per `repo@ref`). Targets map to per-agent skills dirs;
a skill installs only when content differs, and copies whose manifest entry or
target disappears are removed. A fetch failure never removes or overwrites a
good copy.

```toml
[[step]]
type = "skills"
manifest = "skills/manifest"
state-file = "~/.config/kitout/managed-skills"
```

---

## `mcp-server`

Register an MCP server with Claude Code, idempotently (`claude mcp get/add`).
Pick **one** transport: stdio (`command` + `env`) or HTTP (`url` + `headers`).

| Field     | Type              | Default | Meaning |
|-----------|-------------------|---------|---------|
| `name`    | string            | *(required)* | Server name as registered with the agent. |
| `command` | string[]          | `[]`    | stdio transport: the server command and its args. |
| `env`     | table<str,str>    | `{}`    | stdio transport: environment variables for the server process. |
| `url`     | string            | *(none)*| HTTP transport URL. Presence of `url` selects HTTP. |
| `headers` | table<str,str>    | `{}`    | HTTP headers, passed **literally** — `${VAR}` is not expanded by kitout; it reaches the agent config verbatim for the agent's own runtime expansion. |

Auto id: `mcp:<name>`.

```toml
# stdio
[[step]]
type = "mcp-server"
name = "godot"
command = ["node", "/path/to/godot-mcp/build/index.js"]
env = { GODOT_PATH = "/Applications/Godot.app/Contents/MacOS/Godot" }
needs = ["brewfile"]        # the claude CLI must exist first

# HTTP
[[step]]
type = "mcp-server"
name = "example"
url = "https://mcp.example.com/mcp"
headers = { Authorization = "Bearer ${EXAMPLE_TOKEN}" }
```

---

## `command-if-missing`

Probe for a binary; run an installer only when it's absent. For tools that
aren't in Homebrew (npm globals, dotnet tools, custom installers).

| Field     | Type     | Default      | Meaning |
|-----------|----------|--------------|---------|
| `probe`   | string   | *(required)* | Binary to look for. A bare name is searched on `PATH`; a value containing `/` is checked as a literal path. |
| `install` | string[] | *(required)* | Installer argv, spawned directly (no shell). |

Auto id: `cmd:<probe>`.

```toml
[[step]]
type = "command-if-missing"
probe = "~/.dotnet/tools/pwsh"
install = ["dotnet", "tool", "install", "--global", "PowerShell"]
```

---

## `absent`

The inverse of `command-if-missing`: probe for something, and run a removal
command only when it's **present**. For uninstalling what a machine shouldn't
have — bundled apps (Pages, GarageBand), an App Store app, a stray brew/npm
package. Stateless like every step: presence is read off the machine, so once
the thing is gone the step is satisfied — no state file, nothing to track.

| Field    | Type     | Default      | Meaning |
|----------|----------|--------------|---------|
| `probe`  | string   | *(required)* | What to look for. A bare name is searched on `PATH`; a value containing `/` is checked as a literal path. **Present → remove; absent → satisfied.** |
| `remove` | string[] | *(required)* | Removal argv, spawned directly (no shell). Prefix with `sudo` (with `sudo = true`) for admin-owned paths. |

Auto id: `absent:<probe>`. After a successful removal kitout re-checks the
probe; a command that exits `0` but leaves it present (a SIP-protected system
app, a wrong package name) is reported as a failure rather than silently
looping as pending.

```toml
# Debloat a bundled app
[[step]]
type = "absent"
probe = "/Applications/GarageBand.app"
remove = ["sudo", "rm", "-rf", "/Applications/GarageBand.app"]

# Drop a global npm package
[[step]]
type = "absent"
probe = "node-sass"
remove = ["npm", "uninstall", "-g", "node-sass"]
```

Only SIP-writable paths can be removed: the iWork/iLife apps in `/Applications`
go, but sealed system apps (Safari, Mail) can't be, even with sudo.

---

## `brewfile`

Run `brew bundle` against a Brewfile, trusting any taps it declares.

| Field  | Type   | Default      | Meaning |
|--------|--------|--------------|---------|
| `path` | string | *(required)* | Brewfile path, relative to the manifest's directory. |

Auto id: `brewfile`. Convergence tracks whether every entry is installed and
current, so an available upgrade shows as pending.

```toml
[[step]]
type = "brewfile"
path = "Brewfile"
```

---

## `brew`

Declare Homebrew packages inline, without a sidecar Brewfile — handy for a small,
self-contained fragment (see [Inheritance](#inheritance-extends)). kitout renders
the lists into a Brewfile and runs the same `brew bundle` as `brewfile`.

| Field      | Type     | Default | Meaning |
|------------|----------|---------|---------|
| `taps`     | string[] | `[]`    | Taps to add, `user/repo` — rendered as `tap "..."`. |
| `formulae` | string[] | `[]`    | Formulae — rendered as `brew "..."`. A tap-scoped `user/tap/name` is trusted automatically. |
| `casks`    | string[] | `[]`    | Casks — rendered as `cask "..."`. |
| `vscode`   | string[] | `[]`    | VS Code extension ids — rendered as `vscode "..."`. |

Auto id: `brew:<first-package>` (first formula, else cask, else tap) — give
multiple `brew` steps an explicit `id` so they don't collide. Like `brewfile`,
all brew steps share one Homebrew resource, so they run serially, never
concurrently.

```toml
[[step]]
type = "brew"
id = "game-dev"
taps = ["hashicorp/tap"]
formulae = ["jq", "hashicorp/tap/terraform"]
casks = ["godot-mono"]
vscode = ["golang.go"]
```

Inline `brew` covers string-list entries only. Anything structured — `mas` apps
with numeric ids, per-formula `args`, custom-URL taps — belongs in a real
`brewfile` step, so kitout isn't reimplementing Brewfile syntax.

---

## `secret`

Prompt once for a secret, stash it in the login Keychain, reuse it forever.
Values are never printed; unattended runs never prompt (they report `Kept`).

| Field     | Type   | Default   | Meaning |
|-----------|--------|-----------|---------|
| `service` | string | *(required)* | Keychain service name. |
| `prompt`  | string | *(required)* | Text shown when stashing interactively. |
| `account` | string | `$USER`   | Keychain account. |

Auto id: `secret:<service>`.

```toml
[[step]]
type = "secret"
service = "example-token"
prompt = "Example API token"
```

Consume it in a later step via the environment (e.g. an `mcp-server` header
referencing `${EXAMPLE_TOKEN}`), keeping the value out of every config file.

---

## `json-merge` / `toml-merge`

Converge specific keys inside a structured config file without clobbering the
rest. `toml-merge` uses a format-preserving parser, so comments and layout in
the target survive.

| Field    | Type       | Default    | Meaning |
|----------|------------|------------|---------|
| `target` | string     | *(required)* | File to merge into; `~` expanded. Created if absent. |
| `mode`   | enum       | `converge` | `converge` enforces the manifest's values (overwrites divergent keys); `seed` only writes keys that are **absent**, so user tweaks survive. |
| `value`  | TOML table | *(required)* | The keys to merge. Nested tables deep-merge. |

Auto id: `json-merge:<target>` / `toml-merge:<target>`.

```toml
# Enforce a key
[[step]]
type = "json-merge"
target = "~/.claude/settings.json"
mode = "converge"
value = { statusLine = { type = "command", command = "~/.claude/statusline.sh" } }

# Seed defaults the user may later edit
[[step]]
type = "toml-merge"
target = "~/.codex/config.toml"
mode = "seed"
value = { model = "gpt-5" }
```

---

## `block-in-file`

Own a marked region of a file kitout only partially manages — everything
outside the markers stays the user's.

| Field            | Type   | Default | Meaning |
|------------------|--------|---------|---------|
| `target`         | string | *(required)* | File to edit; `~` expanded. Created if absent. |
| `marker`         | string | *(required)* | Name embedded in the begin/end marker lines. |
| `block`          | string | *(required)* | Content managed between the markers. |
| `comment-prefix` | string | `#`     | Comment leader for the marker lines (e.g. `//` for JS). |

Auto id: `block:<marker>`. The managed region is delimited by
`<prefix> >>> <marker> >>>` and `<prefix> <<< <marker> <<<`; kitout rewrites
only what's between them.

```toml
[[step]]
type = "block-in-file"
target = "~/.zshrc"
marker = "kitout:aliases"
block = '''
alias ll='eza -lah --icons --git'
alias cat='bat --paging=never'
'''
```

---

## `defaults`

Apply macOS `defaults` preferences with per-key change detection, restarting
affected apps only when something actually changed.

| Field   | Type            | Default | Meaning |
|---------|-----------------|---------|---------|
| `write` | array of tables | *(required)* | Each entry is `{ domain, key, value }`. `value` must be a bool, int, or string. |
| `kill`  | string[]        | `[]`    | Processes to `killall` — but only if at least one key was written this run. |

Auto id: `defaults`.

```toml
[[step]]
type = "defaults"
kill = ["Dock", "Finder"]
write = [
  { domain = "com.apple.dock",   key = "autohide",     value = true },
  { domain = "com.apple.finder", key = "ShowPathbar",  value = true },
  { domain = "NSGlobalDomain",   key = "KeyRepeat",    value = 2 },
]
```
