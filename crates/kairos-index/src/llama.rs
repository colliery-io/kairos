//! The real summarizer: Qwen3-4B-Instruct-2507, GGUF Q4_K_M, through
//! llama.cpp (`llama-cpp-2`). Metal on a Mac, the CPU on Linux
//! (COLLIERY-I-0264, "The runtime").
//!
//! The settings are those of the model comparison
//! (`~/code-index-eval/run_llamacpp.py`): the Qwen chat template with the
//! system prompt, greedy decoding, a context of 8,192 tokens and a cap of 220
//! output tokens. A code that does not fit keeps its first lines and is marked
//! as cut, as in the benchmark.

use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;

use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use crate::IndexError;
use crate::summary::{SYSTEM_PROMPT, Summarizer, SummaryRequest};

/// The context of the model comparison.
const N_CTX: u32 = 8192;
/// The cap of output tokens of the model comparison.
pub const MAX_NEW_TOKENS: usize = 220;
/// Tokens kept free between the prompt and the output, as in the benchmark.
const MARGIN: usize = 8;

/// llama.cpp starts its backend one time for each process.
fn backend() -> Result<&'static LlamaBackend, IndexError> {
    static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            // No llama.cpp or ggml log lines on stderr.
            llama_cpp_2::send_logs_to_tracing(
                llama_cpp_2::LogOptions::default().with_logs_enabled(false),
            );
            LlamaBackend::init().map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| IndexError::Model(format!("llama.cpp did not start: {e}")))
}

/// The model file, loaded. Make a [`LlamaSummarizer`] from it.
pub struct LlamaModelFile {
    model: LlamaModel,
}

impl LlamaModelFile {
    /// Load the GGUF file at `path`. On a Mac, all layers go to the GPU.
    pub fn load(path: &Path) -> Result<Self, IndexError> {
        if !path.is_file() {
            return Err(IndexError::Model(format!(
                "The model file {} is not on disk.",
                path.display()
            )));
        }
        let backend = backend()?;
        let gpu_layers = if cfg!(target_os = "macos") {
            u32::MAX
        } else {
            0
        };
        let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
        let model = LlamaModel::load_from_file(backend, path, &params).map_err(|e| {
            IndexError::Model(format!("Cannot load the model {}: {e}", path.display()))
        })?;
        Ok(LlamaModelFile { model })
    }

    /// A summarizer with its own context of 8,192 tokens.
    pub fn summarizer(&self) -> Result<LlamaSummarizer<'_>, IndexError> {
        let threads = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).max(1))
            .unwrap_or(4) as i32;
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(N_CTX)
            .with_n_threads(threads)
            .with_n_threads_batch(threads);
        let ctx = self
            .model
            .new_context(backend()?, params)
            .map_err(|e| IndexError::Model(format!("Cannot make a model context: {e}")))?;
        Ok(LlamaSummarizer {
            model: &self.model,
            ctx,
            batch: LlamaBatch::new(N_CTX as usize, 1),
        })
    }
}

/// Writes summaries with the loaded model. One request at a time.
pub struct LlamaSummarizer<'m> {
    model: &'m LlamaModel,
    ctx: LlamaContext<'m>,
    batch: LlamaBatch<'static>,
}

impl LlamaSummarizer<'_> {
    /// The prompt in the Qwen3 chat template (ChatML), ready for the
    /// assistant turn. The same text as `apply_chat_template` of the Hugging
    /// Face tokenizer of Qwen3-4B-Instruct-2507.
    fn chat(user: &str) -> String {
        format!(
            "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n\
             <|im_start|>user\n{user}<|im_end|>\n\
             <|im_start|>assistant\n"
        )
    }

    fn tokens(&self, user: &str) -> Vec<LlamaToken> {
        self.model
            .vocab()
            .tokenize(Self::chat(user).as_bytes(), false, true)
    }

    /// The prompt tokens of `request`. If the code does not fit in the
    /// context, keep its first lines, as the benchmark did.
    fn prompt_tokens(&self, request: &SummaryRequest) -> Vec<LlamaToken> {
        let limit = N_CTX as usize - MAX_NEW_TOKENS - MARGIN;
        let mut tokens = self.tokens(&request.prompt());
        let Some(code) = request.code.as_deref() else {
            return tokens;
        };
        let mut lines: Vec<&str> = code.lines().collect();
        while tokens.len() > limit && lines.len() > 1 {
            let keep = (lines.len() * (N_CTX as usize - MAX_NEW_TOKENS - 200) / tokens.len())
                .clamp(1, lines.len() - 1);
            lines.truncate(keep);
            let cut = format!("{}\n// ... (cut)", lines.join("\n"));
            tokens = self.tokens(&request.prompt_with_code(&cut));
        }
        tokens
    }
}

impl Summarizer for LlamaSummarizer<'_> {
    fn summarize(&mut self, request: &SummaryRequest) -> Result<String, String> {
        let prompt = self.prompt_tokens(request);
        if prompt.len() + MAX_NEW_TOKENS + MARGIN > N_CTX as usize {
            return Err(format!(
                "the prompt has {} tokens, and the context has {N_CTX}",
                prompt.len()
            ));
        }
        self.ctx.clear_kv_cache();
        self.batch.clear();
        let last = prompt.len() - 1;
        for (i, token) in prompt.iter().enumerate() {
            self.batch
                .add(*token, i as i32, &[0], i == last)
                .map_err(|e| e.to_string())?;
        }
        self.ctx
            .decode(&mut self.batch)
            .map_err(|e| e.to_string())?;

        let vocab = self.model.vocab();
        let mut sampler = LlamaSampler::greedy();
        let mut output = Vec::new();
        let mut pos = prompt.len() as i32;
        while output.len() < MAX_NEW_TOKENS {
            let token = sampler.sample(&self.ctx, -1);
            if vocab.is_eog(token) {
                break;
            }
            output.push(token);
            self.batch.clear();
            self.batch
                .add(token, pos, &[0], true)
                .map_err(|e| e.to_string())?;
            self.ctx
                .decode(&mut self.batch)
                .map_err(|e| e.to_string())?;
            pos += 1;
        }
        let bytes = vocab.detokenize(&output, false, false);
        Ok(String::from_utf8_lossy(&bytes).trim().to_string())
    }
}
