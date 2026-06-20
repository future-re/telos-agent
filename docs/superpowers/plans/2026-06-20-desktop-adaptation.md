# Desktop Adaptation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the approved reinforced two-column desktop UI for the Tauri React client.

**Architecture:** Keep all behavior in the existing `App` component and reuse the current `settings`, `state`, and Tauri command flow. Update `desktop/src/main.tsx` to present derived run metadata and a right-side run panel, then update `desktop/src/styles.css` to make the desktop layout dense, readable, and responsive.

**Tech Stack:** React 19, TypeScript, Vite, Tauri 2, CSS.

---

## File Structure

- Modify `desktop/src/main.tsx`: add small derived display helpers, replace the old raw settings aside with a run panel, and move tool activity from the bottom strip into the right panel.
- Modify `desktop/src/styles.css`: restyle the desktop shell, top bar, conversation empty state, composer, run panel sections, tool activity list, and narrow viewport fallback.
- No new production files are needed.
- No backend files are touched.

## Task 1: Main React Structure

**Files:**
- Modify: `desktop/src/main.tsx`

- [ ] **Step 1: Add derived run metadata**

Add these constants inside `App`, immediately after `canSend`:

```tsx
  const providerLabel = settings.provider === "deepseek" ? "DeepSeek" : "Mock";
  const modelLabel = settings.model?.trim() || "auto";
  const cwdLabel = settings.cwd?.trim() || "App launch directory";
  const approvalLabel = settings.autoApprove ? "Auto-approved" : "Manual approval";
  const runMetadata = `${providerLabel} · ${modelLabel} · ${state.status}`;
```

- [ ] **Step 2: Replace the existing returned JSX**

Replace the entire `return (...)` block in `App` with this JSX. Keep the existing `submit`, `resetSession`, state setters, and `normalizeSettings` function unchanged.

```tsx
  return (
    <main className="app-shell">
      <section className="workspace">
        <header className="topbar">
          <div className="topbar-title">
            <h1>telos</h1>
            <span>{runMetadata}</span>
          </div>
          <div className="topbar-actions">
            <button
              type="button"
              className="secondary-action"
              onClick={() => setSettingsOpen((open) => !open)}
            >
              {settingsOpen ? "Hide Panel" : "Show Panel"}
            </button>
            <button type="button" className="secondary-action" onClick={resetSession}>
              New Chat
            </button>
          </div>
        </header>

        <section className="conversation" aria-live="polite">
          {state.messages.length === 0 ? (
            <div className="empty-state">
              <p className="eyebrow">Agent workspace</p>
              <h2>Start a focused session</h2>
              <p>
                Ask telos to inspect files, answer project questions, or run approved
                tools. Keep provider, workspace, approvals, and live tool activity in the
                run panel.
              </p>
              <div className="empty-prompts" aria-label="Example prompts">
                <span>Review this repo</span>
                <span>Explain the current change</span>
                <span>Run the relevant tests</span>
              </div>
            </div>
          ) : (
            state.messages.map((message) => (
              <article className={`message ${message.role}`} key={message.id}>
                <div className="message-role">{message.role}</div>
                <p>{message.content}</p>
              </article>
            ))
          )}
        </section>

        <form className="composer" onSubmit={submit}>
          <textarea
            value={prompt}
            onChange={(event) => setPrompt(event.target.value)}
            placeholder="Ask telos anything..."
            rows={3}
          />
          <button type="submit" disabled={!canSend}>
            Send
          </button>
        </form>
      </section>

      {settingsOpen && (
        <aside className="run-panel" aria-label="Run panel">
          <div className="run-panel-header">
            <div>
              <h2>Run panel</h2>
              <p>Settings and live activity</p>
            </div>
            <span className={`status-pill ${state.running ? "running" : ""}`}>
              {state.running ? "Running" : "Idle"}
            </span>
          </div>

          <section className="summary-grid" aria-label="Run summary">
            <div className="summary-tile">
              <span>Provider</span>
              <strong>{providerLabel}</strong>
            </div>
            <div className="summary-tile">
              <span>Approvals</span>
              <strong>{approvalLabel}</strong>
            </div>
            <div className="summary-tile wide">
              <span>Working directory</span>
              <strong title={cwdLabel}>{cwdLabel}</strong>
            </div>
          </section>

          <section className="panel-section">
            <div className="section-heading">
              <h3>Settings</h3>
              <span>{modelLabel}</span>
            </div>
            <label>
              Provider
              <select
                value={settings.provider}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    provider: event.target.value as ProviderKind,
                  }))
                }
              >
                <option value="mock">Mock</option>
                <option value="deepseek">DeepSeek</option>
              </select>
            </label>
            <label>
              API key
              <input
                type="password"
                value={settings.apiKey ?? ""}
                onChange={(event) =>
                  setSettings((current) => ({ ...current, apiKey: event.target.value }))
                }
                placeholder="Required for DeepSeek"
              />
            </label>
            <label>
              Model
              <select
                value={settings.model ?? "auto"}
                onChange={(event) =>
                  setSettings((current) => ({ ...current, model: event.target.value }))
                }
              >
                <option value="auto">Auto</option>
                <option value="deepseek-v4-pro">DeepSeek V4 Pro</option>
                <option value="deepseek-v4-flash">DeepSeek V4 Flash</option>
              </select>
            </label>
            <label>
              Working directory
              <input
                value={settings.cwd ?? ""}
                onChange={(event) =>
                  setSettings((current) => ({ ...current, cwd: event.target.value }))
                }
                placeholder="Default: app launch directory"
              />
            </label>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={settings.autoApprove}
                onChange={(event) =>
                  setSettings((current) => ({
                    ...current,
                    autoApprove: event.target.checked,
                  }))
                }
              />
              Auto-approve tools
            </label>
          </section>

          <section className="panel-section tool-activity">
            <div className="section-heading">
              <h3>Tool activity</h3>
              <span>{state.tools.length} recent</span>
            </div>
            {state.tools.length === 0 ? (
              <div className="empty-tools">
                Tool calls will appear here while telos works.
              </div>
            ) : (
              <div className="tool-list">
                {state.tools.map((tool) => (
                  <div className={`tool-item ${tool.status}`} key={tool.id}>
                    <div>
                      <strong>{tool.name}</strong>
                      <span>{tool.detail || tool.status}</span>
                    </div>
                    <em>{tool.status}</em>
                  </div>
                ))}
              </div>
            )}
          </section>
        </aside>
      )}
    </main>
  );
```

