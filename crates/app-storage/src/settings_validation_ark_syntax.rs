#[derive(Debug)]
pub(super) enum NativeValue<'a> {
    Scalar(&'a str),
    Group(Vec<(Option<&'a str>, NativeValue<'a>)>),
}

struct Parser<'a> {
    text: &'a str,
    position: usize,
}

pub(super) fn parse_tuple(text: &str) -> Result<NativeValue<'_>, String> {
    let mut parser = Parser { text, position: 0 };
    parser.space();
    if parser.peek() != Some(b'(') {
        return Err(String::from("expected a parenthesized native rule"));
    }
    let result = parser.value(0)?;
    parser.space();
    if parser.position != text.len() {
        return Err(String::from("unexpected text after native rule"));
    }
    Ok(result)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.position).copied()
    }
    fn space(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<NativeValue<'a>, String> {
        self.space();
        if depth > 64 {
            return Err(String::from("native rule nesting exceeds 64 levels"));
        }
        match self.peek() {
            Some(b'(') => self.group(depth + 1),
            Some(b'"') => {
                let start = self.position;
                self.position += 1;
                while let Some(byte) = self.peek() {
                    self.position += 1;
                    match byte {
                        b'\\' => {
                            if self.peek().is_none() {
                                break;
                            }
                            self.position += 1;
                        }
                        b'"' => return Ok(NativeValue::Scalar(&self.text[start..self.position])),
                        _ => {}
                    }
                }
                Err(String::from("unterminated quoted value"))
            }
            _ => {
                let start = self.position;
                while self
                    .peek()
                    .is_some_and(|byte| !matches!(byte, b',' | b')' | b'(' | b'=' | b'"'))
                {
                    self.position += 1;
                }
                let value = self.text[start..self.position].trim();
                if value.is_empty() {
                    Err(String::from("native rule contains an empty value"))
                } else {
                    Ok(NativeValue::Scalar(value))
                }
            }
        }
    }

    fn group(&mut self, depth: usize) -> Result<NativeValue<'a>, String> {
        self.position += 1;
        self.space();
        let mut entries = Vec::new();
        if self.peek() == Some(b')') {
            self.position += 1;
            return Ok(NativeValue::Group(entries));
        }
        loop {
            self.space();
            let start = self.position;
            let mut scan = start;
            while self
                .text
                .as_bytes()
                .get(scan)
                .is_some_and(|byte| !matches!(byte, b'=' | b',' | b'(' | b')' | b'"'))
            {
                scan += 1;
            }
            let name = if self.text.as_bytes().get(scan) == Some(&b'=') {
                let name = self.text[start..scan].trim();
                if name.is_empty() || name.chars().any(char::is_whitespace) {
                    return Err(String::from("native property name is invalid"));
                }
                self.position = scan + 1;
                Some(name)
            } else {
                None
            };
            entries.push((name, self.value(depth)?));
            self.space();
            match self.peek() {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b')') => {
                    self.position += 1;
                    return Ok(NativeValue::Group(entries));
                }
                _ => {
                    return Err(String::from(
                        "native rule needs a comma or closing parenthesis",
                    ));
                }
            }
        }
    }
}

pub(super) fn nonnegative_number(text: &str, integer: bool) -> Result<f64, String> {
    let value = text
        .parse::<f64>()
        .map_err(|_| String::from("value must be numeric"))?;
    if !value.is_finite() || value < 0.0 || (integer && value.fract() != 0.0) {
        Err(format!(
            "value must be a finite non-negative {}",
            if integer { "integer" } else { "number" }
        ))
    } else {
        Ok(value)
    }
}

pub(super) fn native_index(text: &str) -> Result<u64, String> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(String::from("index must be a non-negative integer"));
    }
    text.parse::<u64>()
        .map_err(|_| String::from("index is too large"))
}

pub(super) fn validate_numeric_properties(value: &NativeValue<'_>) -> Result<(), String> {
    let NativeValue::Group(entries) = value else {
        return Ok(());
    };
    for (name, value) in entries {
        if let Some(name) = name {
            let integer = matches!(
                *name,
                "LevelToAutoUnlock"
                    | "EngramPointsCost"
                    | "EngramLevelRequirement"
                    | "EngramIndex"
                    | "MinItemSets"
                    | "MaxItemSets"
                    | "MinNumItems"
                    | "MaxNumItems"
                    | "MaxItemQuantity"
            );
            let probability = matches!(
                *name,
                "ChanceToBeBlueprintOverride"
                    | "SpawnLimitPercentage"
                    | "MaxPercentageOfDesiredNumToAllow"
            );
            let numeric = integer
                || probability
                || matches!(
                    *name,
                    "SpawnWeightMultiplier"
                        | "EntryWeight"
                        | "SetWeight"
                        | "MinQuantity"
                        | "MaxQuantity"
                        | "MinQuality"
                        | "MaxQuality"
                        | "BaseResourceRequirement"
                        | "NumItemSetsPower"
                        | "NumItemsPower"
                        | "ResourceItemAmount"
                        | "Multiplier"
                        | "Weight"
                );
            if numeric {
                let NativeValue::Scalar(text) = value else {
                    return Err(format!("{name} must be numeric"));
                };
                let number = nonnegative_number(text, integer)
                    .map_err(|error| format!("{name}: {error}"))?;
                if probability && number > 1.0 {
                    return Err(format!("{name} must be between 0 and 1"));
                }
            }
            if matches!(*name, "ItemsWeights" | "ItemWeights") {
                let NativeValue::Group(weights) = value else {
                    return Err(String::from("ItemsWeights must be a numeric list"));
                };
                for (name, weight) in weights {
                    let NativeValue::Scalar(text) = weight else {
                        return Err(String::from("ItemsWeights must contain numbers"));
                    };
                    if name.is_some() {
                        return Err(String::from("ItemsWeights must contain unnamed numbers"));
                    }
                    nonnegative_number(text, false)?;
                }
            }
        }
        // Mod extension objects retain their own property semantics. Only descend
        // through native containers whose numeric fields are defined by ARK.
        if name.is_none_or(|name| {
            matches!(
                name,
                "ItemSets"
                    | "ItemEntries"
                    | "BaseCraftingResourceRequirements"
                    | "Quantity"
                    | "NPCSpawnEntries"
                    | "NPCSpawnLimits"
            )
        }) {
            validate_numeric_properties(value)?;
        }
    }
    Ok(())
}
