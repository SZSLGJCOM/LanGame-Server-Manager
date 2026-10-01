# Assistant evaluation

The acceptance target is a general tool-using assistant inside the manager's authorized instance workspace. A game-specific evidence adapter can add metadata, but cannot be a prerequisite for file discovery, diagnosis or repair. Ordinary conversation remains a model response, not a keyword classifier. File syntax, saved state and runtime recovery are separate claims.

## Reproducible scenarios

| Scenario | Observable acceptance | Tests |
| --- | --- | --- |
| Unfamiliar synthetic service | Discover and read JSON/TOML in a private plugin directory, report their real syntax errors, preview both changes, confirm once, retain original backups, parse the repaired files and preserve unrelated bytes. No service adapter exists. | `assistant_generic_evaluation_tests` |
| File changes after preview | A changed second source rejects the entire set before the first file is written. | `generic_synthetic_second_file_conflict` |
| Failure during a set of writes | Reverse rollback verifies the original files. An external edit or rollback failure remains a partial-recovery receipt and cannot authorize startup. | `instance_file_patch::batch`, `authorization_tests` |
| Continuous task authorization | Only the same task, instance, module and conversation revision can continue. Read-only requests, install/download, broadcasts and admin commands remain outside the grant. | `authorization_tests`, `authorization_workflow_tests` |
| Restart and interrupted work | Original requirements, receipts, operation fingerprints and cumulative budgets survive. Incomplete calls become unknown, old confirmations never replay, damaged archives fail explicitly. | `assistant_sessions::persistence`, `persistence_tests` |
| Large receipts | Complete multi-file receipts remain in the archive and can be reconstructed through paged history; capacity failures are explicit. | `session_event_archive_tests` |
| Public streaming | Split UTF-8 and tool arguments reassemble; truncated streams never produce executable calls; thinking/signature blocks are not shown; cancellation stops observation. | `tool_protocol::stream`, `assistant_session_progress` |
| Conversation and user corrections | Model-native dialogue, target clarification, original request sources, cancelled/stale turns and frontend recovery preserve their existing contracts. | `conversation`, `session_workflow`, frontend assistant tests |
| Sourced game guidance | Every shipped game has readable source metadata; Chinese lookup, byte-bounded pagination, stale/unknown labels and application-owned game scope survive the actual tool-result boundary. | `app-modules knowledge`, `game_knowledge_tests` |
| Save backup confirmation | A preview does not create or restore anything. Confirmation is single-use; source or current-save changes refuse replacement; a successful restore keeps the previous saves in a safeguard. | `commands_assistant_backup_workflow_tests`, `backups_checked_tests` |
| Managed stop and restart | An owned native fixture process stops only after confirmation. Restart must identify a new run; a replaced run is not stopped, and a failed new launch retains the old stop receipt while failing the task. | `commands_assistant_lifecycle_workflow_tests` |
| Lifecycle request constraints | A first stop/restart/backup proposal activates requirement drafting; record/finish freezes the original request constraints before preview. A forbidden backup also prevents automatic-stop backups and restore safeguards. Ordinary change actions retain their existing flow. | `lifecycle_requirements_tests`, `lifecycle_operations_tests`, `requirements_tests` |
| English and Chinese confirmations | Interface copy follows the chosen language through confirmation and readback. Previews retain actual instance names, exact backup metadata, preservation rules and original request excerpts. Hashes stay in backend evidence, and the exact displayed summary remains confirmation-bound. | `lifecycle_copy_tests`, `commands_assistant_backup_workflow_tests` |
| Provider completion and LAN voice | All three native request bodies carry the shared persona while focused output retains its exact artifact format. Incomplete responses fail, and refusal cannot schedule a tool or become a generated artifact. | `assistant_provider_tests`, `assistant_tool_conversation_tests`, `assistant_tool_protocol_tests`, `assistant_tool_stream_tests` |

Deterministic tests use scripted provider responses to verify execution and refusal behavior. They do **not** measure whether an arbitrary model will discover the repair. File and syntax assertions are made against actual isolated files, not a model's success statement. Existing launch/recovery tests additionally distinguish saved changes from readiness of a newly identified managed process.

