use super::*;

#[path = "launch_templates_ark_asa_data.rs"]
mod data;
pub(super) use data::ARK_ASA_ADDITIONAL_LAUNCH_SETTINGS;

pub(super) fn render_ark_ascended_server_url(context: &TemplateContext<'_>) -> String {
    let map_name = lookup_json_text(context.settings, "map_name")
        .unwrap_or_else(|| String::from("TheIsland_WP"));
    let query_port = lookup_port_path(&context.instance.ports, "query.port")
        .unwrap_or_else(|| String::from("27015"));
    let max_players =
        lookup_json_text(context.settings, "max_players").unwrap_or_else(|| String::from("30"));
    let mut server_url = format!(
        "{}?AltSaveDirectoryName={}?QueryPort={}?MaxPlayers={}",
        map_name.trim(),
        render_ark_save_directory(context),
        query_port.trim(),
        max_players.trim()
    );

    let bind_ip = context.instance.summary.bind_ip.trim();
    if !bind_ip.is_empty() && bind_ip != "0.0.0.0" {
        server_url.push_str("?MultiHome=");
        server_url.push_str(bind_ip);
    }
    // Passwords are materialized in GameUserSettings.ini. ARK records the full
    // startup URL in its native log, so it must not duplicate those secrets.
    append_non_empty_url_setting(
        &mut server_url,
        context.settings,
        "server_name",
        "SessionName",
    );
    if lookup_json_bool(context.settings, "rcon_enabled").unwrap_or(false)
        && let Some(rcon_port) = lookup_port_path(&context.instance.ports, "rcon.port")
    {
        server_url.push_str("?RCONEnabled=true?RCONPort=");
        server_url.push_str(rcon_port.trim());
    }
    server_url
}

fn append_non_empty_url_setting(
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
