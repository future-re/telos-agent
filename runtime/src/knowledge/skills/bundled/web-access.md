---
name: web-access
description: Source-first web research and browser automation workflow.
whenToUse: |
  Use when a task needs current web information, dynamic pages, local URL discovery,
  or browser interaction without relying on paid search APIs.
prompt: |
  Use a source-first web workflow for: {{args}}

  - If a likely official/source URL is known, call WebFetch first.
  - If discovery is needed, use WebSearch once, then fetch or browse the best sources.
  - Prefer BrowserNavigate and BrowserState for JavaScript-heavy pages, login-gated flows the user explicitly permits, forms, screenshots, or multi-step navigation.
  - Use BrowserState before BrowserClick/BrowserType so element_id values are current.
  - Use allowed_domains when the task is scoped to a site.
  - Use BrowserFindUrl only when the user has approved reading local bookmark/history metadata.
  - Do not try to bypass CAPTCHA, bot checks, paywalls, or access controls. If blocked, explain the block and ask for a source or manual help.
  - Keep claims tied to sources. For browser-only observations, include the page URL and screenshot/path when useful.
---

## Strategy

1. Start with known sources. Official docs, project repositories, vendor pages, standards, and primary publications are preferred over search snippets.
2. Use search as discovery, not truth. After WebSearch returns candidates, open the relevant source with WebFetch or BrowserNavigate.
3. Escalate to browser automation only when static fetch is insufficient: dynamic content, interactive forms, pages that require rendering, or visual verification.
4. Keep browser sessions scoped. Use `allowed_domains` for focused tasks and close sessions when finished.
5. Treat local browser metadata as private. Do not call BrowserFindUrl unless the user asked for it or approved it.
