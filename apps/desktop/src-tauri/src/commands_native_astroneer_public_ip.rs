//! Opt-in proof that production startup prepares a default-empty PublicIP.
use super::*;
use std::io::Read;

pub(super) fn verify(
    before: &InstanceDetails,
    running: &InstanceDetails,
    cycle: &str,
    install: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("LANGAME_NATIVE_ASTRO_AUTO_PUBLIC_IP").as_deref() != Ok("true") {
        return Ok(());
    }
    if before.summary.module_id != "astroneer" || !matches!(cycle, "initial" | "restart") {
        return Err("PublicIP automatic-start proof requires the two ASTRONEER cycles".into());
    }
    let discovered = crate::astroneer_public_ip::verify_startup_settings(
        &before.settings_json,
        &running.settings_json,
    )?;
    if discovered != (cycle == "initial") {
        return Err(
            "PublicIP proof requires a default-empty initial address and persisted restart reuse"
                .into(),
        );
    }
    let settings: Value = serde_json::from_str(&running.settings_json)?;
    let expected = settings["public_ip"]
        .as_str()
        .ok_or("PublicIP proof setting missing")?;
    let mut body = Vec::new();
    fs::File::open(install.join("Astro/Saved/Config/WindowsServer/AstroServerSettings.ini"))?
        .take(262_145)
        .read_to_end(&mut body)?;
    if body.len() > 262_144 || !rendered_address_matches(std::str::from_utf8(&body)?, expected) {
        return Err(
            "PublicIP proof requires exactly one matching native registration setting".into(),
        );
    }
    println!(
        "NATIVE_ASTRO_PUBLIC_IP cycle={cycle} phase=verified source={} persisted=true rendered=true address_not_reported=true",
        if discovered {
            "product_discovery"
        } else {
            "persisted_reuse"
        }
    );
    Ok(())
}

fn rendered_address_matches(body: &str, expected: &str) -> bool {
    let mut section = false;
    let mut count = 0;
    for line in body.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') {
            section = line.eq_ignore_ascii_case("[/Script/Astro.AstroServerSettings]");
        } else if section
            && let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case("PublicIP")
        {
            if value.trim() != expected {
                return false;
            }
            count += 1;
        }
    }
    count == 1
}

#[test]
fn native_astro_public_ip_proof_rejects_wrong_duplicate_or_other_section_values() {
    let correct = "[/Script/Astro.AstroServerSettings]\nPublicIP=1.1.1.1\n";
    assert!(rendered_address_matches(correct, "1.1.1.1"));
    for body in [
        "[Other]\nPublicIP=1.1.1.1\n",
        "[/Script/Astro.AstroServerSettings]\nPublicIP=8.8.8.8\n",
        "[/Script/Astro.AstroServerSettings]\nPublicIP=1.1.1.1\nPublicIP=1.1.1.1\n",
        "[/Script/Astro.AstroServerSettings]\n;PublicIP=1.1.1.1\n",
    ] {
        assert!(!rendered_address_matches(body, "1.1.1.1"));
    }
}
