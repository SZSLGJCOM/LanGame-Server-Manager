use serde::{Deserialize, Deserializer, Serialize, de::Error};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DownloadIntegritySpec {
    #[serde(deserialize_with = "deserialize_sha256")]
    pub sha256: String,
    #[serde(deserialize_with = "deserialize_size")]
    pub size: u64,
}

fn deserialize_sha256<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(D::Error::custom(
            "sha256 must contain exactly 64 hexadecimal digits",
        ));
    }
    Ok(value.to_ascii_lowercase())
}

fn deserialize_size<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value = u64::deserialize(deserializer)?;
    if value == 0 {
        return Err(D::Error::custom("download size must be greater than zero"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integrity_roundtrip_preserves_size_and_normalizes_hex() {
        let value = json!({"sha256": "AB".repeat(32), "size": 29448397});
        let integrity: DownloadIntegritySpec = serde_json::from_value(value).unwrap();
        assert_eq!(integrity.sha256, "ab".repeat(32));
        assert_eq!(integrity.size, 29448397);
        assert_eq!(
            serde_json::from_value::<DownloadIntegritySpec>(
                serde_json::to_value(&integrity).unwrap()
            )
            .unwrap(),
            integrity
        );
    }

    #[test]
    fn malformed_or_partial_integrity_is_rejected() {
        for value in [
            json!({"sha256": "a".repeat(63), "size": 1}),
            json!({"sha256": "g".repeat(64), "size": 1}),
            json!({"sha256": "a".repeat(64), "size": 0}),
            json!({"sha256": "a".repeat(64), "size": -1}),
            json!({"sha256": "a".repeat(64)}),
            json!({"size": 1}),
            json!({"sha256": "a".repeat(64), "size": 1, "sha265": "typo"}),
        ] {
            assert!(
                serde_json::from_value::<DownloadIntegritySpec>(value.clone()).is_err(),
                "{value}"
            );
        }
    }
}
