mod common;

use serde_json::Value;
use std::sync::Arc;

use common::*;
use telos_agent::*;

#[test]
fn core_tools_expose_claude_names_and_accept_legacy_aliases() {
    let mut tools = ToolRegistry::new();
    register_core_tools(&mut tools);

    let names =
        tools.definitions().into_iter().map(|definition| definition.name).collect::<Vec<_>>();
    assert!(names.contains(&DefaultShell::current_platform().tool_name().to_string()));
    assert!(names.contains(&"Read".to_string()));
    assert!(names.contains(&"Edit".to_string()));
    assert!(names.contains(&"Write".to_string()));
    assert!(!names.contains(&"shell".to_string()));
    assert!(tools.get("shell").is_ok());
    assert!(tools.get("file_read").is_ok());
}

#[cfg(windows)]
#[test]
fn register_core_tools_uses_powershell_as_native_windows_default_shell() {
    let mut tools = ToolRegistry::new();
    register_core_tools(&mut tools);

    let names =
        tools.definitions().into_iter().map(|definition| definition.name).collect::<Vec<_>>();
    assert!(names.contains(&"PowerShell".to_string()));
    assert!(!names.contains(&"Bash".to_string()));
    assert_eq!(tools.get("shell").unwrap().definition().name, "PowerShell");
}

#[cfg(unix)]
#[test]
fn register_core_tools_uses_bash_as_native_unix_default_shell() {
    let mut tools = ToolRegistry::new();
    register_core_tools(&mut tools);

    let names =
        tools.definitions().into_iter().map(|definition| definition.name).collect::<Vec<_>>();
    assert!(names.contains(&"Bash".to_string()));
    assert!(!names.contains(&"PowerShell".to_string()));
    assert_eq!(tools.get("shell").unwrap().definition().name, "Bash");
}

#[test]
fn default_shell_detects_windows_as_powershell_and_unix_as_bash() {
    assert_eq!(DefaultShell::for_target_os("windows"), DefaultShell::PowerShell);
    assert_eq!(DefaultShell::for_target_os("macos"), DefaultShell::Bash);
    assert_eq!(DefaultShell::for_target_os("linux"), DefaultShell::Bash);
}

#[test]
fn core_tools_can_register_powershell_as_default_shell() {
    let mut tools = ToolRegistry::new();
    register_core_tools_with_shell(&mut tools, DefaultShell::PowerShell);

    let names =
        tools.definitions().into_iter().map(|definition| definition.name).collect::<Vec<_>>();
    assert!(names.contains(&"PowerShell".to_string()));
    assert!(!names.contains(&"Bash".to_string()));
    assert_eq!(tools.get("shell").unwrap().definition().name, "PowerShell");
}

#[test]
fn core_tools_can_register_bash_as_default_shell() {
    let mut tools = ToolRegistry::new();
    register_core_tools_with_shell(&mut tools, DefaultShell::Bash);

    let names =
        tools.definitions().into_iter().map(|definition| definition.name).collect::<Vec<_>>();
    assert!(names.contains(&"Bash".to_string()));
    assert!(!names.contains(&"PowerShell".to_string()));
    assert_eq!(tools.get("shell").unwrap().definition().name, "Bash");
}

#[test]
fn tool_prompt_text_defaults_to_none() {
    struct NoPromptTool;
    #[async_trait::async_trait]
    impl Tool for NoPromptTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "no_prompt".into(),
                description: "x".into(),
                input_schema: serde_json::json!({ "type": "object" }),
            }
        }
        async fn invoke(&self, _args: Value, _ctx: ToolContext) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }
    assert!(NoPromptTool.prompt_text().is_none());
}

#[test]
fn tool_registry_iterates_tools() {
    let mut registry = ToolRegistry::new();
    registry.register(AddTool);
    let names: Vec<_> = registry.iter().map(|(n, _)| n.clone()).collect();
    assert!(names.contains(&"add".to_string()));
}

#[tokio::test]
async fn tool_prompts_section_renders_registered_prompts() {
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use telos_agent::prompt::builtins::ToolPromptsSection;
    use telos_agent::prompt::{PromptSection, PromptStability};
    use telos_agent::tool::{Tool, ToolContext, ToolDefinition, ToolOutput};

    struct PromptedTool;
    #[async_trait]
    impl Tool for PromptedTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "prompted".into(),
                description: "d".into(),
                input_schema: json!({ "type": "object" }),
            }
        }
        fn prompt_text(&self) -> Option<&'static str> {
            Some("Always run this tool first.")
        }
        async fn invoke(&self, _args: Value, _ctx: ToolContext) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    let mut registry = ToolRegistry::new();
    registry.register(PromptedTool);
    let section = ToolPromptsSection::new(Arc::new(registry));
    let text = section.render(&()).await;
    assert!(text.contains("## Tool-specific guidance"));
    assert!(text.contains("prompted"));
    assert!(text.contains("Always run this tool first."));
    assert_eq!(section.stability(), PromptStability::Static);
}

#[test]
fn default_assembly_is_minimal_by_default() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut tools = ToolRegistry::new();
        register_core_tools_with_shell(&mut tools, DefaultShell::PowerShell);
        let assembly = telos_agent::prompt::default_coding_assembly(
            Arc::new(tools),
            std::env::current_dir().unwrap(),
            None,
            telos_agent::TaskPath::default(),
        );
        let text = assembly.build().await;
        assert!(!text.contains("## Tool-specific guidance"));
        assert!(!text.contains("## Available Tools"));
        assert!(!text.contains("Use the PowerShell tool for shell commands"));
        assert!(!text.contains("### PowerShell"));
        assert!(text.contains("You are telos-agent"));
        assert!(text.contains("# Safety"));
    });
}

#[test]
fn full_assembly_includes_tool_prompts() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut tools = ToolRegistry::new();
        register_core_tools_with_shell(&mut tools, DefaultShell::PowerShell);
        let assembly = telos_agent::prompt::default_coding_assembly_for_profile(
            Arc::new(tools),
            std::env::current_dir().unwrap(),
            None,
            telos_agent::TaskPath::default(),
            telos_agent::PromptProfile::Full,
        );
        let text = assembly.build().await;
        assert!(text.contains("## Tool-specific guidance"));
        assert!(text.contains("## Available Tools"));
        assert!(text.contains("Use PowerShell syntax, not Bash syntax"));
        assert!(text.contains("### PowerShell"));
        assert!(text.contains("### Read"));
    });
}
