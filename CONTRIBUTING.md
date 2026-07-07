# Contributing

kitout is young — issues and PRs are welcome.

## Ground rules

- Every PR is human-reviewed, and CI must pass: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- **AI-assisted contributions are welcome.** This project is itself largely
  AI-written under human direction (see the README's AI transparency
  section), so there is no stigma — but the bar is identical either way:
  you must understand the change you're submitting, have actually run it,
  and be able to answer review questions about it. Low-effort unreviewed
  AI output pasted into a PR will be closed without ceremony.
- Respect the engine's invariants:
  - `check` and `plan` are read-only, always.
  - Local edits are sacred: nothing may destroy a user's file without an
    explicit interactive confirmation or `--force-replace`.
  - Steps must be idempotent, and steps mutating a shared file must report
    it via `resource()` so the scheduler serializes them.
- One logical change per PR, described with a [Conventional Commit](https://www.conventionalcommits.org)
  message (`feat:`, `fix:`, `docs:`, `chore:`; `feat!:` for anything breaking —
  including changes to the kitout.toml manifest schema). These drive
  `make version` (next-version suggestion) and the changelog.

## Setup

Run `make hooks` once per clone — it installs the commit-msg hook that
enforces the commit convention below.

## Dev loop

```bash
cargo test
cargo fmt && cargo clippy
make check    # the strict gate (fmt, clippy, tests) — same as CI
make demo     # toy manifest end-to-end
```
