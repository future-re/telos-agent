# Changelog Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automate CHANGELOG.md generation and semver version bumping via git-cliff, with a local script for preview/release and a CI workflow for GitHub Releases.

**Architecture:** A single `cliff.toml` configuration drives both local and CI workflows. The local `scripts/changelog.sh` wrapper provides `preview` (dry-run) and `release` (bump + tag) subcommands. The `.github/workflows/release.yml` triggers on `v*.*.*` tag push to create a GitHub Release with git-cliff-generated body.

**Tech Stack:** git-cliff (Rust binary), bash, GitHub Actions (`orhun/git-cliff-action`, `softprops/action-gh-release`)

## Global Constraints

- Keep a Changelog 1.1.0 format output
- Conventional Commits mapping: `feat`→Added, `fix`→Fixed, `refactor`/`perf`→Changed, `BREAKING CHANGE`→Breaking
- `docs`, `style`, `test`, `chore`, `ci` commits are skipped in changelog
- Semver: BREAKING CHANGE→major, feat→minor, fix/perf/refactor→patch
- Tag pattern: `v*.*.*` (matching existing `release-desktop.yml` convention)
- Local script uses `set -euo pipefail` (matching existing `scripts/generate-core-api-docs.sh`)

---

### Task 1: Create `cliff.toml` — git-cliff configuration

**Files:**
- Create: `cliff.toml`

**Interfaces:**
- Produces: `cliff.toml` consumed by local script (Task 2) and CI workflow (Task 3)

- [ ] **Step 1: Create `cliff.toml`**

```toml
[changelog]
header = "# Changelog\n\nAll notable changes to this project will be documented in this file.\n\nThe format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),\nand this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).\n"
body = """
{% if version != \"Unreleased\" %}\
## [{{ version }}] - {{ timestamp | date(format=\"%Y-%m-%d\") }}
{% else %}\
## [Unreleased]
{% endif %}\

{% for group, commits in commits | group_by(attribute=\"group\") %}\
### {{ group }}
{%- for commit in commits %}\
- {{ commit.conventional.description | upper_first | trim }}
{% endfor %}

{% endfor %}\
"""
trim = true

[git]
conventional_commits = true
filter_commits = true
tag_pattern = "v[0-9]+.*"

commit_parsers = [
  { body = "BREAKING[ -]CHANGE", group = "Breaking" },
  { message = "^feat", group = "Added" },
  { message = "^fix", group = "Fixed" },
  { message = "^refactor", group = "Changed" },
  { message = "^perf", group = "Changed" },
  { message = "^docs", skip = true },
  { message = "^style", skip = true },
  { message = "^test", skip = true },
  { message = "^chore", skip = true },
  { message = "^ci", skip = true },
]

[semver]
bump_major = ["BREAKING CHANGE"]
bump_minor = ["feat"]
bump_patch = ["fix", "perf", "refactor"]
```

- [ ] **Step 2: Verify git-cliff parses the config correctly**

Run: `git cliff --unreleased --current`

Expected: Prints a preview of the Unreleased changelog section with commits since `v0.1.0`, grouped by category. No Tera template errors.

- [ ] **Step 3: Verify the semver bump calculation**

Run: `git cliff --bumped-version`

Expected: Prints the next version (e.g., `0.2.0`) based on commits since last tag.

- [ ] **Step 4: Commit**

```bash
git add cliff.toml
git commit -m "feat: add git-cliff config for automated changelog generation

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Create `scripts/changelog.sh` — local wrapper script

**Files:**
- Create: `scripts/changelog.sh`

**Interfaces:**
- Consumes: `cliff.toml` from Task 1
- Produces: `preview` and `release` subcommands for developer workflow

- [ ] **Step 1: Create the script**

```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v git-cliff &> /dev/null; then
  echo "error: git-cliff is not installed."
  echo "  Install: cargo install git-cliff"
  echo "  Or:       brew install git-cliff"
  echo "  Or:       pacman -S git-cliff"
  exit 1
fi

case "${1:-}" in
  preview)
    echo "Previewing changelog from last tag to HEAD..."
    echo "---"
    git cliff --unreleased --current "${@:2}"
    ;;
  release)
    echo "Bumping version and updating CHANGELOG.md..."
    git cliff --bump --tag --unreleased "${@:2}"
    echo ""
    echo "Done. Review CHANGELOG.md, then push with:"
    echo "  git push --follow-tags"
    ;;
  *)
    echo "Usage: $0 {preview|release}"
    echo ""
    echo "  preview   Preview changelog from current HEAD (dry-run, no changes)"
    echo "  release   Bump version, update CHANGELOG.md, and create git tag"
    exit 1
    ;;
esac
```

- [ ] **Step 2: Make the script executable**

Run: `chmod +x scripts/changelog.sh`

- [ ] **Step 3: Verify preview mode works**

Run: `./scripts/changelog.sh preview`

Expected: Prints changelog preview for unreleased commits. No files modified. Error message if git-cliff is not installed.

- [ ] **Step 4: Commit**

```bash
git add scripts/changelog.sh
git commit -m "feat: add changelog script with preview and release subcommands

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Create `.github/workflows/release.yml` — CI changelog release

**Files:**
- Create: `.github/workflows/release.yml`

**Interfaces:**
- Consumes: `cliff.toml` from Task 1
- Produces: GitHub Release with changelog body on `v*.*.*` tag push

- [ ] **Step 1: Create the workflow file**

```yaml
name: Create Release

on:
  push:
    tags: ['v*.*.*']

jobs:
  release:
    runs-on: ubuntu-latest
    permissions:
      contents: write
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Generate changelog
        uses: orhun/git-cliff-action@v4
        with:
          config: cliff.toml
          args: --latest --tag ${{ github.ref_name }} --output CHANGELOG_RELEASE.md

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          name: "${{ github.ref_name }}"
          body_path: CHANGELOG_RELEASE.md
          draft: false
          prerelease: false
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
```

- [ ] **Step 2: Validate YAML syntax**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"`

Expected: No output (parse succeeds). If `pyyaml` is not available, this step may be skipped — GitHub will validate on push.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat: add changelog-driven GitHub Release workflow

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Verification Checklist

After all 3 tasks are complete, verify end-to-end:

1. `./scripts/changelog.sh preview` — shows unreleased commits grouped by category
2. `git cliff --bumped-version` — prints correct next version
3. CI workflow file is valid YAML
4. All 3 commits are on the branch
