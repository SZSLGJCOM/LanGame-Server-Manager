use std::collections::BTreeSet;

pub(super) type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy)]
pub(super) struct Chunk<'a> {
    pub tag: [u8; 4],
    pub body: &'a [u8],
    pub raw: &'a [u8],
}

pub(super) struct Reader<'a> {
    pub data: &'a [u8],
    pub position: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    pub fn take(&mut self, size: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(size)
            .ok_or("SPUD length overflow")?;
        let result = self
            .data
            .get(self.position..end)
            .ok_or("Truncated SPUD data")?;
        self.position = end;
        Ok(result)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().map_err(|_| "Invalid u16")?,
        ))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().map_err(|_| "Invalid u32")?,
        ))
    }

    pub fn count(&mut self, minimum_bytes: usize) -> Result<usize> {
        let count = usize::try_from(self.u32()?).map_err(|_| "SPUD count overflow")?;
        if count > self.data.len().saturating_sub(self.position) / minimum_bytes {
            return Err("SPUD count exceeds available data".into());
        }
        Ok(count)
    }

    pub fn string(&mut self) -> Result<String> {
        let count = i32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| "Invalid string length")?,
        );
        if count == 0 {
            return Ok(String::new());
        }
        let magnitude =
            usize::try_from(count.unsigned_abs()).map_err(|_| "String length overflow")?;
        let result = if count > 0 {
            let bytes = self.take(magnitude)?;
            if bytes.last() != Some(&0) {
                return Err("SPUD string has no terminator".into());
            }
            String::from_utf8(bytes[..bytes.len() - 1].to_vec())
                .map_err(|_| "Invalid SPUD UTF-8 string")?
        } else {
            let size = magnitude.checked_mul(2).ok_or("String length overflow")?;
            let bytes = self.take(size)?;
            if !bytes.ends_with(&[0, 0]) {
                return Err("SPUD Unicode string has no terminator".into());
            }
            let units: Vec<u16> = bytes[..bytes.len() - 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                .collect();
            String::from_utf16(&units).map_err(|_| "Invalid SPUD UTF-16 string")?
        };
        if result.contains('\0') {
            return Err("SPUD string contains an embedded terminator".into());
        }
        Ok(result)
    }

    pub fn finish(&self) -> Result<()> {
        if self.position != self.data.len() {
            return Err("Unexpected trailing SPUD data".into());
        }
        Ok(())
    }
}

pub(super) fn chunks(mut data: &[u8]) -> Result<Vec<Chunk<'_>>> {
    let mut result = Vec::new();
    while !data.is_empty() {
        let mut reader = Reader::new(data);
        let tag = reader
            .take(4)?
            .try_into()
            .map_err(|_| "Invalid chunk tag")?;
        let size = usize::try_from(reader.u32()?).map_err(|_| "Chunk size overflow")?;
        let body = reader.take(size)?;
        result.push(Chunk {
            tag,
            body,
            raw: &data[..reader.position],
        });
        data = &data[reader.position..];
    }
    Ok(result)
}

pub(super) fn unique(chunks: &[Chunk<'_>], tag: &[u8; 4]) -> Result<usize> {
    let mut matching = chunks.iter().enumerate().filter(|(_, c)| &c.tag == tag);
    let first = matching
        .next()
        .ok_or_else(|| format!("Missing {} chunk", String::from_utf8_lossy(tag)))?;
    if matching.next().is_some() {
        return Err(format!("Duplicate {} chunks", String::from_utf8_lossy(tag)));
    }
    Ok(first.0)
}

pub(super) fn encode_chunk(tag: &[u8; 4], body: &[u8]) -> Result<Vec<u8>> {
    let mut result = tag.to_vec();
    put_count(&mut result, body.len())?;
    result.extend_from_slice(body);
    Ok(result)
}

pub(super) fn replace_chunk(chunks: &[Chunk<'_>], index: usize, replacement: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for (position, chunk) in chunks.iter().enumerate() {
        result.extend_from_slice(if position == index {
            replacement
        } else {
            chunk.raw
        });
    }
    result
}

pub(super) fn put_count(output: &mut Vec<u8>, value: usize) -> Result<()> {
    output.extend_from_slice(
        &u32::try_from(value)
            .map_err(|_| "SPUD output exceeds u32 capacity")?
            .to_le_bytes(),
    );
    Ok(())
}

pub(super) fn encode_string(value: &str) -> Result<Vec<u8>> {
    if value.contains('\0') {
        return Err("Cannot serialize a string containing a terminator".into());
    }
    let mut result = Vec::new();
    if value.is_ascii() {
        let size = value.len().checked_add(1).ok_or("String length overflow")?;
        result.extend_from_slice(
            &i32::try_from(size)
                .map_err(|_| "String is too long")?
                .to_le_bytes(),
        );
        result.extend_from_slice(value.as_bytes());
        result.push(0);
    } else {
        let units: Vec<u16> = value.encode_utf16().collect();
        let count = i32::try_from(units.len().checked_add(1).ok_or("String length overflow")?)
            .map_err(|_| "String is too long")?;
        result.extend_from_slice(&(-count).to_le_bytes());
        for unit in units {
            result.extend_from_slice(&unit.to_le_bytes());
        }
        result.extend_from_slice(&[0, 0]);
    }
    Ok(result)
}

pub(super) struct Table<'a> {
    pub fields: Vec<&'a [u8]>,
}

