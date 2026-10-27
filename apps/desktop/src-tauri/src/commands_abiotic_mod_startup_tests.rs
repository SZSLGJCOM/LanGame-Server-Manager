use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).into()).collect()
}

#[test]
fn abiotic_default_download_limit_is_added_to_the_original_listener_budget() {
    let normal = Duration::from_secs(60);
    let launched = args(&["-Mods=6472275;6472552"]);
    assert!(needs_download_allowance("abioticfactor", &launched));
    assert_eq!(
        listener_timeout("abioticfactor", &launched, normal).unwrap(),
        Duration::from_secs(960)
    );
    assert_eq!(
        listener_timeout(
            "abioticfactor",
            &args(&["-Mods=6472275", "-ModInstallTimeout=900"]),
            normal
        )
        .unwrap(),
        Duration::from_secs(960)
    );
}

#[test]
fn abiotic_download_limit_uses_the_launched_override_without_replacing_listener_time() {
    for seconds in [1, 1200, 172800] {
        let launched = args(&["-mOdS=6472275", &format!("-modinstalltimeout={seconds}")]);
        assert_eq!(
            listener_timeout("abioticfactor", &launched, Duration::from_secs(300)).unwrap(),
            Duration::from_secs(seconds + 300)
        );
    }
}

#[test]
fn no_mods_sideloading_and_other_games_keep_their_original_budget() {
    let normal = Duration::from_secs(60);
    for launched in [
        vec![],
        args(&["-Mods="]),
        args(&["-ModInstallTimeout=1200"]),
        args(&["-Mods=6472275", "-sIdElOaDmOdS", "-ModInstallTimeout=1200"]),
    ] {
        assert!(!needs_download_allowance("abioticfactor", &launched));
        assert_eq!(
            listener_timeout("abioticfactor", &launched, normal).unwrap(),
            normal
        );
    }
    for module in ["arksurvivalascended", "dontstarve", "vrising"] {
        let launched = args(&["-Mods=6472275", "-ModInstallTimeout=1200"]);
        assert!(!needs_download_allowance(module, &launched));
        assert_eq!(listener_timeout(module, &launched, normal).unwrap(), normal);
    }
}

#[test]
fn malformed_or_unrepresentable_native_download_limits_fail_without_echoing_credentials() {
    for value in [
        "0",
        "-1",
        "0.5",
        "not-seconds",
        "18446744073709551616",
        "18446744073709551615",
    ] {
        let launched = args(&[
            "-Mods=6472275",
            "-ModioToken=fixture-private-token",
            &format!("-ModInstallTimeout={value}"),
        ]);
        let error =
            listener_timeout("abioticfactor", &launched, Duration::from_secs(60)).unwrap_err();
        assert!(!error.contains("fixture-private-token"));
        assert!(!error.contains(value));
    }
    assert!(
        listener_timeout(
            "abioticfactor",
            &args(&["-Mods=1", "-Mods=2"]),
            Duration::from_secs(60)
        )
        .is_err()
    );
    assert!(
        listener_timeout(
            "abioticfactor",
            &args(&[
                "-Mods=1",
                "-ModInstallTimeout=900",
                "-ModInstallTimeout=1200"
            ]),
            Duration::from_secs(60)
        )
        .is_err()
    );
}

#[test]
fn option_like_text_inside_other_argument_values_cannot_change_the_download_budget() {
    let launched = args(&[
        "-Mods=6472275",
        "-ModInstallTimeout=1",
        "-SteamServerName=-ModInstallTimeout=99999",
        "-WorldSaveName=-SideloadMods",
    ]);
    assert_eq!(
        listener_timeout("abioticfactor", &launched, Duration::from_secs(60)).unwrap(),
        Duration::from_secs(61)
    );
}
