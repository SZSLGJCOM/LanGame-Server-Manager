use serde::{Deserialize, Serialize};

use crate::{KnowledgeError, Result};

/// Publisher content-use ceiling, separate from permission to fetch a URL.
/// Reference permits a local index and short cited excerpts, not full delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentUse {
    Full,
    Reference,
}

impl ContentUse {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Reference => "reference",
        }
    }

    pub(crate) fn from_stored(value: &str) -> Result<Self> {
        match value {
            "full" => Ok(Self::Full),
            "reference" => Ok(Self::Reference),
            _ => Err(KnowledgeError::Policy(
                "Unknown stored content-use policy".into(),
            )),
        }
    }

    pub(crate) fn restrict(self, other: Self) -> Self {
        if self == Self::Reference || other == Self::Reference {
            Self::Reference
        } else {
            Self::Full
        }
    }

    /// Apply a Content-Signal field. Explicit prohibitions override other
    /// declarations; absence of a signal is not a copyright license grant.
    pub(crate) fn apply(self, field: &str) -> Result<Self> {
        let mut use_level = self;
        for directive in field.split(',') {
            let Some((name, value)) = directive.split_once('=') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_ascii_lowercase();
            match (name.as_str(), value.as_str()) {
                ("ai-input" | "search", "no") | ("use", "immediate") => {
                    return Err(KnowledgeError::Policy(format!(
                        "Publisher Content-Signal {name}={value} prevents persistent documentation RAG"
                    )));
                }
                ("use", "reference") => use_level = Self::Reference,
                ("use", "full") => {}
                ("use", _) => {
                    return Err(KnowledgeError::Policy(
                        "Unsupported publisher content-use level".into(),
                    ));
                }
                _ => {}
            }
        }
        Ok(use_level)
    }
}
