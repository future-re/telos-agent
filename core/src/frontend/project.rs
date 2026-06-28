use std::path::{Path, PathBuf};

pub fn find_project_root(start_dir: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    find_project_root_with_temp_root(start_dir, std::env::temp_dir())
}

fn find_project_root_with_temp_root(
    start_dir: impl AsRef<Path>,
    ambient_temp_root: impl AsRef<Path>,
) -> std::io::Result<PathBuf> {
    let start_dir = start_dir.as_ref();
    let canonical = start_dir.canonicalize()?;
    let ambient_temp_root = ambient_temp_root.as_ref().canonicalize().ok();

    let mut current: Option<&Path> = Some(canonical.as_path());
    while let Some(dir) = current {
        let is_ambient_temp_root = ambient_temp_root.as_deref() == Some(dir);
        if !is_ambient_temp_root && dir.join(".telos.toml").exists() {
            return Ok(dir.to_path_buf());
        }
        if !is_ambient_temp_root && dir.join(".git").exists() {
            return Ok(dir.to_path_buf());
        }
        if is_ambient_temp_root {
            break;
        }
        current = dir.parent();
    }

    canonical.canonicalize()
}