- [ ] **Step 3: Run TypeScript/build to catch JSX errors**

Run:

```bash
npm run build
```

from `desktop/`.

Expected: Even before Task 2 updates the CSS, TypeScript still passes and Vite builds successfully. Visual styling may still be incomplete until Task 2.

- [ ] **Step 4: Commit Task 1**

Run:

```bash
git add desktop/src/main.tsx
git commit -m "feat: reshape desktop run panel"
```

Expected: one commit containing only `desktop/src/main.tsx`.

## Task 2: Desktop CSS

**Files:**
- Modify: `desktop/src/styles.css`

- [ ] **Step 1: Replace stylesheet**

Replace the full contents of `desktop/src/styles.css` with:

```css
:root {
  color: #1f292d;
  background: #f4f1ec;
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

* {
  box-sizing: border-box;
}

body {
  margin: 0;
  min-width: 320px;
  min-height: 100vh;
}

button,
input,
select,
textarea {
  font: inherit;
}

button {
  border: 1px solid #263238;
  border-radius: 7px;
  background: #263238;
  color: #fff;
  cursor: pointer;
  min-height: 38px;
  padding: 8px 13px;
}

button:disabled {
  cursor: not-allowed;
  opacity: 0.45;
}

.secondary-action {
  background: #fff;
  border-color: #c9c0b5;
  color: #263238;
}

.app-shell {
  background: #f4f1ec;
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(340px, 372px);
  min-height: 100vh;
}

.workspace {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  min-height: 100vh;
  min-width: 0;
}

.topbar {
  align-items: center;
  background: #fbfaf8;
  border-bottom: 1px solid #d9d2c8;
  display: flex;
  gap: 18px;
  justify-content: space-between;
  min-height: 72px;
  padding: 14px 24px;
}

.topbar-title {
  min-width: 0;
}

.topbar h1 {
  font-size: 24px;
  line-height: 1;
  margin: 0;
}

.topbar span {
  color: #657174;
  display: block;
  font-size: 13px;
  margin-top: 7px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.topbar-actions {
  display: flex;
  flex: 0 0 auto;
  gap: 8px;
}

.conversation {
  display: flex;
  flex-direction: column;
  gap: 16px;
  overflow-y: auto;
  padding: 26px clamp(24px, 5vw, 72px);
}

.empty-state {
  align-self: center;
  margin: auto;
  max-width: 680px;
  text-align: left;
}

.eyebrow {
  color: #697578;
  font-size: 12px;
  font-weight: 800;
  letter-spacing: 0;
  margin: 0 0 10px;
  text-transform: uppercase;
}

.empty-state h2 {
  font-size: 34px;
  line-height: 1.08;
  margin: 0 0 12px;
}

.empty-state p {
  color: #687174;
  line-height: 1.55;
  margin: 0;
}

.empty-prompts {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 22px;
}

.empty-prompts span {
  background: #fff;
  border: 1px solid #d9d2c8;
  border-radius: 7px;
  color: #364246;
  font-size: 13px;
  padding: 8px 10px;
}

.message {
  border: 1px solid #dad4cb;
  border-radius: 8px;
  max-width: 780px;
  padding: 14px 16px;
  white-space: pre-wrap;
}

.message.user {
  align-self: flex-end;
  background: #263238;
  border-color: #263238;
  color: #fff;
  width: min(72%, 720px);
}

.message.assistant {
  align-self: flex-start;
  background: #fff;
  width: min(84%, 780px);
}

.message.thinking {
  align-self: flex-start;
  background: #eaf0f0;
  border-color: #d3dddd;
  color: #4e5b5f;
  width: min(68%, 640px);
}

.message.system {
  align-self: center;
  background: #fff4e5;
  color: #7a4a00;
  width: min(84%, 720px);
}

.message-role {
  font-size: 12px;
  font-weight: 800;
  margin-bottom: 8px;
  text-transform: uppercase;
}

.message p {
  line-height: 1.55;
  margin: 0;
}

.composer {
  align-items: end;
  background: #fff;
  border-top: 1px solid #d9d2c8;
  display: grid;
  gap: 12px;
  grid-template-columns: minmax(0, 1fr) auto;
  padding: 16px 24px 18px;
}

.composer textarea {
  background: #fbfaf8;
  border: 1px solid #c9c0b5;
  border-radius: 8px;
  min-height: 92px;
  padding: 12px;
  resize: vertical;
}

.run-panel {
  background: #fff;
  border-left: 1px solid #d9d2c8;
  display: grid;
  gap: 16px;
  grid-template-rows: auto auto auto minmax(0, 1fr);
  min-height: 100vh;
  min-width: 0;
  overflow-y: auto;
  padding: 18px;
}

.run-panel-header,
.section-heading,
.tool-item {
  align-items: center;
  display: flex;
  justify-content: space-between;
  gap: 12px;
}

.run-panel h2 {
  font-size: 19px;
  line-height: 1.1;
  margin: 0;
}

.run-panel-header p {
  color: #657174;
  font-size: 12px;
  margin: 5px 0 0;
}

.status-pill {
  background: #f3f5f5;
  border: 1px solid #d9dfe0;
  border-radius: 999px;
  color: #566265;
  flex: 0 0 auto;
  font-size: 12px;
  font-weight: 800;
  padding: 5px 9px;
}

.status-pill.running {
  background: #eef6f1;
  border-color: #cfe2d6;
  color: #2d6a46;
}

.summary-grid {
  display: grid;
  gap: 10px;
  grid-template-columns: 1fr 1fr;
}

.summary-tile,
.panel-section {
  background: #f7f9f8;
  border: 1px solid #dce1df;
  border-radius: 8px;
}

.summary-tile {
  min-width: 0;
  padding: 10px;
}

.summary-tile.wide {
  grid-column: 1 / -1;
}

.summary-tile span,
.section-heading span {
  color: #657174;
  font-size: 11px;
  font-weight: 800;
  text-transform: uppercase;
}

.summary-tile strong {
  display: block;
  font-size: 14px;
  margin-top: 6px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.panel-section {
  padding: 14px;
}

.section-heading {
  margin-bottom: 12px;
}

.section-heading h3 {
  font-size: 15px;
  margin: 0;
}

.run-panel label {
  color: #3c4649;
  display: grid;
  font-size: 13px;
  font-weight: 800;
  gap: 6px;
  margin-bottom: 13px;
}

.run-panel input,
.run-panel select {
  background: #fff;
  border: 1px solid #c9c0b5;
  border-radius: 6px;
  min-height: 36px;
  min-width: 0;
  padding: 7px 9px;
}

.checkbox-row {
  align-items: center;
  display: flex !important;
  flex-direction: row;
  margin-bottom: 0 !important;
}

.checkbox-row input {
  min-height: auto;
}

.tool-activity {
  min-height: 0;
}

.tool-list {
  display: grid;
  gap: 10px;
}

.tool-item {
  background: #fff;
  border: 1px solid #d9dfe0;
  border-radius: 8px;
  min-height: 58px;
  padding: 9px 10px;
}

.tool-item.failed {
  border-color: #b64d4d;
}

.tool-item.completed {
  border-color: #4c8a67;
}

.tool-item strong,
.tool-item span {
  display: block;
}

.tool-item strong {
  font-size: 13px;
}

.tool-item span,
.tool-item em,
.empty-tools {
  color: #687174;
  font-size: 12px;
}

.tool-item span {
  margin-top: 4px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.tool-item em {
  flex: 0 0 auto;
  font-style: normal;
  font-weight: 800;
  text-transform: uppercase;
}

.empty-tools {
  background: #fff;
  border: 1px dashed #cfd6d7;
  border-radius: 8px;
  line-height: 1.45;
  padding: 12px;
}

@media (max-width: 860px) {
  .app-shell {
    grid-template-columns: 1fr;
  }

  .workspace {
    min-height: 70vh;
  }

  .topbar {
    align-items: flex-start;
    flex-direction: column;
  }

  .topbar-actions {
    width: 100%;
  }

  .topbar-actions button {
    flex: 1;
  }

  .conversation {
    padding: 22px 18px;
  }

  .message.user,
  .message.assistant,
  .message.thinking,
  .message.system {
    width: 100%;
  }

  .composer {
    grid-template-columns: 1fr;
    padding: 14px 18px 16px;
  }

  .run-panel {
    border-left: 0;
    border-top: 1px solid #d9d2c8;
    min-height: auto;
  }
}
```

