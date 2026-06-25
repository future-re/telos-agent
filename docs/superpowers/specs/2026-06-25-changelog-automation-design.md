# Changelog Automation Design

**Status:** approved
**Date:** 2026-06-25

## Goal

Automate CHANGELOG.md generation and semver version bumping using
[git-cliff](https://github.com/orhun/git-cliff), integrated into both
local scripting and GitHub Actions CI.

## Architecture

```
                    cliff.toml
                        │
     ┌──────────────────┼──────────────────┐
     ▼                                     ▼
scripts/changelog.sh               .github/workflows/release.yml
 (preview / release)                 (tag push → GitHub Release)
```

A single `cliff.toml` configuration drives both local and CI workflows.

## Design Decisions

- **Local controls release timing**: the developer decides when to release.
  CI only creates the GitHub Release page after a tag is pushed.
- **git-cliff chosen over release-please and custom scripts**: git-cliff is
  Rust-native (same ecosystem as the project), configurable, and works both
  locally and in CI without being tied to GitHub.
- **Version bumping is automated**: git-cliff computes the semver bump from
  conventional commit types. `BREAKING CHANGE` → major, `feat` → minor,
  `fix`/`perf`/`refactor` → patch.

## Deliverables

| File | Purpose |
|---|---|
| `cliff.toml` | Configuration: commit → category mapping, semver rules, output template |
| `scripts/changelog.sh` | Local wrapper: `preview` (dry-run) and `release` (bump + tag + update changelog) |
| `.github/workflows/release.yml` | CI: on `v*` tag push, create GitHub Release with changelog body |

## cliff.toml Design

### Commit Classification

Commit parsers align with the existing CHANGELOG.md naming conventions:

| Conventional commit | Changelog section |
|---|---|
| `feat:` | `### Added` |
| `fix:` | `### Fixed` |
| `refactor:`, `perf:` | `### Changed` |
| `BREAKING CHANGE` (body) | `### Breaking` |
| `docs:`, `style:`, `test:`, `chore:`, `ci:` | skipped |

### Semver Bump Rules

| Trigger | Bump |
|---|---|
| `BREAKING CHANGE` in body | major |
| `feat:` | minor |
| `fix:`, `perf:`, `refactor:` | patch |

### Template

Output follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format
with GitHub comparison links.

## Local Script Usage

```
# Preview the changelog before releasing (does not modify anything)
./scripts/changelog.sh preview

# Bump version, update CHANGELOG.md, and create a git tag
./scripts/changelog.sh release

# Push code and tags to trigger CI release
git push --follow-tags
```

## CI Integration

`.github/workflows/release.yml` triggers on `v*` tag push (`v0.1.0`, `v1.0.0`, etc.):

1. Checks out the repo with full history (`fetch-depth: 0`)
2. Runs `git-cliff` targeting the pushed tag to generate changelog content
3. Creates a GitHub Release via `softprops/action-gh-release` with the
   generated changelog as the release body

## Workflow Diagram

```
Development
    │
    │  Write conventional commits (feat:, fix:, etc.)
    ▼
./scripts/changelog.sh preview   → review what will go in the changelog
    │
    ▼
./scripts/changelog.sh release   → bump version + update CHANGELOG.md + git tag
    │
    ▼
git push --follow-tags           → push code + tag
    │
    ▼
GitHub Actions                    → generate changelog + create GitHub Release
```

## Scope / Non-goals

- **In scope**: single unified changelog for the entire workspace (not per-crate)
- **In scope**: Keep a Changelog format output
- **Out of scope**: per-crate changelogs, changelog linting in CI, commit message
  validation in pre-commit hooks
