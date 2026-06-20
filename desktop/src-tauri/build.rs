fn main() {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    tauri_build::build();
}
