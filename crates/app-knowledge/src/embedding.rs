//! Bounded CPU inference for IBM's pinned multilingual Granite INT8 model.
//! The publisher uses CLS pooling, L2 normalization, and no retrieval prefixes.

use std::path::Path;
use std::sync::{Mutex, atomic::AtomicBool};

use ort::session::{Session, builder::GraphOptimizationLevel};
use ort::value::{Tensor, TensorElementType, ValueType};
use serde::Deserialize;
use tokenizers::{Encoding, Tokenizer};

use crate::{KnowledgeError, Result, check_cancel};

#[path = "embedding_download.rs"]
mod download;
pub use download::{DOWNLOAD_BYTES, ensure_model, installed};

pub const MODEL_ID: &str = "ibm-granite/granite-embedding-97m-multilingual-r2";
pub const MODEL_SLUG: &str = "granite-embedding-97m-multilingual-r2-int8";
pub const CHECKPOINT_REVISION: &str = "835ad14087e140460703cf0fae09f97d469d65c2";
// Vector identity includes the artifact and preprocessing contract. A change to
// either must reindex documents even when the upstream checkpoint is unchanged.
pub const REVISION: &str = "835ad14087e140460703cf0fae09f97d469d65c2-quint8-avx2-cls-l2-512";
pub const DIMENSIONS: usize = 384;
// Keep indexing latency and activations bounded; the checkpoint's 32K context
// is unnecessary for short, fully preserved documentation passages.
pub const MAX_TOKENS: usize = 512;
const VOCABULARY_SIZE: usize = 180_000;
const CLS_TOKEN_ID: u32 = 179_934;
const SEP_TOKEN_ID: u32 = 179_938;
const MAX_TEXT_BYTES: usize = 32 * 1024;
const MAX_BATCH_SIZE: usize = 256;
const MAX_BATCH_BYTES: usize = 2 * 1024 * 1024;
const INFERENCE_THREADS: usize = 2;

fn model_error(error: impl std::fmt::Display) -> KnowledgeError {
    KnowledgeError::Model(error.to_string())
}

#[derive(Deserialize)]
struct Configuration {
    model_type: String,
    vocab_size: usize,
    hidden_size: usize,
    num_hidden_layers: usize,
    num_attention_heads: usize,
    intermediate_size: usize,
    max_position_embeddings: usize,
    cls_token_id: u32,
    sep_token_id: u32,
}

pub struct Embedder {
    tokenizer: Tokenizer,
    // Query and indexing share a single bounded forward pass. ORT owns two
    // intra-op CPU threads; idle spinning and parallel graph execution are off.
    session: Mutex<Session>,
}

impl Embedder {
    /// Blocking verification and initialization; invoke from a bounded worker.
    pub fn load(model_dir: &Path) -> Result<Self> {
        validate_config(&download::read_verified(model_dir, &download::FILES[2])?)?;
        download::verify_license(model_dir)?;
        let tokenizer = download::read_verified(model_dir, &download::FILES[1])?;
        let tokenizer = prepare_tokenizer(&tokenizer, VOCABULARY_SIZE)?;
        let probe = encode_tokens(&tokenizer, "server")?;
        if probe.get_ids().first() != Some(&CLS_TOKEN_ID)
            || probe.get_ids().last() != Some(&SEP_TOKEN_ID)
        {
            return Err(model_error(
                "Tokenizer special tokens do not match CLS pooling",
            ));
        }
        let weights = download::read_verified(model_dir, &download::FILES[0])?;
        crate::embedding_runtime::initialize(model_dir)?;
        // The graph is loaded from the owned verified buffer, never a mutable
        // file or external tensor path. Only the bundled CPU backend is enabled.
        let session = Session::builder()
            .map_err(model_error)?
            .with_intra_threads(INFERENCE_THREADS)
            .map_err(model_error)?
            .with_inter_threads(1)
            .map_err(model_error)?
            .with_parallel_execution(false)
            .map_err(model_error)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(model_error)?
            .with_config_entry("session.intra_op.allow_spinning", "0")
            .map_err(model_error)?
            .with_config_entry("session.inter_op.allow_spinning", "0")
            .map_err(model_error)?
            .commit_from_memory(&weights)
            .map_err(model_error)?;
        validate_session(&session)?;
        Ok(Self {
            tokenizer,
            session: Mutex::new(session),
        })
    }

    /// Counts the complete passage and special tokens, without truncation.
    pub fn token_count(&self, text: &str) -> Result<usize> {
        Ok(encode_tokens(&self.tokenizer, text)?.len())
    }

    pub fn encode(&self, text: &str) -> Result<Vec<f32>> {
        self.encode_one(&encode_tokens(&self.tokenizer, text)?)
    }

