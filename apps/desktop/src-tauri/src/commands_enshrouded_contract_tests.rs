use super::*;

const IDS: [u64; 2] = [9_007_199_254_740_993, u64::MAX];

pub(super) struct ContractProbeContext<'a> {
    pub(super) runtime: &'a Path,
    pub(super) fixture: &'a Path,
    pub(super) evidence: &'a Path,
    pub(super) query_port: u16,
    pub(super) allow_native_udp: bool,
    pub(super) secrets: &'a [String],
}

fn exact_native_ids(value: &Value) -> bool {
    let Some(records) = value.as_array() else {
        return false;
    };
    if records.len() != IDS.len() {
        return false;
    }
    let actual = records
        .iter()
        .map(|record| {
            record
                .as_object()
                .and_then(|record| record.get("accountId"))
                .and_then(Value::as_u64)
        })
        .collect::<Option<std::collections::BTreeSet<_>>>();
    actual == Some(IDS.into_iter().collect())
}

pub(super) async fn run(
    plan: &app_core::LaunchPlan,
    details: &InstanceDetails,
    owned: &mut OwnedNative,
    context: ContractProbeContext<'_>,
) -> ProbeResult<Vec<Value>> {
    let ContractProbeContext {
        runtime,
        fixture,
        evidence,
        query_port: port,
        allow_native_udp: allow_udp,
        secrets,
    } = context;
    if !plan.ready_to_launch || plan.requires_admin {
        return Err("contract probe requires an unelevated ready native launch plan".into());
    }
    let config_path = runtime.join("enshrouded_server.json");
    let baseline: Value = serde_json::from_slice(&fs::read(&config_path)?)?;
    let objects = |field: &str, wrapped: bool| {
        Value::Array(
            IDS.iter()
                .map(|id| {
                    let identity = if wrapped {
                        json!({"value": id})
                    } else {
                        json!(id)
                    };
                    let mut entry = serde_json::Map::new();
                    entry.insert(field.into(), identity);
                    entry.insert(
                        "displayName".into(),
                        json!("LGSM synthetic native contract"),
                    );
                    entry.insert("characterName".into(), json!("Synthetic probe"));
                    entry.insert("banDate".into(), json!({"value": 0}));
                    entry.insert("lgsmUnrecognizedEntryProbe".into(), json!(true));
                    Value::Object(entry)
                })
                .collect(),
        )
    };
    let cases = [
        ("legacy-objects", "bans", objects("accountId", false)),
        (
            "native-objects",
            "bannedAccounts",
            objects("accountId", false),
        ),
        ("native-scalars", "bannedAccounts", json!(IDS)),
        (
            "native-hash-objects",
            "bannedAccounts",
            objects("accountIdHash", false),
        ),
        (
            "native-wrapped-hashes",
            "bannedAccounts",
            objects("accountIdHash", true),
        ),
        (
            "native-wrapped-ids",
            "bannedAccounts",
            objects("accountId", true),
        ),
    ];
    let mut results = Vec::new();
    let mut canonical_accepted = false;
    for (name, key, input) in cases {
        let mut config = baseline.clone();
        let object = config
            .as_object_mut()
            .ok_or("native configuration must be an object")?;
        object.remove("bans");
        object.remove("bannedAccounts");
        object.insert(key.into(), input.clone());
        object.insert("lgsmUnrecognizedRootProbe".into(), json!(true));
        fs::write(&config_path, serde_json::to_vec_pretty(&config)?)?;
        let phase = format!("contract-{name}");
        let log = creation::instance_root(details)?
            .join("logs")
            .join(format!("native-{phase}.log"));
        if log.exists() {
            return Err("contract probe needs a fresh per-launch log".into());
        }
        owned.cleanup_allowed.store(false, Ordering::SeqCst);
        owned.process = Some(app_runtime::spawn_launch_plan(plan, &log)?);
        let process = owned
            .process
            .as_mut()
            .ok_or("missing owned native contract process")?;
        let pid = process.pid;
        let identity = process.process_identity.clone();
        let observed = observe_native(
            process,
            NativeObservation {
                query_port: port,
                evidence,
                phase: &phase,
                observation_seconds: 0,
                allow_native_udp: allow_udp,
                log_path: &log,
                expected_flame_level: 1,
                world_path: &Path::new(&details.saves_path).join("world0.ksd"),
            },
        )
        .await;
        let exit = app_runtime::stop_spawned_process_gracefully(process, Duration::from_secs(20))?;
        if process.child.is_some() || app_runtime::process_matches_identity(pid, &identity)? {
            return Err("contract probe did not release its complete owned process tree".into());
        }
        owned.process.take();
        owned.cleanup_allowed.store(true, Ordering::SeqCst);
        let written: Value = serde_json::from_slice(&fs::read(&config_path)?)?;
        let canonical = written
            .get("bannedAccounts")
            .cloned()
            .unwrap_or(Value::Null);
        let exact_ids = exact_native_ids(&canonical);
        let observation_error = observed.as_ref().err().map(ToString::to_string);
        let native_rewritten = written.get("lgsmUnrecognizedRootProbe").is_none()
            && written.get("bans").is_none()
            && !serde_json::to_string(&canonical)?.contains("lgsmUnrecognizedEntryProbe");
        let accepted = observed.is_ok() && exit == Some(0) && exact_ids && native_rewritten;
        let result = json!({"case": name, "pid": pid, "input_key": key, "input": input,
            "native_world_ready": observed.is_ok(), "observation_error": observation_error,
            "exit_code": exit, "canonical_output": canonical, "legacy_output": written.get("bans"),
            "exact_wide_ids_retained": exact_ids, "unknown_sentinels_removed_by_native": native_rewritten, "accepted": accepted});
        // These records contain only synthetic probe identities; never role credentials.
        println!(
            "ENSHROUDED_NONEMPTY_CONTRACT {}",
            serde_json::to_string(&result)?
        );
        write_new(
            &evidence.join(format!("{phase}.json")),
            &serde_json::to_vec_pretty(&result)?,
        )?;
        capture(evidence, &phase, details, runtime, fixture, secrets)?;
        results.push(result);
        // A rejected network scope or failed world readiness ends the probe;
        // trying another JSON shape cannot resolve a runtime safety failure.
        observed?;
        if key == "bannedAccounts" && accepted {
            canonical_accepted = true;
            break;
        }
    }
    if !canonical_accepted {
        return Err(
            "no candidate nonempty native ban contract was confirmed; review retained evidence"
                .into(),
        );
    }
    Ok(results)
}
