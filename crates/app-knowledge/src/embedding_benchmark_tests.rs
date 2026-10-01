//! Opt-in comparison probe for externally supplied, public benchmark texts.
//! This module never downloads a model or discovers any additional input files.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::embedding::{DIMENSIONS, Embedder, MODEL_ID, REVISION};

const MAX_INPUT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_TEXTS: usize = 10_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkInput {
    texts: Vec<BenchmarkText>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BenchmarkText {
    id: String,
    text: String,
    kind: TextKind,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TextKind {
    Query,
    Passage,
}

#[derive(Serialize)]
struct BenchmarkOutput {
    model_id: &'static str,
    revision: &'static str,
    dimensions: usize,
    input_sha256: String,
    load_ms: f64,
    results: Vec<BenchmarkResult>,
}

#[derive(Serialize)]
struct BenchmarkResult {
    id: String,
    vector: Vec<f32>,
    elapsed_ms: f64,
}

fn invalid(message: impl Into<String>) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
}

fn absolute_env_path(name: &str) -> std::io::Result<PathBuf> {
    let path = PathBuf::from(
        std::env::var_os(name).ok_or_else(|| invalid(format!("Set {name} explicitly")))?,
    );
    if !path.is_absolute() {
        return Err(invalid(format!("{name} must be an absolute path")));
    }
    Ok(path)
}

fn outside_repository(path: &Path, repository: &Path) -> std::io::Result<PathBuf> {
    let resolved = std::fs::canonicalize(path)?;
    if resolved.starts_with(repository) {
        return Err(invalid(
            "Benchmark files and model cache must be outside the repository",
        ));
    }
    Ok(resolved)
}

fn read_input(path: &Path) -> Result<(BenchmarkInput, String), Box<dyn std::error::Error>> {
    let file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() > MAX_INPUT_BYTES {
        return Err(invalid("Benchmark input must be a file of at most 16 MiB").into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return Err(invalid("Benchmark input grew beyond 16 MiB").into());
    }
    let input: BenchmarkInput = serde_json::from_slice(&bytes)?;
    validate_input(&input)?;
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((input, digest))
}

fn validate_input(input: &BenchmarkInput) -> std::io::Result<()> {
    if input.texts.is_empty() || input.texts.len() > MAX_TEXTS {
        return Err(invalid(
            "Benchmark input must contain between 1 and 10,000 texts",
        ));
    }
    let mut ids = HashSet::with_capacity(input.texts.len());
    for item in &input.texts {
        if item.id.trim().is_empty() || !ids.insert(item.id.as_str()) {
            return Err(invalid("Benchmark text IDs must be nonempty and unique"));
        }
        if item.text.trim().is_empty() {
            return Err(invalid("Benchmark texts must be nonempty"));
        }
    }
    Ok(())
}

#[test]
#[ignore = "Explicit local embedding comparison; set LANGAME_EMBEDDING_BENCHMARK_INPUT, LANGAME_EMBEDDING_BENCHMARK_OUTPUT and LANGAME_EMBEDDING_MODEL_DIR outside the repository"]
fn local_embedding_comparison_probe() -> Result<(), Box<dyn std::error::Error>> {
    let repository = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))?;
    let input_path = outside_repository(
        &absolute_env_path("LANGAME_EMBEDDING_BENCHMARK_INPUT")?,
        &repository,
    )?;
    let model_dir = outside_repository(
        &absolute_env_path("LANGAME_EMBEDDING_MODEL_DIR")?,
        &repository,
    )?;
    let output_path = absolute_env_path("LANGAME_EMBEDDING_BENCHMARK_OUTPUT")?;
    let output_parent = outside_repository(
        output_path
            .parent()
            .ok_or_else(|| invalid("Benchmark output must have a parent directory"))?,
        &repository,
    )?;
    let output_path = output_parent.join(
        output_path
            .file_name()
            .ok_or_else(|| invalid("Benchmark output must name a file"))?,
    );
    if output_path.try_exists()? {
        return Err(
            invalid("Benchmark output already exists; choose a new result filename").into(),
        );
    }
    let (input, input_sha256) = read_input(&input_path)?;
    let started = Instant::now();
    let model = Embedder::load(&model_dir)?;
    let load_ms = started.elapsed().as_secs_f64() * 1000.0;
    let cancel = AtomicBool::new(false);
    let mut results = Vec::with_capacity(input.texts.len());
    for item in input.texts {
        let started = Instant::now();
        let vector = match item.kind {
            TextKind::Query => model.encode(&item.text)?,
            TextKind::Passage => {
                let mut vectors = model.encode_batch(std::slice::from_ref(&item.text), &cancel)?;
                if vectors.len() != 1 {
                    return Err(
                        invalid("Embedding backend returned an unexpected batch size").into(),
                    );
                }
                vectors.pop().expect("single vector checked above")
            }
        };
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        if vector.len() != DIMENSIONS || vector.iter().any(|value| !value.is_finite()) {
            return Err(invalid("Embedding backend returned invalid dimensions or values").into());
        }
        results.push(BenchmarkResult {
            id: item.id,
            vector,
            elapsed_ms,
        });
    }
    let output = BenchmarkOutput {
        model_id: MODEL_ID,
        revision: REVISION,
        dimensions: DIMENSIONS,
        input_sha256,
        load_ms,
        results,
    };
    // Publish only after every inference succeeded. create_new also rejects a
    // concurrently created result and never overwrites an earlier comparison.
    let mut writer = BufWriter::new(File::create_new(&output_path)?);
    serde_json::to_writer(&mut writer, &output)?;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    eprintln!(
        "Embedding comparison: model={MODEL_ID}@{REVISION}; texts={}; dimensions={DIMENSIONS}; load_ms={load_ms:.3}",
        output.results.len()
    );
    Ok(())
}

#[test]
fn comparison_input_rejects_ambiguous_ids_and_unbounded_text_count() {
    let input: BenchmarkInput = serde_json::from_str(
        r#"{"texts":[{"id":"same","text":"first","kind":"query"},{"id":"same","text":"second","kind":"passage"}]}"#,
    )
    .unwrap();
    assert!(validate_input(&input).is_err());
    let input = BenchmarkInput {
        texts: (0..=MAX_TEXTS)
            .map(|id| BenchmarkText {
                id: id.to_string(),
                text: "public text".into(),
                kind: TextKind::Passage,
            })
            .collect(),
    };
    assert!(validate_input(&input).is_err());
}

#[test]
fn comparison_input_preserves_public_text_and_rejects_unknown_kinds() {
    let input: BenchmarkInput = serde_json::from_str(
        r#"{"texts":[{"id":"question","text":"服务器上限是多少？","kind":"query"},{"id":"manual","text":"Set MaxPlayers to limit the player count.","kind":"passage"}]}"#,
    )
    .unwrap();
    validate_input(&input).unwrap();
    assert_eq!(input.texts[0].text, "服务器上限是多少？");
    assert_eq!(
        input.texts[1].text,
        "Set MaxPlayers to limit the player count."
    );
    assert!(
        serde_json::from_str::<BenchmarkInput>(
            r#"{"texts":[{"id":"bad","text":"public text","kind":"document"}]}"#,
        )
        .is_err()
    );
}
