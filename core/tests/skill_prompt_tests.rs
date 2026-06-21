mod common;

use telos_agent::*;

#[tokio::test]
async fn skill_tool_invokes_and_returns_prompt() {
    use std::sync::Arc;
    use telos_agent::skills::{Skill, SkillArg, SkillRegistry, SkillSource};
    use telos_agent::tool::{Tool, ToolContext};
    use telos_agent::tools::SkillTool;

    let mut reg = SkillRegistry::new();
    reg.register(Skill {
        name: "greet".into(),
        description: "Greets the user".into(),
        when_to_use: None,
        prompt: "Say hello to {{args}}!".into(),
        arguments: vec![SkillArg {
            name: "name".into(),
            description: "Who to greet".into(),
            required: true,
        }],
        body: String::new(),
        source: SkillSource::Bundled,
    });

    let tool = SkillTool::new(Arc::new(reg));
    let def = tool.definition();
    assert_eq!(def.name, "Skill");

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

    let result =
        tool.invoke(serde_json::json!({"skill": "greet", "args": "World"}), ctx).await.unwrap();

    let content = result.content;
    assert!(content["text"].as_str().unwrap().contains("Say hello to World"));
    assert_eq!(content["skill_name"].as_str().unwrap(), "greet");
}

#[tokio::test]
async fn skill_tool_preserves_windows_path_arguments() {
    use std::sync::Arc;
    use telos_agent::skills::{Skill, SkillRegistry, SkillSource};
    use telos_agent::tool::{Tool, ToolContext};
    use telos_agent::tools::SkillTool;

    let mut reg = SkillRegistry::new();
    reg.register(Skill {
        name: "path-check".into(),
        description: "Checks a path".into(),
        when_to_use: None,
        prompt: "Inspect {{args}}".into(),
        arguments: vec![],
        body: r"Use %LOCALAPPDATA%\Telos as fallback.".into(),
        source: SkillSource::Project,
    });
    let tool = SkillTool::new(Arc::new(reg));
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

    let result = tool
        .invoke(serde_json::json!({"skill": "path-check", "args": r"C:\Users\alice\repo"}), ctx)
        .await
        .unwrap();

    let text = result.content["text"].as_str().unwrap();
    assert!(text.contains(r"Inspect C:\Users\alice\repo"));
    assert!(text.contains(r"Use %LOCALAPPDATA%\Telos as fallback."));
}

#[tokio::test]
async fn skill_loader_parses_valid_markdown() {
    use telos_agent::skills::{SkillLoader, SkillSource};

    let dir = tempfile::tempdir().unwrap();
    let skill_content = r#"---
name: test-skill
description: A test skill
whenToUse: When testing
prompt: "You are a test skill. Args: {{args}}"
arguments:
  - name: args
    description: Optional args
    required: false
---
This is the body text.
"#;
    std::fs::write(dir.path().join("test-skill.md"), skill_content).unwrap();

    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();
    assert_eq!(skills.len(), 1);
    let s = &skills[0];
    assert_eq!(s.name, "test-skill");
    assert_eq!(s.description, "A test skill");
    assert_eq!(s.when_to_use, Some("When testing".into()));
    assert!(s.prompt.contains("You are a test skill"));
    assert!(s.body.contains("This is the body text"));
    assert_eq!(s.arguments.len(), 1);
    assert_eq!(s.arguments[0].name, "args");
    assert_eq!(s.source, SkillSource::Project);
}

#[tokio::test]
async fn skill_loader_parses_crlf_markdown_with_windows_paths() {
    use telos_agent::skills::SkillLoader;

    let dir = tempfile::tempdir().unwrap();
    let skill_content = concat!(
        "---\r\n",
        "name: windows-skill\r\n",
        "description: A Windows path skill\r\n",
        "prompt: \"Open {{args}}\"\r\n",
        "---\r\n",
        "Use C:\\\\Users\\\\alice\\\\.telos.\r\n"
    );
    std::fs::write(dir.path().join("windows-skill.md"), skill_content).unwrap();

    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();

    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "windows-skill");
    assert_eq!(skills[0].prompt, "Open {{args}}");
    assert!(skills[0].body.contains(r"C:\\Users\\alice\\.telos"));
}

#[test]
fn skill_loader_empty_directory_returns_empty() {
    use telos_agent::skills::SkillLoader;

    let dir = tempfile::tempdir().unwrap();
    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();
    assert!(skills.is_empty());
}

#[test]
fn skill_loader_skips_non_md_files() {
    use telos_agent::skills::SkillLoader;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("notes.txt"), "not a skill").unwrap();
    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();
    assert!(skills.is_empty());
}

#[test]
fn skill_loader_skips_malformed_yaml() {
    use telos_agent::skills::SkillLoader;

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("bad.md"), "---\nname: bad\nnot: valid:\n---\nbody").unwrap();
    let skills = SkillLoader::load_from_dir(dir.path()).unwrap();
    // Malformed YAML should be gracefully skipped
    assert!(skills.is_empty());
}

