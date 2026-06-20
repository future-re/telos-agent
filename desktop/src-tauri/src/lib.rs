#[cfg(any(test, target_os = "macos", target_os = "windows"))]
mod agent_host;
#[cfg(any(test, target_os = "macos", target_os = "windows"))]
mod desktop_event;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod tauri_app;

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use tauri_app::run;
