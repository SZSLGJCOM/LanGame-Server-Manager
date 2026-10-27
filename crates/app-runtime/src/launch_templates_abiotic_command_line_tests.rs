use super::*;

// Current public Shipping.exe/PDB: 705106BF-5384-E32D-4A1A-C96EB49D4250, age 1.
// Native calls at RVAs 0x4929d12, 0x492cdfa, and 0x4930454 use
// FParse::Value(FCommandLine::Get(), key, FString&, true, nullptr).
// Value skips keys inside quotes (0x105babe), accepts value quotes (0x105bb57),
// and otherwise stops on ",) \r\n\t" (separator literal RVA 0x61b67b8).
const ABIOTIC_EXE: &str = "server package/AbioticFactorServer-Win64-Shipping.exe";

// Reference model for this image's FParse::Value buffer overload (0x105b9f0),
// not CRT argv: the key scan toggles on every quote, including after '\\'
// (0x105bafa), and quoted values use wcsstr(L"\"") then memcpy without unescaping
// (0x105bb6b, 0x105bc57). Keep expectations tied to the package trace fixture.
fn native_fparse_value(command_line: &str, key: &str) -> Option<String> {
    let bytes = command_line.as_bytes();
    let mut quoted = false;
    let mut previous_alphanumeric = false;
    for (index, &character) in bytes.iter().enumerate() {
        if !quoted
            && !previous_alphanumeric
            && bytes
                .get(index..index + key.len())
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(key.as_bytes()))
        {
            let value = &command_line[index + key.len()..];
            if let Some(value) = value.strip_prefix('"') {
                return Some(value[..value.find('"').unwrap_or(value.len())].to_string());
            }
            let value = value.trim_start_matches([' ', '\r', '\n', '\t']);
            let end = value
                .find([',', ')', ' ', '\r', '\n', '\t'])
                .unwrap_or(value.len());
            return Some(value[..end].to_string());
        }
        if character == b'"' {
            quoted = !quoted;
        }
        previous_alphanumeric = character.is_ascii_alphanumeric();
    }
    None
}

#[test]
fn abiotic_native_room_values_round_trip_spaces_and_native_delimiters() {
    for key in [
        "-SteamServerName",
        "-WorldSaveName",
        "-ServerPassword",
        "-AdminPassword",
    ] {
        for value in [
            "synthetic-test-only room",
            "synthetic-test-only,room",
            "synthetic-test-only)room",
            "synthetic-test-only (room,one)",
            "合成世界 (one,two)",
            " synthetic-test-only ",
            " ",
        ] {
            let argument = format!("{key}={value}");
            let command_line = quote_launch_argument(Path::new(ABIOTIC_EXE), &argument);
            assert_eq!(
                native_fparse_value(&command_line, &format!("{key}=")),
                Some(value.to_string())
            );
            assert_eq!(command_line, format!("{key}=\"{value}\""));
        }
    }
}

#[test]
fn abiotic_native_password_backslashes_are_literal_and_do_not_hide_later_keys() {
    for key in ["-ServerPassword", "-AdminPassword"] {
        for value in [
            r"synthetic-test-only\",
            r"synthetic-test-only room\",
            r"synthetic-test-only,room\\",
            r"synthetic\test-only\room",
        ] {
            let first = quote_launch_argument(Path::new(ABIOTIC_EXE), &format!("{key}={value}"));
            let second =
                quote_launch_argument(Path::new(ABIOTIC_EXE), "-WorldSaveName=following world");
            let command_line = format!("{first} {second}");
            assert_eq!(
                native_fparse_value(&command_line, &format!("{key}=")),
                Some(value.to_string())
            );
            assert_eq!(
                native_fparse_value(&command_line, "-WorldSaveName="),
                Some("following world".to_string())
            );
        }
    }
}

