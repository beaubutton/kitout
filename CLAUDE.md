# CLAUDE.md

Guidance for Claude Code (and other coding agents) working in this repository.

## What this is

kitout — the agent-era workstation bootstrapper. Rust binary; declarative
TOML manifest executed as a DAG of typed, idempotent steps. See README for
the user story and CONTRIBUTING for engine invariants.

## Commands

```bash
make check      # strict gate: fmt --check, clippy -D warnings, tests (= CI)
make demo       # toy manifest end-to-end (examples/demo)
make version    # suggested next version (from conventional commits)
make release VERSION=X.Y.Z   # guarded release; CI does the rest
make hooks      # install the commit-msg hook (once per clone)
```

## Commit messages — Conventional Commits, enforced by hook

Format: `type(optional-scope)!: subject` — subject ≤ 72 chars, imperative.
A committed `commit-msg` hook rejects anything else. **No AI trailers** (no
Co-Authored-By, no session links): attribution is repo-level (README).

Classify by the *scope of the change*, not the size of the diff:

| Change touches | Type |
|---|---|
| Manifest schema: any `StepDef`/field change in `src/manifest.rs` that makes existing `kitout.toml` files parse differently, CLI flag renames/removals, changed step semantics that could alter what apply does to an existing machine | **`feat!:`** (breaking — this is the rule that matters most) |
| New step type, new manifest field (backward-compatible), new CLI capability | `feat:` |
| Incorrect behavior corrected, race/crash/output bugs | `fix:` |
| README, CONTRIBUTING, comments, examples | `docs:` |
| `.github/`, dist-workspace.toml, release plumbing | `ci:` |
| Makefile, dev tooling, dependency bumps | `chore:` |
| Internal restructuring with no behavior change | `refactor:` |
| Tests only | `test:` |

When a commit mixes types, prefer splitting; if it must land together, use
the highest-impact type (breaking > feat > fix > the rest).

## Invariants (do not violate in any change)

- `check()` and `plan()` are read-only, always.
- Local edits are sacred: never destroy a user file without interactive
  confirmation or `--force-replace`.
- Steps are idempotent; steps mutating a shared file must report it via
  `resource()` so the scheduler serializes them.
