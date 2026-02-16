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

        // Greedy decode with incremental KV cache.
        // First step: feed pad token (0). Subsequent steps: feed only the new token.
        // The KV cache accumulates key-value pairs across steps.
        let mut decoded_ids: Vec<u32> = vec![0];
        let mut next_input = vec![0u32]; // first step: pad token

        for _ in 0..MAX_DECODE_TOKENS {
            let decoder_input = Tensor::new(next_input.as_slice(), &self.device)
                .map_err(|e| VoicshError::Other(format!("Create decoder input: {e}")))?
                .unsqueeze(0)
                .map_err(|e| VoicshError::Other(format!("Unsqueeze decoder: {e}")))?;

            let logits = self
                .model
                .decode(&decoder_input, &encoder_output)
                .map_err(|e| VoicshError::Other(format!("Decoder forward: {e}")))?;

            // Take last token logits (seq dim = last position)
            let seq_len = logits
                .dim(1)
                .map_err(|e| VoicshError::Other(format!("Get logits dim: {e}")))?;
            let next_logits = logits
                .get_on_dim(1, seq_len - 1)
                .map_err(|e| VoicshError::Other(format!("Slice logits: {e}")))?;

            let argmax = next_logits
                .argmax(candle_core::D::Minus1)
                .map_err(|e| VoicshError::Other(format!("Argmax: {e}")))?;
            let next_token = argmax
                .reshape(())
                .map_err(|e| VoicshError::Other(format!("Reshape argmax: {e}")))?
                .to_scalar::<u32>()
                .map_err(|e| VoicshError::Other(format!("Token scalar: {e}")))?;

            // EOS token (1 for T5)
            if next_token == 1 {
                break;
            }

            decoded_ids.push(next_token);
            next_input = vec![next_token]; // incremental: only the new token
        }

        // Skip the leading pad token for decoding
        let output = self
            .tokenizer
            .decode(&decoded_ids[1..], true)
            .map_err(|e| VoicshError::Other(format!("Detokenize: {e}")))?;

        Ok(output)
    }
}

impl Corrector for CandleT5Corrector {
    fn correct(&mut self, prompt: &str) -> Result<String> {
        self.model.clear_kv_cache();
        self.generate(prompt)
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
}
