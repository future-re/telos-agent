use super::format::none_if_whitespace;

/// tmux client terminal identification captured via `tmux display-message`.
///
/// `termtype` corresponds to `#{client_termtype}` and typically reflects the
/// underlying terminal program with an optional version suffix. `termname`
/// comes from `#{client_termname}` and preserves the TERM capability string.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct TmuxClientInfo {
    pub termtype: Option<String>,
    pub termname: Option<String>,
}

pub(super) fn tmux_client_info() -> TmuxClientInfo {
    let termtype = tmux_display_message("#{client_termtype}");
    let termname = tmux_display_message("#{client_termname}");

    TmuxClientInfo { termtype, termname }
}

fn tmux_display_message(format: &str) -> Option<String> {
    let output =
        std::process::Command::new("tmux").args(["display-message", "-p", format]).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8(output.stdout).ok()?;
    none_if_whitespace(value.trim().to_string())
}

pub(super) fn zellij_version_from_command() -> Option<String> {
    // Best-effort fallback: missing or broken zellij binaries should not affect
    // terminal detection.
    let output = std::process::Command::new("zellij").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_zellij_version(stdout.trim())
}

fn parse_zellij_version(value: &str) -> Option<String> {
    let value = none_if_whitespace(value.to_string())?;
    let mut parts = value.split_whitespace();
    match (parts.next(), parts.next()) {
        (Some(command), Some(version)) if command.eq_ignore_ascii_case("zellij") => {
            Some(version.to_string())
        }
        _ => Some(value),
    }
}