impl<'a> Table<'a> {
    pub fn read(reader: &mut Reader<'a>) -> Result<Self> {
        let count = reader.count(4)?;
        let offsets: Vec<usize> = (0..count)
            .map(|_| reader.u32().map(|v| v as usize))
            .collect::<Result<_>>()?;
        let size = usize::try_from(reader.u32()?).map_err(|_| "Property data size overflow")?;
        let data = reader.take(size)?;
        reader.finish()?;
        if offsets.first().is_some_and(|v| *v != 0)
            || offsets.windows(2).any(|v| v[0] >= v[1])
            || offsets.last().is_some_and(|v| *v >= size)
            || (offsets.is_empty() && !data.is_empty())
        {
            return Err("Invalid SPUD property offsets".into());
        }
        let fields = offsets
            .iter()
            .enumerate()
            .map(|(i, start)| &data[*start..offsets.get(i + 1).copied().unwrap_or(size)])
            .collect();
        Ok(Self { fields })
    }

    pub fn encode(fields: &[Vec<u8>]) -> Result<Vec<u8>> {
        let mut result = Vec::new();
        put_count(&mut result, fields.len())?;
        let mut size = 0usize;
        for field in fields {
            if field.is_empty() {
                return Err("Empty SPUD property data is unsupported".into());
            }
            put_count(&mut result, size)?;
            size = size
                .checked_add(field.len())
                .ok_or("Property data overflow")?;
        }
        put_count(&mut result, size)?;
        for field in fields {
            result.extend_from_slice(field);
        }
        Ok(result)
    }
}

pub(super) struct NamedTable<'a> {
    pub names: Vec<String>,
    pub raw_names: Vec<&'a [u8]>,
    pub table: Table<'a>,
}

impl<'a> NamedTable<'a> {
    pub fn read(data: &'a [u8]) -> Result<Self> {
        let mut reader = Reader::new(data);
        let count = reader.count(4)?;
        let mut names = Vec::new();
        let mut raw_names = Vec::new();
        let mut seen = BTreeSet::new();
        for _ in 0..count {
            let start = reader.position;
            let name = reader.string()?;
            if !seen.insert(name.clone()) {
                return Err("Duplicate CINF property names".into());
            }
            names.push(name);
            raw_names.push(&data[start..reader.position]);
        }
        let table = Table::read(&mut reader)?;
        if table.fields.len() != names.len() {
            return Err("CINF names and offsets differ".into());
        }
        Ok(Self {
            names,
            raw_names,
            table,
        })
    }

    pub fn field(&self, name: &str) -> Result<&'a [u8]> {
        let index = self
            .names
            .iter()
            .position(|v| v == name)
            .ok_or_else(|| format!("Missing CINF {name}"))?;
        Ok(self.table.fields[index])
    }

    pub fn encode(names: &[Vec<u8>], fields: &[Vec<u8>]) -> Result<Vec<u8>> {
        if names.len() != fields.len() {
            return Err("CINF output names and fields differ".into());
        }
        let mut result = Vec::new();
        put_count(&mut result, names.len())?;
        for name in names {
            result.extend_from_slice(name);
        }
        result.extend_from_slice(&Table::encode(fields)?);
        Ok(result)
    }
}

pub(super) struct Definition {
    pub class_name: String,
    pub properties: Vec<(String, Option<String>, u16)>,
}

fn string_array(data: &[u8]) -> Result<Vec<String>> {
    let mut reader = Reader::new(data);
    let count = reader.count(4)?;
    let result = (0..count).map(|_| reader.string()).collect::<Result<_>>()?;
    reader.finish()?;
    Ok(result)
}

pub(super) fn metadata(data: &[u8]) -> Result<Vec<Definition>> {
    let children = chunks(data)?;
    let property_names = string_array(children[unique(&children, b"PNIX")?].body)?;
    let class_names = string_array(children[unique(&children, b"CNIX")?].body)?;
    let classes = chunks(children[unique(&children, b"CLST")?].body)?;
    let mut result = Vec::new();
    for class in classes {
        let body = match &class.tag {
            b"CDEF" => class.body,
            b"CDVE" => {
                let mut reader = Reader::new(class.body);
                if reader.u8()? != 0 {
                    return Err("Unsupported CDVE archive framing".into());
                }
                let nested = chunks(&class.body[reader.position..])?;
                if nested.len() != 1 || nested[0].tag != *b"CDEF" {
                    return Err("Invalid CDVE class definition".into());
                }
                nested[0].body
            }
            _ => return Err("Unsupported class definition chunk".into()),
        };
        let mut reader = Reader::new(body);
        let class_name = reader.string()?;
        let count = usize::from(reader.u16()?);
        if count > body.len().saturating_sub(reader.position) / 10 {
            return Err("Class property count exceeds available data".into());
        }
        let mut properties = Vec::new();
        for _ in 0..count {
            let name = reader.u32()?;
            let prefix = reader.u32()?;
            let kind = reader.u16()?;
            let name = property_names
                .get(name as usize)
                .ok_or("Invalid property name index")?
                .clone();
            let prefix = if prefix == u32::MAX {
                None
            } else {
                Some(
                    property_names
                        .get(prefix as usize)
                        .ok_or("Invalid property prefix index")?
                        .clone(),
                )
            };
            properties.push((name, prefix, kind));
        }
        reader.finish()?;
        result.push(Definition {
            class_name,
            properties,
        });
    }
    if class_names.len() != result.len()
        || result
            .iter()
            .zip(class_names)
            .any(|(v, name)| v.class_name != name)
    {
        return Err("SPUD class names and definitions differ".into());
    }
    Ok(result)
}
