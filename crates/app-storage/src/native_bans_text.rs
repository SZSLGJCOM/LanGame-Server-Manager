use std::collections::BTreeMap;
use std::path::Path;

use app_core::{NativeBanEntry, RuntimePlayerIdentityKind};

use super::native_bans::MAX_ROWS;
use super::native_bans_io::invalid;
use crate::StorageError;

pub(super) fn parse(
    content: &str,
    module: &str,
    path: &Path,
    observed_at_unix_ms: u64,
) -> Result<Vec<NativeBanEntry>, StorageError> {
    let mut accounts = BTreeMap::new();
    let mut rows = 0;
    for line in content.lines() {
        let account = if module == "necesse" {
            // java.lang.String.trim removes code points <= U+0020 only.
            line.trim_matches(|character| character <= '\u{20}')
        } else {
            line.trim()
        };
        if account.is_empty() {
            continue;
        }
        if matches!(module, "arksurvivalascended" | "arksurvivalevolved")
            && account.starts_with('#')
        {
            continue;
        }
        rows += 1;
        if rows > MAX_ROWS {
            return Err(invalid(
                module,
                path,
                "Native ban list exceeds 4096 records.",
            ));
        }
        let mut expires_at_unix_seconds = 0;
        let (account, display_name, identity_kind) = match module {
            "soulmask" => {
                let kind = if is_steam_id(account) {
                    RuntimePlayerIdentityKind::SteamId
                } else if is_soulmask_account_id(account) {
                    RuntimePlayerIdentityKind::SoulmaskAccountId
                } else {
                    return Err(invalid(
                        module,
                        path,
                        "A native ban record is not a Steam64 or complete Soulmask Epic identity.",
                    ));
                };
                (account.to_owned(), None, kind)
            }
            "necesse" => {
                if account.chars().count() > 256 || account.chars().any(char::is_control) {
                    return Err(invalid(
                        module,
                        path,
                        "A native ban record has an unsupported username or authentication identity.",
                    ));
                }
                (
                    account.to_lowercase(),
                    None,
                    RuntimePlayerIdentityKind::NecesseBanId,
                )
            }
            "conanexiles" => {
                // The installed AServerBlacklist writer serializes native ID,
                // ':', the stored display name, then CRLF for every record.
                let (identity, name) = account.split_once(':').ok_or_else(|| {
                    invalid(
                        module,
                        path,
                        "A native blacklist record lacks its account/name separator.",
                    )
                })?;
                if !is_native_token(identity) {
                    return Err(invalid(
                        module,
                        path,
                        "A native blacklist record has an unsupported account identity.",
                    ));
                }
                let name = (!name.trim().is_empty()
                    && name.chars().count() <= 256
                    && !name.chars().any(char::is_control))
                .then(|| name.to_owned());
                (
                    identity.to_owned(),
                    name,
                    RuntimePlayerIdentityKind::ConanUserId,
                )
            }
            "palworld" => {
                // Current native parser accepts a prefixed UserID followed by
                // optional CSV player GUID metadata. A GUID is not a username.
                let mut fields = account.split(',');
                let identity = fields.next().unwrap_or_default();
                if identity.len() > 1 && identity.contains('_') && is_native_token(identity) {
                    (
                        identity.to_owned(),
                        None,
                        RuntimePlayerIdentityKind::PalworldUserId,
                    )
                } else {
                    let guid = fields.next().unwrap_or(identity);
                    let guid = uuid::Uuid::parse_str(guid)
                        .map_err(|_| invalid(module, path, "A native Palworld ban has neither a supported UserID nor a valid player GUID."))?;
                    if guid.is_nil() {
                        return Err(invalid(
                            module,
                            path,
                            "A native Palworld ban has no nonzero player GUID or supported UserID.",
                        ));
                    }
                    (
                        guid.simple().to_string(),
                        None,
                        RuntimePlayerIdentityKind::PalworldPlayerUid,
                    )
                }
            }
            "arksurvivalascended" | "arksurvivalevolved" => {
                // Current native ASA SaveBannedList writes id,name,u32 expiry.
                // ASE loads by splitting at the first comma only; every later
                // comma belongs to the display name, never to an expiry field.
                // ASE ignores rows without a comma in its native loader.
                let (identity, name, expiry) = if module == "arksurvivalascended" {
                    let mut fields = account.split(',');
                    let identity = fields.next().unwrap_or_default();
                    let name = fields.next();
                    // The native loader defaults a missing third field to 0,
                    // and ignores fields after it. Refuse malformed numeric
                    // expirations instead of reproducing _wtoi's silent zero.
                    let expiry = if let Some(expiry) = fields.next() {
                        let expiry = expiry.trim();
                        if expiry.is_empty() || !expiry.bytes().all(|byte| byte.is_ascii_digit()) {
                            return Err(invalid(
                                module,
                                path,
                                "A native ARK ASA ban lacks a valid Unix expiration timestamp.",
                            ));
                        }
                        expiry.parse::<u32>().map_err(|_| invalid(module, path, "A native ARK ASA ban expiration is outside the unsigned 32-bit range."))?
                    } else {
                        0
                    };
                    (identity, name, expiry)
                } else {
                    let Some((identity, name)) = account.split_once(',') else {
                        continue;
                    };
                    (identity, Some(name), 0)
                };
                let identity = identity.trim();
                let name = name
                    .map(str::trim)
                    .filter(|name| {
                        !name.is_empty()
                            && name.chars().count() <= 256
                            && !name.chars().any(char::is_control)
                    })
                    .map(str::to_owned);
                let kind = if is_steam_id(identity) {
                    RuntimePlayerIdentityKind::SteamId
                } else if identity.len() == 19
                    && identity.bytes().all(|byte| byte.is_ascii_digit())
                    && identity.parse::<u64>().is_ok_and(|value| value > 0)
                {
                    RuntimePlayerIdentityKind::ArkAccountId
                } else if identity.len() == 32
                    && identity.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    RuntimePlayerIdentityKind::EosId
                } else {
                    return Err(invalid(
                        module,
                        path,
                        "A native ARK ban has an unsupported account identity or record shape.",
                    ));
                };
                expires_at_unix_seconds = expiry;
                (identity.to_owned(), name, kind)
            }
            _ => {
                return Err(invalid(
                    module,
                    path,
                    "The native ban text format is not verified.",
                ));
            }
        };
        let record = (display_name, identity_kind, expires_at_unix_seconds);
        if matches!(module, "arksurvivalascended" | "arksurvivalevolved") {
            // Native TMap Emplace replaces an earlier record for the same ID.
            // Decide the final record before evaluating ASA expiration.
            accounts.insert(account, record);
        } else {
            accounts.entry(account).or_insert(record);
        }
    }
    Ok(accounts
        .into_iter()
        .filter_map(|(account_id, (display_name, identity_kind, expiry))| {
            // ASA retains the exact expiry second; only now > expiry is
            // expired. Other readers provide zero for a permanent record.
            if expiry != 0 && observed_at_unix_ms / 1000 > u64::from(expiry) {
                return None;
            }
            Some(NativeBanEntry {
                account_id,
                display_name,
                identity_kind,
            })
        })
        .collect())
}

fn is_soulmask_account_id(account: &str) -> bool {
    let Some((epic, product)) = account.split_once('|') else {
        return false;
    };
    [epic, product]
        .into_iter()
        .all(|part| part.len() == 32 && part.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn is_native_token(account: &str) -> bool {
    !account.is_empty()
        && account.len() <= 128
        && account
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) fn is_steam_id(account: &str) -> bool {
    account.len() == 17
        && account.bytes().all(|byte| byte.is_ascii_digit())
        && account.parse::<u64>().is_ok_and(|value| value > 0)
}
