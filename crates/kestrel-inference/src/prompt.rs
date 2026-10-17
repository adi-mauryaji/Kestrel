//! Chat wrappers for Tier-4 prompts. The instructions are data
//! (`datasets/contracts/tier4_intent_prompt.txt`, the same for R0 and the cascade's Tier 4);
//! this module adds the model's chat format around them.
//!
//! Qwen3 with thinking off: ChatML plus the empty `<think>` block that Qwen3's template inserts
//! for `enable_thinking=false`, so the answer starts right after it. Other formats (Gemma) are
//! added when those models are evaluated.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatFormat {
    Qwen3NoThink,
}

#[derive(Debug, Clone)]
pub struct ChatPrompt {
    format: ChatFormat,
    system: String,
}

impl ChatPrompt {
    pub fn new(format: ChatFormat, system: &str) -> Self {
        Self {
            format,
            system: system.trim_end().to_string(), // files end with a newline; the template adds its own
        }
    }

    /// Everything before the user's text. The same for every request, so it is KV-cached.
    pub fn prefix(&self) -> String {
        match self.format {
            ChatFormat::Qwen3NoThink => format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n",
                self.system
            ),
        }
    }

    /// The user's text and the cue for the answer.
    pub fn suffix(&self, user: &str) -> String {
        match self.format {
            ChatFormat::Qwen3NoThink => {
                format!("{user}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n")
            }
        }
    }

    /// The token text that ends an answer; it closes every label in the label trie.
    pub fn end_of_answer(&self) -> &'static str {
        match self.format {
            ChatFormat::Qwen3NoThink => "<|im_end|>",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen3_prompt_is_chatml_with_an_empty_think_block() {
        let p = ChatPrompt::new(ChatFormat::Qwen3NoThink, "Pick a label.\n");
        let full = p.prefix() + &p.suffix("remind me to call asha");
        assert_eq!(
            full,
            "<|im_start|>system\nPick a label.<|im_end|>\n<|im_start|>user\n\
             remind me to call asha<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"
        );
        assert_eq!(p.end_of_answer(), "<|im_end|>");
    }

    #[test]
    fn the_prefix_does_not_depend_on_the_request() {
        let p = ChatPrompt::new(ChatFormat::Qwen3NoThink, "x");
        assert!(!p.prefix().contains("asha"));
        assert!(p.suffix("asha").starts_with("asha<|im_end|>"));
    }
}
