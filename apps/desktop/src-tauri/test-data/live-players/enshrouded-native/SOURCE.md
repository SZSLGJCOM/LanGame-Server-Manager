# Enshrouded native protocol fixtures

These are synthetic protocol-1 boundary fixtures, not captured player data or a real moderation result. PID, process creation time, nonce, sequence, name and session handle are synthetic. The Steam platform 3 account/hash pair is independently verified against the official executable's pure function in `modules/enshrouded/reference-fixtures/2026-10-16-native_account_hash.json`.

Supported executable SHA256: `4de2acdd3b18e562f129bc488a2daea2da16b00341497304423e1e18c49a83ba`.
Native helper local-copy ABI and native moderation inputs are independently disassembled. The implementation and exact wire/lifecycle contract are in `modules/enshrouded/control/README.md`. Each native call returns only dispatch evidence; confirmed departure and canonical `bannedAccounts` persistence remain mandatory. Real-account ban, rejected reconnect and unban recovery require separate runtime validation.
