use crate::agent::prompt::PromptBlock;
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
