#[tokio::test]
async fn memory_write_and_read_tools_roundtrip() {
    use std::sync::{Arc, Mutex};
    use telos_agent::memory::{MemoryReadTool, MemoryStore, MemoryWriteTool};
    use telos_agent::tool::{Tool, ToolContext};

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(MemoryStore::new(dir.path().to_path_buf())));
    let write_tool = MemoryWriteTool::new(store.clone());
    let read_tool = MemoryReadTool::new(store.clone());

    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    // Write
    write_tool
        .invoke(
            serde_json::json!({
                "name": "test-memory",
                "description": "A test memory entry",
                "category": "fact",
                "body": "This is the body content.",
                "tags": ["test", "example"]
            }),
            ctx.clone(),
        )
        .await
        .unwrap();

    // Read
    let result =
        read_tool.invoke(serde_json::json!({"name": "test-memory"}), ctx.clone()).await.unwrap();
    let content = result.content;
    assert_eq!(content["name"].as_str().unwrap(), "test-memory");
    assert_eq!(content["body"].as_str().unwrap(), "This is the body content.");
    assert!(content["tags"].as_array().unwrap().iter().any(|t| t.as_str() == Some("test")));
}

#[tokio::test]
async fn memory_write_sanitizes_windows_path_like_names_and_links_with_portable_separator() {
    use std::sync::{Arc, Mutex};
    use telos_agent::memory::{MemoryReadTool, MemoryStore, MemoryWriteTool};
    use telos_agent::tool::{Tool, ToolContext};

    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Mutex::new(MemoryStore::new(dir.path().to_path_buf())));
    let write_tool = MemoryWriteTool::new(store.clone());
    let read_tool = MemoryReadTool::new(store);
    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    write_tool
        .invoke(
            serde_json::json!({
                "name": r"C:\Users\alice\.telos\fact",
                "description": "A memory named after a Windows path",
                "category": "fact",
                "body": r"Stored from %LOCALAPPDATA%\Telos",
                "tags": ["windows", "path"]
            }),
            ctx.clone(),
        )
        .await
        .unwrap();

    let memory_file = dir.path().join("facts").join("c--users-alice--telos-fact.md");
    assert!(memory_file.exists(), "expected sanitized file at {}", memory_file.display());
    let index = std::fs::read_to_string(dir.path().join("MEMORY.md")).unwrap();
    assert!(index.contains("facts/c--users-alice--telos-fact.md"));

    let read = read_tool
        .invoke(serde_json::json!({"name": r"C:\Users\alice\.telos\fact"}), ctx)
        .await
        .unwrap()
        .content;
    assert_eq!(read["body"], r"Stored from %LOCALAPPDATA%\Telos");
}

#[tokio::test]
async fn memory_maintenance_tool_dry_run_and_apply() {
    use std::sync::{Arc, Mutex};
    use telos_agent::memory::{MemoryCategory, MemoryEntry, MemoryStatus, MemoryStore};
    use telos_agent::tool::ToolContext;
    use telos_agent::{ToolRegistry, register_memory_tools};

    let dir = tempfile::tempdir().unwrap();
    let mut raw_store = MemoryStore::new(dir.path().to_path_buf());
    let old_auto = MemoryEntry {
        name: "old-auto".into(),
        description: "Old auto command".into(),
        category: MemoryCategory::Command,
        tags: vec!["auto-learned".into(), "bash".into()],
        created: "100".into(),
        updated: "100".into(),
        status: MemoryStatus::Working,
        times_used: 0,
        confidence: None,
        related: vec![],
        source_session: None,
        body: "old".into(),
    };
    let mut new_auto = old_auto.clone();
    new_auto.name = "new-auto".into();
    new_auto.description = "New auto command".into();
    new_auto.created = "200".into();
    new_auto.updated = "200".into();
    raw_store.write(old_auto).unwrap();
    raw_store.write(new_auto).unwrap();

    let store = Arc::new(Mutex::new(raw_store));
    let mut registry = ToolRegistry::new();
    register_memory_tools(&mut registry, store.clone());
    let tool = registry.get("MemoryMaintenance").unwrap();
    let ctx = ToolContext {
        session_id: "test".into(),
        turn_id: 1,
        tool_call_id: None,
        cwd: std::env::current_dir().unwrap(),
        env: Default::default(),
        messages: Arc::new(vec![]),
        progress: None,
        read_file_state: Arc::new(tokio::sync::Mutex::new(Default::default())),
        timeout: None,
        max_file_read_bytes: 50 * 1024 * 1024,
    };

    let dry_run = tool
        .invoke(
            serde_json::json!({"max_auto_learned_commands": 1, "archive_deprecated": false}),
            ctx.clone(),
        )
        .await
        .unwrap()
        .content;
    assert_eq!(dry_run["applied"], false);
    assert_eq!(dry_run["archived_count"], 0);
    assert_eq!(dry_run["actions"].as_array().unwrap()[0]["name"], "old-auto");
    assert!(store.lock().unwrap().read("old-auto").is_some());

    let applied = tool
        .invoke(
            serde_json::json!({
                "apply": true,
                "max_auto_learned_commands": 1,
                "archive_deprecated": false
            }),
            ctx,
        )
        .await
        .unwrap()
        .content;
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["archived_count"], 1);
    assert!(store.lock().unwrap().read("old-auto").is_none());
    assert!(dir.path().join("_archived").join("old-auto.md").exists());
}