    fn encode_one(&self, encoding: &Encoding) -> Result<Vec<f32>> {
        validate_encoding(encoding)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| model_error("Inference lock poisoned"))?;
        let ids: Vec<i64> = encoding.get_ids().iter().map(|id| i64::from(*id)).collect();
        let mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|value| i64::from(*value))
            .collect();
        let ids = Tensor::from_array(([1, encoding.len()], ids)).map_err(model_error)?;
        let mask = Tensor::from_array(([1, encoding.len()], mask)).map_err(model_error)?;
        let output = session
            .run(ort::inputs! {
                "input_ids" => ids,
                "attention_mask" => mask,
            })
            .map_err(model_error)?;
        let (shape, hidden) = output["last_hidden_state"]
            .try_extract_tensor::<f32>()
            .map_err(model_error)?;
        cls_pool(shape, hidden, encoding.len())
    }

    /// Sequential inference bounds activation memory and observes cancellation
    /// between short passes, rather than after an entire batch.
    pub fn encode_batch(&self, texts: &[String], cancel: &AtomicBool) -> Result<Vec<Vec<f32>>> {
        check_cancel(cancel)?;
        validate_batch(texts)?;
        let mut result = Vec::with_capacity(texts.len());
        for text in texts {
            check_cancel(cancel)?;
            result.push(self.encode(text)?);
        }
        check_cancel(cancel)?;
        Ok(result)
    }
}

fn validate_config(bytes: &[u8]) -> Result<()> {
    let config: Configuration = serde_json::from_slice(bytes).map_err(model_error)?;
    if config.model_type != "modernbert"
        || config.vocab_size != VOCABULARY_SIZE
        || config.hidden_size != DIMENSIONS
        || config.num_hidden_layers != 12
        || config.num_attention_heads != 12
        || config.intermediate_size != 1536
        || config.max_position_embeddings != 32_768
        || config.cls_token_id != CLS_TOKEN_ID
        || config.sep_token_id != SEP_TOKEN_ID
    {
        return Err(model_error("Unsupported embedding configuration"));
    }
    Ok(())
}

fn validate_session(session: &Session) -> Result<()> {
    if session.inputs().len() != 2 || session.outputs().len() != 1 {
        return Err(model_error("Unsupported embedding graph inputs or outputs"));
    }
    for name in ["input_ids", "attention_mask"] {
        let valid = session.inputs().iter().any(|input| {
            input.name() == name && matches!(input.dtype(), ValueType::Tensor { ty: TensorElementType::Int64, shape, .. } if shape.len() == 2)
        });
        if !valid {
            return Err(model_error(
                "Embedding input must be a rank-two INT64 tensor",
            ));
        }
    }
    let output = &session.outputs()[0];
    if output.name() != "last_hidden_state"
        || !matches!(output.dtype(), ValueType::Tensor { ty: TensorElementType::Float32, shape, .. } if shape.len() == 3 && shape[2] == DIMENSIONS as i64)
    {
        return Err(model_error("Unsupported embedding output tensor"));
    }
    Ok(())
}

fn validate_batch(texts: &[String]) -> Result<()> {
    if texts.len() > MAX_BATCH_SIZE
        || texts.iter().map(String::len).sum::<usize>() > MAX_BATCH_BYTES
    {
        return Err(KnowledgeError::Invalid(
            "Embedding batch exceeds 256 texts or 2 MiB".into(),
        ));
    }
    Ok(())
}

fn prepare_tokenizer(bytes: &[u8], vocabulary_size: usize) -> Result<Tokenizer> {
    let mut tokenizer = Tokenizer::from_bytes(bytes).map_err(model_error)?;
    if tokenizer.get_vocab_size(true) != vocabulary_size
        || tokenizer
            .get_vocab(true)
            .values()
            .any(|id| *id as usize >= vocabulary_size)
    {
        return Err(model_error(
            "Tokenizer vocabulary does not match learned weights",
        ));
    }
    tokenizer.with_padding(None);
    tokenizer.with_truncation(None).map_err(model_error)?;
    Ok(tokenizer)
}

fn encode_tokens(tokenizer: &Tokenizer, text: &str) -> Result<Encoding> {
    if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES {
        return Err(KnowledgeError::Invalid(
            "Embedding text is empty or exceeds 32 KiB".into(),
        ));
    }
    tokenizer.encode(text, true).map_err(model_error)
}

fn validate_encoding(encoding: &Encoding) -> Result<()> {
    if encoding.is_empty() || encoding.len() > MAX_TOKENS {
        return Err(KnowledgeError::Invalid("Split embedding text into complete chunks of at most 512 tokens, including special tokens".into()));
    }
    if encoding.get_attention_mask().first() != Some(&1) {
        return Err(model_error("Embedding CLS token is masked"));
    }
    Ok(())
}

fn cls_pool(shape: &[i64], hidden: &[f32], tokens: usize) -> Result<Vec<f32>> {
    if tokens == 0
        || shape != [1, tokens as i64, DIMENSIONS as i64]
        || hidden.len() != tokens * DIMENSIONS
        || hidden.iter().any(|value| !value.is_finite())
    {
        return Err(model_error(
            "Embedding has invalid dimensions or non-finite values",
        ));
    }
    let mut vector = hidden[..DIMENSIONS].to_vec();
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if !norm.is_finite() || norm <= f32::MIN_POSITIVE {
        return Err(model_error("Embedding has zero or non-finite norm"));
    }
    for value in &mut vector {
        *value /= norm;
    }
    Ok(vector)
}

#[cfg(test)]
#[path = "embedding_tests.rs"]
mod tests;
