#[derive(Debug, Clone, PartialEq, Eq)]
struct AssistantRequiredModObservation {
    present: bool,
    enabled: bool,
    selected: bool,
    compatible: bool,
    failed: bool,
    load_index: usize,
}

const ASSISTANT_MOD_PROBE_BYTES: usize = 64 * 1024;
const ASSISTANT_MOD_PROBE_LIMIT: usize = 64;

pub(super) async fn verify_assistant_required_mods(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    requirements: &[AssistantRequiredMod],
) -> AssistantTaskCheck {
    use AssistantTaskCheckStatus::{Satisfied, Unknown};
    if requirements.is_empty() {
        return assistant_task_check(
            "required_mods_running",
            Satisfied,
            "没有需要保留的 MOD 运行要求。",
            json!({"entries": []}),
        );
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    let outcome = tokio::time::timeout_at(
        deadline,
        collect_assistant_required_mods(state, storage, details, requirements, deadline),
    )
    .await;
    let (status, summary, evidence) = match outcome {
        Ok(Ok((rows, entries))) => {
            let status = assess_assistant_required_mod_probe(&rows);
            let summary = if status == Satisfied {
                "当前运行分片确认所需 MOD 已启用，存在加载环境，且这些 MOD 没有原生加载失败。"
            } else {
                "当前运行分片未满足所需 MOD 的启用与加载要求。"
            };
            (
                status,
                summary,
                json!({"entries": entries, "limitation": "Native runtime state confirms selection and absence of recorded load failure, not complete Mod functionality."}),
            )
        }
        Ok(Err(reason)) => (
            Unknown,
            "无法完整确认所需 MOD 的实际运行状态。",
            json!({"gap": reason}),
        ),
        Err(_) => (
            Unknown,
            "MOD 运行验证超时，未将其判定为成功。",
            json!({"gap": "Runtime Mod verification deadline exceeded."}),
        ),
    };
    assistant_task_check("required_mods_running", status, summary, evidence)
}

async fn collect_assistant_required_mods(
    state: &tauri::State<'_, DesktopState>,
    storage: &StorageBootstrap,
    details: &InstanceDetails,
    requirements: &[AssistantRequiredMod],
    deadline: tokio::time::Instant,
) -> Result<(Vec<AssistantRequiredModObservation>, Vec<Value>), String> {
    if details.summary.module_id != "dontstarve" || requirements.len() > ASSISTANT_MOD_PROBE_LIMIT {
        return Err("Runtime Mod verification requires DST and at most 64 directories.".into());
    }
    let mut groups = std::collections::BTreeMap::<String, Vec<String>>::new();
    for requirement in requirements {
        crate::dst_mods::validate_dst_mod_directory_name(&requirement.folder_name)?;
        let shard = match requirement.shard.as_str() {
            "master" | "caves" => requirement.shard.as_str(),
            _ => return Err("Unsupported required Mod shard.".into()),
        };
        let names = groups.entry(shard.into()).or_default();
        if !names.contains(&requirement.folder_name) {
            names.push(requirement.folder_name.clone());
        }
    }
    let operation = state.begin_storage_context_operation("Assistant runtime Mod verification")?;
    ensure_storage_context_snapshot_current(state, storage, "runtime Mod verification")?;
    // The fixed stdin probe must finish using this run before a stop can take
    // its console or replace the process. The outer deadline bounds this wait.
    let _instance_lock = state.acquire_instance_mutation(&details.summary.id).await;
    let fresh = read_instance_details(&storage.paths, &details.summary.id)
        .await
        .map_err(|_| "Cannot read the selected runtime.".to_owned())?;
    if !assistant_mod_run_unchanged(details, &fresh) {
        return Err("The server run changed before Mod verification.".into());
    }
    let run = fresh.active_run.as_ref().ok_or("No active server run.")?;
    let mut observed = Vec::new();
    let mut entries = Vec::new();
    let mut remaining_bytes = ASSISTANT_MOD_PROBE_BYTES;
    for (shard, names) in groups {
        let process = run
            .processes
            .iter()
            .find(|process| process.process_key == shard && process.status == "running")
            .ok_or("The required shard is not running.")?;
        if !assistant_mod_target_current(state, &fresh.summary.id, run.run_id, process)? {
            return Err("The server run changed before Mod verification.".into());
        }
        let path = PathBuf::from(
            process
                .log_path
                .as_ref()
                .ok_or("The required shard has no runtime log.")?,
        );
        let log = crate::state::spawn_blocking_storage_context_task(&operation, move || {
            AssistantModProbeLog::open(path, remaining_bytes)
        })
        .await
        .map_err(|_| "Cannot prepare the runtime Mod log.".to_owned())??;
        let log = Arc::new(StdMutex::new(log));
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let command = assistant_required_mod_probe_command(&nonce, &names)?;
        let receipt = dispatch_managed_stdin_command_with_budget(
            state,
            &fresh.summary.id,
            Some(&shard),
            &command,
            Some(run.run_id),
            RuntimeStdinDispatchBudget::tracked_until(
                deadline.into_std(),
                app_runtime::RuntimeCommandSubmissionTracker::default(),
            ),
        )
        .await
        .map_err(|_| "The fixed runtime Mod probe could not be submitted.".to_owned())?;
        if Some(receipt.target.pid) != process.pid
            || receipt.target.process_key != process.process_key
        {
            return Err("The server run changed while submitting the Mod probe.".into());
        }
        let rows = await_assistant_mod_frame(&nonce, names.len(), deadline, || {
            let log = log.clone();
            let operation = operation.clone();
            let instance_id = &fresh.summary.id;
            async move {
                ensure_storage_context_snapshot_current(
                    state,
                    storage,
                    "runtime Mod verification",
                )?;
                let current =
                    assistant_mod_target_current(state, instance_id, run.run_id, process)?;
                if !current {
                    return Ok((false, Vec::new()));
                }
                let lines =
                    crate::state::spawn_blocking_storage_context_task(&operation, move || {
                        log.lock()
                            .map_err(|_| "Runtime Mod log lock unavailable.".to_owned())?
                            .read()
                    })
                    .await
                    .map_err(|_| "Runtime Mod log worker failed.".to_owned())??;
                Ok((
                    assistant_mod_target_current(state, instance_id, run.run_id, process)?,
                    lines,
                ))
            }
        })
        .await?;
        remaining_bytes = remaining_bytes.saturating_sub(
            log.lock()
                .map_err(|_| "Runtime Mod log lock unavailable.")?
                .bytes,
        );
        for (name, row) in names.iter().zip(&rows) {
            entries.push(json!({"shard": shard, "folderName": name, "processRunId": process.run_id,
                "environmentPresent": row.present, "runtimeEnabled": row.enabled, "selectedForLoad": row.selected,
                "nativeCompatible": row.compatible, "loadFailed": row.failed, "loadIndex": row.load_index}));
        }
        observed.extend(rows);
    }
    let current = read_instance_details(&storage.paths, &details.summary.id)
        .await
        .map_err(|_| "Cannot recheck the selected runtime.".to_owned())?;
    if !assistant_mod_run_unchanged(&fresh, &current) {
        return Err("The server run changed during Mod verification.".into());
    }
    for process in run.processes.iter().filter(|process| {
        requirements
            .iter()
            .any(|required| required.shard == process.process_key)
    }) {
        if !assistant_mod_target_current(state, &fresh.summary.id, run.run_id, process)? {
            return Err("The server run changed after Mod verification.".into());
        }
    }
    ensure_storage_context_snapshot_current(state, storage, "runtime Mod verification")?;
    Ok((observed, entries))
}

fn assistant_mod_run_unchanged(before: &InstanceDetails, after: &InstanceDetails) -> bool {
    let (Some(before_run), Some(after_run)) = (&before.active_run, &after.active_run) else {
        return false;
    };
    AssistantOperationPrecondition::from_details(before)
        .validate(after)
        .is_ok()
        && before_run.processes.iter().all(|left| {
            after_run.processes.iter().any(|right| {
                left.run_id == right.run_id
                    && left.process_key == right.process_key
                    && left.log_path == right.log_path
            })
        })
}

fn assistant_mod_target_current(
    state: &DesktopState,
    id: &str,
    run_id: i64,
    process: &app_core::InstanceProcessState,
) -> Result<bool, String> {
    let Some(pid) = process.pid else {
        return Ok(false);
    };
    let mut supervisor = state
        .runtime_supervisor
        .lock()
        .map_err(|_| "Runtime supervisor unavailable.".to_owned())?;
    if ensure_expected_supervisor_run(&supervisor, id, Some(run_id)).is_err() {
        return Ok(false);
    }
    supervisor
        .matches_running_process(id, process.run_id, &process.process_key, pid)
        .map_err(|_| "Cannot verify the managed process identity.".to_owned())
}

fn assistant_required_mod_probe_command(nonce: &str, names: &[String]) -> Result<String, String> {
    if nonce.len() != 32
        || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
        || names.is_empty()
        || names.len() > ASSISTANT_MOD_PROBE_LIMIT
    {
        return Err("Invalid bounded runtime Mod probe request.".into());
    }
    let names = names
        .iter()
        .map(|name| {
            crate::dst_mods::validate_dst_mod_directory_name(name)?;
            serde_json::to_string(name).map_err(|_| "Cannot encode the Mod directory.".to_owned())
        })
        .collect::<Result<Vec<_>, String>>()?
        .join(",");
    // Fixed read-only code. Directory literals and the random nonce are the only
    // inputs; no game functions are replaced and no persistent globals are added.
    let source = r#"do local q={__NAMES__};local nonce='__NONCE__';local ok,rows=pcall(function()
        local mm=rawget(_G,'ModManager');local ki=rawget(_G,'KnownModIndex');local w=rawget(_G,'TheWorld');local playing=rawget(_G,'InGamePlay');
        assert(type(playing)=='function' and playing() and w and w.ismastersim and type(mm)=='table' and type(ki)=='table' and type(ki.IsModEnabledAny)=='function');
        local function array(t) assert(type(t)=='table' and #t<=512);local n=0;for k in pairs(t) do n=n+1;assert(n<=512 and type(k)=='number' and k>=1 and k<=#t and k==math.floor(k)) end;assert(n==#t);return t end;
        local loaded,selected,failed={},{},{};
        for i,m in ipairs(array(mm.mods)) do assert(type(m)=='table' and type(m.modname)=='string' and type(m.modinfo)=='table' and not loaded[m.modname]);loaded[m.modname]={i,m.modinfo.dst_compatible and true or false,m.modinfo.failed and true or false} end;
        for _,name in ipairs(array(mm.enabledmods)) do assert(type(name)=='string');selected[name]=true end;
        for _,m in ipairs(array(mm.failedmods)) do assert(type(m)=='table' and type(m.name)=='string');failed[m.name]=true end;
        local out={};local function bit(v)return v and '1' or '0'end;
        for i,name in ipairs(q) do local m=loaded[name];out[i]=i..' '..bit(m~=nil)..' '..bit(ki:IsModEnabledAny(name))..' '..bit(selected[name])..' '..bit(m and m[2])..' '..bit(failed[name] or (m and m[3]))..' '..(m and m[1] or 0) end;return out end);
        if not ok then print('[LGSM-DST-MODS-UNKNOWN:'..nonce..']') else print('[LGSM-DST-MODS-BEGIN:'..nonce..']');for _,row in ipairs(rows) do print('[LGSM-DST-MOD:'..nonce..'] '..row) end;print('[LGSM-DST-MODS-END:'..nonce..'] '..#rows) end end"#;
    Ok(source
        .replace("__NONCE__", nonce)
        .replace("__NAMES__", &names)
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" "))
}

fn parse_assistant_required_mod_probe(
    lines: &[String],
    nonce: &str,
    count: usize,
) -> Result<Option<Vec<AssistantRequiredModObservation>>, String> {
    if count == 0 || count > ASSISTANT_MOD_PROBE_LIMIT {
        return Err("Invalid runtime Mod response count.".into());
    }
    let begin = format!("[LGSM-DST-MODS-BEGIN:{nonce}]");
    let row_marker = format!("[LGSM-DST-MOD:{nonce}] ");
    let end_marker = format!("[LGSM-DST-MODS-END:{nonce}] ");
    let unknown = format!("[LGSM-DST-MODS-UNKNOWN:{nonce}]");
    let mut started = false;
    let mut complete = false;
    let mut rows = Vec::new();
    let malformed = || "Malformed or incomplete native Mod response.".to_owned();
    for line in lines {
        let Some(body) = assistant_mod_native_line(line) else {
            continue;
        };
        if body == unknown {
            return Err("Native Mod runtime state is unavailable.".into());
        }
        if body == begin {
            if started {
                return Err(malformed());
            }
            started = true;
        } else if let Some(fields) = body.strip_prefix(&row_marker) {
            if !started || complete || rows.len() >= count {
                return Err(malformed());
            }
            let fields = fields.split_whitespace().collect::<Vec<_>>();
            if fields.len() != 7 || fields[0].parse::<usize>().ok() != Some(rows.len() + 1) {
                return Err(malformed());
            }
            let flag = |index: usize| match fields[index] {
                "0" => Ok(false),
                "1" => Ok(true),
                _ => Err(malformed()),
            };
            let load_index = fields[6].parse::<usize>().map_err(|_| malformed())?;
            let present = flag(1)?;
            if load_index > 512 || present != (load_index != 0) {
                return Err(malformed());
            }
            rows.push(AssistantRequiredModObservation {
                present,
                enabled: flag(2)?,
                selected: flag(3)?,
                compatible: flag(4)?,
                failed: flag(5)?,
                load_index,
            });
        } else if let Some(declared_count) = body.strip_prefix(&end_marker) {
            if !started
                || complete
                || rows.len() != count
                || declared_count.parse::<usize>().ok() != Some(count)
            {
                return Err(malformed());
            }
            complete = true;
        }
    }
    Ok(complete.then_some(rows))
}

fn assess_assistant_required_mod_probe(
    rows: &[AssistantRequiredModObservation],
) -> AssistantTaskCheckStatus {
    if rows.is_empty() {
        return AssistantTaskCheckStatus::Unknown;
    }
    if rows
        .iter()
        .all(|row| row.present && row.enabled && row.selected && row.compatible && !row.failed)
    {
        AssistantTaskCheckStatus::Satisfied
    } else {
        AssistantTaskCheckStatus::Failed
    }
}

async fn await_assistant_mod_frame<M, MF>(
    nonce: &str,
    count: usize,
    deadline: tokio::time::Instant,
    mut observe: M,
) -> Result<Vec<AssistantRequiredModObservation>, String>
where
    M: FnMut() -> MF,
    MF: std::future::Future<Output = Result<(bool, Vec<String>), String>>,
{
    let mut lines = Vec::new();
    let mut bytes = 0usize;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err("Runtime Mod response deadline exceeded.".into());
        }
        let (current, observed) = tokio::time::timeout_at(deadline, observe())
            .await
            .map_err(|_| "Runtime Mod response deadline exceeded.".to_owned())??;
        if !current {
            return Err("The server run changed while collecting Mod state.".into());
        }
        for line in observed {
            bytes = bytes.saturating_add(line.len() + 1);
            if line.len() > 4096 || bytes > ASSISTANT_MOD_PROBE_BYTES {
                return Err("Runtime Mod response exceeded its capture limit.".into());
            }
            lines.push(line);
        }
        if let Some(rows) = parse_assistant_required_mod_probe(&lines, nonce, count)? {
            return Ok(rows);
        }
        tokio::time::sleep_until(
            (tokio::time::Instant::now() + Duration::from_millis(100)).min(deadline),
        )
        .await;
    }
}

