# Desktop Adaptation Design

## Context

The current desktop client is functional but sparse on wide screens. It uses a simple two-column shell: the chat workspace on the left and a settings panel on the right. The right panel is useful, but it behaves like a raw form instead of a desktop run surface, while the chat area leaves a lot of unstructured space when there are few messages.

The approved direction is the "reinforced two-column" layout. It keeps the current architecture and avoids adding a session navigation system that the app does not yet support.

## Goals

- Make the desktop client feel intentional and dense without obscuring the chat.
- Reuse the existing React state and Tauri command flow.
- Turn the right side into a run panel that combines settings, working context, and live tool activity.
- Improve the empty state so wide screens do not feel unfinished.
- Preserve mobile and narrow-window behavior with a single-column fallback.

## Non-Goals

- Add persistent session history or project navigation.
- Add new backend commands.
- Change provider behavior, tool execution, approval semantics, or chat state reduction.
- Introduce a component library.

## Layout

The desktop shell remains a two-column grid. The main column contains:

- A top bar with product name, current run status, compact metadata, and primary actions.
- A scrollable conversation area with a constrained reading width and more deliberate empty state content.
- A bottom composer that is visually anchored and sized for repeated desktop use.

The right column becomes a fixed-width run panel, approximately 340-380px on desktop. It contains:

- Provider/model summary.
- Working directory summary.
- Approval mode summary.
- Collapsible or grouped settings controls.
- Tool activity list, using the existing `state.tools` data.

On narrow viewports, the shell collapses to one column. The run panel moves below the workspace or becomes visually integrated as a stacked settings/activity section, matching the current mobile behavior while improving spacing.

## Components

All work can stay in `desktop/src/main.tsx` and `desktop/src/styles.css` for this pass.

`App`
Keeps ownership of chat state, settings state, prompt submission, and reset behavior. It can compute a small amount of derived display data such as provider label, model label, working directory label, and approval mode label.

`Topbar`
May remain inline JSX or be extracted only if the markup becomes hard to scan. It should show status and compact run metadata.

`RunPanel`
Can be inline for this pass. It presents the existing settings fields and existing tool activity in grouped sections. Empty tool activity should show a compact placeholder, not a large empty block.

`Conversation`
Keeps the existing message mapping. Message max widths and spacing should be tuned for desktop readability.

`Composer`
Keeps the current form behavior. The send button remains disabled when no prompt is available or a run is active.

## Data Flow

No data model changes are required.

- Settings continue to live in `settings`.
- Prompt submission still calls `send_prompt` with `normalizeSettings(settings)`.
- Tool activity still comes from `state.tools`.
- Status still comes from `state.status`.
- Reset still calls `reset_session` and dispatches `reset`.

The UI may derive display strings from this data, but should not introduce separate state for values already present in `settings` or `state`.

## Visual Direction

The interface should stay quiet and operational. The visual language should use restrained contrast, clear boundaries, compact grouped panels, and readable spacing. The app should not become a marketing-style landing page.

The palette should avoid feeling like a one-note beige page. Keep the current warm base if useful, but add enough neutral, white, charcoal, and subtle status colors for hierarchy.

Cards should be limited to repeated items and grouped run-panel sections. Page-level areas should be structured as bands or panels rather than nested decorative cards.

## Error Handling

Existing error handling remains unchanged. Errors from `send_prompt` continue to produce a system message and stop the run. The run panel should not hide errors; system messages remain visible in the conversation.

Tool failures should remain visible through `tool.status === "failed"` styling.

## Testing

Run the existing desktop checks:

- `npm run build` from `desktop/`.
- `npm run test` from `desktop/`.

Manual visual verification should cover:

- Wide desktop with settings open.
- Narrow viewport under the existing 860px breakpoint.
- Empty conversation state.
- Conversation with user, assistant, thinking, system, and tool activity items.

## Acceptance Criteria

- Desktop width no longer feels sparse in the initial empty state.
- The right column reads as a run panel, not just an isolated settings form.
- Tool activity is visible in the right panel when present.
- Existing prompt submission, reset, settings changes, and event streaming behavior remain intact.
- The mobile/narrow layout remains usable without horizontal overflow.
