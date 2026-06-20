//! DeepSeek API provider.
//!
//! DeepSeek's chat API is close to OpenAI's shape, but exposes provider-specific
//! fields such as `thinking` and cache-token usage details. This provider keeps
//! those fields native instead of routing through OpenAI SDK structs.

mod client;
mod config;
mod error;
mod request;
mod response;
mod stream;
mod types;
mod url;

#[cfg(test)]
mod tests;

pub use client::DeepSeekProvider;
pub use config::DeepSeekConfig;
pub use types::{
    DeepSeekBalance, DeepSeekBalanceInfo, DeepSeekChatOptions, DeepSeekFimChoice,
    DeepSeekFimRequest, DeepSeekFimResponse, DeepSeekModel, DeepSeekModelList,
    DeepSeekResponseFormat,
};
