use crate::agent::prompt::PromptBlock;
use crate::error::AgentError;
use crate::model::message::{Message, SystemReminder, ToolResult};

pub(crate) struct Conversation {
    pub(crate) messages: Vec<Message>,
    pub(crate) cached_system_prompt_blocks: Option<Vec<PromptBlock>>,
    pub(crate) last_memory_injection_fingerprint: Option<u64>,
    pub(crate) last_skill_injection_fingerprint: Option<u64>,
    pub(crate) memory_state_dirty: bool,
    pub(crate) current_turn_memory_injected: bool,
    pub(crate) current_turn_memory_mutation_notified: bool,
}

impl Conversation {
    pub(crate) fn journal(&mut self) -> ConversationJournal<'_> {
        ConversationJournal { conversation: self }
    }
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
            cached_system_prompt_blocks: None,
            last_memory_injection_fingerprint: None,
            last_skill_injection_fingerprint: None,
            memory_state_dirty: false,
            current_turn_memory_injected: false,
            current_turn_memory_mutation_notified: false,
        }
    }

    pub fn with_messages(messages: Vec<Message>) -> Self {
        Self { messages, ..Self::new() }
    }

    pub fn reset(&mut self) {
        let system_msg = self
            .messages
            .first()
            .filter(|m| m.role == crate::model::message::Role::System)
            .cloned();
        self.messages = system_msg.into_iter().collect();
        self.cached_system_prompt_blocks = None;
        self.last_memory_injection_fingerprint = None;
        self.last_skill_injection_fingerprint = None;
        self.memory_state_dirty = false;
        self.current_turn_memory_injected = false;
        self.current_turn_memory_mutation_notified = false;
    }

    pub fn initial_messages(&mut self, config: &crate::config::AgentConfig) {
        if config.prompt_assembly.is_none()
            && let Some(sp) = config.base_system_prompt.as_ref()
        {
            self.messages.push(Message::system(sp.clone()));
        }
    }
}

/// Protocol-aware mutation boundary used by the turn transaction.
pub(crate) struct ConversationJournal<'a> {
    conversation: &'a mut Conversation,
}

impl ConversationJournal<'_> {
    pub(crate) fn append_user(&mut self, message: Message) -> Result<(), AgentError> {
        self.ensure_no_unresolved_tools()?;
        self.conversation.messages.push(message);
        Ok(())
    }

    pub(crate) fn append_assistant(&mut self, message: Message) -> Result<(), AgentError> {
        self.ensure_no_unresolved_tools()?;
        self.conversation.messages.push(message);
        Ok(())
    }

    pub(crate) fn resolve_tool_calls(&mut self, message: Message) -> Result<(), AgentError> {
        let expected: Vec<_> = self
            .conversation
            .messages
            .last()
            .filter(|message| message.role == crate::model::message::Role::Assistant)
            .map(|message| message.tool_calls().map(|call| call.id.as_str()).collect())
            .unwrap_or_default();
        let actual: Vec<_> =
            message.tool_results_iter().map(|result| result.tool_call_id.as_str()).collect();
        if expected.is_empty() || expected != actual {
            return Err(AgentError::Config(format!(
                "tool result protocol mismatch: expected {expected:?}, got {actual:?}"
            )));
        }
        self.conversation.messages.push(message);
        Ok(())
    }

    fn ensure_no_unresolved_tools(&self) -> Result<(), AgentError> {
        if self.conversation.messages.last().is_some_and(|message| {
            message.role == crate::model::message::Role::Assistant
                && message.tool_calls().next().is_some()
        }) {
            return Err(AgentError::Config(
                "cannot append a conversational message before resolving pending tool calls".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod journal_tests {
    use super::*;
    use crate::model::message::{ContentBlock, Role, ToolCall};

    #[test]
    fn feedback_cannot_split_tool_call_and_result() {
        let mut conversation = Conversation::new();
        conversation
            .journal()
            .append_assistant(Message {
                role: Role::Assistant,
                blocks: vec![ContentBlock::ToolCall(ToolCall {
                    id: "call-1".into(),
                    name: "Read".into(),
                    arguments: serde_json::json!({}),
                })],
            })
            .unwrap();
        assert!(conversation.journal().append_user(Message::user("feedback")).is_err());
        conversation
            .journal()
            .resolve_tool_calls(Message::tool_results(vec![ToolResult {
                tool_call_id: "call-1".into(),
                name: "Read".into(),
                content: serde_json::json!({"ok": true}),
                is_error: false,
            }]))
            .unwrap();
        conversation.journal().append_user(Message::user("feedback")).unwrap();
    }
}

impl Conversation {
    pub(crate) fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub(crate) fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }

    pub(crate) fn push_message(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    pub(crate) fn push_system_reminder(&mut self, reminder: SystemReminder) {
        self.messages.push(Message::system(reminder.render()));
    }

    pub(crate) fn cached_system_prompt(&self) -> &Option<Vec<PromptBlock>> {
        &self.cached_system_prompt_blocks
    }

    pub(crate) fn set_cached_system_prompt(&mut self, blocks: Option<Vec<PromptBlock>>) {
        self.cached_system_prompt_blocks = blocks;
    }

    pub(crate) fn last_memory_fingerprint(&self) -> Option<u64> {
        self.last_memory_injection_fingerprint
    }

    pub(crate) fn set_last_memory_fingerprint(&mut self, fp: Option<u64>) {
        self.last_memory_injection_fingerprint = fp;
    }

    pub(crate) fn last_skill_fingerprint(&self) -> Option<u64> {
        self.last_skill_injection_fingerprint
    }

    pub(crate) fn set_last_skill_fingerprint(&mut self, fp: Option<u64>) {
        self.last_skill_injection_fingerprint = fp;
    }

    pub(crate) fn memory_dirty(&self) -> bool {
        self.memory_state_dirty
    }

    pub(crate) fn set_memory_dirty(&mut self, dirty: bool) {
        self.memory_state_dirty = dirty;
    }

    pub(crate) fn turn_memory_injected(&self) -> bool {
        self.current_turn_memory_injected
    }

    pub(crate) fn set_turn_memory_injected(&mut self, val: bool) {
        self.current_turn_memory_injected = val;
    }

    pub(crate) fn turn_memory_mutation_notified(&self) -> bool {
        self.current_turn_memory_mutation_notified
    }

    pub(crate) fn set_turn_memory_mutation_notified(&mut self, val: bool) {
        self.current_turn_memory_mutation_notified = val;
    }

    pub(crate) fn repair_incomplete_tool_call_tail(&mut self) {
        let Some(last_message) = self.messages.last() else {
            return;
        };
        if last_message.role != crate::model::message::Role::Assistant {
            return;
        }

        let tool_results = last_message
            .tool_calls()
            .map(|call| ToolResult {
                tool_call_id: call.id.clone(),
                name: call.name.clone(),
                content: serde_json::json!({
                    "error": {
                        "kind": "cancelled",
                        "message": "Tool execution was interrupted before a result was recorded."
                    }
                }),
                is_error: true,
            })
            .collect::<Vec<_>>();

        if !tool_results.is_empty() {
            self.messages.push(Message::tool_results(tool_results));
        }
    }
}
