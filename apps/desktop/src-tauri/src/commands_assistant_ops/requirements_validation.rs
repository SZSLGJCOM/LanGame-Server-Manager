fn assistant_requirement_value_valid(
    value: &Value,
    schema: Option<&Value>,
    actual: Option<&Value>,
) -> bool {
    if schema == Some(&Value::Bool(false)) {
        return false;
    }
    let matches_type = |kind: &str| match kind {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "string" => value.is_string(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "number" => value.is_number(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        _ => false,
    };
    let declared = schema.and_then(|schema| schema.get("type"));
    let type_valid = match declared {
        Some(Value::String(kind)) => matches_type(kind),
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).any(matches_type),
        Some(_) => false,
        None => actual.is_none_or(|actual| match actual {
            Value::Null => value.is_null(),
            Value::Bool(_) => value.is_boolean(),
            Value::Number(number) => {
                if number.is_i64() || number.is_u64() {
                    value.is_i64() || value.is_u64()
                } else {
                    value.is_number()
                }
            }
            Value::String(_) => value.is_string(),
            Value::Array(_) => value.is_array(),
            Value::Object(_) => value.is_object(),
        }),
    };
    if !type_valid {
        return false;
    }
    let Some(schema) = schema else {
        return true;
    };
    if schema
        .get("enum")
        .and_then(Value::as_array)
        .is_some_and(|choices| !choices.contains(value))
        || schema
            .get("const")
            .is_some_and(|expected| expected != value)
    {
        return false;
    }
    if let Some(number) = value.as_f64() {
        for (key, exclusive, lower) in [
            ("minimum", false, true),
            ("maximum", false, false),
            ("exclusiveMinimum", true, true),
            ("exclusiveMaximum", true, false),
        ] {
            if let Some(bound) = schema.get(key).and_then(Value::as_f64)
                && (if lower {
                    number < bound || exclusive && number == bound
                } else {
                    number > bound || exclusive && number == bound
                })
            {
                return false;
            }
        }
    }
    let size = match value {
        Value::String(text) => Some((text.chars().count(), "minLength", "maxLength")),
        Value::Array(items) => Some((items.len(), "minItems", "maxItems")),
        Value::Object(items) => Some((items.len(), "minProperties", "maxProperties")),
        _ => None,
    };
    if let Some((size, minimum, maximum)) = size
        && (schema
            .get(minimum)
            .and_then(Value::as_u64)
            .is_some_and(|bound| (size as u64) < bound)
            || schema
                .get(maximum)
                .and_then(Value::as_u64)
                .is_some_and(|bound| (size as u64) > bound))
    {
        return false;
    }
    true
}
