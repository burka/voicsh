//! Flan-T5 error corrector using candle quantized models.
//!
//! Downloads model artifacts from HuggingFace on first use,
//! then runs greedy T5 decoding to correct ASR errors.

use crate::correction::corrector::Corrector;
use crate::error::{Result, VoicshError};
use crate::models::correction_catalog::CorrectionModelInfo;

use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_t5::{Config as T5Config, T5ForConditionalGeneration};
use candle_transformers::quantized_var_builder::VarBuilder;
use hf_hub::api::sync::Api;
use tokenizers::Tokenizer;

/// Maximum number of tokens to generate during correction.
const MAX_DECODE_TOKENS: usize = 128;

/// Task prefix prepended to all T5 correction prompts.
const TASK_PREFIX: &str = "correct grammar: ";

/// Strip a task-like prefix ending with ": " from model output.
fn strip_task_prefix(text: &str) -> &str {
    if let Some(colon_pos) = text.find(": ") {
        let prefix = &text[..colon_pos];
        let word_count = prefix.split_whitespace().count();
        if word_count <= 4 {
            return &text[colon_pos + 2..];
        }
    }
    text
}

/// Clean T5 model output: strip echoed task prefix and reject garbage.
fn clean_t5_output(raw_output: &str) -> Option<String> {
    let text = strip_task_prefix(raw_output).trim();
    if text.is_empty() {
        return None;
    }
    // Reject garbage: mostly non-alphanumeric
    let alnum_count = text.chars().filter(|c| c.is_alphanumeric()).count();
    let total = text.chars().count();
    if total > 2 && alnum_count * 3 < total {
        return None;
    }
    Some(text.to_string())
}

/// Flan-T5 corrector that runs quantized inference via candle.
pub struct CandleT5Corrector {
    model: T5ForConditionalGeneration,
    tokenizer: Tokenizer,
    device: Device,
    model_name: String,
}

impl CandleT5Corrector {
    /// Load a quantized Flan-T5 model from HuggingFace cache.
    ///
    /// Downloads model, config, and tokenizer on first call.
    pub fn load(info: &CorrectionModelInfo) -> Result<Self> {
        let device = Device::Cpu;
        let api = Api::new().map_err(|e| VoicshError::Other(format!("HF Hub API init: {e}")))?;
        let repo = api.model(info.hf_repo.to_string());

        // Download / resolve paths
        let model_path = repo
            .get(info.hf_filename)
            .map_err(|e| VoicshError::Other(format!("Download model {}: {e}", info.hf_filename)))?;

        let config_path = repo.get(info.config_filename).map_err(|e| {
            VoicshError::Other(format!("Download config {}: {e}", info.config_filename))
        })?;

        let tokenizer_path = repo
            .get(crate::models::correction_catalog::TOKENIZER_FILENAME)
            .map_err(|e| VoicshError::Other(format!("Download tokenizer: {e}")))?;

        // Load config
        let config_bytes = std::fs::read(&config_path).map_err(|e| {
            VoicshError::Other(format!("Read config {}: {e}", config_path.display()))
        })?;
        let config: T5Config = serde_json::from_slice(&config_bytes)
            .map_err(|e| VoicshError::Other(format!("Parse T5 config: {e}")))?;

        // Load quantized model
        let vb = VarBuilder::from_gguf(&model_path, &device).map_err(|e| {
            VoicshError::Other(format!("Load GGUF model {}: {e}", model_path.display()))
        })?;
        let model = T5ForConditionalGeneration::load(vb, &config)
            .map_err(|e| VoicshError::Other(format!("Init T5 model: {e}")))?;

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(&tokenizer_path).map_err(|e| {
            VoicshError::Other(format!("Load tokenizer {}: {e}", tokenizer_path.display()))
        })?;

        Ok(Self {
            model,
            tokenizer,
            device,
            model_name: info.name.to_string(),
        })
    }

    /// Encode input text and run greedy decoding.
    fn generate(&mut self, prompt: &str) -> Result<String> {
        let encoding = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| VoicshError::Other(format!("Tokenize: {e}")))?;

        let input_ids: Vec<u32> = encoding.get_ids().to_vec();
        let input_tensor = Tensor::new(input_ids.as_slice(), &self.device)
            .map_err(|e| VoicshError::Other(format!("Create input tensor: {e}")))?
            .unsqueeze(0)
            .map_err(|e| VoicshError::Other(format!("Unsqueeze input: {e}")))?;

        // Encode
        let encoder_output = self
            .model
            .encode(&input_tensor)
            .map_err(|e| VoicshError::Other(format!("Encoder forward: {e}")))?;

