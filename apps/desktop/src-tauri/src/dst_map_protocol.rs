use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

pub(super) const MAX_TILES: usize = 1_048_576;
pub(super) const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
// Keep headroom for the shared runtime response envelope (16 MiB wire limit).
const MAX_RESPONSE_BYTES: usize = 14 * 1024 * 1024;
// Native stdin accepted 16,300 bytes but stopped processing at 16,400 bytes.
// Bound the complete UTF-8 line, including its newline, below that boundary.
const MAX_QUERY_BYTES: usize = 15 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DstMapPlayer {
    pub id: String,
    pub name: String,
    pub character: String,
    pub icon: Option<String>,
    pub x: f64,
    pub z: f64,
    pub map_x: f64,
    pub map_z: f64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DstMapInventoryIcon {
    pub atlas: String,
    pub element: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DstMapLandmark {
    pub id: String,
    pub prefab: String,
    pub icon: String,
    #[serde(default)]
    pub priority: f64,
    pub map_x: f64,
    pub map_z: f64,
    #[serde(
        default,
        deserialize_with = "inventory_icon",
        skip_serializing_if = "Option::is_none"
    )]
    pub inventory_icon: Option<DstMapInventoryIcon>,
}

fn inventory_icon<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DstMapInventoryIcon>, D::Error> {
    // Optional artwork metadata cannot invalidate an otherwise valid entity.
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value::<DstMapInventoryIcon>(value)
        .ok()
        .filter(|source| {
            !source.atlas.is_empty()
                && !source.element.is_empty()
                && source.atlas.len() <= 512
                && source.element.len() <= 128
                && bounded_text(&source.atlas, 512)
                && bounded_text(&source.element, 128)
        }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DstTerrainKind {
    Land,
    Ocean,
    Void,
    Unknown,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DstMapArt {
    pub textures: BTreeMap<String, String>,
    pub icons: BTreeMap<String, String>,
    #[serde(default)]
    pub icon_fallbacks: Vec<app_modules::DstMapIconFallback>,
    pub terrain_base_color: Option<[u8; 4]>,
    pub terrain_base_colors: BTreeMap<String, [u8; 4]>,
    pub terrain_edges: Vec<DstMapTerrainEdge>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DstMapTerrainLayer {
    pub tile_id: u16,
    pub atlas: Option<String>,
    pub texture: Option<String>,
    pub noise_texture: Option<String>,
    pub sample_style: Option<u8>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DstMapTerrainEdge {
    pub atlas: String,
    pub texture: String,
    pub image: String,
    pub opaque: [f64; 4],
    pub samples: BTreeMap<u8, [f64; 4]>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DstMapSnapshot {
    pub instance_id: String,
    pub shard: String,
    pub run_id: i64,
    pub captured_at_unix_ms: u64,
    pub session: String,
    pub width: usize,
    pub height: usize,
    pub tile_scale: f64,
    pub day: u32,
    pub season: String,
    pub phase: String,
    pub tiles: Option<Vec<u16>>,
    pub tile_names: BTreeMap<u16, String>,
    pub tile_kinds: BTreeMap<u16, DstTerrainKind>,
    pub tile_textures: BTreeMap<u16, String>,
    pub tile_base_textures: BTreeMap<u16, String>,
    pub tile_colors: BTreeMap<u16, [u8; 4]>,
    pub terrain_layers: Vec<DstMapTerrainLayer>,
    pub landmarks: Option<Vec<DstMapLandmark>>,
    pub roads: Option<Vec<Vec<[f64; 2]>>>,
    pub landmarks_truncated: bool,
    pub art: Option<DstMapArt>,
    pub players: Vec<DstMapPlayer>,
    // Internal engine evidence; host roots are never supplied by the UI.
    #[serde(skip)]
    pub enabled_mods: Vec<String>,
    #[serde(skip)]
    pub mod_versions: BTreeMap<String, String>,
    #[serde(skip)]
    pub client_required_mods: BTreeSet<String>,
    #[serde(skip)]
    pub registered_atlases: Vec<String>,
    #[serde(skip)]
    pub collection_issues: Vec<String>,
    #[serde(skip)]
    pub icons_uncertain: bool,
}

pub(super) fn constrain_response(snapshot: &mut DstMapSnapshot) -> Result<(), String> {
    let bytes = serde_json::to_vec(snapshot)
        .map_err(|error| error.to_string())?
        .len();
    if bytes <= MAX_RESPONSE_BYTES {
        return Ok(());
    }
    if let Some(art) = &mut snapshot.art {
        art.textures.clear();
        art.icons.clear();
        art.icon_fallbacks.clear();
        art.terrain_edges.clear();
        art.issues = vec![
            "Map artwork was omitted because the world exceeds the artwork transfer budget.".into(),
        ];
    }
    // Applied render-layer metadata is artwork, not the captured tile grid.
    snapshot.terrain_layers.clear();
    if serde_json::to_vec(snapshot)
        .map_err(|error| error.to_string())?
        .len()
        > MAX_RESPONSE_BYTES
    {
        return Err("The world's map geometry exceeds the transfer limit.".into());
    }
    Ok(())
}

pub(super) fn final_terrain_layers(
    snapshot: &DstMapSnapshot,
) -> impl Iterator<Item = &DstMapTerrainLayer> {
    let last: BTreeMap<_, _> = snapshot
        .terrain_layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.tile_id, index))
        .collect();
    snapshot
        .terrain_layers
        .iter()
        .enumerate()
        .filter(move |(index, layer)| last.get(&layer.tile_id) == Some(index))
        .map(|(_, layer)| layer)
}

pub(super) fn uses_texture_material(snapshot: &DstMapSnapshot, tile: u16) -> bool {
    match snapshot.tile_kinds.get(&tile) {
        Some(DstTerrainKind::Land) => true,
        Some(DstTerrainKind::Ocean) => !snapshot.tile_colors.contains_key(&tile),
        _ => false,
    }
}

pub(super) fn terrain_material_issues(snapshot: &DstMapSnapshot) -> Vec<String> {
    let mut unknown = BTreeSet::new();
    let mut schematic = BTreeSet::new();
    let mut schematic_tiles = 0usize;
    for id in snapshot.tiles.iter().flatten() {
        match snapshot.tile_kinds.get(id) {
            Some(DstTerrainKind::Unknown) | None => {
                unknown.insert(*id);
            }
            Some(DstTerrainKind::Land | DstTerrainKind::Ocean)
                if !snapshot.tile_textures.contains_key(id)
                    && !snapshot.tile_colors.contains_key(id) =>
            {
                schematic.insert(*id);
                schematic_tiles += 1;
            }
            _ => {}
        }
    }
    let sample = |ids: &BTreeSet<u16>| {
        ids.iter()
            .take(8)
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let mut issues = Vec::new();
    if !unknown.is_empty() {
        issues.push(format!(
            "Native terrain classification is unknown for {} tile types ({}); captured tile IDs and available artwork are retained.",
            unknown.len(), sample(&unknown)
        ));
    }
    if !schematic.is_empty() {
        issues.push(format!(
            "No native minimap texture or color was observed for {} tile types ({}); {} map tiles use schematic colors.",
            schematic.len(), sample(&schematic), schematic_tiles
        ));
    }
    let unsupported: BTreeSet<_> = final_terrain_layers(snapshot)
        .filter(|layer| uses_texture_material(snapshot, layer.tile_id))
        .filter_map(|layer| layer.sample_style.filter(|style| *style != 0))
        .collect();
    if !unsupported.is_empty() {
        issues.push(format!("Native terrain sample styles {unsupported:?} are unsupported; native noise/base retained."));
    }
    issues
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum NativeErrorCode {
    WorldNotReady,
}

#[derive(Deserialize)]
#[serde(tag = "k", rename_all = "lowercase")]
enum Frame {
    #[serde(rename_all = "camelCase")]
    Begin {
        #[serde(default)]
        game_version: String,
        width: usize,
        height: usize,
        tile_scale: f64,
        session: String,
        day: u32,
        season: String,
        phase: String,
    },
    Tiles {
        offset: usize,
        runs: Vec<[u16; 2]>,
    },
    Name {
        id: u16,
        name: String,
        kind: DstTerrainKind,
        #[serde(rename = "noiseTexture")]
        noise_texture: Option<String>,
        #[serde(rename = "baseTexture")]
        base_texture: Option<String>,
        #[serde(rename = "minimapColor")]
        minimap_color: Option<[u8; 4]>,
    },
    Landmark(DstMapLandmark),
    Layer(DstMapTerrainLayer),
    Road {
        index: usize,
        offset: usize,
        points: Vec<[f64; 2]>,
        complete: bool,
    },
    Mod {
        name: String,
        #[serde(default)]
        version: String,
        #[serde(rename = "requiredOnClient")]
        required_on_client: bool,
    },
    Atlas {
        path: String,
    },
    Issue {
        message: String,
        #[serde(default, rename = "iconsUncertain")]
        icons_uncertain: bool,
    },
    Player(DstMapPlayer),
    End {
        players: usize,
        #[serde(default)]
        landmarks: usize,
        #[serde(default)]
        roads: usize,
        #[serde(default, rename = "landmarksTruncated")]
        landmarks_truncated: bool,
    },
    Error {
        message: String,
        code: Option<NativeErrorCode>,
    },
}

pub(super) fn query(nonce: &str, terrain: bool) -> Result<String, String> {
    // The only substitutions are a generated UUID and a boolean, never user Lua.
    let command = include_str!("dst_map_query.lua")
        .replace("__LANDMARK_ICONS__", include_str!("dst_map_landmarks.lua"))
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("--"))
        .collect::<Vec<_>>()
        .join(" ")
        .replace("__NONCE__", nonce)
        .replace("__TERRAIN__", if terrain { "true" } else { "false" });
    bounded_query(command)
}

fn bounded_query(command: String) -> Result<String, String> {
    if command.len() < MAX_QUERY_BYTES {
        Ok(command)
    } else {
        Err("The map query exceeds the native console input limit.".into())
    }
}

pub(super) struct Capture {
    prefix: String,
    terrain: bool,
    snapshot: Option<DstMapSnapshot>,
    bytes: usize,
    landmark_ids: HashSet<String>,
    pending_road: Vec<[f64; 2]>,
    road_points: usize,
}

impl Capture {
    pub(super) fn new(nonce: &str, terrain: bool) -> Self {
        Self {
            prefix: format!("[LGSM-DST-MAP:{nonce}] "),
            terrain,
            snapshot: None,
            bytes: 0,
            landmark_ids: HashSet::new(),
            pending_road: Vec::new(),
            road_points: 0,
        }
    }

    pub(super) fn push(&mut self, line: &str) -> Result<Option<DstMapSnapshot>, String> {
        let body = log_body(line);
        let Some(json) = body.strip_prefix(&self.prefix) else {
            return Ok(None);
        };
        self.bytes = self.bytes.saturating_add(json.len());
        if json.len() > 16 * 1024 || self.bytes > MAX_CAPTURE_BYTES {
            return Err("DST map response exceeds the capture limit.".into());
        }
        let frame: Frame =
            serde_json::from_str(json).map_err(|_| "Invalid native DST map frame.".to_string())?;
        if let Frame::Error { message, code } = frame {
            if matches!(code, Some(NativeErrorCode::WorldNotReady)) {
                return Err(super::map_error(
                    super::MapErrorCode::WorldNotReady,
                    message,
                ));
            }
            return Err(format!(
                "DST map: {}",
                message
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(256)
                    .collect::<String>()
            ));
        }
        if let Frame::Begin {
            game_version,
            width,
            height,
            tile_scale,
            session,
            day,
            season,
            phase,
        } = frame
        {
            if self.snapshot.is_some()
                || width == 0
                || height == 0
                || width > 2048
                || height > 2048
                || width.saturating_mul(height) > MAX_TILES
                || !tile_scale.is_finite()
                || !(0.1..=100.0).contains(&tile_scale)
                || !bounded_text(&session, 128)
                || !bounded_text(&game_version, 64)
                || !bounded_text(&season, 64)
                || !bounded_text(&phase, 64)
            {
                return Err("Invalid native DST map dimensions or world identity.".into());
            }
            self.snapshot = Some(DstMapSnapshot {
                instance_id: String::new(),
                shard: String::new(),
                run_id: 0,
                captured_at_unix_ms: 0,
                session,
                width,
                height,
                tile_scale,
                day,
                season,
                phase,
                tiles: self.terrain.then(|| Vec::with_capacity(width * height)),
                tile_names: BTreeMap::new(),
                tile_kinds: BTreeMap::new(),
                tile_textures: BTreeMap::new(),
                tile_base_textures: BTreeMap::new(),
                tile_colors: BTreeMap::new(),
                terrain_layers: Vec::new(),
                landmarks: self.terrain.then(Vec::new),
                roads: self.terrain.then(Vec::new),
                landmarks_truncated: false,
                art: None,
                players: Vec::new(),
                enabled_mods: Vec::new(),
                mod_versions: BTreeMap::new(),
                client_required_mods: BTreeSet::new(),
                registered_atlases: Vec::new(),
                collection_issues: Vec::new(),
                icons_uncertain: false,
            });
            return Ok(None);
        }
        let snapshot = self
            .snapshot
            .as_mut()
            .ok_or("DST map frame preceded its world header.")?;
        match frame {
            Frame::Tiles { offset, runs } => {
                let tiles = snapshot
                    .tiles
                    .as_mut()
                    .ok_or("Unexpected terrain in position query.")?;
                let count: usize = runs.iter().map(|run| usize::from(run[1])).sum();
                if offset != tiles.len()
                    || runs.is_empty()
                    || runs.len() > 128
                    || count > 128
                    || runs.iter().any(|run| run[1] == 0)
                    || tiles.len() + count > snapshot.width * snapshot.height
                {
                    return Err("DST map terrain is incomplete or out of order.".into());
                }
                for [tile, count] in runs {
                    tiles.extend(std::iter::repeat_n(tile, usize::from(count)));
                }
            }
            Frame::Name {
                id,
                name,
                kind,
                noise_texture,
                base_texture,
                minimap_color,
            } => {
                if !self.terrain
                    || snapshot.tile_names.len() >= 4096
                    || !bounded_text(&name, 128)
                    || (kind != DstTerrainKind::Ocean && minimap_color.is_some())
                    || snapshot.tile_names.insert(id, name).is_some()
                {
                    return Err("Invalid DST map terrain type.".into());
                }
                snapshot.tile_kinds.insert(id, kind);
                if let Some(texture) = noise_texture {
                    if !bounded_text(&texture, 512) {
                        return Err("Invalid DST map texture path.".into());
                    }
                    snapshot.tile_textures.insert(id, texture);
                }
                if let Some(color) = minimap_color {
                    snapshot.tile_colors.insert(id, color);
                }
                if let Some(texture) = base_texture {
                    if !bounded_text(&texture, 512) {
                        return Err("Invalid DST map base texture path.".into());
                    }
                    snapshot.tile_base_textures.insert(id, texture);
                }
            }
            Frame::Layer(layer) => {
                if !self.terrain
                    || snapshot.terrain_layers.len() >= 4096
                    || [&layer.atlas, &layer.texture, &layer.noise_texture]
                        .into_iter()
                        .flatten()
                        .any(|path| path.is_empty() || !bounded_text(path, 512))
                {
                    return Err("Invalid native DST map render layer.".into());
                }
                snapshot.terrain_layers.push(layer);
            }
            Frame::Landmark(landmark) => {
                let landmarks = snapshot
                    .landmarks
                    .as_mut()
                    .ok_or("Unexpected map landmark.")?;
                if landmarks.len() >= 32768
                    || !bounded_text(&landmark.id, 128)
                    || !bounded_text(&landmark.prefab, 128)
                    || !bounded_text(&landmark.icon, 128)
                    || !landmark.priority.is_finite()
                    || landmark.priority.abs() > 1e7
                    || ![landmark.map_x, landmark.map_z]
                        .iter()
                        .all(|v| v.is_finite() && v.abs() <= 1e7)
                    || !self.landmark_ids.insert(landmark.id.clone())
                {
                    return Err("Invalid or duplicate DST map landmark.".into());
                }
                landmarks.push(landmark);
            }
            Frame::Road {
                index,
                offset,
                points,
                complete,
            } => {
                let roads = snapshot.roads.as_mut().ok_or("Unexpected map road.")?;
                if roads.len() >= 1024
                    || index != roads.len()
                    || offset != self.pending_road.len()
                    || points.is_empty()
                    || points.len() > 32
                    || offset + points.len() > 256
                    || self.road_points + points.len() > 65536
                    || points
                        .iter()
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > 1e7)
                {
                    return Err("Invalid DST map road geometry.".into());
                }
                self.road_points += points.len();
                self.pending_road.extend(points);
                if complete {
                    if self.pending_road.len() < 2 {
                        return Err("Invalid DST map road geometry.".into());
                    }
                    roads.push(std::mem::take(&mut self.pending_road));
                }
            }
            Frame::Mod {
                name,
                version,
                required_on_client,
            } => {
                if !self.terrain
                    || snapshot.enabled_mods.len() >= 128
                    || name.is_empty()
                    || !bounded_text(&name, 128)
                    || !bounded_text(&version, 128)
                    || snapshot.enabled_mods.contains(&name)
                {
                    return Err("Invalid DST map enabled Mod list.".into());
                }
                snapshot.mod_versions.insert(name.clone(), version);
                if required_on_client {
                    snapshot.client_required_mods.insert(name.clone());
                }
                snapshot.enabled_mods.push(name);
            }
            Frame::Atlas { path } => {
                if !self.terrain
                    || snapshot.registered_atlases.len() >= 512
                    || path.is_empty()
                    || !bounded_text(&path, 512)
                {
                    return Err("Invalid DST map registered atlas.".into());
                }
                snapshot.registered_atlases.push(path);
            }
            Frame::Issue {
                message,
                icons_uncertain,
            } => {
                if !self.terrain
                    || snapshot.collection_issues.len() >= 32
                    || !bounded_text(&message, 256)
                {
                    return Err("Invalid DST map collection issue.".into());
                }
                snapshot.collection_issues.push(message);
                snapshot.icons_uncertain |= icons_uncertain;
            }
            Frame::Player(player) => {
                if snapshot.players.len() >= 256
                    || !bounded_text(&player.id, 128)
                    || player.id.is_empty()
                    || !bounded_text(&player.name, 256)
                    || !bounded_text(&player.character, 128)
                    || player
                        .icon
                        .as_ref()
                        .is_some_and(|icon| !bounded_text(icon, 128))
                    || [player.x, player.z, player.map_x, player.map_z]
                        .iter()
                        .any(|value| !value.is_finite() || value.abs() > 1e7)
                    || snapshot.players.iter().any(|p| p.id == player.id)
                {
                    return Err("Invalid or duplicate DST map player.".into());
                }
                snapshot.players.push(player);
            }
            Frame::End {
                players,
                landmarks,
                roads,
                landmarks_truncated,
            } => {
                if players != snapshot.players.len()
                    || !self.pending_road.is_empty()
                    || landmarks != snapshot.landmarks.as_ref().map_or(0, Vec::len)
                    || roads != snapshot.roads.as_ref().map_or(0, Vec::len)
                    || snapshot.tiles.as_ref().is_some_and(|tiles| {
                        tiles.len() != snapshot.width * snapshot.height
                            || tiles
                                .iter()
                                .copied()
                                .collect::<HashSet<_>>()
                                .iter()
                                .any(|id| {
                                    !snapshot.tile_names.contains_key(id)
                                        || !snapshot.tile_kinds.contains_key(id)
                                })
                    })
                {
                    return Err("DST map response is incomplete.".into());
                }
                snapshot.landmarks_truncated = landmarks_truncated;
                return Ok(self.snapshot.take());
            }
            Frame::Begin { .. } | Frame::Error { .. } => unreachable!(),
        }
        Ok(None)
    }
}

fn bounded_text(value: &str, max: usize) -> bool {
    value.len() <= max * 4 && !value.chars().any(char::is_control)
}

fn log_body(line: &str) -> &str {
    let line = line.trim();
    if let Some((timestamp, body)) = line
        .strip_prefix('[')
        .and_then(|line| line.split_once("]: "))
        && !timestamp.is_empty()
        && timestamp.bytes().all(|b| b.is_ascii_digit() || b == b':')
    {
        return body.trim();
    }
    line
}

#[cfg(test)]
#[path = "dst_map_protocol_tests.rs"]
mod tests;