#[test]
fn skill_registry_override_priority() {
    use telos_agent::skills::{Skill, SkillRegistry, SkillSource};

    let mut reg = SkillRegistry::new();
    reg.register(Skill {
        name: "my-skill".into(),
        description: "bundled desc".into(),
        when_to_use: Some("for testing".into()),
        prompt: "bundled prompt".into(),
        arguments: vec![],
        body: String::new(),
        source: SkillSource::Bundled,
    });
    reg.register(Skill {
        name: "my-skill".into(),
        description: "user desc".into(),
        when_to_use: Some("for testing".into()),
        prompt: "user prompt".into(),
        arguments: vec![],
        body: String::new(),
        source: SkillSource::User,
    });
    let skill = reg.get("my-skill").unwrap();
    assert_eq!(skill.prompt, "user prompt");
}

#[test]
fn skill_registry_render_for_prompt() {
    use telos_agent::skills::{Skill, SkillArg, SkillRegistry, SkillSource};

    let mut reg = SkillRegistry::new();
    reg.register(Skill {
        name: "verify".into(),
        description: "Verify code changes".into(),
        when_to_use: Some("Before committing".into()),
        prompt: "Verify prompt".into(),
        arguments: vec![SkillArg {
            name: "target".into(),
            description: "What to verify".into(),
            required: false,
        }],
        body: String::new(),
        source: SkillSource::Bundled,
    });
    let rendered = reg.render_for_prompt();
    assert!(rendered.contains("verify"));
    assert!(rendered.contains("Verify code changes"));
    assert!(rendered.contains("Before committing"));
}

#[test]
fn skill_registry_empty_renders_empty_string() {
    use telos_agent::skills::SkillRegistry;
    let reg = SkillRegistry::new();
    assert_eq!(reg.render_for_prompt(), "");
}

#[test]
fn skill_registry_get_missing_returns_none() {
    use telos_agent::skills::SkillRegistry;
    let reg = SkillRegistry::new();
    assert!(reg.get("nonexistent").is_none());
}

#[test]
fn bundled_skills_load_successfully() {
    use telos_agent::skills::SkillLoader;
    let skills = SkillLoader::load_bundled_skills();
    assert!(skills.len() >= 6, "expected >=6 bundled skills, got {}", skills.len());
    for s in &skills {
        assert!(!s.name.is_empty(), "skill has empty name");
        assert!(!s.description.is_empty(), "skill '{}' has empty description", s.name);
        assert!(!s.prompt.is_empty(), "skill '{}' has empty prompt", s.name);
        assert_eq!(s.source, telos_agent::skills::SkillSource::Bundled);
    }
}

#[test]
fn bundled_skills_load_and_render() {
    use telos_agent::skills::SkillRegistry;
    let mut registry = SkillRegistry::new();
    registry.load_bundled_skills();
    assert!(registry.get("explore").is_some());
    let rendered = registry.render_for_prompt();
    assert!(rendered.contains("explore"));
}

#[tokio::test]
async fn prompt_assembly_caches_static_sections() {
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use telos_agent::prompt::{PromptAssembly, PromptSection, PromptStability};

    static CALL_COUNT: AtomicUsize = AtomicUsize::new(0);

    struct StaticSection;
    #[async_trait]
    impl PromptSection for StaticSection {
        fn name(&self) -> &str {
            "static_test"
        }
        fn stability(&self) -> PromptStability {
            PromptStability::Static
        }
        async fn render(&self, _ctx: &()) -> String {
            CALL_COUNT.fetch_add(1, Ordering::Relaxed);
            "static content".into()
        }
    }

    struct DynamicSection;
    #[async_trait]
    impl PromptSection for DynamicSection {
        fn name(&self) -> &str {
            "dynamic_test"
        }
        fn stability(&self) -> PromptStability {
            PromptStability::Dynamic
        }
        async fn render(&self, _ctx: &()) -> String {
            CALL_COUNT.fetch_add(1, Ordering::Relaxed);
            "dynamic content".into()
        }
    }

    let mut assembly = PromptAssembly::new();
    assembly.add(StaticSection);
    assembly.add(DynamicSection);

    let result1 = assembly.build().await;
    assert!(result1.contains("static content"));
    assert!(result1.contains("dynamic content"));

    CALL_COUNT.store(0, Ordering::Relaxed);
    let result2 = assembly.build().await;
    // Static cached: only dynamic re-renders = 1 call
    let calls = CALL_COUNT.load(Ordering::Relaxed);
    assert_eq!(calls, 1, "static section should be cached, only dynamic re-rendered");
    assert!(result2.contains("static content"));
}

