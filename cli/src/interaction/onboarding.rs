use anyhow::{Context, Result};
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;

use crate::cli::ProviderArg;
use crate::config::FileConfig;

/// Result of a successful onboarding interaction.
pub struct OnboardingResult {
    pub provider: ProviderArg,
    pub api_key: String,
    /// Thinking/reasoning model (planning, complex decisions).
    pub thinking_model: String,
    /// Fast/execution model (tool calls, simple operations).
    pub fast_model: String,
}

// ── Model catalogue ────────────────────────────────────────────────────────
// Each provider has two available models with key specs.

struct ModelInfo {
    id: &'static str,
    label: &'static str,
    desc: &'static str,
}

fn models_for(provider: ProviderArg) -> &'static [ModelInfo] {
    match provider {
        ProviderArg::Deepseek => &[
            ModelInfo {
                id: "deepseek-v4-flash",
                label: "V4 Flash (recommended)",
                desc: "fast • 1M ctx • $0.14/$0.28 per 1M",
            },
            ModelInfo {
                id: "deepseek-v4-pro",
                label: "V4 Pro",
                desc: "powerful reasoning • 1M ctx • $0.44/$0.87 per 1M",
            },
        ],
        ProviderArg::Mock => &[],
    }
}

// ── Public API ─────────────────────────────────────────────────────────────

/// Run the interactive setup wizard.
///
/// Returns `Ok(Some(result))` on success, `Ok(None)` if the user cancelled
/// (Ctrl+C or EOF), or `Err(...)` on I/O failure.
pub fn run() -> Result<Option<OnboardingResult>> {
    // ── Welcome banner ────────────────────────────────────────────────
    eprintln!();
    eprintln!("╔══════════════════════════════════════════════╗");
    eprintln!("║        telos · First-time Setup             ║");
    eprintln!("╚══════════════════════════════════════════════╝");
    eprintln!();
    eprintln!("  No provider configured. Let's set one up.");
    eprintln!();

    // ── Provider selection ────────────────────────────────────────────
    eprintln!("  Available provider:");
    eprintln!("    [1] DeepSeek — api.deepseek.com");
    eprintln!();

    let provider = loop {
        let choice = match prompt_input("  Select provider [1]: ")? {
            Some(s) if s.is_empty() => "1".to_string(),
            Some(s) => s,
            None => return Ok(None),
        };
        match choice.trim() {
            "1" | "deepseek" | "deep" => break ProviderArg::Deepseek,
            _ => {
                eprintln!("  Invalid choice. Enter 1.");
                continue;
            }
        }
    };

    // ── API key ────────────────────────────────────────────────────────
    eprintln!();
    eprintln!("  {} API key", provider_display(provider));
    eprintln!("  Get one at: {}", provider_signup_url(provider));
    eprintln!();

    let api_key = match read_password_masked("  API key: ") {
        Ok(key) => key,
        Err(_) => return Ok(None), // Ctrl+C or I/O error
    };

    if api_key.is_empty() {
        eprintln!("  ✗ API key cannot be empty. Setup cancelled.");
        return Ok(None);
    }
    eprintln!("  ✓ API key received ({} characters)", api_key.len());

    // ── Model selection (dual-model) ──────────────────────────────────────
    let models = models_for(provider);

    eprintln!();
    eprintln!("  telos uses two models by default:");
    eprintln!();
    eprintln!("    🧠 Thinking  → deepseek-v4-pro");
    eprintln!("       Planning, complex reasoning, error recovery");
    eprintln!();
    eprintln!("    ⚡ Fast      → deepseek-v4-flash");
    eprintln!("       Tool execution, file ops, summarization");
    eprintln!();
    eprintln!("  This gives you powerful reasoning when needed,");
    eprintln!("  and fast, cheap execution for routine work.");
    eprintln!();

    // Default dual-model setup
    let thinking_model = "deepseek-v4-pro".to_string();
    let fast_model = "deepseek-v4-flash".to_string();

    let customize = loop {
        match prompt_input("  Press Enter to accept, or type 'c' to customize: ")? {
            Some(s) if s.is_empty() => break false,
            Some(s) if s.trim().to_lowercase() == "c" => break true,
            Some(_) => {
                eprintln!("  Press Enter to continue, or 'c' to customize.");
                continue;
            }
            None => return Ok(None),
        }
    };

    let (thinking_model, fast_model) = if customize {
        eprintln!();
        eprintln!("  Available {} models:", provider_display(provider));
        for (i, m) in models.iter().enumerate() {
            eprintln!("    [{}] {} — {}", i + 1, m.id, m.label);
            eprintln!("        {}", m.desc);
        }
        eprintln!();

        let thinking = loop {
            match prompt_input("  Select thinking model [1] deepseek-v4-pro: ")? {
                Some(s) if s.is_empty() => break "deepseek-v4-pro".to_string(),
                Some(s) => match s.parse::<usize>() {
                    Ok(n) if n >= 1 && n <= models.len() => break models[n - 1].id.to_string(),
                    _ => {
                        eprintln!("  Invalid choice. Enter 1-{}.", models.len());
                        continue;
                    }
                },
                None => return Ok(None),
            }
        };

        let fast = loop {
            match prompt_input("  Select fast model [2] deepseek-v4-flash: ")? {
                Some(s) if s.is_empty() => break "deepseek-v4-flash".to_string(),
                Some(s) => match s.parse::<usize>() {
                    Ok(n) if n >= 1 && n <= models.len() => break models[n - 1].id.to_string(),
                    _ => {
                        eprintln!("  Invalid choice. Enter 1-{}.", models.len());
                        continue;
                    }
                },
                None => return Ok(None),
            }
        };

        (thinking, fast)
    } else {
        eprintln!("  ✓ Using default dual-model setup");
        (thinking_model, fast_model)
    };

    // ── Save to config? ────────────────────────────────────────────────
    eprintln!();
    let save = loop {
        match prompt_input("  Save to ~/.config/telos/config.toml? [Y/n]: ")? {
            Some(s) if s.is_empty() => break true,
            Some(s) => match s.to_lowercase().as_str() {
                "y" | "yes" => break true,
                "n" | "no" => break false,
                _ => {
                    eprintln!("  Please enter Y or n.");
                    continue;
                }
            },
            None => return Ok(None),
        }
    };

    let result = OnboardingResult { provider, api_key, thinking_model, fast_model };

    if save {
        save_config(&result)?;
    }

    Ok(Some(result))
}

