# Design: PyPI Distribution for `telos-cli`

## Problem

The project can be installed today through Cargo, but Python users cannot install
the CLI with `pip`. The repository is a Rust workspace: `cli` builds the `telos`
binary and depends on the sibling `core` crate. Publishing to PyPI therefore
needs a wheel that contains the Rust CLI binary, not a Python reimplementation.

## Goal

Publish a PyPI package named `telos-cli` so users can run:

```bash
pip install telos-cli
telos --help
```

## Chosen Approach

Use maturin's binary package support from the repository root. The Python
distribution name is `telos-cli`; the installed command remains `telos`.

This keeps the Rust crate layout intact:

- `cli/Cargo.toml` continues to define the `telos` binary.
- `core/Cargo.toml` continues to define the `telos_agent` Rust library.
- `pyproject.toml` only describes the Python wheel and points maturin at
  `cli/Cargo.toml`.

## Alternatives Considered

1. Publish a Python wrapper that shells out to a separately installed Cargo
   binary. This makes installation fragile because users would still need Rust.
2. Expose `core` as a Python module through PyO3. This is larger scope and would
   create a Python API commitment that the project has not designed yet.
3. Publish only to crates.io. This does not satisfy Python installation.

## Release Flow

GitHub Actions builds wheels for Linux, macOS, and Windows on version tags. PyPI
publishing uses Trusted Publishing so the workflow does not need a long-lived
PyPI token.

## Verification

The minimum local verification is:

```bash
python -m build --wheel
python -m pip install --force-reinstall dist/*.whl
telos --help
```

CI should also keep running Rust tests through the existing workflow.
