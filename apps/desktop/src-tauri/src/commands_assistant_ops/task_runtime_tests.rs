use super::*;

const NONCE: &str = "0123456789abcdef0123456789abcdef";

fn native_rows(flags: &str) -> Vec<String> {
    [
        format!("[00:00:12]: [LGSM-DST-MODS-BEGIN:{NONCE}]"),
        format!("[00:00:12]: [LGSM-DST-MOD:{NONCE}] 1 {flags}"),
        format!("[00:00:12]: [LGSM-DST-MODS-END:{NONCE}] 1"),
    ]
    .to_vec()
}

#[test]
fn assistant_task_runtime_requires_complete_nonce_bound_frame() {
    let lines = native_rows("1 1 1 1 0 2");
    let rows = parse_assistant_required_mod_probe(&lines, NONCE, 1)
        .unwrap()
        .unwrap();
    assert_eq!(rows[0].load_index, 2);
    assert!(matches!(
        assess_assistant_required_mod_probe(&rows),
        AssistantTaskCheckStatus::Satisfied
    ));
    assert!(
        parse_assistant_required_mod_probe(&lines[..2], NONCE, 1)
            .unwrap()
            .is_none()
    );
    assert!(
        parse_assistant_required_mod_probe(&lines, "ffffffffffffffffffffffffffffffff", 1)
            .unwrap()
            .is_none()
    );
}

#[test]
fn assistant_task_runtime_rejects_echo_duplicates_wrong_counts_and_malformed_flags() {
    let echoes = [format!(
        "RemoteCommandInput: print('[LGSM-DST-MODS-BEGIN:{NONCE}]')"
    )];
    assert!(
        parse_assistant_required_mod_probe(&echoes, NONCE, 1)
            .unwrap()
            .is_none()
    );
    for flags in ["1 1 1 1 2 1", "1 1 1 1 0 0", "0 1 1 1 0 1", "1 1 1 1 0 513"] {
        assert!(
            parse_assistant_required_mod_probe(&native_rows(flags), NONCE, 1).is_err(),
            "{flags}"
        );
    }
    let mut duplicate = native_rows("1 1 1 1 0 1");
    duplicate.insert(2, duplicate[1].clone());
    assert!(parse_assistant_required_mod_probe(&duplicate, NONCE, 1).is_err());
    assert!(parse_assistant_required_mod_probe(&native_rows("1 1 1 1 0 1"), NONCE, 2).is_err());
}

#[test]
fn assistant_task_runtime_missing_disabled_incompatible_or_failed_mod_is_not_satisfied() {
    for flags in [
        "0 0 0 0 0 0",
        "1 0 1 1 0 1",
        "1 1 0 1 0 1",
        "1 1 1 0 0 1",
        "1 1 1 1 1 1",
    ] {
        let rows = parse_assistant_required_mod_probe(&native_rows(flags), NONCE, 1)
            .unwrap()
            .unwrap();
        assert!(
            matches!(
                assess_assistant_required_mod_probe(&rows),
                AssistantTaskCheckStatus::Failed
            ),
            "{flags}"
        );
    }
}

fn execute_probe(setup: &str) -> Vec<String> {
    let lua = mlua::Lua::new();
    lua.load("out={};function print(s)table.insert(out,s)end;function InGamePlay()return true end;TheWorld={ismastersim=true};ModManager={mods={},enabledmods={},failedmods={}};KnownModIndex={};function KnownModIndex:IsModEnabledAny(name)return name=='local_library'end").exec().unwrap();
    lua.load(setup).exec().unwrap();
    lua.load(assistant_required_mod_probe_command(NONCE, &["local_library".into()]).unwrap())
        .exec()
        .unwrap();
    lua.load("return out").eval().unwrap()
}

#[test]
fn assistant_task_runtime_probe_uses_engine_state_without_fixture_globals() {
    let lines = execute_probe(
        "ModManager.mods={{modname='local_library',modinfo={dst_compatible=true}}};ModManager.enabledmods={'local_library'}",
    );
    let rows = parse_assistant_required_mod_probe(&lines, NONCE, 1)
        .unwrap()
        .unwrap();
    assert!(matches!(
        assess_assistant_required_mod_probe(&rows),
        AssistantTaskCheckStatus::Satisfied
    ));
    for failure in [
        "ModManager.failedmods={{name='local_library',error='failed'}}",
        "ModManager.mods[1].modinfo.failed=true",
        "ModManager.mods[1].modinfo.dst_compatible=false",
    ] {
        let lines = execute_probe(&format!(
            "ModManager.mods={{{{modname='local_library',modinfo={{dst_compatible=true}}}}}};ModManager.enabledmods={{'local_library'}};{failure}"
        ));
        let rows = parse_assistant_required_mod_probe(&lines, NONCE, 1)
            .unwrap()
            .unwrap();
        assert!(
            matches!(
                assess_assistant_required_mod_probe(&rows),
                AssistantTaskCheckStatus::Failed
            ),
            "{failure}"
        );
    }
}

