# CLI Startup Update Check Design

## Context

The `telos` CLI is distributed as a Rust binary through crates.io and as a Python wheel through PyPI. The CLI should notify users when either registry has a newer `telos-cli` version, but it must not replace binaries itself or require GitHub Release assets.

## Goals

- Check crates.io and PyPI for newer `telos-cli` versions during normal CLI startup.
- Recommend package-manager-native upgrade commands only.
- Avoid repeated registry traffic by caching check results for 24 hours.
- Keep normal CLI behavior working when the network is offline, slow, rate-limited, or either registry is unavailable.
- Suppress notices in quiet mode and when `TELOS_DISABLE_UPDATE_CHECK=1` is set.

## Non-Goals

- Self-update the executable.
- Query GitHub Releases.
- Add desktop updater behavior.
- Detect which installer produced the current executable.

## Behavior

On startup, before dispatching chat or prompt execution, the CLI attempts an update check unless disabled. Completion generation is skipped because completion scripts should be deterministic and fast.

The checker reads a JSON cache from the user's cache directory under `telos/update-check.json`. If the cache is newer than 24 hours, it uses the cached registry results. If the cache is missing or stale, it queries crates.io and PyPI independently with a short HTTP timeout. A failure from one registry does not hide the other registry's result.

If either registry reports a version newer than `CARGO_PKG_VERSION`, the CLI prints a concise stderr notice with the current version, the newer registry version, and the recommended command:

- crates.io: `cargo install --force telos-cli`
- PyPI: `pip install -U telos-cli`

If no registry reports a newer version, the CLI prints nothing.

## Architecture

All update-check logic lives in `cli/src/update_check.rs`. The public startup function takes the current version and quiet flag, handles environment/config gating, loads or refreshes cache data, and prints notices. The registry fetch functions return small typed results so tests can exercise version comparison and failure handling without network calls.

The CLI entrypoint calls the update checker immediately after parsing CLI arguments and before config loading. This keeps the behavior consistent across chat and single-prompt runs while avoiding the completion subcommand.

## Error Handling

Update-check errors are non-fatal. Cache read/write failures, invalid registry payloads, request errors, timeout errors, and semver parse failures only suppress that registry result for the current run. Normal CLI execution continues.

## Testing

Unit tests cover:

- semantic version comparison with a leading `v` accepted from registry-like values;
- no notice when versions are current or older;
- one registry failure while the other still reports a newer version;
- cache freshness decisions;
- environment and quiet-mode gating.

Integration tests keep the existing CLI smoke tests and ensure quiet mode does not emit update notices.