fn assistant_mod_native_line(line: &str) -> Option<&str> {
    let line = line.trim();
    let Some(timestamped) = line.strip_prefix('[') else {
        return Some(line);
    };
    if line.starts_with("[LGSM-DST-") {
        return Some(line);
    }
    let (timestamp, body) = timestamped.split_once("]: ")?;
    let parts = timestamp.split(':').collect::<Vec<_>>();
    if parts.len() != 3
        || parts[0].len() < 2
        || !parts[0].bytes().all(|byte| byte.is_ascii_digit())
        || !parts[1..].iter().all(|part| {
            part.len() == 2
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u8>().is_ok_and(|value| value < 60)
        })
    {
        return None;
    }
    Some(body.trim())
}

struct AssistantModProbeLog {
    path: PathBuf,
    identity: crate::runtime_log_stream::file_identity::LogFileIdentity,
    tail: crate::runtime_log_stream::RuntimeLogTailState,
    bytes: usize,
    limit: usize,
}

impl AssistantModProbeLog {
    fn open(path: PathBuf, limit: usize) -> Result<Self, String> {
        let (identity, length) = Self::snapshot(&path)?;
        let mut tail = crate::runtime_log_stream::RuntimeLogTailState::default();
        tail.byte_offset = length;
        Ok(Self {
            path,
            identity,
            tail,
            bytes: 0,
            limit,
        })
    }

