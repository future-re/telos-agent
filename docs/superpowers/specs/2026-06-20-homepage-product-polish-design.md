# Homepage Product Polish Design

## Purpose

Update the `site/` homepage from a cramped documentation splash into a more
product-style landing page for telos, while keeping developer entry points
visible and fixing confirmed homepage bugs.

## Chosen Direction

Use the approved **C: Product-style landing page** direction.

The homepage should feel more branded and memorable than a plain docs portal,
but it must still function as the front door to the documentation. The first
screen should immediately show:

- The `telos` name and runtime positioning.
- Primary paths to Quick Start / Docs and Rust API.
- A concise runtime visual that explains model turns, approvals, tools, and
  results.
- Install or CLI context without overwhelming the hero.

## Scope

In scope:

- Rewrite `site/src/pages/index.astro` homepage structure around a stronger
  product narrative.
- Refine `site/src/styles/global.css` for the new hero, runtime visual,
  product sections, docs entry grid, and mobile layouts.
- Fix the language duplication bug where both English and Chinese copies can
  display in the surface strip.
- Add automated homepage checks for language duplication risk and primary
  route/link expectations.
- Keep the existing Astro + Starlight setup and docs content.

Out of scope:

- Replacing Starlight.
- Rewriting all documentation pages.
- Adding a custom visual asset pipeline.
- Changing package publishing or crate release behavior.

## Homepage Structure

### Header

Use a normal in-flow header instead of relying on negative-margin overlay
behavior. Keep navigation compact:

- Product / Docs / API / GitHub or equivalent high-value links.
- Language switch remains visible without crowding mobile layouts.

### Hero

Create a product-style first screen:

- Strong headline focused on controlled autonomy or safe tool use.
- Supporting copy describing telos as a Rust runtime boundary for model turns,
  tools, approvals, MCP, plugins, memory, and clients.
- Primary action to start building / Quick Start.
- Secondary action to Rust API or docs.
- A runtime visual panel that shows the flow:
  prompt -> model -> approval -> tools -> result.

### Product Surfaces

Replace the repeated chip strip with a cleaner set of product surfaces:

- CLI / TUI workflow.
- Rust library integration.
- MCP and plugins.
- Desktop/client direction.

Each surface should render only the active language copy.

### Narrative Sections

Use short sections that explain why telos exists:

- Control boundary for side effects.
- Runtime loop visibility.
- Extension surface for MCP and plugins.
- Product/client fit.

The copy should stay practical and avoid generic marketing claims.

### Documentation Entry

Keep a strong docs grid near the lower half of the page:

- Quick Start.
- Library API Guide.
- CLI Guide.
- Configuration.
- Plugins and MCP.
- Rust API Reference.

## Bug Fixes

### Language Duplication

Root cause: `LocalizedText.astro` renders both language spans. The global
language hiding selector can be overridden by later, more specific CSS such as
`.landing .surface-strip span { display: inline-flex; }`.

Fix by making the language hiding rule resilient against later component-level
span display styles, and add a check that catches this class of regression.

### Local Link Verification

The deployed Pages workflow copies generated rustdoc into
`site/dist/api/rust/`, but a plain local `npm run build` does not. Homepage
checks should distinguish between Starlight routes that must exist after
`npm run build` and generated Rust API docs that require the wider Pages build
flow.

## Testing

Run after implementation:

```bash
cd site && npm run check
cd site && npm run check:i18n
cd site && npm run build
```

Add or extend a lightweight Node check so it verifies:

- Homepage contains the language switch and both English/Chinese copy.
- Language hiding CSS cannot be overridden by the current homepage span styles.
- Primary Starlight links referenced by the homepage exist in `site/dist` after
  `npm run build`.
- Rust API links are treated as generated-doc links, not as plain Astro routes.

Manual browser check:

- Desktop first viewport presents the product-style C direction.
- Mobile header, hero, runtime visual, and docs grid do not overlap.
- Switching to Chinese does not show duplicated English labels.
