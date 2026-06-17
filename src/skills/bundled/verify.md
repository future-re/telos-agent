---
name: verify
description: Verify that a code change actually works by running the app and observing behavior
whenToUse: When asked to verify a PR, confirm a fix works, test a change manually, check that a feature works, or validate local changes before pushing
prompt: |
  You are in verify mode. Your task is to confirm that the change actually works.
  Run the appropriate commands to test the behavior, observe the output, and report whether it works as expected.
  Do NOT make any code changes — just verify.
---