    fn snapshot(
        path: &Path,
    ) -> Result<
        (
            crate::runtime_log_stream::file_identity::LogFileIdentity,
            u64,
        ),
        String,
    > {
        let file = fs::File::open(path).map_err(|_| "Runtime Mod log unavailable.".to_owned())?;
        let metadata = file
            .metadata()
            .map_err(|_| "Runtime Mod log metadata unavailable.".to_owned())?;
        if !metadata.is_file() {
            return Err("Runtime Mod log is not a file.".into());
        }
        let identity = crate::runtime_log_stream::file_identity::log_identity(path, &file)
            .map_err(|_| "Runtime Mod log identity unavailable.".to_owned())?;
        Ok((identity, metadata.len()))
    }

    fn check(&self) -> Result<(), String> {
        let (identity, length) = Self::snapshot(&self.path)?;
        if identity != self.identity || length < self.tail.byte_offset {
            return Err("Runtime Mod log changed during collection.".into());
        }
        Ok(())
    }

    fn read(&mut self) -> Result<Vec<String>, String> {
        self.check()?;
        let remaining = self.limit.saturating_sub(self.bytes);
        if remaining == 0 {
            return Err("Runtime Mod response exceeded its capture limit.".into());
        }
        let start = self.tail.byte_offset;
        let delta = crate::runtime_log_stream::read_runtime_log_generation_delta_bounded(
            &self.path,
            &mut self.tail,
            remaining.min(16 * 1024),
            4096,
        )
        .map_err(|_| "Cannot read the runtime Mod log.".to_owned())?;
        self.bytes += delta.bytes_read;
        if self.tail.byte_offset != start.saturating_add(delta.bytes_read as u64)
            || delta.byte_offset != self.tail.byte_offset
        {
            return Err("Runtime Mod log position changed during collection.".into());
        }
        self.check()?;
        Ok(delta.lines)
    }
}

#[cfg(test)]
#[path = "task_runtime_tests.rs"]
mod task_runtime_tests;
