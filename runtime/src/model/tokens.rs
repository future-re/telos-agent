//! Token counting using OpenAI-compatible `cl100k_base` BPE tokenizer.
//!
//! All providers supported by tiny-agent-core (DeepSeek, Kimi, etc.) use
//! tokenizers that are highly compatible with `cl100k_base`. This module
//! provides a shared, lazily-loaded counter that replaces the old 4-char-per-
//! token heuristic with an accurate (±5%) token count.

use std::sync::OnceLock;
use tiktoken_rs::CoreBPE;

/// Global `cl100k_base` BPE instance — loaded once and reused.
static CL100K: OnceLock<Option<CoreBPE>> = OnceLock::new();

fn get_bpe() -> Option<&'static CoreBPE> {
    CL100K
        .get_or_init(|| {
            let bpe = tiktoken_rs::cl100k_base().ok();
            if bpe.is_none() {
                tracing::warn!(
                    "tiktoken-rs failed to load cl100k_base vocabulary; \
                     falling back to char/4 heuristic"
                );
            }
            bpe
        })
        .as_ref()
}

/// Count tokens in `text` using the `cl100k_base` tokenizer.
///
/// If the tokenizer fails to load (e.g. corrupted vocabulary file), falls back
/// to the legacy heuristic: `ceil(text.len() / 4)`.
pub fn count_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    match get_bpe() {
        Some(bpe) => bpe.encode_ordinary(text).len(),
        None => {
            // Legacy fallback — keep the agent running even if the tokenizer
            // vocabulary is somehow unavailable.
            (text.len() as f64 / 4.0_f64).ceil() as usize
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_zero() {
        assert_eq!(count_tokens(""), 0);
    }

    #[test]
    fn english_text_is_counted() {
        // "hello world" should be ~2 tokens with cl100k_base
        let count = count_tokens("hello world");
        assert!(count > 0, "token count should be > 0");
        assert!(count <= 10, "short text shouldn't be many tokens");
    }

    #[test]
    fn chinese_text_is_counted() {
        // Chinese text: each character is typically 1-2 tokens in cl100k_base,
        // NOT 0.25 as the old chars/4 heuristic would give.
        let count = count_tokens("你好世界");
        assert!(count > 0);
        // Old heuristic: 4 chars / 4 = 1 token — wildly underestimates.
        // Actual should be 2-8 tokens.
        assert!(count >= 2, "Chinese chars should be > 1 token (old heuristic was wrong)");
    }

    #[test]
    fn code_text_is_counted() {
        let count = count_tokens("fn main() { println!(\"hi\"); }");
        assert!(count > 0);
        assert!(count <= 50);
    }

    #[test]
    fn larger_text_is_reasonable() {
        let text = "This is a longer piece of text that should be counted. ".repeat(100);
        let count = count_tokens(&text);
        let chars = text.len();
        // Token count should be materially less than chars
        assert!(count < chars);
        // And should be roughly in the expected range
        assert!(count > chars / 5, "token count should be within reasonable range");
    }

    #[test]
    fn count_is_stable() {
        let text = "The quick brown fox jumps over the lazy dog.";
        let a = count_tokens(text);
        let b = count_tokens(text);
        assert_eq!(a, b, "same text should always give same count");
    }
}
