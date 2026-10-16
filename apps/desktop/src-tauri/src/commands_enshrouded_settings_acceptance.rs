use super::*;

fn equivalent(expected: &Value, observed: &Value) -> bool {
    if expected == observed {
        return true;
    }
    if let Some(value) = observed.as_object().and_then(|object| object.get("value")) {
        return expected == value;
    }
    let (Some(wanted), Some(text)) = (expected.as_f64(), observed.as_str()) else {
        return false;
    };
    // Native float diagnostics use decimal text or IEEE-754 hexadecimal bits.
    let wanted = (wanted as f32).to_bits();
    let decimal_matches = text
        .parse::<f32>()
        .is_ok_and(|actual| actual.to_bits() == wanted);
    let hexadecimal_matches =
        text.len() == 8 && u32::from_str_radix(text, 16).is_ok_and(|bits| bits == wanted);
    decimal_matches || hexadecimal_matches
}

pub(super) fn audit(log: &str, requested: &Value) -> ProbeResult<Value> {
    let text = log
        .split_once("[server] Game Settings 'Custom'")
        .ok_or("missing native Custom settings echo")?
        .1
        .trim_start();
    let effective: Value = serde_json::Deserializer::from_str(text)
        .into_iter::<Value>()
        .next()
        .ok_or("missing native settings object")??;
    let requested = requested
        .as_object()
        .ok_or("requested gameplay settings must be an object")?;
    let effective = effective
        .as_object()
        .ok_or("native gameplay echo must be an object")?;
    if requested.len() != 38
        || effective.len() != 38
        || requested.keys().collect::<std::collections::BTreeSet<_>>()
            != effective.keys().collect::<std::collections::BTreeSet<_>>()
    {
        return Err(
            "review modeled and native gameplay key coverage; expected exact 38 keys".into(),
        );
    }
    let mut rows = Vec::new();
    for (key, expected) in requested {
        let observed = &effective[key];
        if !equivalent(expected, observed) {
            return Err(format!("native effective gameplay setting differs at {key}: requested={expected}, observed={observed}").into());
        }
        rows.push(
            json!({"key": key,"requested": expected,"native_effective": observed,"matched": true}),
        );
    }
    Ok(json!({"modeled_keys": 38,"native_effective_keys": 38,"all_matched": true,"settings": rows}))
}

#[test]
fn native_float_diagnostics_and_duration_wrappers_are_compared_semantically() {
    assert!(equivalent(&json!(0.5), &json!("3f000000")));
    assert!(equivalent(&json!(2.5), &json!("40200000")));
    assert!(equivalent(&json!(2.25), &json!("40100000")));
    assert!(equivalent(&json!(1.0), &json!("1")));
    assert!(equivalent(
        &json!(600000000000u64),
        &json!({"value":600000000000u64})
    ));
    assert!(!equivalent(&json!(0.5), &json!("1")));
    assert!(!equivalent(&json!(1), &json!("Automatic")));
}