/// Save onboarding result to `~/.config/telos/config.toml`.
///
/// Creates the directory and file if they don't exist. Merges with any
/// existing config — only overwrites the provider/model/api-key fields.
pub fn save_config(result: &OnboardingResult) -> Result<()> {
    let path = config_path()?;

    // Create parent directory if needed.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory: {}", parent.display()))?;
    }

    // Read existing config (if any), or start fresh.
    let mut existing = if path.exists() {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config: {}", path.display()))?;
        toml::from_str::<FileConfig>(&contents).unwrap_or_default()
    } else {
        FileConfig::default()
    };

    // Update agent section with dual-model setup.
    let agent = existing.agent.get_or_insert_with(Default::default);
    agent.provider = Some(provider_to_config_str(result.provider).to_string());
    let models = agent.models.get_or_insert_with(Default::default);
    models.thinking = Some(result.thinking_model.clone());
    models.fast = Some(result.fast_model.clone());

    // Update env section with the API key.
    let env = existing.env.get_or_insert_with(Default::default);
    match result.provider {
        ProviderArg::Deepseek => {
            env.insert("DEEPSEEK_API_KEY".to_string(), result.api_key.clone());
        }
        ProviderArg::Mock => {}
    }

    // Serialize and write.
    let toml_str = toml::to_string_pretty(&existing).context("failed to serialize config")?;
    fs::write(&path, &toml_str)
        .with_context(|| format!("failed to write config: {}", path.display()))?;

    // Restrict permissions on Unix so the API key isn't world-readable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            if perms.mode() & 0o077 != 0 {
                perms.set_mode(0o600);
                let _ = fs::set_permissions(&path, perms);
            }
        }
    }

    eprintln!();
    eprintln!("  ✓ Config saved to {}", path.display());
    Ok(())
}

// ── Helpers ─────────────────────────────────────────────────────────────

/// Determine the config file path: `~/.config/telos/config.toml`.
fn config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("could not determine user config directory")?;
    Ok(base.join("telos").join("config.toml"))
}

