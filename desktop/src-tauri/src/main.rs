#[cfg(any(target_os = "macos", target_os = "windows"))]
fn main() {
    telos_desktop_lib::run();
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("telos desktop is configured for macOS and Windows targets.");
}
