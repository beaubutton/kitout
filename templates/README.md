# Workstation templates

Persona-based starting points for a kitout machine. Each `templates/<persona>/`
is a complete, trimmable config — a `kitout.toml` plus a `Brewfile` and a
`skills/manifest`. Copy one, delete the sub-categories you don't want, and
`kitout apply` (or let `kitout create-config <dir> --type <persona>` scaffold it
for you).

> **Skills are starting points, not blessed pins.** The `skills/manifest` in
> each template lists the best skill per sub-category (researched from
> [skills.sh](https://www.skills.sh), ranked by install count + recency). They
> use the un-pinned `owner/repo:path` form. **Run each through the `add-skill`
> review to resolve a reviewed commit SHA and confirm agent compatibility
> before you rely on it.** Research-captured SHAs are noted below for
> convenience.

## The shared base

Every template's `Brewfile` opens with the same base — the tools any modern
agent-era workstation wants — then adds persona tools below it:

```
git gh ripgrep fd bat eza fzf jq starship uv node
cask: ghostty visual-studio-code claude-code
```

And every `skills/manifest` opens with agent-agnostic process skills that
aren't persona-specific: `brainstorming` (obra/superpowers), `grill-me` /
`tdd` (mattpocock/skills). There's no inheritance mechanism — the base is just
repeated in each file (duplication is fine for starting points).

## The dozen

| Persona | Sub-categories | Flagship skill (installs) | Signature MCP |
|---------|----------------|---------------------------|---------------|
| **backend** | .NET, TypeScript/Node, Go, Python, Java/Kotlin, Rust | `wshobson/agents:…/nodejs-backend-patterns` (39K) | Context7, postgres |
| **frontend** | React/Next, Vue/Nuxt, Svelte, Angular | `vercel-labs/agent-skills:…/react-best-practices` (533K) | Playwright |
| **ai-llm** | Claude apps, agents/MCP, RAG, eval | `anthropics/skills:…/mcp-builder` (86K) | qdrant, langfuse |
| **ml-data-science** | PyTorch/MLX, notebooks, classical ML, MLOps | `probabl-ai/skills:…/build-ml-pipeline` | jupyter, huggingface |
| **data-engineering** | SQL/warehouse, dbt, streaming, orchestration | `supabase/agent-skills:…/postgres-best-practices` (273K) | dbt, duckdb |
| **devops** | Kubernetes, CI/CD, Terraform, containers, observability | `sickn33/…:docker-expert` (23K) | kubernetes, grafana |
| **cloud** | AWS, Azure, GCP, multi-cloud IaC | `microsoft/azure-skills` (10.2M repo) | azure, terraform |
| **game** | Godot/C#, Unity/C#, Unreal/C++, web games | `gamedev-skills/awesome-gamedev-agent-skills` | godot, unity, blender |
| **creative** | gen-image, gen-audio, video/motion, creative coding | `remotion-dev/skills:…/remotion` (413K) | comfyui, blender |
| **mobile** | iOS/Swift, Android/Kotlin, React Native, Flutter | `vercel-labs/agent-skills:…/react-native` (160K) | XcodeBuild, mobile-mcp |
| **security** | AppSec/SAST, cloud sec, container sec, defensive | `getsentry/skills:…/security-review` (10K) | semgrep, trivy |
| **systems** | Rust systems, C/C++, embedded, WASM | `jeffallan/claude-skills:…/embedded-systems` (5K) | rust-docs, embedded-debugger |

## Sourcing notes (from the research)

- **Prefer first-party skill families** even when installs trail community ones:
  `anthropics/skills`, `vercel-labs/agent-skills`, `microsoft/azure-skills`,
  `google/skills`, `angular/skills`, `sveltejs/ai-tools`, `expo/skills`,
  `flutter/skills`, `android/skills`, `dbt-labs`, `dagster-io`, `astronomer`,
  `duckdb`, `supabase`. `wshobson/agents` (37.6K★, audited) is the best
  cross-persona community set — it supplies backend + devops picks from one repo.
- **Homebrew name traps** encoded into the Brewfiles: `hashicorp/tap/terraform`
  (not core), `gcloud-cli` (not `google-cloud-sdk`), `open-ocd` (not `openocd`),
  `pkgconf` (not `pkg-config`), `docker-desktop` cask (not `docker`),
  `mobile-dev-inc/tap/maestro`. And `dbt` / `airflow` / `dagster` /
  vector DBs aren't Homebrew — they're `uv`/pip, so they appear as notes, not
  brew lines.
- **Thin skill spaces** (reported honestly, few/no high-install skills):
  generative-audio, Processing/openFrameworks, MLX, Kafka/Flink, modern-C++,
  WASM, detection-engineering. Those templates lean on the toolchain + MCP.
- **License/audit flags** for the review: `actionbook/rust-skills` has **no
  LICENSE**; `getsentry/skills/security-review` shows a Snyk audit failure.
