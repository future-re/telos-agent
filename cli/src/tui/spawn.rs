//! Convenience entry point — creates the agent session, spawns the
//! background turn runner, and launches the TUI event loop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use telos_agent::{AgentConfig, AgentSession, ModelProvider, ToolRegistry};
use tokio::sync::mpsc;

use crate::tui::turn::{TurnCommand, TurnUpdate, run_turn};

/// Launch the full TUI with a connected agent backend.
pub async fn run_with_agent(
    config: AgentConfig,
    provider: Arc<dyn ModelProvider>,
    tools: ToolRegistry,
    status_text: String,
    auto_mode: Arc<AtomicBool>,
) -> Result<()> {
    let mut session = AgentSession::new(config)?;

    // Channels: turn_updates go from agent → TUI, commands go TUI → agent.
    let (update_tx, update_rx) = mpsc::unbounded_channel::<TurnUpdate>();
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<TurnCommand>();
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Spawn the background turn runner — owns the session.
    // Runs turns sequentially in the same session.
    let cancel = Arc::clone(&cancel_flag);
    let p = provider.clone();
    let t = tools.clone();
    let tx = update_tx.clone();
    tokio::spawn(async move {
        while let Some(cmd) = cmd_rx.recv().await {
            match cmd {
                TurnCommand::Prompt(prompt) => {
                    cancel.store(false, Ordering::Relaxed);
                    run_turn(&mut session, &*p, &t, prompt, &tx, &cancel).await;
                }
                TurnCommand::Cancel => {
                    cancel.store(true, Ordering::Relaxed);
                }
            }
        }
    });

    // Launch the TUI event loop.
    crate::tui::run::run(status_text, auto_mode, update_rx, cmd_tx).await
}
