use std::collections::HashSet;
use std::time::Duration;

use super::{
    MAX_LOOKUP_IDS, SteamWorkshopLookupItem, lookup_public_workshop_items, normalize_lookup_ids,
};

const MAX_DOWNLOAD_INPUT_IDS: usize = 8192;

pub async fn validate_workshop_download_items(
    app_id: u32,
    ids: &[String],
    preference: app_network::SourcePreference,
) -> Result<(), String> {
    let ids = normalize_download_ids(ids)?;
    if app_id == 0 || ids.is_empty() {
        return Err(String::from(
            "Workshop download requires an app and at least one item.",
        ));
    }
    // A lookup batch limit is not a collection/download limit. Validate every
    // chunk before returning permission to the caller to start any download.
    tokio::time::timeout(Duration::from_secs(120), async {
        for chunk in ids.chunks(MAX_LOOKUP_IDS) {
            let items = lookup_public_workshop_items(chunk.to_vec(), preference).await?;
            validate_download_details(app_id, chunk, &items)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| String::from("Workshop download validation timed out before all items were verified. Please retry with fewer items."))?
}

fn normalize_download_ids(ids: &[String]) -> Result<Vec<String>, String> {
    if ids.len() > MAX_DOWNLOAD_INPUT_IDS {
        return Err(format!(
            "Workshop download is limited to {MAX_DOWNLOAD_INPUT_IDS} input IDs per request."
        ));
    }
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for chunk in ids.chunks(MAX_LOOKUP_IDS) {
        for id in normalize_lookup_ids(chunk.to_vec())? {
            if seen.insert(id.clone()) {
                normalized.push(id);
            }
        }
    }
    Ok(normalized)
}

fn validate_download_details(
    app_id: u32,
    ids: &[String],
    items: &[SteamWorkshopLookupItem],
) -> Result<(), String> {
    for id in ids {
        let item = items.iter().find(|item| item.id == *id).ok_or_else(|| {
            workshop_download_error(
                "missing",
                id,
                format!("Steam did not verify Workshop item {id}."),
            )
        })?;
        if item.status == "not_found" {
            return Err(workshop_download_error(
                "missing",
                id,
                format!("Steam did not return public Workshop details for item {id}."),
            ));
        }
        if item.status != "resolved" && item.status != "unsupported" {
            return Err(workshop_download_error(
                "unresolved",
                id,
                item.message.clone().unwrap_or_else(|| {
                    format!("Steam has not verified Workshop item {id}. Retry before downloading.")
                }),
            ));
        }
        if item.status == "unsupported" || item.item_kind != "item" {
            return Err(workshop_download_error(
                "unsupported",
                id,
                format!(
                    "Workshop item {id} is not installable game content. Guides, media and collection IDs cannot be downloaded as Mods."
                ),
            ));
        }
        if item.consumer_app_id.is_none() {
            return Err(workshop_download_error(
                "unresolved",
                id,
                format!("Steam has not verified the game for Workshop item {id}."),
            ));
        }
        if item.consumer_app_id != Some(app_id) {
            return Err(workshop_download_error(
                "wrong-game",
                id,
                format!("Workshop item {id} does not belong to app {app_id}."),
            ));
        }
        if app_id == 322330
            && item
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case("client_only_mod"))
        {
            return Err(workshop_download_error(
                "client-only",
                id,
                format!(
                    "Workshop item {id} is a client-only Don't Starve Together Mod and cannot be installed on a dedicated server."
                ),
            ));
        }
    }
    Ok(())
}

fn workshop_download_error(reason: &str, item_id: &str, message: String) -> String {
    serde_json::json!({
        "code": "workshop-collection-install", "reason": reason, "item_id": item_id, "message": message,
    }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steam_workshop::build_lookup_item;
    use serde_json::json;
    use std::collections::HashMap;

    fn item(id: &str, app_id: u32, file_type: u32, tags: &[&str]) -> SteamWorkshopLookupItem {
        let details = HashMap::from([(
            id.to_string(),
            json!({
                "result": 1, "consumer_app_id": app_id, "file_type": file_type,
                "tags": tags.iter().map(|tag| json!({"tag": tag})).collect::<Vec<_>>()
            }),
        )]);
        build_lookup_item(id, &details, &HashMap::new(), &HashMap::new()).expect("lookup item")
    }

    #[test]
    fn guide_is_rejected_even_when_its_game_app_matches() {
        let ids = vec!["441378551".to_string()];
        let guide = item(&ids[0], 322330, 9, &[]);
        assert_eq!(guide.status, "unsupported");
        assert_eq!(guide.item_kind, "guide");
        assert!(validate_download_details(322330, &ids, &[guide]).is_err());
    }

    #[test]
    fn unverified_items_preserve_the_actual_failure_and_cannot_authorize_downloads() {
        let id = "123456".to_string();
        let details = HashMap::from([(
            id.clone(),
            json!({"result": 1,
            "consumer_app_id": 322330, "title": "Saved Mod", "type_error": "HTTP 429 fixture"}),
        )]);
        let mut pending =
            build_lookup_item(&id, &details, &HashMap::new(), &HashMap::new()).unwrap();
        pending.localization_warning = Some(String::from("Showing original text after HTTP 429"));
        let error: serde_json::Value = serde_json::from_str(
            &validate_download_details(322330, std::slice::from_ref(&id), &[pending]).unwrap_err(),
        )
        .unwrap();
        assert_eq!(error["code"], "workshop-collection-install");
        assert_eq!(error["reason"], "unresolved");
        assert_eq!(error["item_id"], id);
        assert_eq!(error["message"], "HTTP 429 fixture");
    }

    #[test]
    fn every_download_item_must_be_a_verified_mod_for_the_expected_app() {
        let ids = vec!["123456".to_string(), "234567".to_string()];
        let good = item(&ids[0], 322330, 0, &[]);
        for (reason, invalid) in [
            ("unsupported", item(&ids[1], 322330, 9, &[])),
            ("unsupported", item(&ids[1], 322330, 2, &[])),
            ("wrong-game", item(&ids[1], 108600, 0, &[])),
        ] {
            let error: serde_json::Value = serde_json::from_str(
                &validate_download_details(322330, &ids, &[good.clone(), invalid]).unwrap_err(),
            )
            .unwrap();
            assert_eq!(error["code"], "workshop-collection-install");
            assert_eq!(error["reason"], reason);
            assert_eq!(error["item_id"], ids[1]);
        }
        assert!(validate_download_details(322330, &ids, std::slice::from_ref(&good)).is_err());
        assert!(
            validate_download_details(322330, &ids, &[good, item(&ids[1], 322330, 15, &[])])
                .is_ok()
        );
    }

    #[test]
    fn client_only_tag_is_a_dst_server_boundary_without_requiring_optional_tags() {
        let ids = vec!["3739491677".to_string()];
        for tag in ["client_only_mod", "CLIENT_ONLY_MOD", "Client_Only_Mod"] {
            let error: serde_json::Value = serde_json::from_str(
                &validate_download_details(322330, &ids, &[item(&ids[0], 322330, 0, &[tag])])
                    .unwrap_err(),
            )
            .unwrap();
            assert_eq!(error["code"], "workshop-collection-install");
            assert_eq!(error["reason"], "client-only");
            assert_eq!(error["item_id"], ids[0]);
        }
        assert!(validate_download_details(322330, &ids, &[item(&ids[0], 322330, 0, &[])]).is_ok());
        assert!(
            validate_download_details(
                108600,
                &ids,
                &[item(&ids[0], 108600, 0, &["client_only_mod"])]
            )
            .is_ok()
        );
    }

    #[test]
    fn mixed_direct_downloads_do_not_silently_skip_a_client_only_request() {
        let ids = vec!["123456".to_string(), "1365141672".to_string()];
        let items = vec![
            item(&ids[0], 322330, 0, &[]),
            item(&ids[1], 322330, 0, &["client_only_mod"]),
        ];
        let error: serde_json::Value =
            serde_json::from_str(&validate_download_details(322330, &ids, &items).unwrap_err())
                .unwrap();
        assert_eq!(error["reason"], "client-only");
        assert_eq!(error["item_id"], ids[1]);
        assert!(validate_download_details(322330, &ids[..1], &items[..1]).is_ok());
        let mut missing = items[1].clone();
        missing.status = "not_found".into();
        let error: serde_json::Value = serde_json::from_str(
            &validate_download_details(322330, &ids[1..], &[missing]).unwrap_err(),
        )
        .unwrap();
        assert_eq!(error["reason"], "missing");
        assert_eq!(error["item_id"], ids[1]);
    }

    #[test]
    fn large_downloads_keep_all_ids_across_lookup_chunks() {
        let ids = (100000..100130)
            .map(|id| id.to_string())
            .collect::<Vec<_>>();
        let normalized = normalize_download_ids(&ids).expect("large collection input");
        assert_eq!(normalized, ids);
        assert_eq!(
            normalized
                .chunks(MAX_LOOKUP_IDS)
                .map(<[String]>::len)
                .collect::<Vec<_>>(),
            [64, 64, 2]
        );
        let mut repeated = ids.clone();
        repeated.extend(ids);
        assert_eq!(
            normalize_download_ids(&repeated).expect("cross-chunk duplicates"),
            normalized
        );
    }

    #[test]
    fn large_downloads_validate_later_chunks_and_bound_the_entire_input() {
        let mut ids = (100000..100130)
            .map(|id| id.to_string())
            .collect::<Vec<_>>();
        ids.push("not-an-id".to_string());
        assert!(normalize_download_ids(&ids).is_err());
        let at_limit = vec!["123456".to_string(); MAX_DOWNLOAD_INPUT_IDS];
        assert_eq!(
            normalize_download_ids(&at_limit).expect("bounded duplicate input"),
            ["123456"]
        );
        assert!(
            normalize_download_ids(&vec!["123456".to_string(); MAX_DOWNLOAD_INPUT_IDS + 1])
                .is_err()
        );
    }
}
