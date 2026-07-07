# kitout development & release automation.
# `make` / `make help` lists targets.

.PHONY: help check fmt test build install demo plan version changelog release hooks

help: ## List available targets
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z_-]+:.*## / {printf "  \033[36m%-10s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

check: ## The strict gate — fmt, clippy -D warnings, tests (same as CI)
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings
	cargo test

fmt: ## Format the tree
	cargo fmt

test: ## Run tests
	cargo test

build: ## Release build
	cargo build --release

install: ## Install the local build onto PATH (~/.cargo/bin)
	cargo install --path .

demo: ## Run the toy manifest end-to-end (plan)
	cargo run -- -m examples/demo/kitout.toml plan

plan: ## Preview what a release would produce
	dist plan

hooks: ## Install the commit-msg hook (conventional commits enforcement)
	git config core.hooksPath .githooks
	@echo "hooks installed (core.hooksPath = .githooks)"

version: ## Suggest the next version from conventional commits
	@git cliff --bumped-version

changelog: ## Regenerate CHANGELOG.md from commit history
	git cliff -o CHANGELOG.md

release: ## Cut a release: make release VERSION=X.Y.Z (CI does the rest)
	@test -n "$(VERSION)" || { echo "usage: make release VERSION=X.Y.Z  (suggested: $$(git cliff --bumped-version 2>/dev/null | sed s/^v//))"; exit 1; }
	@echo "$(VERSION)" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "VERSION must be X.Y.Z"; exit 1; }
	@git diff --quiet && git diff --cached --quiet || { echo "working tree not clean — commit or stash first"; exit 1; }
	@! git rev-parse "v$(VERSION)" >/dev/null 2>&1 || { echo "tag v$(VERSION) already exists"; exit 1; }
	$(MAKE) check
	sed -i '' 's/^version = ".*"/version = "$(VERSION)"/' Cargo.toml
	cargo build --release
	git cliff --tag "v$(VERSION)" -o CHANGELOG.md
	git add Cargo.toml Cargo.lock CHANGELOG.md
	git commit -m "chore(release): v$(VERSION)"
	git tag "v$(VERSION)"
	git push origin main "v$(VERSION)"
	@echo "→ v$(VERSION) is in CI's hands: strict gate → binaries → GitHub Release → tap → crates.io"
	@echo "  watch: gh run list --repo beaubutton/kitout"