#[test]
fn assistant_task_runtime_probe_rejects_unsafe_inputs_and_unavailable_engine_state() {
    for name in ["../other", "CON", "x:stream", "bad\nname"] {
        assert!(assistant_required_mod_probe_command(NONCE, &[name.into()]).is_err());
    }
    assert!(
        assistant_required_mod_probe_command("x');Shutdown()", &["local_library".into()]).is_err()
    );
    assert!(
        assistant_required_mod_probe_command(NONCE, &vec!["local_library".into(); 65]).is_err()
    );
    for setup in [
        "TheWorld=nil",
        "ModManager=nil",
        "function InGamePlay()return false end",
        "KnownModIndex=nil",
        "for i=1,513 do ModManager.enabledmods[i]='mod'..i end",
    ] {
        let lines = execute_probe(setup);
        assert!(
            parse_assistant_required_mod_probe(&lines, NONCE, 1).is_err(),
            "{setup}"
        );
    }
}

#[tokio::test]
async fn assistant_task_runtime_read_errors_and_capture_limits_never_satisfy() {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    let error = await_assistant_mod_frame(NONCE, 1, deadline, || async {
        Err(String::from("Runtime Mod log changed during collection."))
    })
    .await
    .unwrap_err();
    assert!(error.contains("log changed"));
    let error = await_assistant_mod_frame(NONCE, 1, deadline, || async {
        Ok((true, vec!["x".repeat(4097)]))
    })
    .await
    .unwrap_err();
    assert!(error.contains("capture limit"));
}

#[test]
fn assistant_task_runtime_log_baseline_skips_old_frames_and_rejects_replacement() {
    use std::io::Write;
    let base = std::env::var_os("LANGAME_ASSISTANT_TEST_WORK_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let path = base.join(format!(
        "assistant-mod-probe-{}.log",
        uuid::Uuid::new_v4().simple()
    ));
    let retained = path.with_extension("retained");
    fs::write(
        &path,
        format!("{}\n", native_rows("1 1 1 1 0 1").join("\n")),
    )
    .unwrap();
    let mut capture = AssistantModProbeLog::open(path.clone(), ASSISTANT_MOD_PROBE_BYTES).unwrap();
    assert!(
        capture.read().unwrap().is_empty(),
        "old nonce frames precede the baseline"
    );
    {
        let mut writer = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(writer, "[00:00:13]: fresh output").unwrap();
    }
    assert_eq!(capture.read().unwrap(), ["[00:00:13]: fresh output"]);
    fs::rename(&path, &retained).unwrap();
    fs::write(&path, "x".repeat(4096)).unwrap();
    assert!(capture.read().unwrap_err().contains("log changed"));
    fs::remove_file(&path).unwrap();
    fs::remove_file(&retained).unwrap();
}

#[tokio::test]
async fn assistant_task_runtime_timeout_and_runtime_replacement_remain_unknown() {
    let error = await_assistant_mod_frame(NONCE, 1, tokio::time::Instant::now(), || async {
        panic!("expired probe must not observe")
    })
    .await
    .unwrap_err();
    assert!(error.contains("deadline"), "{error}");
    let error = await_assistant_mod_frame(
        NONCE,
        1,
        tokio::time::Instant::now() + Duration::from_secs(1),
        || async { Ok((false, native_rows("1 1 1 1 0 1"))) },
    )
    .await
    .unwrap_err();
    assert!(error.contains("run changed"), "{error}");
    let rows = await_assistant_mod_frame(
        NONCE,
        1,
        tokio::time::Instant::now() + Duration::from_secs(1),
        || async { Ok((true, native_rows("1 1 1 1 0 1"))) },
    )
    .await
    .unwrap();
    assert!(matches!(
        assess_assistant_required_mod_probe(&rows),
        AssistantTaskCheckStatus::Satisfied
    ));
}
