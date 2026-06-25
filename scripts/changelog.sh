#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v git-cliff &> /dev/null; then
  echo >&2 "error: git-cliff is not installed."
  echo >&2 "  Install: cargo install git-cliff"
  echo >&2 "  Or:       brew install git-cliff"
  echo >&2 "  Or:       pacman -S git-cliff"
  exit 1
fi

case "${1:-}" in
  preview)
    echo "Previewing changelog from last tag to HEAD..."
    echo "---"
    git cliff --unreleased "${@:2}"
    ;;
  release)
    echo "Bumping version and updating CHANGELOG.md..."
    NEXT_VERSION=$(git cliff --bumped-version)
    git cliff --bump --unreleased --prepend CHANGELOG.md
    git tag "v$NEXT_VERSION"
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
