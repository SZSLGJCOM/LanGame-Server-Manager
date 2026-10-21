use std::collections::BTreeSet;
use std::path::Path;

use super::{Candidate, DstMapIconSources, MAX_XML_BYTES, ModAssets, ReadBudget, atlas, source};

pub(super) fn candidate(
    root: &Path,
    mods: &ModAssets,
    sources: DstMapIconSources<'_>,
    name: &str,
    budget: &mut ReadBudget,
) -> Result<Option<Candidate>, String> {
    if sources.entity_inventory.len() > super::super::MAX_ICONS {
        return Err("entity inventory source item limit exceeded".into());
    }
    let mut declarations = sources
        .entity_inventory
        .iter()
        .filter(|source| source.icon.eq_ignore_ascii_case(name));
    let Some(declared) = declarations.next() else {
        return Ok(None);
    };
    if declarations.any(|other| {
        other.atlas != declared.atlas || !other.element.eq_ignore_ascii_case(&declared.element)
    }) {
        return Err("conflicting entity inventory declarations".into());
    }
    if declared.atlas.len() > 512
        || declared.element.len() > 128
        || !atlas::valid_element_name(&declared.element)
    {
        return Err("invalid entity inventory declaration".into());
    }
    let (asset_root, relative) = source(root, mods, &declared.atlas)?;
    let xml = budget
        .read(asset_root, &relative, MAX_XML_BYTES)?
        .ok_or("declared inventory atlas is not installed as a readable loose resource")?;
    let key = atlas::map_element_key(&declared.element);
    let (parsed, _) = atlas::parse_selected_map_elements(&xml, &BTreeSet::from([key.clone()]))?;
    let rectangle = *parsed
        .elements
        .get(&key)
        .ok_or("declared inventory element is absent")?;
    Ok(Some(Candidate {
        atlas: declared.atlas.clone(),
        element: parsed.element_names[&key].clone(),
        rectangle,
        texture_filename: parsed.texture_filename,
        straight_alpha: parsed.straight_alpha,
    }))
}
