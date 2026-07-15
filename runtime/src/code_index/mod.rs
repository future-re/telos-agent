//! Lightweight local code index for fast path/line search and context lookup.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const INDEX_REL_PATH: &[&str] = &[".telos", "index", "code_index.json"];
const MAX_FILE_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodeIndex {
    pub root: PathBuf,
    pub files: Vec<IndexedFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexedFile {
    pub path: String,
    pub language: String,
    pub modified_secs: u64,
    pub size: u64,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodeSearchMatch {
    pub path: String,
    pub line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CodeContextLine {
    pub line: usize,
    pub text: String,
}

impl CodeIndex {
    pub fn load_or_refresh(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        match Self::load(&root) {
            Ok(index) => Ok(index),
            Err(_) => Self::refresh(root),
        }
    }

    pub fn load(root: impl AsRef<Path>) -> std::io::Result<Self> {
        let root = root.as_ref();
        let content = std::fs::read_to_string(index_path(root))?;
        let mut index: Self = serde_json::from_str(&content)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        index.root = root.to_path_buf();
        Ok(index)
    }

    pub fn refresh(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        let mut index = Self { root: root.clone(), files: Vec::new() };
        visit_dir(&root, &root, &mut index.files)?;
        index.files.sort_by(|a, b| a.path.cmp(&b.path));
        let path = index_path(&root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&index)?;
        std::fs::write(path, content)?;
        Ok(index)
    }

    pub fn search(
        &self,
        query: &str,
        path_prefix: Option<&str>,
        max_results: usize,
        case_sensitive: bool,
    ) -> Vec<CodeSearchMatch> {
        let needle = if case_sensitive { query.to_string() } else { query.to_lowercase() };
        let mut matches = Vec::new();
        for file in &self.files {
            if let Some(prefix) = path_prefix
                && !file.path.contains(prefix)
            {
                continue;
            }
            for (idx, line) in file.lines.iter().enumerate() {
                let haystack = if case_sensitive { line.to_string() } else { line.to_lowercase() };
                if haystack.contains(&needle) {
                    matches.push(CodeSearchMatch {
                        path: file.path.clone(),
                        line: idx + 1,
                        text: line.clone(),
                    });
                    if matches.len() >= max_results {
                        return matches;
                    }
                }
            }
        }
        matches
    }

    pub fn context(
        &self,
        path: &str,
        line: usize,
        before: usize,
        after: usize,
    ) -> Option<Vec<CodeContextLine>> {
        let file = self.files.iter().find(|file| file.path == path)?;
        if file.lines.is_empty() || line == 0 {
            return Some(Vec::new());
        }
        let start = line.saturating_sub(before).max(1);
        let end = (line + after).min(file.lines.len());
        Some(
            (start..=end)
                .map(|line_no| CodeContextLine {
                    line: line_no,
                    text: file.lines[line_no - 1].clone(),
                })
                .collect(),
        )
    }

    pub fn index_path(root: impl AsRef<Path>) -> PathBuf {
        index_path(root.as_ref())
    }
}

fn index_path(root: &Path) -> PathBuf {
    INDEX_REL_PATH.iter().fold(root.to_path_buf(), |path, component| path.join(component))
}

fn visit_dir(root: &Path, dir: &Path, files: &mut Vec<IndexedFile>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if should_skip_name(&name) {
            continue;
        }
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            visit_dir(root, &path, files)?;
        } else if metadata.is_file()
            && metadata.len() <= MAX_FILE_BYTES
            && let Some(file) = index_file(root, &path, metadata.len())?
        {
            files.push(file);
        }
    }
    Ok(())
}

fn index_file(root: &Path, path: &Path, size: u64) -> std::io::Result<Option<IndexedFile>> {
    let bytes = std::fs::read(path)?;
    if bytes.contains(&0) {
        return Ok(None);
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let relative = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    let modified_secs = std::fs::metadata(path)?
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    Ok(Some(IndexedFile {
        language: language_for(path),
        path: relative,
        modified_secs,
        size,
        lines: content.lines().map(str::to_string).collect(),
    }))
}

fn should_skip_name(name: &str) -> bool {
    matches!(name, ".git" | ".telos" | "target" | "node_modules" | ".next" | "dist" | "build")
}

fn language_for(path: &Path) -> String {
    match path.extension().and_then(|ext| ext.to_str()).unwrap_or_default() {
        "rs" => "rust",
        "toml" => "toml",
        "md" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "js" | "jsx" => "javascript",
        "ts" | "tsx" => "typescript",
        "py" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" | "cc" | "cpp" | "hpp" => "cpp",
        "" => "text",
        other => other,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_indexes_text_files_and_skips_telos_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join(".telos")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "fn target() {}\nfn other() {}\n").unwrap();
        std::fs::write(dir.path().join(".telos/ignored.rs"), "fn target() {}\n").unwrap();

        let index = CodeIndex::refresh(dir.path().to_path_buf()).unwrap();

        assert_eq!(index.files.len(), 1);
        assert_eq!(index.files[0].path, "src/lib.rs");
        assert!(CodeIndex::index_path(dir.path()).exists());
    }

    #[test]
    fn search_returns_path_line_and_context() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "first\nfn target() {}\nlast\n").unwrap();
        let index = CodeIndex::refresh(dir.path().to_path_buf()).unwrap();

        let matches = index.search("TARGET", None, 10, false);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "src/lib.rs");
        assert_eq!(matches[0].line, 2);

        let context = index.context("src/lib.rs", 2, 1, 1).unwrap();
        assert_eq!(context.len(), 3);
        assert_eq!(context[1].text, "fn target() {}");
    }
}
