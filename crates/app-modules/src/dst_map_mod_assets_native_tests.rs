use std::path::PathBuf;

use super::super::{DstMapIconFallbackKind, DstMapModSource, load_dst_map_assets};

#[test]
#[ignore = "reads explicitly supplied owned game and Workshop resources"]
fn installed_mod_map_artwork_decodes_registered_atlases_and_target_icons() {
    let game = PathBuf::from(std::env::var_os("LANGAME_DST_SMOKE_PACKAGE").expect("owned game"));
    let mods = PathBuf::from(
        std::env::var_os("LANGAME_DST_ATLAS_SMOKE_ROOT").expect("owned Workshop resources"),
    );
    // These are independent expectations from the actual Mod registrations,
    // not a production lookup table. Exercise both upstream resource layouts.
    for (id, atlases, icon, terrain) in [
        (
            "1505270912",
            vec!["map_icons/hamleticon.xml"],
            "slot_machine.png",
            "levels/textures/mini_noise_beach.tex",
        ),
        (
            "2986194136",
            vec![
                "images/inventoryimages/inventory_shipwrecked.xml",
                "images/inventoryimages/inventory_hamlet.xml",
                "images/inventoryimages/inventory_extension.xml",
            ],
            "slot_machine.tex",
            "levels/textures/sw/mini_ground_beach.tex",
        ),
    ] {
        let sources = [id, "1289779251"].map(|id| DstMapModSource {
            name: format!("workshop-{id}"),
            root: mods.join(id),
        });
        let mut registered: Vec<_> = atlases
            .into_iter()
            .map(|path| format!("../mods/workshop-{id}/{path}"))
            .collect();
        registered.push("../mods/workshop-1289779251/images/cherryminimap.xml".into());
        let mut textures = vec![format!("../mods/workshop-{id}/{terrain}")];
        textures.extend(["crimson", "lime", "pink"].map(|color| {
            format!("../mods/workshop-1289779251/levels/textures/mini_noise_cherry{color}.tex")
        }));
        let mut icons = vec![icon.into(), "cherry_stonetree.png".into()];
        if id == "1505270912" {
            icons.push("volcano.png".into());
        }
        let result = load_dst_map_assets(
            &game,
            &sources,
            crate::DstMapIconSources {
                registered_atlases: &registered,
                ..Default::default()
            },
            &textures,
            &[],
            &icons,
            &[],
        )
        .unwrap();
        assert_eq!(result.textures.len(), 4, "{:?}", result.issues);
        assert_eq!(result.icons.len(), icons.len(), "{:?}", result.issues);
        if id == "1505270912" {
            let volcano = image::load_from_memory(&result.icons["volcano.png"]).unwrap();
            assert_eq!((volcano.width(), volcano.height()), (64, 64));
        }
        assert!(result.issues.is_empty(), "{:?}", result.issues);
        for png in result.textures.values().chain(result.icons.values()) {
            let image = image::load_from_memory(png).unwrap();
            assert!(image.width() > 0 && image.height() > 0);
        }
        println!(
            "NATIVE_MOD_MAP_ARTWORK mod={id} textures=4 icons={} issues={:?}",
            icons.len(),
            result.issues
        );
    }
}

#[test]
#[ignore = "reads explicitly supplied owned game and Workshop resources"]
fn installed_mod_map_artwork_completes_missing_icons_with_explicit_sources() {
    let game = PathBuf::from(std::env::var_os("LANGAME_DST_SMOKE_PACKAGE").expect("owned game"));
    let mods = PathBuf::from(
        std::env::var_os("LANGAME_DST_ATLAS_SMOKE_ROOT").expect("owned Workshop resources"),
    );
    assert_complete_index_probe(&mods);
    for (id, registered, icons) in [
        (
            "1505270912",
            vec![
                "map_icons/creepindedeepicon.xml",
                "map_icons/hamleticon.xml",
            ],
            vec!["tiki.tex", "teleportato_base.png"],
        ),
        (
            "2986194136",
            vec![
                "images/inventoryimages/inventory_shipwrecked.xml",
                "images/inventoryimages/inventory_hamlet.xml",
                "images/inventoryimages/inventory_extension.xml",
            ],
            vec!["pig_ruins_well.png", "pig_ruins_well_vortex.png"],
        ),
    ] {
        let sources = [id, "1289779251"].map(|id| DstMapModSource {
            name: format!("workshop-{id}"),
            root: mods.join(id),
        });
        let mut registered: Vec<_> = registered
            .into_iter()
            .map(|path| format!("../mods/workshop-{id}/{path}"))
            .collect();
        registered.push("../mods/workshop-1289779251/images/cherryminimap.xml".into());
        let requested = icons
            .iter()
            .map(|name| (*name).to_string())
            .collect::<Vec<_>>();
        let result = load_dst_map_assets(
            &game,
            &sources,
            crate::DstMapIconSources {
                registered_atlases: &registered,
                ..Default::default()
            },
            &[],
            &[],
            &requested,
            &[],
        )
        .unwrap();
        assert_eq!(
            result.icons.len(),
            requested.len(),
            "{id}: {:?}",
            result.issues
        );
        assert_eq!(result.icon_fallbacks.len(), requested.len(), "{id}");
        for fallback in &result.icon_fallbacks {
            let expected = if id == "2986194136" {
                DstMapIconFallbackKind::RegisteredAlias
            } else if fallback.icon == "tiki.tex" {
                DstMapIconFallbackKind::IndexedExact
            } else {
                DstMapIconFallbackKind::IndexedAlias
            };
            assert_eq!(fallback.kind, expected, "{fallback:?}");
            assert!(
                fallback
                    .atlas
                    .starts_with(&format!("../mods/workshop-{id}/"))
            );
            let image = image::load_from_memory(&result.icons[&fallback.icon]).unwrap();
            assert!(image.width() <= 64 && image.height() <= 64);
        }
        println!(
            "NATIVE_MOD_MAP_COMPLETION mod={id} sources={:?} issues={:?}",
            result.icon_fallbacks, result.issues
        );
    }
}

fn assert_complete_index_probe(root: &std::path::Path) {
    let wanted = [
        "tiki.tex",
        "tiki.png",
        "teleportato_base.png",
        "teleportato_base.tex",
        "pig_ruins_well.png",
        "pig_ruins_well.tex",
        "pig_ruins_well_vortex.png",
        "pig_ruins_well_vortex.tex",
    ]
    .map(String::from)
    .into_iter()
    .collect();
    let mut errors = Vec::new();
    let mut files = 0;
    for id in ["1505270912", "2986194136", "1289779251"] {
        let mut pending = vec![root.join(id)];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    pending.push(entry.path());
                } else if entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "xml")
                {
                    files += 1;
                    assert!(files <= 512, "acceptance probe XML count limit");
                    let bytes = std::fs::read(entry.path()).unwrap();
                    if let Err(error) =
                        super::super::atlas::parse_atlas_for_map_index(&bytes, &wanted)
                    {
                        errors.push(format!("{}: {error}", entry.path().display()));
                    }
                }
            }
        }
    }
    println!("NATIVE_MOD_INDEX_COMPLETE_DOCUMENTS files={files} errors={errors:?}");
    assert!(errors.is_empty(), "{errors:#?}");
}
