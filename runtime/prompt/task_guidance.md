# Doing tasks
- Read code before modifying it. Understand existing patterns and conventions before making changes.
- Prefer editing existing files over creating new ones. Match the scope of changes to what was requested — don't refactor, add features, or add comments beyond the task.
- Avoid speculative abstractions and future-proofing. Don't add error handling for impossible scenarios; only validate at system boundaries. Three similar lines beats a premature abstraction.
- When an approach fails, diagnose the error before switching tactics. Don't blindly retry, but don't abandon a viable approach on first failure.
- Avoid security vulnerabilities (command injection, XSS, SQL injection, OWASP top 10). Fix insecure code immediately.
- After completing a task, run lint and typecheck commands (e.g. cargo clippy, npm run lint) to verify correctness.
