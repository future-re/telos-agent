# PyPI `telos-cli` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Python packaging and release automation so `pip install telos-cli` installs the Rust `telos` CLI.

**Architecture:** Keep Rust crates unchanged and add a root `pyproject.toml` for maturin. The wheel builds the existing `cli` binary and includes the sibling `core` crate through the workspace path dependency. Release automation builds platform wheels on GitHub Actions and publishes them to PyPI by Trusted Publishing.

**Tech Stack:** Rust, Cargo, Python packaging, maturin, GitHub Actions, PyPI Trusted Publishing

---

## File Map

| File | Responsibility |
|------|----------------|
| `pyproject.toml` | Python package metadata and maturin binary build config |
| `.github/workflows/pypi.yml` | Build wheels and publish on version tags |
| `README.md` | Document pip install as a supported installation path |
| `cli/README.md` | Document pip install for CLI users |
| `docs/superpowers/specs/2026-06-20-pypi-telos-cli-design.md` | Design record |
| `docs/superpowers/plans/2026-06-20-pypi-telos-cli.md` | Implementation plan |

### Task 1: Add Python package metadata

**Files:**
- Create: `pyproject.toml`

- [x] **Step 1: Add maturin build metadata**

Create `pyproject.toml` with:

```toml
[build-system]
requires = ["maturin>=1.9,<2"]
build-backend = "maturin"

[project]
name = "telos-cli"
dynamic = ["version"]
description = "Terminal interface for telos-agent"
readme = "README.md"
requires-python = ">=3.8"
license = "MIT"
keywords = ["agent", "ai", "cli", "llm", "terminal"]
classifiers = [
    "Development Status :: 3 - Alpha",
    "Environment :: Console",
    "Intended Audience :: Developers",
    "License :: OSI Approved :: MIT License",
    "Programming Language :: Rust",
    "Topic :: Software Development",
    "Topic :: Terminals",
]

[project.urls]
Repository = "https://github.com/future-re/telos-agent"
Changelog = "https://github.com/future-re/telos-agent/blob/main/CHANGELOG.md"

[tool.maturin]
manifest-path = "cli/Cargo.toml"
bindings = "bin"
strip = true
sdist-include = ["Cargo.lock", "Cargo.toml", "core/**/*", "cli/**/*", "LICENSE", "README.md"]
```

- [x] **Step 2: Verify metadata builds**

Run:

```bash
python3 -m venv .venv-pypi-build
. .venv-pypi-build/bin/activate
python -m pip install --upgrade pip build maturin
python -m build --wheel
```

Expected: a wheel appears in `dist/`.

### Task 2: Add PyPI release workflow

**Files:**
- Create: `.github/workflows/pypi.yml`

- [ ] **Step 1: Add wheel build and publish workflow**

Create `.github/workflows/pypi.yml` with:

```yaml
name: PyPI

on:
  push:
    tags:
      - "v*.*.*"
  workflow_dispatch:

permissions:
  contents: read

jobs:
  build-wheels:
    name: Build wheels (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: PyO3/maturin-action@v1
        with:
          command: build
          args: --release --out dist --compatibility pypi
          manylinux: "2014"
      - uses: actions/upload-artifact@v4
        with:
          name: wheels-${{ matrix.os }}
          path: dist

  build-sdist:
    name: Build sdist
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: PyO3/maturin-action@v1
        with:
          command: sdist
          args: --out dist
      - uses: actions/upload-artifact@v4
        with:
          name: sdist
          path: dist

  publish:
    name: Publish to PyPI
    needs: [build-wheels, build-sdist]
    runs-on: ubuntu-latest
    if: startsWith(github.ref, 'refs/tags/v')
    environment:
      name: pypi
      url: https://pypi.org/p/telos-cli
    permissions:
      id-token: write
      contents: read
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true
      - uses: pypa/gh-action-pypi-publish@release/v1
```

- [ ] **Step 2: Verify workflow syntax**

Run:

```bash
python - <<'PY'
from pathlib import Path
import yaml
yaml.safe_load(Path(".github/workflows/pypi.yml").read_text())
print("workflow yaml ok")
PY
```

Expected: `workflow yaml ok`

### Task 3: Document pip installation

**Files:**
- Modify: `README.md`
- Modify: `cli/README.md`

- [ ] **Step 1: Add installation snippets**

Add `pip install telos-cli` before the Cargo install path in both README files.

- [ ] **Step 2: Verify docs mention the package and command**

Run:

```bash
rg -n "pip install telos-cli|telos --help|cargo install telos-cli" README.md cli/README.md
```

Expected: both README files mention `pip install telos-cli`.

### Task 4: End-to-end local verification

**Files:**
- No source changes.

- [ ] **Step 1: Build and install wheel in a clean venv**

Run:

```bash
rm -rf dist .venv-pypi-install
python -m build --wheel
python -m venv .venv-pypi-install
. .venv-pypi-install/bin/activate
python -m pip install dist/*.whl
telos --help
```

Expected: `telos --help` exits successfully and prints the CLI usage.

- [ ] **Step 2: Run Rust CLI tests**

Run:

```bash
cargo test -p telos-cli --no-fail-fast
```

Expected: all CLI tests pass.