        // Greedy decode — follows candle's quantized-t5 example.
        // decode() may return [batch, vocab] or [batch, seq, vocab] depending
        // on cache state. We squeeze batch, then use argmax on the last dim.
        let mut output_ids = vec![0u32]; // decoder_start_token_id = pad = 0

        for step in 0..MAX_DECODE_TOKENS {
            let decoder_input = if step == 0 {
                Tensor::new(output_ids.as_slice(), &self.device)
                    .map_err(|e| VoicshError::Other(format!("Create decoder input: {e}")))?
                    .unsqueeze(0)
                    .map_err(|e| VoicshError::Other(format!("Unsqueeze decoder: {e}")))?
            } else {
                let last = output_ids[output_ids.len() - 1];
                Tensor::new(&[last], &self.device)
                    .map_err(|e| VoicshError::Other(format!("Create decoder input: {e}")))?
                    .unsqueeze(0)
                    .map_err(|e| VoicshError::Other(format!("Unsqueeze decoder: {e}")))?
            };

            let logits = self
                .model
                .decode(&decoder_input, &encoder_output)
                .map_err(|e| VoicshError::Other(format!("Decoder forward: {e}")))?;

            // logits may be [1, V] or [1, S, V]. Squeeze batch, then take
            // argmax over the last dimension (vocab) at the last seq position.
            let logits = logits
                .squeeze(0)
                .map_err(|e| VoicshError::Other(format!("Squeeze batch: {e}")))?;

            let vocab_logits = match logits.dims().len() {
                1 => logits.clone(), // [V] — already a single position
                2 => {
                    // [S, V] — take last sequence position
                    let s = logits
                        .dim(0)
                        .map_err(|e| VoicshError::Other(format!("Get seq dim: {e}")))?;
                    logits
                        .get(s - 1)
                        .map_err(|e| VoicshError::Other(format!("Get last position: {e}")))?
                }
                n => {
                    return Err(VoicshError::Other(format!(
                        "Unexpected logits rank {n}: {:?}",
                        logits.shape()
                    )));
                }
            };

            let next_token = vocab_logits
                .argmax(0)
                .map_err(|e| VoicshError::Other(format!("Argmax: {e}")))?
                .to_scalar::<u32>()
                .map_err(|e| VoicshError::Other(format!("Token scalar: {e}")))?;

            // EOS token (1 for T5)
            if next_token == 1 {
                break;
            }

            output_ids.push(next_token);
        }

        // Skip the leading pad token for decoding
        let output = self
            .tokenizer
            .decode(&output_ids[1..], true)
            .map_err(|e| VoicshError::Other(format!("Detokenize: {e}")))?;

        Ok(output)
    }
}

impl Corrector for CandleT5Corrector {
    fn correct(&mut self, text: &str) -> Result<String> {
        self.model.clear_kv_cache();
        let prompt = format!("{TASK_PREFIX}{text}");
        let raw_output = self.generate(&prompt)?;
        // Clean T5-specific output artifacts
        match clean_t5_output(&raw_output) {
            Some(cleaned) => Ok(cleaned),
            None => Ok(text.to_string()), // Fall back to input on garbage
        }
    }

    fn name(&self) -> &str {
        &self.model_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candle_t5_corrector_is_send() {
        fn assert_send<T: Send + 'static>() {}
        assert_send::<CandleT5Corrector>();
    }

    #[test]
    fn strip_task_prefix_removes_known_prefix() {
        assert_eq!(strip_task_prefix("correct grammar: hello"), "hello");
    }

    #[test]
    fn strip_task_prefix_removes_garbled_prefix() {
        assert_eq!(strip_task_prefix("grammologie correct: hello"), "hello");
    }

    #[test]
    fn strip_task_prefix_preserves_long_text() {
        let text = "this is a very long prefix with many words: hello";
        assert_eq!(strip_task_prefix(text), text);
    }

    #[test]
    fn strip_task_prefix_no_prefix() {
        assert_eq!(strip_task_prefix("hello world"), "hello world");
    }

    #[test]
    fn clean_t5_output_strips_prefix() {
        assert_eq!(
            clean_t5_output("correct grammar: hello world"),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn clean_t5_output_rejects_garbage() {
        assert_eq!(clean_t5_output("- - - - - - - - -"), None);
    }

    #[test]
    fn clean_t5_output_empty_returns_none() {
        assert_eq!(clean_t5_output(""), None);
        assert_eq!(clean_t5_output("   "), None);
    }
}
