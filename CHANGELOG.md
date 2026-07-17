## [0.5.2] - 2026-07-17

### 🐛 Bug Fixes

- Don't emit the show-cursor escape to a non-terminal
## [0.5.1] - 2026-07-16

### 🐛 Bug Fixes

- Restore the terminal on interrupt/panic

### ⚙️ Miscellaneous Tasks

- *(release)* V0.5.1
## [0.5.0] - 2026-07-12

### 🚀 Features

- [**breaking**] Skills `all` = claude,shared (pi reads shared)

### ⚙️ Miscellaneous Tasks

- *(release)* V0.5.0
## [0.4.1] - 2026-07-12

### 🐛 Bug Fixes

- Kitout step never bootstrapped sudo

### ⚙️ Miscellaneous Tasks

- *(release)* V0.4.1
## [0.4.0] - 2026-07-11

### 🚀 Features

- Absent step — remove software declaratively

### ⚙️ Miscellaneous Tasks

- *(release)* V0.4.0
## [0.3.0] - 2026-07-11

### 🚀 Features

- Manifest inheritance via `extends`
- Inline brew step (taps/formulae/casks/vscode)

### 📚 Documentation

- How to register kitout's MCP server with an agent

### ⚙️ Miscellaneous Tasks

- *(release)* V0.3.0
## [0.2.1] - 2026-07-08

### 🐛 Bug Fixes

- *(skills)* Extract only the needed subpath; detect oversized tarballs

### ⚙️ Miscellaneous Tasks

- *(release)* V0.2.1
## [0.2.0] - 2026-07-08

### 🚀 Features

- Add `validate` command and global `--json` output
- Add 12 persona workstation templates from skills.sh research
- Add `kitout schema` — emit the manifest JSON Schema
- Add `kitout create-config` to scaffold from a persona template
- Add `kitout mcp-serve` — kitout as an MCP server

### 🐛 Bug Fixes

- *(templates)* Resolve skill pin subpaths against real repo trees
- *(templates)* Correct three Homebrew names caught by catalog audit

### 📚 Documentation

- Drop predecessor reference from status blurb
- Add manifest/step-type reference; genericize source comments
- Add dotfiles and agent-workstation examples
- Document agent-operable commands and persona templates

### ⚙️ Miscellaneous Tasks

- *(release)* V0.2.0
## [0.1.3] - 2026-07-07

### 🐛 Bug Fixes

- Stdio mcp add puts name before variadic -e (claude CLI swallows it)

### ⚙️ Miscellaneous Tasks

- *(release)* V0.1.3
## [0.1.2] - 2026-07-07

### 🐛 Bug Fixes

- Mcp-server passes positionals before --header (claude CLI variadic)
- Release commit uses conventional format (commit-msg hook compat)

### 💼 Other

- Dev loop and guarded one-command releases

### 📚 Documentation

- Status v0.1.x — shipped on three channels, dogfood complete

### ⚙️ Miscellaneous Tasks

- Version suggestion and changelog via git-cliff, conventional commits policy
- Enforce conventional commits via hook, agent commit rubric
- Auto-draft commit messages from staged diffs via claude hook
- *(release)* V0.1.2
## [0.1.1] - 2026-07-07

### 💼 Other

- Install via the Homebrew tap or cargo
## [0.1.0] - 2026-07-07

### 💼 Other

- Cheap no-network check — status is now fully green on a converged machine
