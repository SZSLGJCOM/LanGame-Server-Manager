use super::*;
use crate::commands::commands_mod_staging::{
    ManualModImportPresentation, archive_formats::tests::archive_fixture_bytes,
    stage_website_mod_source,
};
use crate::commands::commands_mods::mod_inventory::read_inventory;

#[tokio::test]
async fn nexus_website_rar_and_seven_z_downloads_extract_and_publish_owned_inventory() {
    for extension in ["rar", "7z"] {
        let body = archive_fixture_bytes(extension);
        let (url, server) = serve_file(&body, false);
        let mut prepared = prepare_website_download(
            "https://cf-files.nexusmods.com/5811/fixture?token=fixture-client-secret".into(),
            "Camera Tweaks v1.0".into(),
            &["zip".into(), "dll".into()],
        )
        .unwrap();
        assert!(prepared.needs_archive_identification);
        // The same local HTTP fixture seam as the existing ZIP download tests.
        prepared.url = url;
        let downloads = download_website_file_with_client(prepared, local_download_client())
            .await
            .unwrap();
        assert_eq!(
            downloads.paths()[0].file_name().unwrap().to_str().unwrap(),
            format!("payload.{extension}")
        );
        let owned_download = downloads.directory.0.clone();
        let installed = TemporaryDirectory::new().unwrap();
        let target = || ResolvedManualModTarget {
            instance_id: "nexus-archive-http-test".into(),
            module_id: "enshrouded".into(),
            source_label: "Nexus Mods".into(),
            target_label: "mods".into(),
            target_path: installed.0.join("mods"),
            accepts: vec!["folder".into(), "zip".into(), "dll".into()],
            id_strategy: None,
        };
        fs::create_dir(target().target_path.clone()).unwrap();
        fs::write(
            target().target_path.join("operator.cfg"),
            b"keep existing settings",
        )
        .unwrap();
        let icon = "https://staticdelivery.nexusmods.com/mods/5811/images/thumbnails/315/315-1790000000-123456789.png";
        let result = stage_website_mod_source(
            target(),
            downloads.paths()[0].clone(),
            ManualModImportPresentation {
                display_name: Some("Camera Tweaks".into()),
                icon_url: Some(icon.into()),
            },
            None,
        )
        .unwrap();
        assert_eq!(result.copied_file_count, 1);
        assert_eq!(
            fs::read(target().target_path.join("CameraTweaks/CameraTweaks.dll")).unwrap(),
            b"test server mod bytes"
        );
        assert_eq!(
            fs::read(target().target_path.join("operator.cfg")).unwrap(),
            b"keep existing settings"
        );
        let inventory = read_inventory(target()).unwrap();
        let item = inventory
            .items
            .iter()
            .find(|item| item.display_name.as_deref() == Some("Camera Tweaks"))
            .unwrap();
        assert_eq!(item.icon_url.as_deref(), Some(icon));
        drop(downloads);
        assert!(!owned_download.exists());
        let request = server.join().unwrap().to_ascii_lowercase();
        for forbidden in ["apikey:", "cookie:", "authorization:"] {
            assert!(!request.contains(forbidden));
        }
    }
}
