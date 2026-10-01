# LAN AI assistant / LAN AI 助手

[English](#english) | [简体中文](#简体中文)

## English

LAN helps diagnose and operate servers managed by LanGame Server Manager. Configure a model provider in the application settings, then describe what you want in the assistant. It interprets the request and conversation context to determine the task and target. The currently viewed server or game provides context; it does not override an explicit request to create or work on another server. No task-type or new/existing-instance selector is required. If the target is ambiguous, the assistant asks a natural-language clarification before preparing an operation.

Ordinary conversation uses native user and assistant messages. The model decides whether to reply directly or request application work through a native tool call; task routing does not use prompt keyword lists. Tool availability follows the resolved target and task phase. Workshop installation previews must contain the exact item IDs; an empty plan cannot install a built-in default Mod.

For questions about the computer, the model can call `read_host_info` without selecting a server. It reads the manager host's CPU profile, memory and OS platform/process architecture; a LAN browser therefore describes the manager machine. It does not collect GPU information, OS version, private paths or network identities. Processor profile counts are not a guarantee of total cores on multi-socket or affinity-limited systems. The tool permits one read per turn, with a 30-second wait and one blocking worker; cancelled or timed-out reads retain the worker slot until collection exits. Missing facts stay unknown. Ordinary conversation uses the existing in-memory catalog without initializing the server database, while its own conversation archive is saved, and a later server task rejects a storage-context change that occurred while the model was interpreting the request.

LAN's shared voice applies to ordinary chat, investigations and focused writing across all three provider protocols. It is the LanGame mascot: warm, direct and lightly playful, without repeated introductions, forced catchphrases or unnecessary permission lectures. Focused output such as an announcement keeps the requested format. Personality does not replace evidence, confirmation or an honest explanation of missing information.

### Game documentation

`search_game_docs` and `read_game_doc` retrieve synchronized upstream document bodies for the selected game through local multilingual vectors and exact-term retrieval. Source policies cover all 32 supported games; results include source URLs, authority, body hashes, retrieval time and update state. Configuration defaults still come from schema tools, and current server facts still require instance reads.

The runtime checks upstream sources every 24 hours by default, with manual refresh and cancellation in LAN settings. Failed updates preserve the last usable source snapshot and expose the failure. Publisher, officially endorsed community and independent community sources remain distinct. Missing or restricted documents remain explicit gaps; source coverage does not guarantee every publisher has a complete public manual. See [game knowledge](game-knowledge.md#english) for synchronization, model download, citations and limits.

### Sessions, pauses and stopping

The backend creates the conversation ID before a model request starts. It owns original user requests, native assistant replies, paired tool calls/results and completed-operation receipts. The frontend keeps an in-memory display cache and sends only the backend ID, current request, provider settings and minimal selection/language context. Displayed messages cannot be uploaded as evidence or authorization. A session is bound to the provider, model, endpoint and storage identity; a request with changed model settings starts a new conversation, while a changed storage identity invalidates reuse of the old session.

Conversations and task checkpoints are saved beside the application database in `assistant-sessions/`, with atomic replacement, bounded reads and a limit of 16 conversations retained for 30 days. The archive contains redacted public messages, paired tool results, original request sources, task constraints, verified file receipts and cumulative budgets. API keys, confirmation tokens, prepared mutations and opaque provider reasoning/signature blocks are not saved. A changed provider or storage identity cannot reuse a saved conversation. Corrupt archives produce a recovery error; they do not silently authorize a new task. Deleting a conversation removes its archive. Each session retains up to 2 MiB and 1,024 combined history/request records, with at most 128 original requests; reaching capacity refuses further recording.

After restart, unfinished work becomes a paused investigation. The assistant closes incomplete tool exchanges as unknown, reloads current evidence and prepares a new preview instead of replaying a pending mutation. Previous operation fingerprints and cumulative budgets remain in force. If redaction removed values needed to preserve the task's exact constraints or baseline, the interface requires a new request instead of pretending to restore those values.

Archive I/O uses one worker and a shared 30-second deadline for queueing and execution. A timed-out write has an unknown outcome and may still finish; the worker retains its slot until the filesystem operation exits, preventing retries from accumulating background workers.

The active model window is bounded to 24 KiB of business evidence and 64 messages, with complete native envelopes separately capped at 256 KiB. The current task context is pinned explicitly so internal feedback cannot displace its constraints. Older complete tool exchanges remain in the backend archive. `read_session_history` returns redacted business evidence with both a record offset and a UTF-8 byte offset, so large records remain readable through their final page. Native thinking and signature envelopes remain intact for provider replay but are excluded from retrieved evidence. Original user requests have stable session-wide `prior-N` IDs and their own paged source; the most recent six excerpts do not replace the complete retained originals. A task may select up to six relevant original requests under the existing combined byte limit. Archived observations do not establish current runtime state or authorize replaying an old operation. Model transport and task-context limits still apply.

A resolved task owns one budget across investigation, confirmation, follow-up and resume. Counts include attempted work; failed or uncertain writes do not become safe to repeat by continuing the task. The backend rejects the same normalized operation against the same precondition again, while allowing a changed plan or new evidence to be assessed.

| Budget | Allowance before a resumable pause | Total task limit |
| --- | --- | --- |
| Model requests | 24 | 128 |
| Evidence reads | 24 | 128 |
| Confirmed operation attempts | 8 | 32 |
| Active work time | Shared across all slices | 20 minutes |

Active-work accounting covers model requests, evidence collection and confirmed operations; time spent waiting for the operator's confirmation does not consume it. A slice pause returns a reason, usage counts and a summary. **Continue** is offered only when the backend reports `canResume=true`. It resumes the saved task, original requirements, evidence and operation preconditions; it does not reinterpret a new prompt or reset cumulative usage. After a pause or restart, the next change requires a fresh confirmation. Hard limits, malformed responses, context overflow and invalid evidence are not automatically resumable. The frontend also has a 32-confirmation fail-safe, which cancels pending work and reports the limit instead of success.

If a resumed investigation or the follow-up investigation after a confirmed operation fails, its task, evidence, requirements and consumed allowance remain available for an explicit retry. Retrying does not grant a fresh slice or replay a confirmed operation. After an uncertain transport failure, **Check task status** only queries the backend; a confirmed paused state offers a separate **Continue** action. A running or unreachable state cannot trigger an automatic retry. An idle state does not establish whether a lost response reported success.

Confirmation and continuation hold a storage lease from session binding through execution and verification, so a storage switch cannot redirect an accepted task. Expired, cancelled, evicted and superseded sessions release their saved checkpoints; stale checkpoints cannot consume the capacity for current tasks.

**Stop request** signals cancellation and invalidates pending confirmations and the saved continuation. The interface remains busy until both the current request and any outstanding cancellation request settle. Cancellation prevents later work but cannot roll back a write that has already committed; completed receipts and backups remain relevant. Blocking readers retain their worker slot and storage lease until their actual read exits. Deleting a conversation invalidates its backend state so late responses cannot recreate it. A fresh user request replaces any pending preview/checkpoint; resuming a paused task requires its dedicated **Continue** action.

### Continuous work, live progress and generic file checks

The confirmation dialog has an optional **Allow continuous repair for this task** choice, off by default. It covers the bound existing instance's settings, editable text files and ports, plus a start required by a restore/launch goal. It does not authorize installation, downloads, file validation that can download, broadcasts or admin commands. The grant lasts only for the current confirmation chain, with the same conversation revision, task and target, for at most 20 minutes and the existing operation budget. Stop, a new request, pause or application restart ends it. Every operation still receives local validation, current-state checks and readback. Out-of-scope proposals return for review; completed steps retain separate receipts.

OpenAI-compatible and Anthropic providers stream SSE; Ollama streams NDJSON. Only public response text is displayed. Repeated protocol envelopes have a separate 16 MiB transport limit and 16,384-event limit; each pending frame and the combined retained text, reasoning and tool arguments remain bounded to 256 KiB, with a final serialized-response check. Tool calls execute only after the full stream, finish status, IDs and JSON arguments pass validation. Truncated or oversized streams fail explicitly. Progress records actual model and tool boundaries in a bounded cursor stream; the interface discards stale revisions and can reconstruct current text after a cursor gap. Hidden reasoning is not displayed. Providers explicitly returning a complete JSON response remain supported without retrying the request.

`validate_instance_file` checks JSON and TOML syntax without running file contents. Its result identifies the source hash and error locations; unsupported formats and application semantics stay unverified. `patch_instance_files` accepts up to eight existing editable files with up to 16 nonoverlapping edits per file, bound to their original hashes. All files are checked and backed up before writing; a failed write triggers reverse rollback only where the current content still matches this operation. Receipts distinguish applied, not applied, rolled back and partial recovery. This is recoverable file replacement, not an operating-system transaction across files. Backups are retained even if rollback fails. Generic workspace tools remain available without an installed-Mod adapter; DST metadata guidance is exposed only in its applicable scope.

See [assistant evaluation](assistant-evaluation.md) for reproducible fault scenarios and the distinction between deterministic execution tests and actual model trials.

### Diagnosis and changes

Explicit stop, restart, create-backup and restore-backup requests each require their own preview confirmation. Stop and restart bind the current managed run; a restart reports the stop receipt even if starting the new run fails, and completion requires the new run's readiness checks. Save backup creation and restoration currently require a stopped instance. Creating a backup follows the instance's existing retention policy. Restoration binds an exact backup ID, source content and current saves, rejects changes after preview, creates a safeguard backup and leaves the server stopped. These four operations are outside continuous repair grants and automatic repair follow-ups. A request to restore a save backup is different from repairing a service so it runs.

These four previews and their result summaries use English or Simplified Chinese according to the interface-language preference, falling back to the request language when that preference is unavailable. The backend retains the chosen language for that confirmation. The preview identifies the actual instance by display name; a restore also shows the selected backup ID, creation time in UTC, file count and size. Applicable automatic-backup and retention effects, preservation rules and extracted request constraints remain visible, including the original request excerpts. Content hashes remain in backend snapshots and evidence rather than the user-facing summary. The exact displayed summary still binds the single-use confirmation. Diagnostic causes are preserved even when their original wording is in another language; localization does not change authorization or completion checks.

| Request | Available behavior | Completion boundary |
| --- | --- | --- |
| Investigate a failed start or console error | Inspect the selected instance's runtime evidence and configuration; explain the evidence and any missing information. | A likely cause is a diagnosis, not proof of a repair. |
| Change a modeled setting or resolve a port conflict | Propose a patch using existing settings or declared port names, then save and read back the result after confirmation. | A verified saved value does not establish that a running game has applied it. |
| Install or validate server files | Use separate install and file-validation operations for the selected game. | The result must identify the game, installed state and available executable; files being ready does not establish a working world. |
| Create and launch a server | Describe the game and desired setup; confirm file preparation, instance creation, configuration and startup as required. | Creating an instance leaves it stopped. Only runtime verification can complete the launch task. |
| Start a server | Invoke the managed start workflow for the selected instance. | Process startup does not establish that a player can join. |
| Install mods or send an administrator command | Use the game's declared mod workflow or an allowed game command. | Supported sources, commands, and response guarantees depend on the game module. |

The assistant can make successive read-only tool requests for runtime health and logs, setting names, values and schemas, configuration files, configured mod order, Workshop installation evidence, launch validation and instance-owned text files. The backend binds these tools to the task's actual instance and game; the model chooses the evidence it needs. Setting directories are paged and distinguish saved keys from schema-only declarations. Mutation planning also receives a bounded configuration snapshot. All investigation reads, including automatic Mod follow-ups and archived-history reads, count toward the task's cumulative read budget. At most two investigations run in the application at once. The selected model determines whether the evidence supports a proposed repair.

Configuration, registered-log and installed-Mod metadata workers share two slots and a 30-second wait limit. A timed-out or cancelled waiter does not release the blocking worker slot or its storage lease until the actual file read exits, so repeated requests cannot accumulate these workers or switch storage roots beneath them.

The model can use `search_settings` to find settings by key, source filename, or schema description, with up to ten matches per page. All query terms must match the metadata; a short filename or distinctive word works better than a list of synonyms. Zero matches prompt a narrower search and do not establish that a setting is unavailable. Each search counts toward the same task budget. Model requests include read-budget guidance so the model can reuse collected evidence.

For DST, `inspect_installed_mods` reads installed local and Workshop mod metadata, including declared versions, API versions, dependencies and load priorities. It accepts exact directory names or a paged directory listing, uses the selected instance's runtime and UGC roots, and reports each installed copy separately. It does not establish which copy the game loaded. Reads use the bounded `modinfo.lua` sandbox; the tool does not execute `modmain.lua`. Missing files, rejected paths and incomplete evidence remain explicit.

When runtime logs identify a native `MOD ERROR`, one metadata read for up to five named mods is automatically added before the next model request if budget remains. Current mod settings are also read if not already supplied and another read fits. A successful metadata read can add one lookup of up to five direct dependency directory candidates not already supplied or queued. This does not recursively resolve a dependency graph or infer compatibility. Display names and ambiguous Workshop values are not guessed. All reads count toward the task budget; the log marker is an investigation lead, not proof of the cause.

For DST, mod-state evidence also projects the exact configuration that the native writer would generate for each shard. Static declarations distinguish enabled, disabled and unspecified entries. Dynamic Lua, ambiguous values or analysis limits produce `unknown` with null lists, never an empty successful list. Installed-mod evidence pairs each directory with these declaration states; Workshop aliases remain unknown when an exact binding cannot be established. No Lua is executed for this projection, and inactive shards are identified separately.

DST loads higher-priority mods first; rearranging keys in `modoverrides.lua` does not change that priority. The assistant can propose enabling an installed dependency through the existing settings flow. It can also propose a bounded change to an instance-owned Mod text file, including `modinfo.lua`, when the server is stopped. Declared metadata and a saved source change alone do not prove compatibility.

For Project Zomboid, preserving existing Mods permits reordering the `mods` setting while retaining exactly the same native Mod IDs. Validation shares the native writer's delimiter, comment and duplicate handling; additions, removals, renames and changes to other protected Mod settings remain rejected. Confirmation saves through the native configuration writer and reads back persisted settings. The installed-metadata tool currently supports DST only, so a Project Zomboid order needs evidence from available files/logs or an explicit operator-supplied order; the assistant must not invent a dependency graph. This check does not establish which Mods loaded or whether the game is compatible, and recovery stays unverified when runtime Mod evidence is unavailable.

`read_runtime` collects separate logs for up to eight registered processes in the current managed session, or the most recent recorded session after exit. It shares a maximum 400-line scan budget across those processes and bounds the returned evidence, retaining error excerpts before ordinary output. Session, process identity, status or path changes during collection invalidate the snapshot. Missing, truncated, historical and unbound logs are labeled explicitly; this is not a complete error history.

`list_instance_files`, `read_instance_file` and `search_instance_files` work across supported games within the instance directory recorded by storage. The model cannot supply a filesystem root. Supported UTF-8 text includes configuration, logs and extension source; hidden manager paths, credential-named paths, backups, traversal, symbolic links and Windows reparse points are excluded. Listings scan at most 2,048 entries and 12 levels, paging both files and directories with up to 64 entries per list and a shared byte bound. Reads accept files up to 256 KiB and redact them before returning 4 KiB pages. Literal, case-sensitive search scans at most 64 files and 1 MiB per call and returns up to 24 matches. Its cursor continues across files and within large result sets; even an empty page can have a continuation. Cursors bind the query and directory listing, and a continued file binds its source hash. Changed evidence requires restarting the search. Snippets include the actual match. Hard scan limits and unreadable files remain evidence gaps; zero matches do not prove absence.

Read access does not grant edit permission. Each file reports `editable` and `protectionReason`. Text changes are restricted to existing private Mod, plugin or script files under the permitted instance `data` and `runtime` layouts, including DST shard Workshop content. Runtime edits require the manager's private-runtime marker. Shared or unverified runtime files, generated settings, saves, worlds, logs, credentials and binaries cannot be patched. Managed settings continue through their native settings writers. This common ownership boundary does not provide game-specific semantic validation for every plugin language.

`inspect_network_endpoints` observes local TCP/UDP endpoints belonging to the verified managed instance process tree. It rechecks process creation identities, the active run and declared ports around collection, with at most 16 registered process targets and 64 returned endpoints. Unknown ownership or a changed run is an error, not an empty successful observation. This tool does not change ports or firewall rules, and observed listeners do not prove remote reachability or multiplayer readiness.

`patch_instance_text` replaces one unique exact text segment, with at most 8 KiB of combined original and replacement text. The confirmation dialog shows the relative path and both segments, masking recognized secrets. The backend retains the original bytes and SHA-256 from the preview and requires the instance to remain stopped with no active process. Confirmation checks the instance and file again, saves the original and a hash manifest in `data/.langame/file-patches/<backupId>`, replaces the file atomically and independently reads it back. A changed file is rejected; a write or readback failure retains the backup and reports its identifier. Up to 64 backups are retained, after which further changes are refused rather than deleting recovery material. These checks coordinate manager operations and detect ordinary external edits; they are not isolation from another malicious process running as the same Windows user.

An Apply request can finish after the exact file change is verified; its receipt explicitly excludes startup and Mod behavior. Restore service carries the expected file hash through subsequent confirmations and startup assessment, so an update overwriting the patch cannot silently count as recovery. Starting requires confirmation or the explicitly selected same-task continuation permission. Running servers must be stopped through the normal server controls before a file patch can be proposed. The assistant does not execute edited Lua merely to validate its syntax, and does not automatically undo changes after an unsuccessful launch. The backup contains the complete original file for recovery.

The `settingCandidates` returned by `read_config_file` are metadata matches. Before proposing a change, the assistant must confirm the target file and shard and inspect the current setting. For settings recognized as sensitive, both current values and schema `default`, `examples`, `enum`, and `const` values are redacted.

Investigation uses provider-native tool messages: each assistant call and its result retain the same identity and order, including rejected proposals and their validation errors. OpenAI-compatible services use function calls and tool results, Ollama uses native chat tools, and Anthropic uses tool-use and tool-result blocks. Message text is never interpreted as a tool call. Only tools for the current target and task phase are exposed. A final proposal must be the only call in its turn.

The retained business messages, call arguments, tool results and tool schemas are counted once and limited to 64 KiB. The original native reply, including protocol-only thinking or signed blocks, is preserved intact under the separate 256 KiB provider request/response limits; it is not counted again as business evidence. This byte budget bounds transport and retained evidence independently of the model's token limit. Oversized results and failed reads become explicit evidence gaps; earlier instructions and evidence are preserved. If the next gap cannot fit, the investigation stops without proposing an operation. Requests too long to preserve the user's requirements and the operation contract are rejected explicitly. Invalid model replies never execute an operation and allow at most two format or task-contract corrections within the same investigation budget. Production investigations share the cumulative task budget described above. Requirement drafting still permits at most eight draft calls within an investigation; protocol validation and context-size limits remain independent of resumable budget slices. Context overflow, malformed protocol identities and truncated provider responses fail explicitly without an operation.

Read-only diagnosis does not require an operation confirmation. An operation that changes server state first returns a preview, including the complete proposed settings patch with recognized secrets masked. Review its target and values before confirming. A preview expires after two minutes, is single-use, and is invalid if the configured provider, model, or endpoint changes. Changes to the target's saved settings, ports or recorded runtime also invalidate the preview. Configuration and port writes recheck the instance state while holding the mutation lock, then independently read the saved result back. A failed readback does not mean that no write occurred; inspect the current settings before requesting another operation.

Configuration tools read bounded text files inside the selected instance's configuration directory. They reject traversal, symbolic links and Windows reparse points. Supported formats include Lua and XML; UTF-8 and UTF-16 with a byte order mark are decoded strictly, and malformed encoding or NUL characters are rejected. File content is redacted before pagination so a page boundary cannot bypass recognized credential masking. Files whose names identify credentials, such as `cluster_token.txt`, are masked even when their contents contain only the token. A failed initial configuration read is reported to the model and does not prevent it from investigating runtime evidence. This is read access; changes continue through the manager's modeled settings and native configuration writers.

Start and game-console operations also check their preview state after acquiring the instance mutation lock. A confirmed command cannot silently switch to a replacement managed server run while waiting for that lock.

### Verification and follow-up confirmations

Ordinary conversation, capability questions and general explanations receive natural text replies without a task or confirmation. Host information is read through `read_host_info`; requests needing server evidence or an operation use the native `resolve_task` tool. A resolved task must bind a catalog target. `target=none` is reserved for a nonempty clarification question; a targetless work request receives a tool error so the model can choose host inspection, direct conversation or a valid target within the existing correction budget. The core validates its goal and target against a bounded catalog of actual games and instances, then captures an internal task contract. Read-only investigations may finish with a natural-language explanation; text, including JSON examples, can never authorize an operation. Making a server operational includes runtime verification. Preparing a configured server without starting it is verified against its saved setup; bare instance creation can finish without a configuration workflow. Invalid or ambiguous interpretation never silently falls back to an operation mode. Unsupported tool calls, rejected batches and invalid arguments receive matching native error results and may receive one correction turn within the same 90-second deadline and two-request concurrency limit. Missing nullable fields mean no value; required authorization and prior-request references are never invented. This conversation entry cannot execute operations. The backend supplies recorded native messages and paired tool evidence from its session, rather than accepting history from the display cache. Only original user requests registered at the command boundary can authorize a task; tool results, application control messages and assistant replies cannot replace them. Protocols that expose tool choice explicitly use auto; no keyword-based chat/task switch is used. Diagnostic logs record reply shape and outcome without recording conversation text or tool arguments.

A new-server request binds to its identified game, then to the actual instance created by the confirmed operation. It does not reuse a server selected on another page. Creation, configuration and starting are separate operations; the same task identifier and target are retained after a failed first start. Preparation, launch and repair share one cumulative task budget across follow-ups and resumes; completing a fixed number of steps never establishes success.

New instance defaults may be changed across successive confirmed configuration steps. The first confirmed start establishes the final saved mod/shard baseline for subsequent repairs. Even using defaults requires a configuration confirmation before the first start in this task. Launch and restore tasks reject settings patches containing unknown keys before confirmation; the assistant must query the actual keys and correct the complete patch. Setting reads distinguish absent keys from existing keys with null values. Installation success continues to a fresh creation preview, while an installation-only request can finish once its file checks pass. File validation uses the game's validation workflow rather than an ordinary install command. Missing executables, incomplete installation, unreadable creation results and unknown runtime evidence cannot complete a launch task.

When a reply answers a clarification or explicitly continues a previous request, interpretation can reference up to six earlier user messages by application-issued source IDs. The core assembles the selected original text and current reply without rewriting or truncating their requirements. Assistant replies and UI context cannot become request sources. Unknown, repeated or out-of-order source IDs are rejected. Oversized earlier messages are marked unavailable without blocking unrelated new requests; unavailable sources cannot authorize an operation. Oversized current requests fail before an operation is prepared. A request to create and configure without starting continues through saved-settings verification and cannot propose startup.

Existing Mods and their configuration are preserved by default. Only an explicit request to change those features can be interpreted as allowing such changes; the exact proposal still requires confirmation. A general repair request does not authorize dropping Mods to make a server start. The backend records the resolved target, goal, preservation baseline and task identifier before confirmation. Follow-up plans inherit that contract without reinterpreting its goal; model replies, conversation evidence and client-supplied policy fields cannot replace it.

Before presenting or executing a configuration change, the backend checks the task's preservation rules. DST checks the actual rendered configuration: initially active shards, enabled mods and existing mod options must be retained. Adding or enabling dependencies is allowed. Dynamic or ambiguous Lua is not executed to establish permission; a protected configuration that cannot be analyzed must remain unchanged. For other games without a semantic validator, protected mod settings must remain unchanged. An invalid proposal receives bounded validation feedback within the existing investigation budget and cannot write configuration.

Runtime command receipts distinguish acceptance from confirmed transport delivery. If a managed stdin write is still pending, the task remains inconclusive and does not resubmit the command. Confirmed delivery does not verify an arbitrary in-game effect.

After a confirmed settings, port, or start operation, the assistant checks the saved result and available runtime evidence. A Restore service task can propose the next operation for the same instance and game within its cumulative budget. The explicit task goal, rather than words in the prompt, determines whether repair continues. Every proposed change or start requires confirmation or the still-valid same-task continuation grant. Without this grant, saving a fix does not automatically start the server, and an already running server is not silently stopped or restarted. If investigation fails or reaches its limit, the completed changes remain in place and the remaining uncertainty is reported.

Operation results have a task receipt with the same identifier and individual checks: `satisfied`, `failed` or `unknown`. The task is `completed` only when all applicable checks are satisfied. A saved configuration can complete a specific change request; it cannot complete a request to restore service. Cancellation, exhausted steps and unknown results do not become success. Confirmation remains bound to the captured task if the operator changes the visible page or drafts another request. A clarification has no operation or confirmation token.

For DST, production verification reads the required mods' native loaded, enabled, selected, compatibility and failure state through a fixed read-only console probe. It uses a fresh nonce, current managed run and process identities, log offsets/generation, a 15-second total budget, at most two shards and 64 names, and bounded output. It does not run model-provided Lua. Configuration or runtime changes during verification invalidate the completion evidence. This proves the checked native loading state, not arbitrary mod behavior, player connectivity or long-term stability. Missing game-specific evidence remains `unknown`.

When a repair was saved and read back successfully, launch preflight passed, and the instance remains stopped with unchanged settings, the application prepares the next start confirmation without another model request. Historical failure logs do not trigger another configuration rewrite at this stage. A failed start returns to evidence-based investigation.

| Verification state | Meaning |
| --- | --- |
| `verified` | Applicable checks passed. For a file-only Apply request, this verifies the saved text. For startup or Restore service, a new run must also have matching process identities and startup readiness across consecutive observations. |
| `failed` | The operation or launch checks failed, or evidence shows that the new run failed. |
| `inconclusive` | Recovery is not established: for example, settings were saved but the server has not started, an existing run has not applied the change, or evidence is unavailable or changed during verification. |

Runtime verification takes at most three observations within a 15-second observation budget and requires two consecutive ready observations. The game's startup workflow has its own timeout. A verified startup does not prove long-term stability, player connectivity, or compatibility of every mod. The result includes its evidence and, when available, the run identifier. A failed verification does not roll back a saved change.

For a crash or mod problem, describe the symptom and when it occurs: before the process opens, while loading the world, after enabling a mod, or when a player joins. Ask the assistant to identify the relevant log evidence before proposing a change. After a change, inspect fresh startup output and test the affected behavior. Keep a world backup before testing changes that can alter saved content.

### Capability limits

LAN uses the manager's available operations and its bounded instance workspace. It cannot run arbitrary Windows CMD or PowerShell commands, patch game executables, edit arbitrary host projects, or install a new tool to expand its own authority. It can propose changes to permitted private extension text, including source files, but it cannot compile or execute that text as an unrestricted validation step. Game-console commands and the operating system shell remain different interfaces.

LAN supports native tool calling, recorded evidence and resumable tasks. Understanding a request, choosing useful tools and diagnosing an unfamiliar game still depend on the model. Unsupported tools, missing evidence, context overflow, provider failures and exhausted task budgets produce explicit limits rather than an assurance that the goal was achieved.

A game log may identify a missing mod or failed dependency without proving a valid load order. The manager's mod list alone cannot establish compatibility between every mod and game version. Unsupported edits and missing evidence must be reported explicitly. A saved setting, a successful command write, a running process, and a working multiplayer session are separate results.

Stopping follows the instance's existing automatic-backup policy. A confirmed prohibition on creating backups also blocks stop/restart when that policy would create one, and blocks restore because its safeguard is mandatory; LAN does not silently change the policy to get around the restriction.

### Model providers and data

See [Privacy and data](../PRIVACY.md#english) for the complete data notice. The privacy button in the LAN header opens the full offline notice inside LAN, with the same back-to-conversation navigation as AI settings. The AI settings page contains the data-use and recipient disclosures. Before sending or continuing a task, check the recipient in AI settings: protocol names such as OpenAI Compatible and Ollama do not identify the actual operator or prove local processing. Keys are stored on the manager host and sent to the configured service for authentication.

**Check connection** in AI settings tests the current service and model through the same native streaming transport used by LAN. It reports chat, tool calling and tool-result replay separately, so a model that chats successfully can still fail the operation checks. The check sends at most three small synthetic requests within a shared 90-second deadline; provider charges may apply. It reads no server, documentation or conversation data, creates no assistant session and executes no application operation. Stored credentials are resolved for the selected protocol and service. It runs only after an explicit click in the desktop host or connected management interface, with no automatic retry.

Changing the configuration clears its result and cancels an active check. Cancellation remains busy until both requests settle. Passing proves the selected model's tested chat/tool exchange; game operation correctness and documentation coverage require their own evidence.

OpenAI-compatible public refusal responses remain visible as assistant text and cannot also dispatch tool calls. Focused text generation rejects truncated or abnormal completion statuses instead of returning incomplete content as success. Ollama service roots and supported endpoint-style addresses are normalized consistently for conversation, auxiliary text generation and model listing.

Ollama conversation and investigation requests use the native `/api/chat` endpoint with a per-request 32K-token context and an 8,192-token total output limit, including thinking and final content. They use native tool calling without JSON response formatting, retain the model's default thinking behavior, and prohibit silent input truncation or context shifting; exceeding the actual token context returns an explicit error. Only native tool calls are dispatched; thinking content is retained as protocol context but is neither displayed nor executed. Conversation entry has a 90-second deadline; task work uses the cumulative active-work budget and bounded read workers described above. These are per-request settings and do not modify the Ollama service configuration.

The selected provider receives your request and the diagnostic context included with it. Depending on the request, this can include instance and module identifiers, settings, configuration and extension-source excerpts, runtime logs, and observed local network endpoints. A provider on another machine receives this data over the configured connection; local server storage does not imply that AI requests stay local.

Model requests follow redirects only within the configured origin (scheme, host and effective port). Changing any of these requires updating the provider address explicitly; diagnostics and provider credentials are not forwarded to a different service, including another loopback port.

The application redacts recognized credential fields and local absolute paths before sending diagnostic text. This filtering does not guarantee removal of every secret or personal identifier in free-form logs. Do not paste credentials or private player data into requests. Choose a local provider when the diagnostic content must remain on your computer, and check that its configured endpoint is local.

### Implementation and verification

Connection checks are implemented in [`assistant_connection.rs`](../apps/desktop/src-tauri/src/assistant_connection.rs) and the [AI settings control](../apps/desktop/src/components/AppAiConnectionCheck.tsx). Native tests cover all three streams, result replay, cancellation before registration, timeouts and connection cleanup. Browser fixtures cover configuration saves, cancellation and stale results using synthetic IPC responses. The ignored `saved_deepseek_connection_probe` requires `LANGAME_ASSISTANT_LIVE=1` and the saved DeepSeek credential; it uses the production transport without accessing application data.

Session ownership and archives are implemented in [`assistant_sessions.rs`](../apps/desktop/src-tauri/src/assistant_sessions.rs), with [command binding and cancellation](../apps/desktop/src-tauri/src/commands_assistant_ops/session.rs), [cumulative budgets](../apps/desktop/src-tauri/src/commands_assistant_ops/run_budget.rs) and [saved continuations](../apps/desktop/src-tauri/src/commands_assistant_ops/continuation.rs). The [instance workspace](../crates/app-storage/src/instance_workspace.rs) defines common file ownership and protection rules; [workspace tools](../apps/desktop/src-tauri/src/commands_assistant_ops/workspace_files.rs) and [network observations](../apps/desktop/src-tauri/src/commands_assistant_ops/workspace_diagnostics.rs) expose bounded evidence. Frontend tests exercise create-before-request, stop during creation or confirmation, waiting for an outstanding response, preserved receipts, checkpoint resume and suppression of client-provided history. Browser tests exercise the composer and confirmation controls; they do not establish native model or game behavior.

The relevant implementation is in [`commands_assistant_ops.rs`](../apps/desktop/src-tauri/src/commands_assistant_ops.rs), its [investigation tools](../apps/desktop/src-tauri/src/commands_assistant_ops/investigation.rs), [configuration reader](../apps/desktop/src-tauri/src/commands_assistant_ops/config_documents.rs), [confirmation and readback checks](../apps/desktop/src-tauri/src/commands_assistant_ops/preconditions.rs), and [`assistant.rs`](../apps/desktop/src-tauri/src/assistant.rs). Tests cover planning, target selection, read budgets, credential masking, allowed operations, confirmation, and preservation of diagnosis text in the frontend. Mock provider tests establish deterministic application behavior; they do not establish diagnosis quality for every model or compatibility for a real game/mod combination.

The [repair chain](../apps/desktop/src-tauri/src/commands_assistant_ops/repair.rs) and [runtime verifier](../apps/desktop/src-tauri/src/commands_assistant_ops/verification.rs) keep confirmation separate from observed recovery. The [native acceptance tests](../apps/desktop/src-tauri/src/commands_assistant_live_acceptance_tests.rs) are disabled by default and require an explicit opt-in, a local Ollama model, and an isolated copy of the installed DST package. They exercise a native configuration failure and a Lua mod dependency failure, then require real engine probes for the loaded world and mods. They retain logs and step evidence outside the repository. These scenarios do not establish arbitrary mod load-order repair, other games' compatibility, or the performance of other models; only a completed run provides evidence for its specific package, model, and scenario.

The [new-server native scenario](../apps/desktop/src-tauri/src/commands_assistant_live_launch_tests.rs) starts with an empty isolated database and an already installed package. It exercises confirmed creation, model configuration and native startup under one launch task, then probes the loaded world and stops the owned process. Before starting, it checks that the instance record, saved settings and actual launch arguments all bind to `127.0.0.1`, and that the executable resolves to the authorized library copy or this instance's private runtime. It verifies the executable and scripts bundle against the certified source with streaming SHA256 before starting. The production startup creates at most seven instance inbound firewall rules restricted to `127.0.0.1`. The test first checks that their exact names are absent, then removes only matching owned rules after stopping, including failed starts. Native endpoint evidence separately records every owned listener; the module strict-bind check covers the Master game port, so saved binding alone does not prove that every Steam socket is loopback. It never confirms installation, file validation or downloads, so it does not certify an external download or a SteamCMD checksum-validation run. The opt-in `assistant_live_saved_deepseek_launches_new_native_dst_server` uses the saved DeepSeek credential through production transport and caps its trial at six separately confirmed operations; it requires the same elevation and isolated package, and does not treat scripted-provider evidence as model acceptance.

Existing package copies without trustworthy clean/initial manifests require a separate official certification step before the model scenario. The opt-in [`native_catalog_preparation` fixture](../crates/app-storage/tests/native_catalog_preparation.rs) creates a fresh isolated database and settings receipt. Run `install_catalog certify dontstarve` with that isolated `LOCALAPPDATA`, SteamCMD and owned game copy; `--games-root` alone does not isolate the desktop database. Certification may update the disposable copy and must complete native Steam validation plus exact depot inventory checks before publishing manifests.

The [deterministic native contract scenario](../apps/desktop/src-tauri/src/commands_assistant_live_launch_contract_tests.rs), `assistant_live_contract_launches_new_native_dst_server`, reuses the same creation, configuration, startup, world probe and cleanup checks with requirement record/finish exchanges followed by three scripted operation responses from a local HTTP provider. It requires the native opt-in, elevation and owned game files, but no model. Its evidence is labeled `scripted-local-contract`; a pass proves that the supplied plan executes correctly, not that a model can derive that plan from a request.

Service-task plans and the four stop/restart/backup operations carry a request requirement checklist. Before a service-task operation proposal, the model uses `record_task_requirements` to record setting values, declared port values, excluded explicit actions and requirements it cannot verify, then calls `finish_task_requirements`. For an explicit change task, the first proposal of one of the four lifecycle/backup actions activates this draft process; an unfinished checklist cannot reach an executable preview. Other ordinary change actions keep their existing flow. The application assigns stable source IDs to exact original request fragments and copies their wording itself. Valid items survive a partially invalid record; the tool returns an error receipt with the accepted entries and indexed errors to correct. Finishing freezes the draft for the proposal; it does not execute an operation. A sensitive fragment hidden by redaction cannot be silently omitted or automatically interpreted. The preview shows the description, source excerpt, target and expected value with recognized credentials masked. The operator reviews the extracted list as part of the existing operation confirmation; extraction is not proof that the model understood every part of the natural-language request. The application retains the typed values and fixed checklist across creation, configuration and recovery. A continuation cannot remove or replace them.

Before an instance exists, `list_module_settings`, `read_module_settings` and `search_module_settings` expose bounded metadata for the selected game. Results distinguish schema declarations and the manager-owned listener address from actual saved instance values. A mixed known/unknown key read returns valid declarations and an explicit list of missing keys, instead of discarding all results. These tools share the existing read, byte and time limits. After creation, ordinary instance reads supply the real settings.

Partial configuration steps may leave requirements unmet, but patches cannot contradict confirmed values and startup is blocked until every requirement is satisfied. Saved settings and declared ports are read back after operations and checked again before task completion. Dynamic DST world Lua cannot turn projected default values into proof of the requested configuration. These checks concern saved canonical values and statically inspectable configuration; runtime readiness and applicable native mod checks remain separate. Excluded actions constrain the dispatcher, not hidden game behavior or indirect network activity. An unverifiable requirement blocks mutations and remains unknown; the user must clarify the request through a new preview rather than allowing the model to weaken the checklist.

For the server configuration lifecycle, see [Server configuration](server-configuration.md#english). For supported game operations and their evidence, see [Game integrations](README.md#game-integrations).

## 简体中文

LAN 用于诊断和操作 LanGame Server Manager 管理的服务器。在应用设置中配置模型服务后，直接向助手描述需求。助手结合请求与对话上下文自动确定目标及完成条件；当前查看的服务器或游戏只是上下文，不会覆盖明确的新建或其他服务器请求。无需手动选择任务类型或新建、已有实例。目标不明确时，助手会先用自然语言澄清，再准备操作。

普通聊天使用原生用户与助手消息。模型自行决定直接回复还是通过原生工具调用请求应用工作；任务分流不依赖提示词关键词列表，工具范围由已确定的目标和任务阶段决定。Workshop 安装预览必须列出确切项目 ID，空方案不会安装内置默认 Mod。

询问本机信息时，模型可自行调用 `read_host_info`，无需选中服务器。它读取管理端电脑的 CPU 信息、内存、操作系统平台和进程架构；从 LAN 网页访问时，信息属于运行管理端的机器。工具不采集显卡、系统版本、私人路径或网络身份，CPU 监控数值也不保证是多插槽或受亲和性限制机器的整机总核数。每轮最多读取一次，等待上限 30 秒，后台最多一个读取工作线程；取消或超时后，实际采集结束才释放名额。未取得的信息保持未知。普通聊天使用内存中的目录，不初始化服务器数据库，但会保存自身会话存档；若模型理解请求期间切换了存储路径，后续服务器任务会拒绝沿用原来的上下文。

三种模型协议下，普通聊天、任务调查和定向文案共用 LAN 的表达风格：作为 LanGame 吉祥物，亲切、直接，适度轻松，不反复自我介绍、不强行卖萌，也不添加无关的权限说教。公告等定向输出仍遵守用户要求的格式。亲和力不会替代事实证据、操作确认和对信息缺口的如实说明。

### 游戏资料

`search_game_docs` 和 `read_game_doc` 用本地多语向量与精确词项检索当前游戏已同步的上游正文。来源策略覆盖全部 32 个已支持游戏，保留来源链接、类别、正文哈希、抓取时间和更新状态，LAN 可据此给出带引用的开服说明。配置默认值仍从配置模型读取，当前服务器事实仍需要实例工具取得。

后台默认每 24 小时检查上游来源，LAN 设置可立即更新、取消或修改周期。更新失败保留上一份可用快照并显示错误。发行商、官方推荐社区和独立社区来源分别标注；尚未同步或限制访问的正文明确显示缺口，来源目录覆盖不等于每个发行商都有完整公开手册。正文同步、模型下载、引用和限制见[游戏知识库说明](game-knowledge.md#简体中文)。

### 会话、暂停与停止

后端先创建会话 ID，再开始模型请求。原始用户请求、原生助手回复、配对的工具调用与结果、已完成操作回执均由后端管理。前端只保留内存中的显示缓存，发送后端 ID、本次请求、模型设置和必要的选择及语言上下文；显示消息不能回传为证据或授权。会话绑定模型服务、模型、端点和存储身份；发送请求时模型身份变化会创建新会话，存储身份变化则拒绝复用旧会话。

会话和任务检查点保存在应用数据库旁的 `assistant-sessions/` 中，采用有界读取和原子替换，最多保留 16 个会话、30 天。保存内容包括脱敏后的公开对话、成对工具回执、原始要求、任务约束、文件修改回执及累计预算；不保存 API 密钥、确认令牌、待执行修改或模型私密推理。更换模型配置或存储路径后不能复用旧会话。损坏的存档明确报告恢复错误；删除会话也删除其存档。每个会话仍限制为 2 MiB、1,024 条合计记录和最多 128 条原始用户请求，满额时拒绝继续记录。

重启后，未完成任务恢复为暂停调查，未配对工具调用记录为结果未知。继续时重新取证并生成新预览，不重放旧修改；已尝试操作的指纹及累计预算保留。如果脱敏移除了恢复原约束或保护基线所需的值，会要求重新陈述任务，不能假装完整恢复。

存档读写只使用一个工作线程，排队与执行共用 30 秒期限。已开始的写入超时后结果保持未知，仍可能随后完成；实际磁盘操作结束前继续占用名额，避免重试积累后台线程。

模型当前可见的窗口最多容纳 24 KiB 业务证据、64 条消息，完整原生协议封套另限 256 KiB。当前任务上下文显式固定，内部纠错反馈不会挤掉原始约束。更早的完整工具交互保留在后端归档中。`read_session_history` 使用记录位置和 UTF-8 字节位置两个游标，可一直读到大记录末尾；返回内容先整体脱敏。原生思考与签名封套完整保留用于服务协议重放，不混入检索证据。原始用户请求有会话内稳定的 `prior-N` ID 和独立分页来源，最近六条摘要不会替代保留的完整原文；每次任务仍最多选择六条相关原请求，并遵守合并字节上限。归档观察不代表当前运行状态，也不能授权重复执行旧操作。活动窗口及工具定义仍受模型传输和任务上下文限制。

解析后的任务持有统一预算，调查、确认、后续操作和恢复共用累计计数。计数包含已尝试的工作；失败或结果未知的写入不会因为点击继续而变成可以安全重放的操作。相同前置状态下重复提交相同规范化操作会被拒绝，方案或实际证据变化后才可重新评估。

| 预算 | 可恢复暂停前的分段额度 | 整个任务上限 |
| --- | --- | --- |
| 模型请求 | 24 次 | 128 次 |
| 证据读取 | 24 次 | 128 次 |
| 已确认操作尝试 | 8 次 | 32 次 |
| 实际工作时间 | 所有分段共享 | 20 分钟 |

工作时间计入模型请求、证据采集和已确认操作，等待用户确认的时间不计入。分段额度用尽时返回原因、使用计数和进度摘要；只有后端返回 `canResume=true` 才显示“继续”。继续恢复保存的原任务、要求、证据和操作前置条件，不重新理解一条新提示，也不重置累计用量；暂停或重启后，下次修改需要重新确认。累计硬上限、错误的模型协议、上下文超限和无效证据不会自动变成可恢复状态。前端另有 32 次确认的防故障上限，触发后撤销待处理操作并显示限制，不能显示成功。

恢复中的调查、或确认操作后的后续调查失败后，任务、证据、要求和已用预算仍保存，可由用户明确重试；重试不会增加预算切片或重放已经确认的操作。网络失败导致结果不明时，“检查任务状态”仅查询后端，确认暂停后才重新提供单独的“继续”操作。运行中或无法连接时不会自动重试；空闲状态也不能证明丢失的回执报告了成功。

确认和恢复从会话绑定开始，直到执行与核验结束都持有存储租约，防止切换存储路径把已接受的任务指向另一份数据。过期、取消、淘汰或被新轮次替代的会话会释放相应检查点，旧记录不能占用当前任务的恢复容量。

“停止请求”会发出取消信号，并使待确认操作及保存的检查点失效；当前请求和尚未结束的取消请求都结束前，界面保持处理中。取消阻止后续工作，不能撤销已经提交的写入，已完成回执和备份仍需保留核对。阻塞读取直到真正结束才释放工作线程名额和存储租约。删除会话会使后端状态失效，迟到回复不能重新创建该会话。发送新的用户请求会替换原有待确认方案或检查点；恢复暂停任务应使用专门的“继续”操作。

### 连续执行、实时进度与通用文件检查

确认预览时可选择“允许本次任务连续修复”，默认不选。范围仅包括已绑定实例的配置、可编辑文本、端口，以及恢复或启动任务所需的启动操作；安装、下载、可能下载的文件校验、广播和管理命令仍需单独确认。授权只在本次确认链中有效，并绑定会话轮次、任务和目标，最多 20 分钟且受原操作预算限制。停止、新请求、暂停或重启都会终止授权。每步仍需应用校验、现场检查和回读；超出范围的方案会重新显示预览，已执行步骤各自保留回执。

模型公开回复会逐段显示，工具进度来自真实执行边界。重复协议字段采用独立的 16 MiB 传输和 16,384 个事件上限；单个待解析帧、累计保留的正文/推理/工具参数分别受 256 KiB 限制，最终序列化结果还会再检查。只在完整流、结束标志、工具 ID 和 JSON 参数全部通过检查后才执行工具；截断或超限不能变成可执行指令。界面会丢弃旧轮次的迟到事件，在进度游标过期时恢复当前文本，不展示模型私密推理。明确返回完整 JSON 的服务也可使用，不为切换协议重复发送请求。

`validate_instance_file` 对 JSON/TOML 做实际语法检查并返回原文哈希和错误位置，不执行文件内容；其他语言及应用语义仍标为未验证。`patch_instance_files` 一次支持最多 8 个现有可编辑文件、每文件最多 16 个不重叠修改，均绑定原文哈希。先核对全组并备份，再逐项写入；中途失败时逆序回滚，且只回滚仍是本次修改结果的文件，避免覆盖外部编辑。回执区分已应用、未应用、已回滚和需部分恢复；这不等于跨文件的系统原子事务。没有游戏专用适配时仍使用通用工作区工具；DST 元数据说明只出现在对应适配范围。

[助手验收说明](assistant-evaluation.md)列出了可复现故障和验收命令，并区分确定性工具链测试与真实模型试验。

### 诊断与修改

明确请求停服、重启、创建备份或恢复备份时，每项操作都需要独立预览确认。停服与重启绑定当前受管理的运行会话；重启即使在启动阶段失败，也会保留停服回执，只有新一轮运行通过就绪检查才算完成。存档备份与恢复目前要求实例已停止。创建备份沿用实例已有的保留策略；恢复绑定确切备份 ID、备份内容与当前存档，预览后任一内容变化都会拒绝执行，并在替换前保留保护备份，恢复后保持停服。这四项不属于连续修复授权或自动修复后续步骤。恢复存档与“修复服务使其运行”是不同请求。

这四项预览和结果摘要按界面语言偏好显示简体中文或英文；没有有效偏好时，按请求语言回退，并由后端为本次确认保留选定语言。预览展示真实实例名称；恢复备份时还显示确切备份 ID、UTC 创建时间、文件数及大小。适用的停服自动备份、旧备份保留与清理规则、保留条件及提取的请求约束都会展示，包括对应的用户原话。内容哈希保留在后端快照和证据中，不出现在面向用户的摘要里；完整展示摘要仍与单次确认令牌精确绑定。诊断原因保持原文，可能含其他语言；本地化不改变授权或完成判定。

| 请求 | 可用行为 | 完成边界 |
| --- | --- | --- |
| 排查启动失败或控制台错误 | 检查选定实例的运行证据和配置，说明依据及缺失信息。 | 推测原因属于诊断，不等于已经修复。 |
| 修改已建模设置或处理端口冲突 | 使用现有设置键或已声明端口名称生成方案，确认后保存并回读核验。 | 已核验保存值，不代表运行中的游戏已经应用配置。 |
| 安装或校验服务器文件 | 分别调用所选游戏的安装和文件校验流程。 | 结果须核对游戏、安装状态和可执行文件；文件就绪不代表世界正常。 |
| 创建并开服 | 描述游戏和所需配置，按需确认文件准备、创建、配置及启动。 | 创建后实例保持停止；只有通过运行验收才能完成开服任务。 |
| 启动服务器 | 调用所选实例的受管启动流程。 | 进程启动不代表玩家已经可以加入。 |
| 安装 Mod 或发送管理命令 | 使用游戏已声明的 Mod 流程或允许的游戏管理命令。 | 支持的来源、命令和响应保证取决于游戏模块。 |

助手可以连续调用只读工具，检查运行健康状态和日志、设置名称、值与 Schema、配置文件、已配置的 Mod 顺序、Workshop 安装证据、启动校验及实例所属文本文件。后端将工具绑定到任务实际指定的实例和游戏，由模型选择所需证据。设置目录分页返回，并区分实际保存的键与只有 Schema 声明的字段；准备修改方案时还会附带有界配置快照。调查中的读取、自动补充的 Mod 证据和历史证据分页共用任务累计读取预算。同一应用最多同时运行两个调查。证据是否足以支持修复方案，仍取决于所用模型的判断。

配置、已登记进程日志及已安装 Mod 元数据读取共用两个阻塞工作槽，单次等待上限为 30 秒。等待超时或取消后，实际文件读取退出前仍保留工作槽与存储路径租约，防止重复请求累积这些阻塞工作，或读取未结束时切换存储路径。

模型可以通过 `search_settings` 按设置键、来源文件名或 Schema 描述检索设置，每页最多十项。所有查询词必须同时匹配元数据，适合使用简短文件名或有辨识度的关键词，不宜堆叠近义词。零匹配会提示缩短查询，不代表设置不可编辑。每次检索计入同一个任务预算。模型请求附带读取预算说明，以便复用已取得的证据。

针对饥荒，`inspect_installed_mods` 可以读取已安装的本地及 Workshop Mod 元数据，包括声明的版本、API 版本、依赖与加载优先级。它支持按准确目录名查询或分页列目录，使用所选实例实际运行目录和 UGC 目录，分别返回不同位置的副本，不将某个副本直接认定为游戏已加载的版本。读取复用有资源上限的 `modinfo.lua` 沙箱，不执行 `modmain.lua`；缺失文件、路径拒绝及证据不足会明确返回。

运行日志出现原生 `MOD ERROR` 时，若读取预算尚有余量，助手会在下次请求模型前自动读取其中最多五个 Mod 的元数据，每次调查最多自动读取一次；若尚未读取当前 Mod 配置，且还能容纳一次读取，则一并补读。成功读取元数据后，还可自动核实一次其中最多五个尚未读取或排队的直接依赖目录；不递归解析依赖图、不推断兼容性，也不猜测显示名或含糊的 Workshop 值。所有读取均计入任务预算；错误标记仅是调查线索，不直接证明根因。

饥荒的 Mod 状态还会基于实际配置生成器，按分片提取配置中声明的启用、禁用和未指定状态。动态 Lua、含糊的值或超出分析上限时返回 `unknown` 和空值名单，不冒充没有启用 Mod。已安装 Mod 的取证结果会附带相应目录的声明状态；无法准确绑定的 Workshop 别名仍标为未知。此过程不执行 Lua，并单独标明未启用的分片。配置声明不等于游戏实际加载或功能正常。

饥荒优先加载优先级较高的 Mod，重排 `modoverrides.lua` 的键不会改变这一优先级。助手可以通过现有设置确认流程启用已安装的依赖，也可以在服务器停止后，提出对实例独享 Mod 文本（包括 `modinfo.lua`）的有界修改。元数据声明和保存源码修改本身都不等于兼容性证明。

对于 Project Zomboid，保留现有 Mod 的任务允许重排 `mods` 设置，但必须保留完全相同的原生 Mod ID。校验复用原生配置写入器对分隔符、注释及重复项的处理；仍拒绝新增、删除、重命名 ID 或改变其他受保护 Mod 设置。确认后通过原生配置写入器保存，并回读持久化设置。已安装元数据工具目前仅支持饥荒，因此 PZ 的顺序需要可用文件、日志证据或操作者明确指定，助手不能编造依赖关系。这项检查不证明游戏实际加载了哪些 Mod 或兼容性；缺少运行时 Mod 证据时，恢复结果仍为未验证。

`read_runtime` 分别收集当前托管会话中最多八个已登记进程的日志；会话结束后读取最近一次已记录会话。各进程共享最多 400 行扫描预算，返回证据还受字节上限约束，优先保留错误片段。收集期间会话、进程身份、状态或路径变化会使快照失效。缺失、截断、历史及未绑定日志均明确标记，不代表完整错误历史。

`list_instance_files`、`read_instance_file` 与 `search_instance_files` 可在各受支持游戏的实例目录内工作，根目录由存储中的实例记录确定，不能由模型提供。支持读取 UTF-8 配置、日志和扩展源码等文本；管理器隐藏目录、凭据命名路径、备份、路径穿越、符号链接和 Windows 重解析点均被排除。列表最多扫描 2,048 项、12 层，文件和子目录均可分页，每个列表每页最多 64 项并共享返回字节上限。单文件限 256 KiB，脱敏后按 4 KiB 分页。搜索对脱敏文本进行区分大小写的字面匹配，每次最多检查 64 个文件、1 MiB 文本，返回最多 24 个匹配。游标可跨文件和单文件的大量匹配继续读取，空结果页也可能仍有下一页。游标绑定查询、目录列表及正在续读的文件哈希，发现变化需重新搜索；返回片段包含实际命中位置。扫描硬上限和无法读取的文件明确表示证据缺口；零匹配不能证明内容不存在。

可读取不等于可修改。每个文件返回 `editable` 和 `protectionReason`。文本修改只开放既有、实例独享的 Mod、插件和脚本，范围是允许的实例 `data`、`runtime` 扩展目录，以及饥荒分片 Workshop 内容；修改 runtime 文件还要求管理器的独享运行目录标记。共享或所有权未确认的运行文件、生成配置、存档、世界、日志、凭据和二进制文件不能直接修改。受管设置仍由原生配置写入器保存。通用工作区提供文件所有权边界，不代表已经实现所有游戏和插件语言的语义校验。

`inspect_network_endpoints` 可观察已确认归属于当前实例受管进程树的本地 TCP/UDP 端点，采集前后复核进程创建身份、当前运行及声明端口。最多接受 16 个已登记进程目标，返回 64 个端点。归属未知或运行变化会明确失败，不会将空结果视为观察成功。该工具不修改端口或防火墙；本地存在监听端点不证明外部可达或多人联机正常。

`patch_instance_text` 只替换一个唯一、精确匹配的文本片段，修改前后文本合计最多 8 KiB。确认窗口展示相对路径及两个完整片段，遮蔽能识别的秘密。后端保留预览时的原始字节和 SHA-256，并要求实例保持停止、没有活动进程。确认时再次核对实例及文件，先将原文件与哈希清单保存到 `data/.langame/file-patches/<backupId>`，再原子替换并独立回读。文件已变化则拒绝；写入或回读失败会保留备份并报告标识。最多保留 64 份备份，达到上限后拒绝继续修改，不自动删除恢复材料。这些检查协调管理器内操作并检测普通外部编辑，不构成对同一 Windows 用户下恶意进程的隔离。

“执行请求”可以在精确文件修改核验后完成，回执明确不包括启动及 Mod 行为。“恢复服务”在后续确认和启动评估中继续检查修改后的文件哈希，更新覆盖补丁不会被静默当作恢复成功。启动需要明确确认，或用户已选择的同任务连续执行授权；正在运行的服务器须先通过正常服务器控制停止，才能提出文件补丁。助手不会为了校验语法而执行被修改的 Lua，也不会在启动失败后自动撤销改动；备份保留完整原文件供恢复。

`read_config_file` 返回的 `settingCandidates` 仅表示元数据匹配候选。提出修改前，助手须核对目标文件及分片，并读取当前设置。对于识别为敏感的设置，当前值及 Schema 中的 `default`、`examples`、`enum`、`const` 值均会脱敏。

业务消息、调用参数、工具结果及工具 Schema 去除重复表示后总共限制为 64 KiB。原生回复及协议推理、签名块完整保留，另受模型服务请求与响应各 256 KiB 的传输上限约束，不重复计作业务证据。这个字节预算用于限制传输和保留的证据量，独立于模型的 token 上限。结果过大或读取失败会明确作为证据缺口返回，保留此前的指令和证据；连新的缺口说明也无法容纳时，调查停止，不生成操作方案。请求过长、无法完整保留用户要求及行动契约时，会明确返回错误。无效的模型回复不会执行操作，同一次调查预算内最多允许两次格式或任务约束纠正。

只读诊断不需要确认操作。改变服务器状态的操作会先显示预览，其中设置修改展示完整补丁，能识别的秘密值会被遮蔽。核对目标和修改值后再确认。预览有效期为两分钟，只能使用一次；模型服务、模型或服务地址改变后，原预览失效。目标实例已保存的设置、端口或已记录运行状态变化也会使预览失效。设置和端口写入会在持有实例修改锁时再次比较状态，保存后独立读取结果并核验。回读失败不代表没有发生写入，应先检查当前设置，再决定下一次操作。

配置工具只读取选定实例配置目录内、有大小限制的文本文件，拒绝目录穿越、符号链接和 Windows 重解析点。支持的格式包括 Lua 和 XML；严格解码 UTF-8 及带字节顺序标记的 UTF-16，拒绝非法编码和 NUL 字符。文件在分页前先完成脱敏，避免通过分页起点绕过能识别的凭据字段。`cluster_token.txt` 等名称明确表示凭据的文件，即使正文只有令牌也会被遮蔽。初始配置读取失败会明确告知模型，助手仍可继续检查运行证据。这些工具提供读取能力，修改仍通过管理器已建模的设置及原生配置写入流程完成。

启动和游戏控制台操作也会在取得实例修改锁后再次检查预览状态。已确认的指令不会因等待锁期间发生受管服务器重启，而悄悄转发给替换后的运行实例。

### 验证与后续确认

普通聊天、能力介绍和一般解释直接以自然语言回复，不创建任务或弹出确认。本机信息通过 `read_host_info` 读取；需要服务器证据或执行操作时，模型通过原生 `resolve_task` 工具确定意图。工作请求必须绑定目录中的目标，`target=none` 只允许用于非空澄清问题；无目标工作请求会收到工具错误，让模型在原有纠错预算内选择读取主机、直接对话或有效目标；后端对照有数量限制的真实游戏和实例目录校验目标，建立内部任务契约。只读调查后也可直接解释结果，文字内容（包括 JSON 示例）不能授权任何操作。要求修好服务器则包含运行验收；要求创建并配置但不启动时，须核验实际保存的设置；只创建空实例则可以在创建后完成。无效或含糊的识别结果不会静默退回某种操作模式。未知工具、被拒绝的调用批次及无效参数会收到逐条配对的原生错误结果，并允许一次纠错，共用原来的 90 秒时限和最多两个并发请求的限制。可空字段缺省表示没有值，必要授权和历史请求来源不会擅自补全。对话入口本身不能执行操作。后端从自身会话提供已记录的原生消息和配对工具证据，不接受显示缓存上传的历史。只有命令入口登记的原始用户请求可以构成任务授权；工具结果、应用控制消息和助手回复不能代替它们。支持工具选择参数的协议显式使用自动选择，不以关键词分流聊天和任务。诊断日志只记录回复结构和处理结果，不记录对话正文或工具参数。

新建请求先绑定识别出的游戏，再绑定确认创建产生的真实实例，不会复用其他页面残留选中的服务器。创建、配置、启动分别确认；首次启动失败后，仍沿用同一任务标识和实例继续调查。准备、开服和修复的后续操作及恢复过程共用一份任务累计预算；执行到固定步数不代表任务成功。

新实例可分几次确认完成初始配置；首次确认启动时，最终保存的配置才成为后续修复须保留的模组及分片基线。即使采用默认值，本次任务首次启动前也须先确认配置。开服和恢复服务任务会在确认前拒绝包含未知字段的配置补丁，助手须查询真实字段后修正完整方案；设置读取明确区分不存在的字段与值为空的已有字段。安装成功后生成新的创建预览；只要求安装文件的单次请求，则可在文件检查满足后完成。文件校验调用游戏的真实校验流程，不再使用普通安装命令。可执行文件缺失、安装不完整、创建结果无法回读或运行证据未知，均不能完成开服任务。

回答澄清问题或明确继续上一请求时，意图识别可通过应用分配的稳定来源标识引用最多六条相关的历史用户消息。后端按顺序合并选中的原文与本次回复，不改写或截断其中的要求；助手回复和界面上下文不能充当请求来源。未知、重复或逆序来源标识会被拒绝。超长历史消息会标为不可用，不阻塞无关的新请求，也不能作为操作授权来源；本次请求超出大小限制时，在准备操作前明确停止。要求创建并配置但不要启动时，会继续到实际设置验证完成，禁止提出启动操作。

默认保留已有模组及其配置；只有明确要求改变这些功能的请求才能被解释为允许相应修改，具体方案仍需预览确认。笼统的修复请求不允许通过丢弃模组来让服务器启动。后端在确认前保存自动识别的目标、保留基线和任务标识，后续步骤继承同一份契约，不重复分类。模型后续回复、对话证据及客户端传入的策略字段均不能重定义它。

运行命令的回执区分接受提交和确认传输。受管 stdin 写入尚未确认时，任务保持未知，不自动重复发送；传输确认也不代表任意游戏内效果已得到验证。

配置方案展示和执行前都会检查保留条件。饥荒按实际生成的配置语义核对：原活跃分片、启用模组及已有模组选项必须保留，允许新增或启用依赖。不会执行动态或含糊的 Lua 来判断修改权限；无法可靠分析的受保护配置必须保持原样。其他尚无语义校验器的游戏，其受保护模组设置必须保持不变。违反约束的方案在现有调查预算内收到有次数上限的校验反馈，不能写入配置。

确认设置、端口或启动操作后，助手会检查保存结果及可用的运行证据。“恢复服务”任务可在累计预算内为同一个实例和游戏提出下一步。是否继续由明确的任务目标决定，不再靠请求中的关键词判断。每次修改或启动都需要确认，或仍有效的同任务连续执行授权。没有这项授权时保存修复不会自动启动服务器，也不会悄悄停止或重启正在运行的服务器。调查失败或达到上限时，已完成的修改会保留，并明确说明尚未解决的问题。

操作结果附带同一任务标识和逐项检查，状态为满足、失败或未知。所有适用检查满足后，任务才完成。保存配置可以完成一次具体修改，但不能完成恢复服务的要求；取消、达到步骤上限或证据未知不会显示为成功。切换页面或编写下一次请求，也不会改变已经绑定的确认任务。澄清问题不包含操作或确认令牌。

饥荒的产品验收通过固定只读控制台探针，检查所需模组的原生加载、启用、选中、兼容声明及失败状态。探针绑定新随机标记、当前受管运行和进程身份、日志偏移及代际；总预算 15 秒，最多两个分片、64 个名称，输出有界，不执行模型提供的 Lua。验收期间配置或运行身份变化会使完成证据失效。这证明已检查的原生加载状态，不代表任意模组功能、玩家连接或长期稳定性；缺少游戏专属证据时保持未知。

修复已成功保存并回读、启动预检查通过，且实例仍已停止、配置未发生变化时，应用直接生成下一步启动确认，无需再次请求模型。此时不会根据历史失败日志重复改写配置；新的启动失败后，再回到证据调查。

| 验证状态 | 含义 |
| --- | --- |
| `verified` | 适用检查已通过。仅修改文件的“执行请求”核验保存文本；启动或“恢复服务”还要求新运行实例在连续观察中保持进程身份一致、具备启动就绪证据。 |
| `failed` | 操作或启动校验失败，或者证据表明新运行实例发生故障。 |
| `inconclusive` | 尚不能证明恢复，例如配置已保存但还未启动、旧进程尚未应用修改，或验证中证据缺失、状态发生变化。 |

运行验证在 15 秒观察预算内最多采样三次，需要连续两次观察到就绪。游戏启动流程另有自己的超时限制。启动已验证不代表长期稳定、玩家能够连接或全部 Mod 兼容。结果附带验证证据，并在可用时提供运行标识。验证失败不会自动撤销已保存的修改。

排查崩溃或 Mod 问题时，请说明现象及发生时机，例如进程打开前、世界加载中、启用某个 Mod 后或玩家加入时。先让助手指出相关日志证据，再提出修改。修改后检查新的启动输出，并验证原来失败的行为。测试可能改变存档内容的方案前，先备份世界。

### 能力限制

LAN 使用管理器现有操作及有界实例工作区，不能运行任意 Windows CMD 或 PowerShell 命令、修改游戏可执行文件、编辑任意宿主项目，或自行安装工具扩大权限。它可以提议修改允许范围内的实例独享扩展文本，包括源码文件，但不能通过任意编译或执行这些内容来验证修改。游戏控制台和操作系统命令行仍是不同接口。

LAN 支持原生工具调用、后端证据历史和可恢复任务。理解需求、选择合适工具和诊断陌生游戏仍依赖模型表现。工具不支持、证据不足、上下文超限、模型服务失败或任务预算耗尽时，应用应明确说明限制，不能据此宣称目标已经完成。

游戏日志可能指出缺失 Mod 或依赖加载失败，但不一定足以证明正确的加载顺序。管理器中的 Mod 列表也不能证明所有 Mod 与游戏版本之间兼容。未支持的修改和缺失的证据需要明确说明。设置已保存、命令已写入、进程正在运行和多人联机正常，分别是不同的结果。

停服沿用实例已有的自动备份策略。如果用户已明确禁止创建备份，则启用停服自动备份的停服、重启请求也会被拦截；恢复存档因必须创建保护备份同样被拦截。LAN 不会悄悄修改策略来绕过限制。

### 模型服务与数据

完整说明见[隐私与数据说明](../PRIVACY.md#简体中文)。点击 LAN header 的隐私按钮，可直接在 LAN 内阅读离线全文，与 AI 设置页一样通过返回按钮回到对话。数据使用和接收地址提示集中在 AI 设置内。发送消息或继续任务前，请在 AI 设置中核对接收地址：OpenAI 兼容、Ollama 等协议名称不代表实际运营方，也不证明处理过程留在本机。密钥保存在管理端本机，请求时会发送给配置的服务用于认证。

AI 设置中的“检测连接”通过 LAN 实际使用的原生流式传输检测当前服务与模型，分别报告聊天、工具调用和工具结果回传。因此，能聊天的模型仍可能无法完成操作通信。检测共用 90 秒时间上限，最多发送三次小型合成请求，模型服务可能计费；不读取服务器、资料或会话数据，不创建助手会话，也不执行应用操作。已保存的密钥按所选协议和服务读取。仅在桌面端或已连接的管理界面中明确点击后运行，不自动重试。

修改配置会清除旧结果并取消正在进行的检测；取消期间会等检测和取消请求都结束后才解除忙碌状态。通过只能证明所选模型完成了本次聊天与工具通信，游戏操作是否正确、资料是否完整仍需各自的证据。

OpenAI 兼容服务返回的公开拒绝内容会正常显示为助手文本，不能同时触发工具调用。定向文本生成遇到长度截断或异常结束状态会明确报错，不将半截内容当作成功结果。Ollama 的服务根地址及受支持的完整端点地址统一归一化，聊天、辅助文本生成和模型列表使用同一地址规则。

Ollama 对话入口和调查使用原生 `/api/chat` 接口，单次请求设置 32K token 上下文和 8,192 token 总输出上限，输出预算包含推理及最终内容。请求使用原生工具调用，不设置 JSON 回复格式，保持模型默认的推理行为，并禁止静默截断输入或通过移动上下文丢弃此前内容；实际 token 超出上下文容量时明确返回错误。助手只调度原生工具调用；thinking 内容作为协议上下文保留，不展示或执行。对话入口受 90 秒总时限约束；任务工作受前述累计工作时间和有界读取线程约束。这些设置只用于单次请求，不修改 Ollama 服务配置。

所选模型服务会收到你的请求及随请求附带的诊断上下文。按请求需要，这些内容可能包括实例和模块标识、设置、配置与扩展源码片段、运行日志及观察到的本地网络端点。如果服务位于另一台机器，数据会通过配置的连接发送给它；服务器文件保存在本机，不等于 AI 请求也始终留在本机。

模型请求仅跟随配置地址同源的重定向，即协议、主机及实际端口均相同。改变任一项都需要明确更新服务地址；诊断正文和服务凭据不会被转交给其他服务，包括同一台机器上的其他端口。

应用在发送诊断文本前会遮蔽能识别的凭据字段和本机绝对路径，但不能保证识别自由格式日志中的所有秘密或个人标识。请勿在请求中粘贴凭据或私有玩家数据。诊断内容必须留在本机时，选择本地模型服务，并核对配置的服务地址确实指向本机。

### 实现与验证

连接检测由 [`assistant_connection.rs`](../apps/desktop/src-tauri/src/assistant_connection.rs) 和 [AI 设置控件](../apps/desktop/src/components/AppAiConnectionCheck.tsx) 实现。原生测试覆盖三个协议的流式通信、结果回传、登记前取消、超时和连接释放；浏览器夹具用合成 IPC 返回值验证配置保存、取消与旧结果失效。忽略运行的 `saved_deepseek_connection_probe` 需要显式设置 `LANGAME_ASSISTANT_LIVE=1` 并已有 DeepSeek 密钥，通过生产传输检测，不读取应用数据。

会话和归档由 [`assistant_sessions.rs`](../apps/desktop/src-tauri/src/assistant_sessions.rs) 管理，配合[命令绑定与取消](../apps/desktop/src-tauri/src/commands_assistant_ops/session.rs)、[累计预算](../apps/desktop/src-tauri/src/commands_assistant_ops/run_budget.rs)和[检查点恢复](../apps/desktop/src-tauri/src/commands_assistant_ops/continuation.rs)。[实例工作区](../crates/app-storage/src/instance_workspace.rs)定义通用文件归属与保护规则，[工作区工具](../apps/desktop/src-tauri/src/commands_assistant_ops/workspace_files.rs)和[网络观察](../apps/desktop/src-tauri/src/commands_assistant_ops/workspace_diagnostics.rs)提供有界证据。前端测试覆盖先创建会话再请求、创建或确认期间停止、等待原请求最终返回、保留已完成回执、恢复检查点及禁止上传客户端历史。浏览器测试验证输入和确认控件，不证明原生模型或游戏行为。

相关实现位于 [`commands_assistant_ops.rs`](../apps/desktop/src-tauri/src/commands_assistant_ops.rs)、[调查工具](../apps/desktop/src-tauri/src/commands_assistant_ops/investigation.rs)、[配置读取器](../apps/desktop/src-tauri/src/commands_assistant_ops/config_documents.rs)、[确认与回读校验](../apps/desktop/src-tauri/src/commands_assistant_ops/preconditions.rs)及 [`assistant.rs`](../apps/desktop/src-tauri/src/assistant.rs)。测试覆盖计划解析、目标选择、读取预算、凭据遮蔽、允许的操作、确认流程，以及前端对诊断原文的保留。模拟模型测试验证应用的确定性行为，不能证明所有模型的诊断质量，也不能替代真实游戏与 Mod 组合的兼容性验证。

[修复链](../apps/desktop/src-tauri/src/commands_assistant_ops/repair.rs)与[运行验证器](../apps/desktop/src-tauri/src/commands_assistant_ops/verification.rs)分别记录操作确认和实际恢复证据。[原生验收测试](../apps/desktop/src-tauri/src/commands_assistant_live_acceptance_tests.rs)默认不运行，需要明确开启、本地 Ollama 模型及已安装 DST 官方文件的独立副本。场景覆盖原生配置错误和 Lua Mod 缺失依赖，恢复后还需通过真实引擎探针检查世界及 Mod 加载；日志和步骤证据保留在仓库外。这些场景不能证明任意 Mod 排序都能修复，也不能代表其他游戏的兼容性或其他模型的表现；只有实际完成的运行，才为对应的游戏包、模型和场景提供证据。

[新建开服原生场景](../apps/desktop/src-tauri/src/commands_assistant_live_launch_tests.rs)从空的隔离数据库和已安装文件副本开始，在同一开服任务中确认创建、模型配置及真实启动，再探测已加载的世界并停止本次进程。启动前检查实例记录、保存配置及实际启动参数均绑定 `127.0.0.1`，且启动程序精确位于本次授权的库副本或该实例的私有目录。启动前以流式 SHA256 核对可执行文件与脚本包，确认它们匹配已认证来源。生产启动会创建最多七条限于 `127.0.0.1` 的实例入站防火墙规则；测试先确认规则名称不存在，停止后仅删除匹配本次所有权的规则，失败启动同样执行清理。原生端口证据另外记录全部已验证进程的监听；模块严格绑定检查覆盖 Master 游戏端口，不能仅凭保存的绑定设置证明每个 Steam socket 都在回环地址。此场景不确认安装、文件校验或下载，因此不证明外部下载或 SteamCMD 校验和检查已经实测通过。 显式开启的 `assistant_live_saved_deepseek_launches_new_native_dst_server` 通过生产传输使用已保存的 DeepSeek 凭据，将本次验收限制为最多六次分别确认的操作；仍需管理员权限和隔离文件副本，固定脚本服务的证据不计作模型验收。

已有副本如果缺少可信的干净程序及初始程序清单，需先单独进行官方认证。[`native_catalog_preparation` 夹具](../crates/app-storage/tests/native_catalog_preparation.rs)在显式开启后生成全新隔离数据库及设置凭据。使用该隔离 `LOCALAPPDATA`、SteamCMD 和本次游戏副本运行 `install_catalog certify dontstarve`；仅传 `--games-root` 不会隔离桌面数据库。认证可能更新可丢弃副本，只有原生 Steam 校验及完整 depot 文件检查通过后才发布清单。

[确定性原生契约场景](../apps/desktop/src-tauri/src/commands_assistant_live_launch_contract_tests.rs) `assistant_live_contract_launches_new_native_dst_server` 使用本机 HTTP 服务先完成需求记录和冻结，再依次返回三步固定操作计划，复用相同的创建、配置、启动、世界探针及清理检查。它仍需明确开启原生验收、管理员权限和已有游戏文件副本，但不需要模型。报告标注为 `scripted-local-contract`；通过只能证明给定计划能够正确执行，不能证明模型能从用户请求生成该计划。

调查采用模型服务的原生工具消息，保留每次调用及对应 ID、顺序和结果；被拒绝的提案及其校验错误也进入后续会话。普通回复文字不会被转成工具调用。工具目录随当前目标和任务阶段确定，最终提案必须单独调用。业务会话及工具 Schema 共受 64 KiB 预算限制；生产调查共用任务累计预算，每次调查中的需求草稿仍最多调用 8 次；协议校验和上下文字节限制独立于可恢复的分段预算。超限、协议标识错误或输出截断均明确停止，不执行操作。

开服、恢复服务及四项停服/重启/备份操作都携带本次请求的要求清单。服务任务在提出操作前，必须先通过 `record_task_requirements` 记录清单，再用 `finish_task_requirements` 完成草稿。明确修改任务首次提议这四项操作之一时，会转入同一草稿流程；清单未完成就不能进入可执行预览，其他普通修改操作仍沿用原流程。清单包括设置期望值、已声明端口的期望值、禁止执行的具体操作及不能验证的事项。原请求按连续片段分配固定 source ID，由后端复制原文，模型无需重新抄写引用。单批中的合法项会保留，工具同时返回失败状态和逐项错误，供后续纠正；完成草稿只冻结本次提案的要求，不执行操作。被脱敏隐藏的片段不能被自动解释或静默遗漏。确认预览显示要求描述、原请求片段、目标和期望值，并遮蔽能识别的凭据。用户在现有操作确认中核对模型提取的清单；清单本身不能证明模型完全理解了自然语言要求。后端保留真正的类型和值，创建、分步配置和修复始终继承同一份要求，后续回复不能删减或重定义。

尚未创建实例时，可用 `list_module_settings`、`read_module_settings`、`search_module_settings` 查询所选游戏的有界元数据。结果明确区分 schema 声明、管理器维护的监听地址和实例实际保存值，沿用既有读取次数、字节及时间预算。创建后改用实例读取工具取得真实配置。一次查询混合有效键和未知键时，会保留有效声明并明确列出缺失键，不会丢弃整批结果。

分步配置可以暂未满足最终要求，但补丁不得与确认的期望值冲突，所有要求满足前不能启动。每次操作后回读保存的设置和端口，完成任务前再次逐项核对。动态饥荒世界 Lua 无法静态确定时，投影出的默认值不能充当满足要求的证据。这些检查证明规范化保存值及可静态检查的配置；实际就绪和适用的原生模组检查仍分别验证。禁止操作只约束具体调度动作，不代表能证明游戏内部没有网络或其他隐式行为。存在无法验证的要求时，不执行修改，也不显示完成；需要通过新预览澄清请求，不能让模型自行降低要求。

配置的保存与生效规则见[服务器配置](server-configuration.md#简体中文)，游戏操作及证据见[游戏接入](README.md#游戏接入)。