#[tokio::test]
async fn builtin_prompt_sections_render_without_error() {
    use telos_agent::prompt::PromptAssembly;
    use telos_agent::prompt::builtins::*;
    use telos_agent::tool::ToolRegistry;

    let mut assembly = PromptAssembly::new();
    assembly.add(IdentitySection::new(Some("Be helpful.".into())));
    assembly.add(ToneStyleSection);
    assembly.add(TaskGuidanceSection);
    assembly.add(SafetySection);
    assembly.add(ToolUsageSection);
    assembly.add(ToolsSection::new(std::sync::Arc::new(ToolRegistry::new())));
    assembly.add(DateSection);
    assembly.add(CwdSection::new(std::env::current_dir().unwrap()));
    assembly.add(GitStatusSection);

    let result = assembly.build().await;
    assert!(result.contains("telos-agent"));
    assert!(result.contains("Tone and style"));
    assert!(result.contains("Doing tasks"));
    assert!(result.contains("Executing actions with care"));
    assert!(result.contains("Using your tools"));
    assert!(result.contains("Today's date"));
    assert!(result.contains("Working directory"));
}

#[tokio::test]
async fn default_coding_assembly_renders_claude_style_sections() {
    use telos_agent::prompt::default_coding_assembly;
    use telos_agent::tool::ToolRegistry;

    let tools = std::sync::Arc::new(ToolRegistry::new());
    let assembly = default_coding_assembly(
        tools,
        std::env::current_dir().unwrap(),
        None,
        telos_agent::TaskPath::default(),
    );
    let result = assembly.build().await;

    assert!(result.contains("You are telos-agent"));
    assert!(result.contains("IMPORTANT: Assist with authorized security testing"));
    assert!(result.contains("# System"));
    assert!(result.contains("# Tone and style"));
    assert!(result.contains("# Output efficiency"));
    assert!(result.contains("# Doing tasks"));
    assert!(result.contains("# Executing actions with care"));
    assert!(result.contains("# Using your tools"));
    assert!(result.contains("Do NOT use the Bash tool to run commands"));
    assert!(result.contains("You can call multiple tools in a single response"));
    assert!(result.contains("Today's date"));
    assert!(result.contains("Working directory"));
}

#[tokio::test]
async fn agent_session_falls_back_to_default_assembly() {
    use telos_agent::mock::MockProvider;
    use telos_agent::provider::{CompletionResponse, StopReason};
    use telos_agent::{Message, ToolRegistry};

    let provider = MockProvider::new(vec![CompletionResponse {
        message: Message::assistant("done"),
        stop_reason: StopReason::EndTurn,
        usage: None,
        model: None,
    }]);

    let config = AgentConfig::default();
    let mut session = AgentSession::new(config).unwrap();
    let tools = ToolRegistry::new();

    let result = session.run_turn(&provider, &tools, "hello").await.unwrap();
    assert_eq!(result.final_message.text_content(), "done");
}

#[test]
fn prompt_assembly_integration_with_session() {
    use async_trait::async_trait;
    use telos_agent::prompt::{PromptAssembly, PromptSection, PromptStability};

    struct TestSection;
    #[async_trait]
    impl PromptSection for TestSection {
        fn name(&self) -> &str {
            "test"
        }
        fn stability(&self) -> PromptStability {
            PromptStability::Static
        }
        async fn render(&self, _ctx: &()) -> String {
            "TEST_SECTION_CONTENT".into()
        }
    }

    let mut assembly = PromptAssembly::new();
    assembly.add(TestSection);

    let config = AgentConfig {
        prompt_assembly: Some(std::sync::Arc::new(assembly)),
        ..AgentConfig::default()
    };

    let session = AgentSession::new(config).unwrap();
    assert!(session.messages().is_empty()); // assembly renders at turn time
}

#[tokio::test]
async fn prompt_assembly_build_blocks_preserves_stability() {
    use telos_agent::prompt::{PromptAssembly, PromptSection, PromptStability};

    struct StaticSection;
    #[async_trait::async_trait]
    impl PromptSection for StaticSection {
        fn name(&self) -> &str {
            "static"
        }
        fn stability(&self) -> PromptStability {
            PromptStability::Static
        }
        async fn render(&self, _ctx: &()) -> String {
            "static text".into()
        }
    }

    struct DynamicSection;
    #[async_trait::async_trait]
    impl PromptSection for DynamicSection {
        fn name(&self) -> &str {
            "dynamic"
        }
        fn stability(&self) -> PromptStability {
            PromptStability::Dynamic
        }
        async fn render(&self, _ctx: &()) -> String {
            "dynamic text".into()
        }
    }

    let mut assembly = PromptAssembly::new();
    assembly.add(StaticSection);
    assembly.add(DynamicSection);
    let blocks = assembly.build_blocks().await;
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].name, "static");
    assert_eq!(blocks[0].stability, PromptStability::Static);
    assert_eq!(blocks[1].name, "dynamic");
    assert_eq!(blocks[1].stability, PromptStability::Dynamic);
}
