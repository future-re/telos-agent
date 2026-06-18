# Design: Rename `telos-cli/` to `cli/` and Restore Clean Git State
> **Historical note:** This document describes the project state before the
> `telos-cli/` → `cli/` rename and before the root workspace was added.
> Some paths and commands may be outdated.


## Problem

The working directory currently has a dirty git status:

- `telos-cli/Cargo.toml`, `telos-cli/src/*.rs`, etc. are all marked as **deleted**.
- `cli/` is **untracked**.
- `.gitignore` has a local modification adding `.claude/`.
- A previous run also left a transient untracked `main` binary (now gone).

This makes the repository look broken and blocks clean builds/commits.

## Verification Already Performed

- `find cli -type f | wc -l` == `git ls-files telos-cli | wc -l` == 15 files.
- Byte-for-byte comparison of every `cli/$f` against `HEAD:telos-cli/$f` shows **zero differences**.
- Conclusion: the directory is a clean rename, not a content rewrite.

## Goal

Restore a clean `git status` while preserving git history for the renamed directory.

## Chosen Approach

**Treat `telos-cli/` → `cli/` as a git rename and commit it together with the `.gitignore` update.**

### Why this approach

- Keeps blame/history continuity for the CLI code.
- Minimal change: no source code edits required.
- Also commits the already-intended `.claude/` ignore rule.

### Alternatives considered

1. **Revert to `telos-cli/`** — would discard the current workspace state and require re-doing the rename later.
2. **Add `cli/` as new files and delete `telos-cli/` in separate commits** — loses rename detection and pollutes history.

## Steps

1. Stage removal of `telos-cli/`: `git rm -r telos-cli`
2. Stage addition of `cli/`: `git add cli`
3. Stage `.gitignore` update: `git add .gitignore`
4. Commit: `git commit -m "chore: rename telos-cli/ to cli/ and ignore .claude/"`
5. Verify `git status` is clean (apart from always-ignored build artifacts).
6. Run `cargo check` / `cargo test --workspace` to confirm the project still builds.

## Notes

- `.claude/` contains ~13 GB of agent worktrees and must remain ignored.
- `target/` and `.worktrees/` are already ignored.
- `cobertura.xml` is tracked in git but also listed in `.gitignore`; this is an existing quirk and out of scope for this cleanup.