These rows describe executable acceptance scenarios, not a record of successful runs. The lifecycle fixture uses the bundled Necesse module with isolated settings, database and an owned synthetic package. A locally compiled C# console process binds only a dynamically allocated loopback UDP port. A narrowly registered `cfg(test)` adapter replaces the external Windows Firewall call; confirmation, native launch validation, process supervision, bind verification and stopping use their real implementation. The fixture installs no game and verifies the managed lifecycle, not real Necesse startup, player connectivity or multiplayer behavior. Provider loopback tests exercise local HTTP protocol contracts; even coverage of all three protocols does not establish a live connection to all three provider services. Report actual commands, nonzero test counts and exit status separately, and identify the exact provider/model and scope for a live trial.

From the repository root, the portable test entry points are:

```powershell
cargo test -p app-storage --lib instance_file_patch --locked
cargo test -p app-storage --lib assistant_session --locked
cargo test -p langame-desktop assistant --locked -- --test-threads=4
```

Test filters must report a nonzero test count.

## Optional real-model trial

`assistant_live_ollama_repairs_generic_synthetic_service_files` uses the installed local Ollama model to investigate previously broken JSON and TOML. The user prompt supplies the defect and preservation constraints, but not corrected text or a prescribed tool sequence. The trial requires a two-file preview, confirms it in an isolated instance, verifies original backups and parsed values, and proves that unrelated files remain unchanged and no server was started. It does not execute the synthetic plugin or establish arbitrary script compatibility.

The trial is ignored by default, never downloads a model and uses only `127.0.0.1:11434`. In PowerShell:

```powershell
$env:LANGAME_ASSISTANT_LIVE = '1'
$env:LANGAME_ASSISTANT_LIVE_MODEL = 'qwen3.5:9b'
cargo test -p langame-desktop assistant_live_ollama_repairs_generic_synthetic_service_files --locked -- --ignored --nocapture --test-threads=1
Remove-Item Env:LANGAME_ASSISTANT_LIVE
Remove-Item Env:LANGAME_ASSISTANT_LIVE_MODEL
```

Use an already installed model name. The test prints `ASSISTANT_GENERIC_LIVE_EVIDENCE` pointing to its isolated evidence directory and writes a JSON report there, including a failed trial's available preview or receipts. Report the exact model, task scope and exit status when citing results. A successful trial is evidence for that model and fixture only; a failed trial remains a failure and must not be converted into a pass by hardcoding the answer.

## Configured-provider conversation and documentation trial

`assistant_live_configured_chat_memory_and_game_docs` is a separate, ignored-by-default trial. It uses production streaming and the provider's existing system-keyring entry, three synthetic Chinese conversation turns, and actual synchronized Minecraft search/read receipts from an existing isolated cache selected by `LANGAME_KNOWLEDGE_LIVE_ROOT`. It requires the model to retain LAN's identity and the synthetic team name, consume the document result and cite its source URL. No desktop instance database, user instance, host inspection or mutation tools are available. The whole trial has an eight-request and 180-second limit.

Set `LANGAME_ASSISTANT_LIVE=1` only after authorizing requests to the selected service. The default test destination is OpenAI-compatible DeepSeek (`deepseek-flash`, `https://api.deepseek.com`). To use another authorized configuration, set `LANGAME_ASSISTANT_LIVE_PROVIDER`, `LANGAME_ASSISTANT_LIVE_MODEL` and `LANGAME_ASSISTANT_LIVE_BASE_URL` together. Keep keys out of environment variables and logs; save them through LGSM's AI settings first. Run the exact test filter with `--ignored --nocapture --test-threads=1`, using the workstation's managed Cargo wrapper where required. `ASSISTANT_CONFIGURED_LIVE` reports the destination, actual request count, scope and cited sources. A pass verifies only this model/configuration and read-only scenario; protocol loopback tests are not live Anthropic or Ollama trials.

## Boundaries

The assistant does not gain unrestricted operating-system access. Its common tools operate on managed instance evidence and editable files. JSON/TOML syntax checks do not execute scripts; unsupported languages return an explicit gap. Cross-file replacement is recoverable with backups and compare-and-swap checks, not an atomic filesystem transaction or a sandbox against another process running as the same OS user. Confirmation, streaming and persistence are orchestration capabilities, not a guarantee of model judgment or long-term runtime stability.
