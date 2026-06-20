# Telos Website and Documentation Site Design

## Purpose

Build a GitHub Pages website for `telos-agent` that presents the project clearly
and keeps documentation synchronized with the Rust API surface.

The site should serve three jobs:

- Explain what `telos` is: a Rust agent runtime with CLI/TUI and future desktop
  client support.
- Give developers fast entry points for installation, library usage, CLI usage,
  configuration, plugins, MCP, and deployment.
- Publish generated Rust API docs automatically on each `main` push.

## Chosen Approach

Use **Astro + Starlight** under a new `site/` directory.

This is the preferred option because Starlight provides a strong documentation
shell, sidebar navigation, search, and static output without requiring a custom
documentation system. Astro also allows a custom landing page, so the homepage
can feel like a polished developer product instead of a generic docs template.

Rejected alternatives:

- **Docusaurus**: mature, but heavier for this Rust project and more React
  customization work than needed.
- **Pure Vite + React**: maximum control, but documentation navigation, search,
  sidebar, content loading, and static publishing would need to be built or
  integrated manually.

## Site Architecture

The website lives in `site/` and remains separate from the Rust workspace:

```text
site/
  astro.config.mjs
  package.json
  src/content/docs/
  src/pages/index.astro
  public/
```

Existing Rust documentation generation remains cargo-native. The build flow
generates rustdoc with:

```bash
cargo doc -p telos_agent --no-deps
```

The generated `target/doc` tree is copied into the static site output under:

```text
site/dist/api/rust/
```

Expected published URLs:

- Project website: `https://future-re.github.io/telos-agent/`
- Starlight docs: `https://future-re.github.io/telos-agent/docs/`
- Rust API docs: `https://future-re.github.io/telos-agent/api/rust/telos_agent/`

## Visual Direction

Use the approved "advanced developer tool" direction:

- Dark, restrained first screen with strong contrast and practical content.
- Green as the primary action color, with neutral warm text and borders.
- No decorative orb backgrounds, no one-note purple/blue gradients, and no
  marketing-only first screen.
- The first viewport must immediately show the project name, what it does,
  install commands, and two primary actions: quick start and Rust API docs.

The homepage should feel like a mature open-source infrastructure tool. It can
be visually polished, but the layout must prioritize scanning, credibility, and
fast developer onboarding.

## Homepage Structure

The homepage is a custom Astro page, not a generic Starlight doc page.

First viewport:

- Top navigation: `telos`, Docs, API, CLI, GitHub.
- Headline: position telos as a Rust runtime for agent clients that can think,
  call tools, and recover.
- Supporting copy: mention provider abstraction, tool registry, approvals, MCP,
  memory, plugins, subagents, and CLI/TUI.
- Install panel:
  - `cargo install telos-cli`
  - `cargo add telos_agent`
- Core API panel:
  - `AgentSession::new(config)`
  - `ToolRegistry::register(tool)`
  - `session.run_turn(&provider, &tools, prompt)`
- Primary actions:
  - Quick Start
  - Rust API Docs

Below the first viewport:

- Four capability blocks:
  - Core Runtime
  - Tools and Approvals
  - MCP and Plugins
  - CLI/TUI and Desktop Direction
- Short "how it works" section showing:
  - User input
  - Provider call
  - Tool execution
  - Tool result return
  - Final assistant response
- A final documentation entry section linking to the major docs pages.

## Documentation Structure

Create Starlight content around the current README and API audit work, starting
with a focused set of pages:

- Introduction
- Quick Start
- Core Concepts
- Library API Guide
- CLI Guide
- Desktop Client
- Configuration
- Plugins and MCP
- Deployment
- Rust API Reference

`docs/api/core-api.md` remains the source for the high-level core API guide
until the content is migrated into Starlight. The generated rustdoc remains the
source of truth for item-level Rust API reference.

## CI and Deployment

Add a GitHub Actions workflow dedicated to the website:

```text
.github/workflows/site.yml
```

Workflow behavior:

- Run on pushes to `main`.
- Run on pull requests for build verification.
- Check out the repo.
- Set up Node for the Astro site.
- Install site dependencies with `npm ci` inside `site/`.
- Set up the Rust toolchain.
- Generate Rust API docs with warnings denied where practical.
- Build the Astro/Starlight site.
- Copy `target/doc/*` into `site/dist/api/rust/`.
- Deploy `site/dist` to GitHub Pages only on `main` pushes.

The existing Rust workflow can continue to build and upload API docs as an
artifact. The site workflow owns public publishing.

## Testing and Verification

Local verification commands after implementation:

```bash
cd site && npm run build
./scripts/generate-core-api-docs.sh
```

Expected verification:

- Astro build succeeds.
- Starlight docs routes render.
- Rust API docs exist at `site/dist/api/rust/telos_agent/index.html`.
- Homepage is checked in desktop and mobile viewport screenshots.
- Links to Docs, API, CLI, GitHub, Quick Start, and Rust API docs resolve.

The existing Rust checks remain unchanged:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Scope Boundaries

In scope:

- New `site/` Astro + Starlight project.
- Custom homepage.
- Initial documentation pages.
- Rust API docs copied into the published site.
- GitHub Pages workflow.

Out of scope for the first pass:

- Custom domain setup.
- Versioned docs for multiple crate releases.
- Full visual brand system.
- Rewriting every rustdoc comment in the crate.
- Publishing crate version `0.1.1`.

## Open Implementation Notes

- Configure Astro `site` and `base` for GitHub Pages:
  - `site: "https://future-re.github.io"`
  - `base: "/telos-agent"`
- Use relative internal links or Astro-aware path helpers so links work under
  the GitHub Pages base path.
- Keep generated rustdoc outside source control.
- Do not commit `.superpowers/brainstorm/` visual exploration files.
