# Changelog Automation Branch -- Fix Report

## Summary

Applied 5 fixes across 3 files to address review findings on the changelog automation branch.

## Files Modified

### `scripts/changelog.sh`
- **Fix 1 (Critical): release subcommand broken**
  - Added `NEXT_VERSION=$(git cliff --bumped-version)` to get the version
  - Changed `--bump --tag --unreleased` to `--bump --unreleased --prepend CHANGELOG.md` (writes to CHANGELOG.md instead of stdout only; `--tag` was consuming `--unreleased` as its value in git-cliff 2.x)
  - Added `git tag "v$NEXT_VERSION"` to actually create the git tag
- **Fix 3 (Important): Remove unnecessary `--current` from preview**
  - Changed `git cliff --unreleased --current` to `git cliff --unreleased`
- **Fix 4 (Minor): Error output should use stderr**
  - All 4 error/warning echos now use `echo >&2` instead of bare `echo`

### `cliff.toml`
- **Fix 2 (Important): Add GitHub comparison links**
  - Added `[git.github]` section with `owner = "future-re"` and `repo = "tiny_agent_core"`

### `.github/workflows/release.yml`
- **Fix 5 (Minor): Consistent actions/checkout version**
  - Changed `actions/checkout@v4` to `actions/checkout@v5` to match `release-desktop.yml`

## Verification

- All 3 files read back and confirmed correct after edits
- Shell syntax verified by inspection (no syntax errors)
- `git diff` shows only the intended changes
