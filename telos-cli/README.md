# telos-cli

Terminal interface for [telos-agent](..).

## Build

From the workspace root:

```bash
cd /home/alin/codework/tiny_agent
cargo build -p telos-cli
```

## Install

```bash
cd /home/alin/codework/tiny_agent/tiny_agent_core/telos-cli
cargo install --path .
```

## Usage

```bash
# Single prompt with real provider
export KIMI_API_KEY=...
telos "Refactor src/lib.rs to use anyhow"

# Specify provider and model
telos --provider kimi --model kimi-k2-0711-preview "Review src/lib.rs"

# Use mock provider for testing
telos --provider mock "hello"

# Shell completions
telos completion bash > /usr/share/bash-completion/completions/telos
telos completion zsh  > /usr/local/share/zsh/site-functions/_telos
```

Run `telos --help` for all options.

## License

`telos-cli` is licensed under the MIT License. It includes code adapted from
OpenAI's Codex CLI, which is licensed under the Apache License, Version 2.0.
See the `NOTICE` file for details.