/// Read a line from stdin, writing the prompt to stderr first.
/// Returns `Ok(None)` on Ctrl+C/EOF.
fn prompt_input(prompt: &str) -> Result<Option<String>> {
    if !prompt.is_empty() {
        write!(std::io::stderr(), "{prompt}")?;
        std::io::stderr().flush()?;
    }

    let mut input = String::new();
    // Lock stdin briefly — the lock is dropped after the match, so
    // rpassword::prompt_password can lock stdin again without conflict.
    match std::io::stdin().lock().read_line(&mut input) {
        Ok(0) => Ok(None), // EOF
        Ok(_) => Ok(Some(input.trim().to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Ok(None), // Ctrl+C
        Err(e) => Err(e.into()),
    }
}

fn provider_display(p: ProviderArg) -> &'static str {
    match p {
        ProviderArg::Deepseek => "DeepSeek",
        ProviderArg::Mock => "Mock",
    }
}

fn provider_signup_url(p: ProviderArg) -> &'static str {
    match p {
        ProviderArg::Deepseek => "https://platform.deepseek.com/api_keys",
        ProviderArg::Mock => "",
    }
}

/// Read a password from stdin, showing `*` for each character typed.
/// Supports Backspace to delete the last character.
/// Returns `Ok(String)` on Enter, `Err(...)` on Ctrl+C or I/O failure.
///
/// Uses raw terminal I/O on Unix; falls back to `rpassword` on other platforms.
fn read_password_masked(prompt: &str) -> Result<String, anyhow::Error> {
    if !prompt.is_empty() {
        write!(std::io::stderr(), "{prompt}")?;
        std::io::stderr().flush()?;
    }

    #[cfg(unix)]
    {
        read_password_masked_unix()
    }
    #[cfg(not(unix))]
    {
        // Fallback: use rpassword (no asterisk feedback on Windows).
        rpassword::prompt_password("").map(|s| s.trim().to_string()).map_err(|e| anyhow::anyhow!(e))
    }
}

#[cfg(unix)]
fn read_password_masked_unix() -> Result<String, anyhow::Error> {
    use std::os::fd::AsRawFd;

    let stdin_fd = std::io::stdin().as_raw_fd();
    let mut termios: libc::termios = unsafe { std::mem::zeroed() };

    // Save current terminal settings.
    if unsafe { libc::tcgetattr(stdin_fd, &mut termios) } != 0 {
        anyhow::bail!("failed to get terminal attributes");
    }
    let saved = termios;

    // Set non-canonical mode, disable echo.
    termios.c_lflag &= !(libc::ICANON | libc::ECHO);
    termios.c_cc[libc::VMIN] = 1;
    termios.c_cc[libc::VTIME] = 0;
    if unsafe { libc::tcsetattr(stdin_fd, libc::TCSANOW, &termios) } != 0 {
        anyhow::bail!("failed to set terminal attributes");
    }

    let result = read_with_asterisks();

    // Restore terminal settings.
    unsafe {
        libc::tcsetattr(stdin_fd, libc::TCSANOW, &saved);
    }

    result
}

#[cfg(unix)]
fn read_with_asterisks() -> Result<String, anyhow::Error> {
    use std::io::Read;

    let mut password = String::new();
    let mut stdin = std::io::stdin();
    let mut buf = [0u8; 1];
    let mut stderr = std::io::stderr();

    loop {
        match stdin.read_exact(&mut buf) {
            Ok(()) => {
                let ch = buf[0];
                match ch {
                    b'\r' | b'\n' => {
                        // Enter — finish.
                        writeln!(stderr)?;
                        return Ok(password);
                    }
                    0x7f | b'\x08' => {
                        // Backspace / Delete.
                        if !password.is_empty() {
                            password.pop();
                            // Erase the last `*` from the terminal.
                            write!(stderr, "\x08 \x08")?;
                            stderr.flush()?;
                        }
                    }
                    0x03 => {
                        // Ctrl+C.
                        writeln!(stderr)?;
                        anyhow::bail!("cancelled");
                    }
                    0x04 => {
                        // Ctrl+D (EOF on empty).
                        if password.is_empty() {
                            writeln!(stderr)?;
                            anyhow::bail!("cancelled");
                        }
                    }
                    _ if ch.is_ascii_graphic() || ch == b' ' => {
                        // Printable character — buffer it and show `*`.
                        password.push(ch as char);
                        write!(stderr, "*")?;
                        stderr.flush()?;
                    }
                    _ => {
                        // Ignore other control characters.
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
                writeln!(stderr)?;
                anyhow::bail!("cancelled");
            }
            Err(e) => {
                return Err(e.into());
            }
        }
    }
}

fn provider_to_config_str(p: ProviderArg) -> &'static str {
    match p {
        ProviderArg::Deepseek => "deepseek",
        ProviderArg::Mock => "mock",
    }
}