#[test]
fn abiotic_native_reference_parser_exposes_crt_quote_and_escape_mismatches() {
    let argument = "-WorldSaveName=synthetic-test-only room";
    assert_eq!(
        native_fparse_value(&quote_command_segment(argument), "-WorldSaveName="),
        None
    );
    assert_eq!(
        native_fparse_value("-WorldSaveName=synthetic-test-only,room", "-WorldSaveName="),
        Some("synthetic-test-only".to_string())
    );
    // FParse copies the slash instead of treating it as an escape for the quote.
    let synthetic_interrupted = r"synthetic-test-only\";
    assert_eq!(
        native_fparse_value(
            &format!(r#"-ServerPassword="{synthetic_interrupted}"rest""#),
            "-ServerPassword="
        ),
        Some(synthetic_interrupted.to_string())
    );
    assert_eq!(
        native_fparse_value(
            &format!(r#"-ServerPassword="{synthetic_interrupted}" -WorldSaveName="next""#),
            "-WorldSaveName="
        ),
        Some("next".to_string())
    );
}

#[test]
fn abiotic_mod_path_values_are_always_quoted_for_native_fparse() {
    for key in ["-ModsDir", "-SideloadModsPath"] {
        for directory in [
            "runtime/AbioticFactor/Mods/native packages",
            "runtime/AbioticFactor/Mods/curated,mods",
            "runtime/AbioticFactor/Mods/curated(mods)",
            "runtime/AbioticFactor/Mods/curated)mods",
            "runtime/AbioticFactor/Mods/plain",
            r"runtime\AbioticFactor\Mods\native packages",
        ] {
            assert_eq!(
                quote_launch_argument(Path::new(ABIOTIC_EXE), &format!("{key}={directory}")),
                format!("{key}=\"{directory}\""),
                "{key}: {directory}"
            );
        }
    }
}

#[test]
fn abiotic_native_path_matching_is_case_insensitive() {
    let executable = Path::new("server/ABIOTICFACTORSERVER-win64-SHIPPING.EXE");
    for key in [
        "-mOdSdIr",
        "-sIdElOaDmOdSpAtH",
        "-sTeAmSeRvErNaMe",
        "-wOrLdSaVeNaMe",
        "-sErVeRpAsSwOrD",
        "-aDmInPaSsWoRd",
    ] {
        assert_eq!(
            quote_launch_argument(executable, &format!("{key}=runtime/curated,mods)")),
            format!("{key}=\"runtime/curated,mods)\"")
        );
    }
}

#[test]
fn abiotic_native_path_quoting_does_not_change_other_options_or_executables() {
    for executable in [
        "FactoryServer.exe",
        "AbioticFactorServer.exe",
        "AbioticFactorServer-Win64-Shipping.exe.bak",
        "ArkAscendedServer.exe",
    ] {
        for key in [
            "-ModsDir",
            "-SideloadModsPath",
            "-SteamServerName",
            "-WorldSaveName",
            "-ServerPassword",
            "-AdminPassword",
        ] {
            assert_eq!(
                quote_launch_argument(Path::new(executable), &format!("{key}=runtime/mod packs")),
                format!("\"{key}=runtime/mod packs\"")
            );
            assert_eq!(
                quote_launch_argument(Path::new(executable), &format!("{key}=runtime/mods,)")),
                format!("{key}=runtime/mods,)")
            );
        }
    }
    for argument in [
        "-LogCmds=Log Category",
        "-ModsDirectory=runtime/mod packs",
        "-OtherModsDir=runtime/mod packs",
        "-WorldSaveNames=world one",
        "-OtherSteamServerName=room one",
        "-SandboxIniPath=Config/generated path.ini",
        "-AdminIniPath=SaveGames/generated path.ini",
    ] {
        assert_eq!(
            quote_launch_argument(Path::new(ABIOTIC_EXE), argument),
            format!("\"{argument}\"")
        );
    }
    for argument in ["-Mods=6472275;6472552", "-ModsDir", "-SideloadMods"] {
        assert_eq!(
            quote_launch_argument(Path::new(ABIOTIC_EXE), argument),
            argument
        );
    }
}

#[test]
fn abiotic_preview_and_windows_spawn_share_native_path_quoting_without_mutating_argv() {
    let executable = Path::new(ABIOTIC_EXE);
    let synthetic_join = r" synthetic-test-only room\";
    let synthetic_admin = r"synthetic-test-only,admin\";
    let args = vec![
        "-Mods=6472275".to_string(),
        "-ModsDir=runtime/AbioticFactor/Saved/curated mods".to_string(),
        "-SideloadModsPath=runtime/AbioticFactor/Mods/curated,mods)".to_string(),
        "-SteamServerName=synthetic-test-only server".to_string(),
        "-WorldSaveName=合成世界 (one,two)".to_string(),
        format!("-ServerPassword={synthetic_join}"),
        format!("-AdminPassword={synthetic_admin}"),
        "-LogCmds=Log Category".to_string(),
    ];
    let canonical_args = args.clone();
    let (spawn_executable, spawn_args) =
        build_spawn_command(executable, Path::new("server package"), &args).unwrap();
    // spawn_native_process supplies this exact serializer to CreateProcessW.
    let spawn_command_line = crate::build_spawn_command_line(&spawn_executable, &spawn_args);
    let prefix = concat!(
        "\"server package/AbioticFactorServer-Win64-Shipping.exe\"",
        " -Mods=6472275",
        " -ModsDir=\"runtime/AbioticFactor/Saved/curated mods\"",
        " -SideloadModsPath=\"runtime/AbioticFactor/Mods/curated,mods)\"",
        " -SteamServerName=\"synthetic-test-only server\"",
        " -WorldSaveName=\"合成世界 (one,two)\""
    );
    let expected = format!(
        "{prefix} -ServerPassword=\"{synthetic_join}\" -AdminPassword=\"{synthetic_admin}\" \"-LogCmds=Log Category\""
    );
    assert_eq!(spawn_command_line, expected);
    assert_eq!(build_command_line(executable, &args), expected);
    assert_eq!(spawn_args, canonical_args);
    assert_eq!(args, canonical_args);
}

#[test]
fn ark_native_ini_path_quoting_remains_unchanged() {
    for executable in ["ArkAscendedServer.exe", "ShooterGameServer.exe"] {
        for key in ["-GameINI", "-GameUserSettingsINI"] {
            assert_eq!(
                quote_launch_argument(
                    Path::new(executable),
                    &format!("{key}=config/game,one).ini")
                ),
                format!("{key}=\"config/game,one).ini\"")
            );
        }
    }
}
