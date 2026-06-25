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
