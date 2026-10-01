use super::super::*;
use std::collections::HashSet;

pub(super) const GAME_MODE: &str = "/script/shootergame.shootergamemode";
pub(super) type NativeKey = (String, String);

pub(super) fn canonical_key(key: &str) -> String {
    key.trim().to_ascii_lowercase()
}

pub(super) fn key_family(key: &str) -> String {
    let key = canonical_key(key);
    let key = key.trim_start_matches(['+', '-', '.', '!']);
    let base = key.split('[').next().unwrap_or(key);
    if base == "perlevelstatsmultiplier_dinotamed_add"
        || base == "perlevelstatsmultiplier_dinotamed_affinity"
    {
        String::from("perlevelstatsmultiplier_dinotamed")
    } else {
        base.to_owned()
    }
}

pub(super) fn owned_keys(module_id: &str, game_ini: bool) -> HashSet<NativeKey> {
    let asa = module_id == "arksurvivalascended";
    let mut result = HashSet::new();
    let mut add = |section: &str, definitions: &[ArkIniSetting]| {
        result.extend(
            definitions
                .iter()
                .map(|definition| (section.to_ascii_lowercase(), key_family(definition.0))),
        );
    };
    if game_ini {
        if asa {
            add(GAME_MODE, ARK_ASA_GAME_INI);
            add(GAME_MODE, ARK_ASA_ADVANCED_GAME_INI);
        } else {
            add(GAME_MODE, ARK_ASE_GAME_INI);
            result.insert((String::from("modinstaller"), String::from("modids")));
        }
    } else {
        if asa {
            add("serversettings", ARK_ASA_GUS_SERVER_SETTINGS);
            add("serversettings", ARK_ASA_PATCH_GUS_SERVER_SETTINGS);
            add("sessionsettings", ARK_ASA_GUS_SESSION_SETTINGS);
            add("messageoftheday", ARK_ASA_GUS_MOTD);
        } else {
            add("serversettings", ARK_ASE_GUS_SERVER_SETTINGS);
            add("sessionsettings", ARK_ASE_GUS_SESSION_SETTINGS);
            add("/script/engine.gamesession", ARK_ASE_GUS_ENGINE_SESSION);
            add("ragnarok", ARK_ASE_GUS_RAGNAROK);
            add("messageoftheday", ARK_ASE_GUS_MOTD);
        }
        for (section, key) in [
            ("serversettings", "rconport"),
            ("serversettings", "activemods"),
            ("sessionsettings", "port"),
            ("sessionsettings", "queryport"),
            ("sessionsettings", "maxplayers"),
            ("sessionsettings", "multihome"),
            ("/script/engine.gamesession", "maxplayers"),
            ("/script/shootergame.shootergameusersettings", "version"),
        ] {
            result.insert((section.to_owned(), key.to_owned()));
        }
    }
    result
}
