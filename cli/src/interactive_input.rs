use std::io::{self};

pub(crate) enum InputLine {
    Line(String),
    Eof,
}

pub(crate) fn read_line(stdin: &io::Stdin) -> io::Result<InputLine> {
    read_line_impl(stdin)
}

#[cfg(not(windows))]
fn read_line_impl(stdin: &io::Stdin) -> io::Result<InputLine> {
    read_buffered_line(stdin)
}

#[cfg(windows)]
fn read_line_impl(stdin: &io::Stdin) -> io::Result<InputLine> {
    use std::io::IsTerminal;
    if stdin.is_terminal() {
        windows_console::read_interactive_line()
    } else {
        read_buffered_line(stdin)
    }
}

fn read_buffered_line(stdin: &io::Stdin) -> io::Result<InputLine> {
    let mut line = String::new();
    match stdin.read_line(&mut line)? {
        0 => Ok(InputLine::Eof),
        _ => Ok(InputLine::Line(line)),
    }
}

#[cfg(windows)]
mod windows_console {
    use super::InputLine;
    use std::io::{self, Write};
    use windows_sys::Win32::System::Console::{GetStdHandle, KEY_EVENT_RECORD};
    use windows_sys::Win32::System::Console::{
        INPUT_RECORD, KEY_EVENT, ReadConsoleInputW, STD_INPUT_HANDLE,
    };

    const CTRL_D: u16 = 0x04;
    const BACKSPACE: u16 = 0x08;
    const CARRIAGE_RETURN: u16 = 0x0d;

    pub(super) fn read_interactive_line() -> io::Result<InputLine> {
        let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        if handle.is_null() {
            return read_buffered_fallback();
        }

        let mut line = String::new();
        loop {
            let key = read_key_event(handle)?;
            let ch = unsafe { key.uChar.UnicodeChar };

            match ch {
                CTRL_D if line.is_empty() => {
                    eprintln!();
                    return Ok(InputLine::Eof);
                }
                CARRIAGE_RETURN => {
                    eprintln!();
                    return Ok(InputLine::Line(line));
                }
                BACKSPACE => {
                    if !line.is_empty() {
                        line.pop();
                        eprint!("\x08 \x08");
                        let _ = io::stderr().flush();
                    }
                }
                _ => {
                    if let Some(ch) = char::from_u32(u32::from(ch))
                        && !ch.is_control()
                    {
                        line.push(ch);
                        eprint!("{ch}");
                        let _ = io::stderr().flush();
                    }
                }
            }
        }
    }

    fn read_key_event(
        handle: windows_sys::Win32::Foundation::HANDLE,
    ) -> io::Result<KEY_EVENT_RECORD> {
        loop {
            let mut record = INPUT_RECORD::default();
            let mut read = 0;
            let ok = unsafe { ReadConsoleInputW(handle, &mut record, 1, &mut read) };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            if read == 0 || u32::from(record.EventType) != KEY_EVENT {
                continue;
            }

            let key = unsafe { record.Event.KeyEvent };
            if key.bKeyDown != 0 {
                return Ok(key);
            }
        }
    }

    fn read_buffered_fallback() -> io::Result<InputLine> {
        let stdin = io::stdin();
        super::read_buffered_line(&stdin)
    }
}
