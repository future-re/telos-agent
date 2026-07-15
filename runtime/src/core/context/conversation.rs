use crate::message::{Message, SystemReminder, ToolResult};
use crate::prompt::PromptBlock;

pub trait ContextOps {
    fn messages(&self) -> &[Message];
    fn messages_mut(&mut self) -> &mut Vec<Message>;
    fn push_message(&mut self, msg: Message);
    fn push_system_reminder(&mut self, reminder: SystemReminder);
    fn cached_system_prompt(&self) -> &Option<Vec<PromptBlock>>;
    fn set_cached_system_prompt(&mut self, blocks: Option<Vec<PromptBlock>>);
    fn last_memory_fingerprint(&self) -> Option<u64>;
    fn set_last_memory_fingerprint(&mut self, fp: Option<u64>);
    fn last_skill_fingerprint(&self) -> Option<u64>;
    fn set_last_skill_fingerprint(&mut self, fp: Option<u64>);
    fn memory_dirty(&self) -> bool;
    fn set_memory_dirty(&mut self, dirty: bool);
    fn turn_memory_injected(&self) -> bool;
    fn set_turn_memory_injected(&mut self, val: bool);
    fn turn_memory_mutation_notified(&self) -> bool;
    fn set_turn_memory_mutation_notified(&mut self, val: bool);
    fn repair_incomplete_tool_call_tail(&mut self);
}

pub struct Conversation {
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
            .filter(|m| m.role == crate::message::Role::System)
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

impl ContextOps for Conversation {
    fn messages(&self) -> &[Message] {
        &self.messages
    }

    fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.messages
    }

    fn push_message(&mut self, msg: Message) {
        self.messages.push(msg);
    }

    fn push_system_reminder(&mut self, reminder: SystemReminder) {
        self.messages.push(Message::system(reminder.render()));
    }

    fn cached_system_prompt(&self) -> &Option<Vec<PromptBlock>> {
        &self.cached_system_prompt_blocks
    }

    fn set_cached_system_prompt(&mut self, blocks: Option<Vec<PromptBlock>>) {
        self.cached_system_prompt_blocks = blocks;
    }

    fn last_memory_fingerprint(&self) -> Option<u64> {
        self.last_memory_injection_fingerprint
    }

    fn set_last_memory_fingerprint(&mut self, fp: Option<u64>) {
        self.last_memory_injection_fingerprint = fp;
    }

    fn last_skill_fingerprint(&self) -> Option<u64> {
        self.last_skill_injection_fingerprint
    }

    fn set_last_skill_fingerprint(&mut self, fp: Option<u64>) {
        self.last_skill_injection_fingerprint = fp;
    }

    fn memory_dirty(&self) -> bool {
        self.memory_state_dirty
    }

    fn set_memory_dirty(&mut self, dirty: bool) {
        self.memory_state_dirty = dirty;
    }

    fn turn_memory_injected(&self) -> bool {
        self.current_turn_memory_injected
    }

    fn set_turn_memory_injected(&mut self, val: bool) {
        self.current_turn_memory_injected = val;
    }

    fn turn_memory_mutation_notified(&self) -> bool {
        self.current_turn_memory_mutation_notified
    }

    fn set_turn_memory_mutation_notified(&mut self, val: bool) {
        self.current_turn_memory_mutation_notified = val;
    }

    fn repair_incomplete_tool_call_tail(&mut self) {
        let Some(last_message) = self.messages.last() else {
            return;
        };
        if last_message.role != crate::message::Role::Assistant {
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
    }
}