#[tokio::test]
async fn memory_section_renders_top_entries() {
    use std::sync::{Arc, Mutex};
    use telos_agent::memory::{MemoryCategory, MemoryEntry, MemoryStatus, MemoryStore};
    use telos_agent::prompt::PromptSection;
    use telos_agent::prompt::builtins::MemorySection;

    let dir = tempfile::tempdir().unwrap();
    let mut store = MemoryStore::new(dir.path().to_path_buf());

    let entry = MemoryEntry {
        name: "test-fact".into(),
        description: "A test fact".into(),
        category: MemoryCategory::Fact,
        tags: vec!["test".into()],
        created: "2026-06-18".into(),
        updated: "2026-06-18".into(),
        status: MemoryStatus::Working,
        times_used: 5,
        confidence: None,
        related: vec![],
        source_session: None,
        body: "This is a test memory body.".into(),
    };
    store.write(entry).unwrap();

    let section = MemorySection::new(Arc::new(Mutex::new(store)));
    let rendered = section.render(&()).await;
    assert!(rendered.contains("Relevant Memories"));
    assert!(rendered.contains("test-fact"));
    assert!(rendered.contains("A test fact"));
}

#[tokio::test]
async fn memory_section_empty_when_no_memories() {
    use std::sync::{Arc, Mutex};
    use telos_agent::memory::MemoryStore;
    use telos_agent::prompt::PromptSection;
    use telos_agent::prompt::builtins::MemorySection;

    let dir = tempfile::tempdir().unwrap();
    let store = MemoryStore::new(dir.path().to_path_buf());
    let section = MemorySection::new(Arc::new(Mutex::new(store)));
    let rendered = section.render(&()).await;
    assert!(rendered.is_empty());
}

#[tokio::test]
async fn memory_section_skips_deprecated_entries() {
    use std::sync::{Arc, Mutex};
    use telos_agent::memory::{MemoryCategory, MemoryEntry, MemoryStatus, MemoryStore};
    use telos_agent::prompt::PromptSection;
    use telos_agent::prompt::builtins::MemorySection;

    let dir = tempfile::tempdir().unwrap();
    let mut store = MemoryStore::new(dir.path().to_path_buf());

    store
        .write(MemoryEntry {
            name: "old-fact".into(),
            description: "Old fact".into(),
            category: MemoryCategory::Fact,
            tags: vec![],
            created: "1".into(),
            updated: "1".into(),
            status: MemoryStatus::Deprecated,
            times_used: 100,
            confidence: None,
            related: vec![],
            source_session: None,
            body: "do not show".into(),
        })
        .unwrap();

    let section = MemorySection::new(Arc::new(Mutex::new(store)));
    let rendered = section.render(&()).await;
    assert!(!rendered.contains("old-fact"));
}

#[tokio::test]
async fn profile_section_renders_profiles() {
    use std::sync::Arc;
    use telos_agent::memory::ProfileManager;
    use telos_agent::prompt::PromptSection;
    use telos_agent::prompt::builtins::ProfileSection;

    let dir = tempfile::tempdir().unwrap();
    let mgr =
        Arc::new(ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap());
    mgr.set_user_profile("Test user profile content").unwrap();
    mgr.set_project_profile("Test project profile content").unwrap();

    let section = ProfileSection::new(mgr);
    let rendered = section.render(&()).await;
    assert!(rendered.contains("User Profile"));
    assert!(rendered.contains("Test user profile content"));
    assert!(rendered.contains("Project Profile"));
    assert!(rendered.contains("Test project profile content"));
}

#[tokio::test]
async fn profile_section_rerenders_when_profiles_change() {
    use std::sync::Arc;
    use telos_agent::memory::ProfileManager;
    use telos_agent::prompt::PromptAssembly;
    use telos_agent::prompt::builtins::ProfileSection;

    let dir = tempfile::tempdir().unwrap();
    let mgr =
        Arc::new(ProfileManager::new(dir.path().to_path_buf(), dir.path().to_path_buf()).unwrap());
    mgr.set_user_profile("Before").unwrap();

    let mut assembly = PromptAssembly::new();
    assembly.add(ProfileSection::new(mgr.clone()));

    let first = assembly.build().await;
    assert!(first.contains("Before"));

    mgr.set_user_profile("After").unwrap();
    let second = assembly.build().await;
    assert!(second.contains("After"));
    assert!(!second.contains("Before"));
}
