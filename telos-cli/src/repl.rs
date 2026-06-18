//! Rustyline-based REPL for the telos interactive session.
//!
//! Provides a configurable [`build_editor`] that loads command history from
//! `~/.local/share/telos/history.txt` and a [`complete_command`] helper for
//! tab-completing slash commands.

use rustyline::history::FileHistory;
use rustyline::{Config, EditMode, Editor};

/// Build and configure a rustyline [`Editor`] with Emacs keybindings, list-style
/// tab completion, and persisted command history.
///
/// History is loaded from (and later saved to)
/// `$XDG_DATA_HOME/telos/history.txt` (typically `~/.local/share/telos/history.txt`).
pub fn build_editor() -> rustyline::Result<Editor<(), FileHistory>> {
    let config = Config::builder()
        .completion_type(rustyline::CompletionType::List)
        .edit_mode(EditMode::Emacs)
        .max_history_size(1000)?
        .build();

    let mut editor = Editor::<(), FileHistory>::with_config(config)?;

    // Load persisted history (best-effort).
    if let Some(data_dir) = dirs::data_dir() {
        let history_path = data_dir.join("telos").join("history.txt");
        if history_path.exists() {
            let _ = editor.load_history(&history_path);
        }
    }

    Ok(editor)
}

/// Return a list of `(command, description)` pairs for all slash commands whose
/// combined `prefix + partial` matches the command name.
///
/// `prefix` is typically `"/"` and `partial` is the text typed after it.
pub fn complete_command(prefix: &str, partial: &str) -> Vec<(String, String)> {
    let needle = format!("{prefix}{partial}");
    ALL_COMMANDS
        .iter()
        .filter(|(cmd, _)| cmd.starts_with(&needle))
        .cloned()
        .map(|(cmd, desc)| (cmd.to_string(), desc.to_string()))
        .collect()
}

/// All recognised slash commands with short descriptions.
const ALL_COMMANDS: &[(&str, &str)] = &[
    ("/exit", "Exit the REPL"),
    ("/quit", "Exit the REPL"),
    ("/reset", "Reset the conversation"),
    ("/clear", "Clear the screen"),
    ("/tools", "List available tools"),
    ("/help", "Show help information"),
    ("/add", "Add a skill"),
    ("/drop", "Drop a skill"),
    ("/model", "Show the current model"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_editor_creates() {
        // Just verify that construction succeeds without panic.
        let _editor = build_editor().expect("build_editor should succeed");
    }

    #[test]
    fn complete_command_returns_commands() {
        let results = complete_command("/", "");
        assert!(!results.is_empty(), "should return at least one command");
        let names: Vec<&str> = results.iter().map(|(cmd, _)| cmd.as_str()).collect();
        assert!(names.contains(&"/exit"));
        assert!(names.contains(&"/help"));
        assert!(names.contains(&"/quit"));
        assert!(names.contains(&"/model"));
    }

    #[test]
    fn complete_command_filters_by_prefix() {
        let results = complete_command("/", "mo");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "/model");
    }

    #[test]
    fn complete_command_no_match() {
        let results = complete_command("/", "zzz");
        assert!(results.is_empty());
    }
}
