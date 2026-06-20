# Homepage Product Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the site homepage into the approved product-style C direction and fix the homepage language/link regressions.

**Architecture:** Keep the existing Astro + Starlight site. The custom homepage remains in `site/src/pages/index.astro`, shared theme and landing styles remain in `site/src/styles/global.css`, and lightweight static checks remain in `site/scripts/check-homepage-i18n.mjs`.

**Tech Stack:** Astro 4, Starlight 0.29, TypeScript/Astro, plain CSS, Node script checks.

---

## File Structure

- Modify `site/scripts/check-homepage-i18n.mjs`: extend the existing homepage smoke check to catch the language display override bug, product-direction copy, and primary link expectations.
- Modify `site/src/pages/index.astro`: restructure the homepage around the approved product-style hero, runtime visual, product surfaces, narrative sections, and docs grid.
- Modify `site/src/styles/global.css`: replace the fragile landing layout with a normal in-flow header, resilient language hiding, product hero styling, responsive runtime visual, and mobile-safe sections.

## Task 1: Add Failing Homepage Checks

**Files:**
- Modify: `site/scripts/check-homepage-i18n.mjs`

- [ ] **Step 1: Extend the check script before changing production files**

Replace `site/scripts/check-homepage-i18n.mjs` with a version that still checks bilingual content, and also checks for:

- approved product-direction copy,
- runtime visual tokens,
- no CSS rule that can override hidden language spans in surface/product areas,
- expected homepage links.

- [ ] **Step 2: Run the check to verify RED**

Run:

```bash
cd site && npm run check:i18n
```

Expected: fail because the current homepage does not yet contain the approved product-direction copy and the current `.surface-strip span` CSS can override language hiding.

## Task 2: Implement Product Homepage and CSS Fix

**Files:**
- Modify: `site/src/pages/index.astro`
- Modify: `site/src/styles/global.css`

- [ ] **Step 1: Rewrite homepage structure**

Update `site/src/pages/index.astro` so it contains:

- in-flow header,
- product-style hero,
- runtime flow visual,
- product surfaces,
- control/runtime/extension narrative sections,
- CLI/library code examples,
- docs entry grid.

Keep the existing `LocalizedText.astro` component and `href()` helper.

- [ ] **Step 2: Replace landing CSS**

Update `site/src/styles/global.css` so:

- language hiding is resilient with `[data-lang-copy] { display: none !important; }` for the inactive language,
- the header no longer depends on `main { margin-top: -78px; }`,
- mobile layouts use single-column grids and do not overlap,
- the palette is product-like but not one-note.

- [ ] **Step 3: Run the homepage check to verify GREEN**

Run:

```bash
cd site && npm run check:i18n
```

Expected: pass.

## Task 3: Build and Type Verification

**Files:**
- Read generated output under `site/dist/`

- [ ] **Step 1: Run Astro type check**

Run:

```bash
cd site && npm run check
```

Expected: 0 errors.

- [ ] **Step 2: Run Astro build**

Run:

```bash
cd site && npm run build
```

Expected: build exits 0 and produces `site/dist/index.html` plus Starlight docs routes.

- [ ] **Step 3: Re-run homepage smoke check after build**

Run:

```bash
cd site && npm run check:i18n
```

Expected: pass, including built route checks.

## Task 4: Review and Commit

**Files:**
- Review: `site/src/pages/index.astro`
- Review: `site/src/styles/global.css`
- Review: `site/scripts/check-homepage-i18n.mjs`

- [ ] **Step 1: Inspect diff**

Run:

```bash
git diff -- site/src/pages/index.astro site/src/styles/global.css site/scripts/check-homepage-i18n.mjs
```

Expected: only homepage, CSS, and check script changes.

- [ ] **Step 2: Commit implementation**

Run:

```bash
git add site/src/pages/index.astro site/src/styles/global.css site/scripts/check-homepage-i18n.mjs docs/superpowers/plans/2026-06-20-homepage-product-polish.md
git commit -m "site: polish product homepage"
```

Expected: commit succeeds.
