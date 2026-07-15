mod common;

use std::sync::Arc;

use telos_agent::*;

#[test]
fn jsonl_storage_roundtrips_messages() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-roundtrip-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = JsonlStorage::new(&dir).unwrap();

        let messages =
            vec![Message::system("sys"), Message::user("hello"), Message::assistant("world")];
        storage.append("s1", &messages).await.unwrap();

        let loaded = storage.load("s1").await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[0].text_content(), "sys");
        assert_eq!(loaded[1].text_content(), "hello");
        assert_eq!(loaded[2].text_content(), "world");

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn session_save_and_resume_works() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-resume-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = Arc::new(JsonlStorage::new(&dir).unwrap());

        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("hi there"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig {
            base_system_prompt: Some("sys".into()),
            storage: Some(storage.clone()),
            ..AgentConfig::default()
        })
        .unwrap();

        session.run_turn(&provider, &tools, "hello").await.unwrap();
        assert_eq!(session.messages().len(), 3); // sys + user + assistant

        let session_id = session.session_id().to_string();
        let resumed = AgentSession::resume(
            session_id,
            AgentConfig {
                base_system_prompt: Some("sys".into()),
                storage: Some(storage.clone()),
                ..AgentConfig::default()
            },
            storage.clone(),
        )
        .await
        .unwrap();

        assert_eq!(resumed.messages().len(), 3);
        assert_eq!(resumed.messages()[0].text_content(), "sys");
        assert_eq!(resumed.messages()[1].text_content(), "hello");
        assert_eq!(resumed.messages()[2].text_content(), "hi there");

        let _ = std::fs::remove_dir_all(&dir);
    });
}

#[test]
fn session_save_replaces_snapshot_without_duplicates() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let dir =
            std::env::temp_dir().join(format!("tiny-agent-snapshot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let storage = Arc::new(JsonlStorage::new(&dir).unwrap());

        let provider = MockProvider::new(vec![CompletionResponse {
            message: Message::assistant("first"),
            stop_reason: StopReason::EndTurn,
            usage: None,
            model: None,
        }]);
        let tools = ToolRegistry::new();
        let mut session = AgentSession::new(AgentConfig {
            storage: Some(storage.clone()),
            ..AgentConfig::default()
        })
        .unwrap();

        session.run_turn(&provider, &tools, "hello").await.unwrap();
        session.save().await.unwrap();
        session.save().await.unwrap();

        let loaded = storage.load(session.session_id()).await.unwrap();
        assert_eq!(loaded.len(), session.messages().len());

        let _ = std::fs::remove_dir_all(&dir);
    });
}
