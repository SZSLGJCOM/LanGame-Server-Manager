use super::*;

#[path = "launch_templates_ark_ase_data.rs"]
mod data;
pub(super) use data::ARK_ASE_ADDITIONAL_LAUNCH_SETTINGS;

const ARK_ASE_URL_SETTINGS: &[(&str, &str)] = &[
    ("event_colors_chance_override", "EventColorsChanceOverride"),
    ("new_year1_utc", "NewYear1UTC"),
    ("new_year2_utc", "NewYear2UTC"),
];

pub(super) fn render_ark_evolved_server_url(context: &TemplateContext<'_>) -> String {
    let map_name =
        lookup_json_text(context.settings, "map_name").unwrap_or_else(|| String::from("TheIsland"));
    let game_port = lookup_port_path(&context.instance.ports, "game.port")
        .unwrap_or_else(|| String::from("7777"));
    let query_port = lookup_port_path(&context.instance.ports, "query.port")
        .unwrap_or_else(|| String::from("27015"));
    let max_players =
        lookup_json_text(context.settings, "max_players").unwrap_or_else(|| String::from("20"));
    let mut server_url = format!(
        "{}?AltSaveDirectoryName={}?Port={}?QueryPort={}?MaxPlayers={}",
        map_name.trim(),
        render_ark_save_directory(context),
        game_port.trim(),
        query_port.trim(),
        max_players.trim()
    );

    let bind_ip = context.instance.summary.bind_ip.trim();
    if !bind_ip.is_empty() && bind_ip != "0.0.0.0" {
        server_url.push_str("?MultiHome=");
        server_url.push_str(bind_ip);
    }
    // The cluster shares INI files; listener identity must remain per map.
    if let Some(server_name) = lookup_json_text(context.settings, "server_name") {
        server_url.push_str("?SessionName=");
        server_url.push_str(server_name.trim());
    }
    if lookup_json_bool(context.settings, "rcon_enabled").unwrap_or(false)
        && let Some(rcon_port) = lookup_port_path(&context.instance.ports, "rcon.port")
    {
        server_url.push_str("?RCONEnabled=true?RCONPort=");
        server_url.push_str(rcon_port.trim());
    }
    for &(setting_key, native_key) in ARK_ASE_URL_SETTINGS {
        append_ark_url_setting(&mut server_url, context.settings, setting_key, native_key);
    }
    server_url
}

fn append_ark_url_setting(
    server_url: &mut String,
    settings: &Value,
    setting_key: &str,
    native_key: &str,
) {
    let Some(value) = lookup_json_text(settings, setting_key) else {
        return;
    };
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    server_url.push('?');
    server_url.push_str(native_key);
    server_url.push('=');
    server_url.push_str(value);
}