- [ ] **Step 2: Run build**

Run:

```bash
npm run build
```

from `desktop/`.

Expected: TypeScript and Vite complete successfully.

- [ ] **Step 3: Run desktop tests**

Run:

```bash
npm run test
```

from `desktop/`.

Expected: existing Vitest tests pass.

- [ ] **Step 4: Commit Task 2**

Run:

```bash
git add desktop/src/styles.css
git commit -m "style: densify desktop layout"
```

Expected: one commit containing only `desktop/src/styles.css`.

## Task 3: Manual Responsive Verification

**Files:**
- Inspect: `desktop/src/main.tsx`
- Inspect: `desktop/src/styles.css`

- [ ] **Step 1: Start Vite dev server**

Run:

```bash
npm run dev
```

from `desktop/`.

Expected: Vite serves the desktop UI at `http://127.0.0.1:1420/`. If the port is already in use, Vite reports the conflict and the worker should restart on another available port with `npm run dev -- --port <port>`.

- [ ] **Step 2: Verify wide desktop layout**

Open the served URL at a wide viewport, approximately 1440x900.

Expected:

- The shell uses two columns.
- The main chat area has a top bar, intentional empty state, and anchored composer.
- The right column reads as "Run panel" and includes summary tiles, settings, and an empty tool activity message.
- There is no horizontal overflow.

- [ ] **Step 3: Verify narrow layout**

Resize the viewport below 860px wide.

Expected:

- The shell becomes one column.
- Top bar actions wrap cleanly.
- Messages use full available width.
- Composer becomes a single-column control.
- Run panel stacks below the workspace without horizontal overflow.

- [ ] **Step 4: Stop Vite dev server**

Stop the running server with `Ctrl-C`.

Expected: no dev server process remains running from this task.

## Task 4: Final Verification

**Files:**
- Inspect: `desktop/src/main.tsx`
- Inspect: `desktop/src/styles.css`

- [ ] **Step 1: Confirm changed files**

Run:

```bash
git status --short
```

Expected: no uncommitted changes in `desktop/src/main.tsx` or `desktop/src/styles.css`. Unrelated untracked files may exist and should not be modified.

- [ ] **Step 2: Re-run desktop build**

Run:

```bash
npm run build
```

from `desktop/`.

Expected: build passes.

- [ ] **Step 3: Re-run desktop tests**

Run:

```bash
npm run test
```

from `desktop/`.

Expected: tests pass.

- [ ] **Step 4: Summarize outcome**

Report:

- Which commits were created.
- Whether build and tests passed.
- Any manual visual verification performed.
- Any unrelated working tree changes left untouched.
