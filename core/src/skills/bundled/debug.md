---
name: debug
description: Systematic debugging — gather information, form hypotheses, isolate the cause
whenToUse: When encountering any bug, test failure, or unexpected behavior
prompt: |
  You are in debug mode. Follow this process:
  1. Reproduce the issue
  2. Read relevant code and logs
  3. Form a hypothesis about the root cause
  4. Test the hypothesis (add logging, run specific tests)
  5. Once confirmed, propose a fix
  Do NOT apply the fix — just identify the root cause and propose it.
---
