use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn is_ctrl_modifier(modifiers: KeyModifiers) -> bool {
    modifiers.contains(KeyModifiers::CONTROL) && !modifiers.contains(KeyModifiers::ALT)
}

pub fn is_ctrl_char(key: KeyEvent, target: char) -> bool {
    if key.modifiers.contains(KeyModifiers::ALT) {
        return false;
    }

    let Some(target_control) = ascii_control_char(target) else {
        return false;
    };

    match key.code {
        KeyCode::Char(c) if c == target_control => true,
        KeyCode::Char(c) if c.to_ascii_lowercase() == target => {
            key.modifiers.contains(KeyModifiers::CONTROL)
        }
        _ => false,
    }
}

fn ascii_control_char(target: char) -> Option<char> {
    let lower = target.to_ascii_lowercase();
    if !lower.is_ascii_lowercase() {
        return None;
    }
    Some(((lower as u8) - b'a' + 1) as char)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_char_accepts_extra_shift_modifier_and_uppercase() {
        let key = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::CONTROL | KeyModifiers::SHIFT);

        assert!(is_ctrl_char(key, 'a'));
    }

    #[test]
    fn ctrl_char_accepts_ascii_control_character() {
        let key = KeyEvent::new(KeyCode::Char('\u{1}'), KeyModifiers::NONE);

        assert!(is_ctrl_char(key, 'a'));
    }

    #[test]
    fn ctrl_char_rejects_alt_modified_chords() {
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL | KeyModifiers::ALT);

        assert!(!is_ctrl_char(key, 'a'));
    }

    #[test]
    fn ctrl_char_rejects_plain_character() {
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);

        assert!(!is_ctrl_char(key, 'a'));
    }
}
