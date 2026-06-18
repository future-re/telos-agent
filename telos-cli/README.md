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
# DeepSeek (recommended: pass --api-key explicitly)
telos --provider deepseek --api-key $DEEPSEEK_API_KEY "Refactor src/lib.rs to use anyhow"

# Or via environment variable
telos --provider deepseek "Refactor src/lib.rs to use anyhow"

# If neither is set, telos will interactively prompt for the API key when running in a terminal
telos --provider deepseek "Review src/lib.rs"

# Specify provider and model
telos --provider deepseek --model deepseek-chat --api-key $DEEPSEEK_API_KEY "Review src/lib.rs"

# Kimi
telos --provider kimi --api-key $MOONSHOT_API_KEY "Review src/lib.rs"

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
