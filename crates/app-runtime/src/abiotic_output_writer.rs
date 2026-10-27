use std::io::{self, Write};

use app_core::abiotic_output::REDACTED_MODIO_TOKEN;

/// Only suffixes that could complete a configured token are retained. Ordinary
/// readiness and save output is written immediately, without waiting for a line.
pub(super) struct ModioTokenWriter<W: Write> {
    inner: W,
    tokens: Vec<Vec<u8>>,
    pending: Vec<u8>,
    failed: bool,
}

impl<W: Write> ModioTokenWriter<W> {
    pub(super) fn new(inner: W, tokens: Vec<String>) -> Self {
        let mut tokens = tokens
            .into_iter()
            .filter(|token| !token.is_empty())
            .map(String::into_bytes)
            .collect::<Vec<_>>();
        tokens.sort_by_key(|token| std::cmp::Reverse(token.len()));
        tokens.dedup();
        Self {
            inner,
            tokens,
            pending: Vec::new(),
            failed: false,
        }
    }

    fn previous_failure() -> io::Error {
        io::Error::other("The credential-filtered output sink failed earlier.")
    }
}

impl<W: Write> Write for ModioTokenWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.failed {
            return Err(Self::previous_failure());
        }
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(bytes);
        let mut output = Vec::with_capacity(data.len());
        let mut position = 0;
        while position < data.len() {
            let remaining = &data[position..];
            let mut matched = false;
            let mut incomplete = false;
            for token in &self.tokens {
                if remaining.len() < token.len() && token.starts_with(remaining) {
                    self.pending.extend_from_slice(remaining);
                    incomplete = true;
                    break;
                }
                if remaining.starts_with(token) {
                    output.extend_from_slice(REDACTED_MODIO_TOKEN.as_bytes());
                    position += token.len();
                    matched = true;
                    break;
                }
            }
            if incomplete {
                break;
            }
            if !matched {
                output.push(data[position]);
                position += 1;
            }
        }
        if let Err(error) = self.inner.write_all(&output) {
            self.failed = true;
            self.pending.clear();
            return Err(error);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.failed {
            return Err(Self::previous_failure());
        }
        // A flush is not EOF: exposing a pending prefix here would leak a token
        // completed by the next write. Drop masks an unfinished prefix at EOF.
        self.inner.flush().inspect_err(|_| self.failed = true)
    }
}

impl<W: Write> Drop for ModioTokenWriter<W> {
    fn drop(&mut self) {
        if self.failed {
            return;
        }
        let result = (|| {
            if !self.pending.is_empty() {
                self.inner.write_all(REDACTED_MODIO_TOKEN.as_bytes())?;
            }
            self.inner.flush()
        })();
        if let Err(error) = result {
            // The managed sink also retains its own persistence failure. Never
            // retry raw bytes or expose a credential while reporting cleanup.
            eprintln!("Managed Abiotic output redaction could not finish: {error}");
        }
    }
}

#[cfg(test)]
#[path = "abiotic_output_writer_tests.rs"]
mod tests;
