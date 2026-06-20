use std::path::Path;

pub(super) fn session_ids_in_dir(sessions_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(sessions_dir) else { return Vec::new() };
    let mut sessions = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_file() {
                return None;
            }
            let path = entry.path();
            (path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
                .then(|| path.file_stem()?.to_str().map(str::to_string))?
        })
        .filter(|id| is_valid_session_id_for_tui(id))
        .collect::<Vec<_>>();
    sessions.sort();
    sessions.reverse();
    sessions
}

fn is_valid_session_id_for_tui(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::session_ids_in_dir;

    #[test]
    fn filters_invalid_storage_ids() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("session-123.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("chat_abc.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("bad.name.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("bad space.jsonl"), "").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "").unwrap();
        std::fs::create_dir(dir.path().join("directory-session.jsonl")).unwrap();

        assert_eq!(
            session_ids_in_dir(dir.path()),
            vec!["session-123".to_string(), "chat_abc".to_string()]
        );
    }
}
