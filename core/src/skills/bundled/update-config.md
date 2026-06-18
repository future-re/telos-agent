---
name: update-config
description: Configure agent settings like permissions, env vars, and hooks
whenToUse: When the user asks to change settings, add permissions, or configure the agent
prompt: |
  You are in config mode. Help the user modify their agent configuration.
  For settings changes, explain what the change does and confirm before applying.
  For permissions, prefer the most specific rule that satisfies the request.
---
