use super::*;

#[test]
fn runtime_files_have_exact_routes() {
    let files = [
        "BepInEx/plugins/One.dll",
        "plugins/Two.dll",
        "BEPINEX/CONFIG/One.cfg",
        "patchers/Patch.dll",
        "README.md",
        "Helper.dll",
    ];
    let routes = thunderstore_payload_routes(&files.map(PathBuf::from)).unwrap();
    assert_eq!(routes.len(), files.len());
    assert_eq!(routes[0].relative, PathBuf::from("One.dll"));
    assert_eq!(routes[2].kind, ThunderstorePayloadKind::Config);
    assert_eq!(routes[3].kind, ThunderstorePayloadKind::Patcher);
    assert_eq!(routes[4].kind, ThunderstorePayloadKind::Metadata);
    assert_eq!(routes[5].kind, ThunderstorePayloadKind::Plugin);
}

#[test]
fn runtime_aliases_collide_before_publication() {
    for paths in [
        ["BepInEx/plugins/One.dll", "plugins/one.dll"],
        ["config/One.cfg", "BepInEx/config/one.cfg"],
        ["patchers/P.dll", "BepInEx/patchers/P.dll"],
    ] {
        assert!(
            thunderstore_payload_routes(&paths.map(PathBuf::from))
                .unwrap_err()
                .contains("collide")
        );
    }
}

#[test]
fn loaders_and_unsafe_paths_cannot_be_routed_as_plugins() {
    for path in [
        "BepInEx/core/Loader.dll",
        "BepInEx/interop/Game.dll",
        "winhttp.dll",
        "doorstop_config.ini",
        "dotnet/coreclr.dll",
        ".langame-online-mods.json",
        "../plugins/Bad.dll",
        "C:/Bad.dll",
        "plugins//Bad.dll",
    ] {
        assert!(
            thunderstore_payload_routes(&[PathBuf::from(path)]).is_err(),
            "{path}"
        );
    }
}

#[test]
fn backslash_paths_preserve_names() {
    let routes =
        thunderstore_payload_routes(&[PathBuf::from(r"BEPINEX\Plugins\插件\Plugin.DLL")]).unwrap();
    assert_eq!(routes[0].relative, Path::new("插件").join("Plugin.DLL"));
}
