use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::{OnceCell, Semaphore};

use super::{extract_u32_field, extract_u64_field, read_workshop_response};

const TYPE_CACHE_LIMIT: usize = 1024;
const TYPE_CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const TYPE_LOOKUP_CONCURRENCY: usize = 4;
const TYPE_LOOKUP_QUEUE_LIMIT: usize = 1024;
const TYPE_LOOKUP_BUDGET: Duration = Duration::from_secs(15);

type SharedTypeLookup = OnceCell<Result<u32, String>>;

struct TypeLookupCoordinator {
    pending: Mutex<HashMap<String, Weak<SharedTypeLookup>>>,
    permit: Semaphore,
}

impl TypeLookupCoordinator {
    fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            // All windows and lookup batches share one Community HTML request.
            permit: Semaphore::new(1),
        }
    }

    fn pending_lookup(&self, id: &str) -> Result<Arc<SharedTypeLookup>, String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| String::from("Workshop type lookup queue is unavailable."))?;
        pending.retain(|_, lookup| lookup.strong_count() > 0);
        if let Some(lookup) = pending.get(id).and_then(Weak::upgrade) {
            return Ok(lookup);
        }
        if pending.len() >= TYPE_LOOKUP_QUEUE_LIMIT {
            return Err(String::from(
                "Too many Workshop type lookups are pending. Please retry.",
            ));
        }
        let lookup = Arc::new(OnceCell::new());
        pending.insert(id.to_string(), Arc::downgrade(&lookup));
        Ok(lookup)
    }

    async fn resolve<F, Fut>(&self, id: &str, fetch: F) -> Result<u32, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<u32, String>>,
    {
        let lookup = self.pending_lookup(id)?;
        lookup
            .get_or_init(|| async {
                let _permit = self
                    .permit
                    .acquire()
                    .await
                    .map_err(|_| String::from("Workshop type lookup queue is unavailable."))?;
                // A browse response or another batch may have verified this ID
                // while it waited. Only Steam-supplied type evidence is reused.
                if let Some(file_type) = cached_file_type(id)? {
                    return Ok(file_type);
                }
                let file_type = fetch().await?;
                remember_file_type(id, file_type)?;
                Ok(file_type)
            })
            .await
            .clone()
    }
}

fn lookup_coordinator() -> &'static TypeLookupCoordinator {
    static COORDINATOR: OnceLock<TypeLookupCoordinator> = OnceLock::new();
    COORDINATOR.get_or_init(TypeLookupCoordinator::new)
}

struct TypeCacheEntry {
    id: String,
    file_type: u32,
    expires_at: Instant,
}

fn type_cache() -> &'static Mutex<VecDeque<TypeCacheEntry>> {
    static CACHE: OnceLock<Mutex<VecDeque<TypeCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(VecDeque::new()))
}

pub(super) fn cached_file_type(id: &str) -> Result<Option<u32>, String> {
    let cache = type_cache()
        .lock()
        .map_err(|_| String::from("Workshop type cache is unavailable."))?;
    Ok(cache
        .iter()
        .find(|entry| entry.id == id && entry.expires_at > Instant::now())
        .map(|entry| entry.file_type))
}

pub(super) fn remember_file_type(id: &str, file_type: u32) -> Result<(), String> {
    let mut cache = type_cache()
        .lock()
        .map_err(|_| String::from("Workshop type cache is unavailable."))?;
    let now = Instant::now();
    cache.retain(|entry| entry.id != id && entry.expires_at > now);
    while cache.len() >= TYPE_CACHE_LIMIT {
        cache.pop_front();
    }
    cache.push_back(TypeCacheEntry {
        id: id.to_string(),
        file_type,
        expires_at: now + TYPE_CACHE_TTL,
    });
    Ok(())
}

pub(super) fn item_kind(file_type: Option<u32>) -> &'static str {
    // Steam's EWorkshopFileType: Community=0, Collection=2, WebGuide=9,
    // IntegratedGuide=10, GameManagedItem=15. Other shared files are not game UGC.
    match file_type {
        Some(0 | 15) => "item",
        Some(2) => "collection",
        Some(9 | 10) => "guide",
        None => "unknown",
        _ => "unsupported",
    }
}

pub(super) async fn populate_file_types(
    client: &reqwest::Client,
    details: &mut HashMap<String, Value>,
    collections: &HashMap<String, Vec<String>>,
    preference: app_network::SourcePreference,
    html_error: Option<&str>,
) -> Result<HashMap<String, String>, String> {
    let mut missing_ids = Vec::new();
    for (id, detail) in details.iter_mut() {
        if extract_u64_field(detail, "result") != Some(1) {
            continue;
        }
        let file_type = if let Some(file_type) = extract_u32_field(detail, "file_type") {
            // Current response metadata takes precedence over cached evidence.
            remember_file_type(id, file_type)?;
            Some(file_type)
        } else if collections.contains_key(id) {
            Some(2)
        } else {
            cached_file_type(id)?
        };
        if let Some(file_type) = file_type {
            detail["file_type"] = Value::from(file_type);
        } else {
            missing_ids.push(id.clone());
        }
    }
    // A failed details page already exhausted this request's Community attempt.
    // Reuse authoritative API/cache evidence, but never retry its HTML restriction
    // via root or child type lookups while returning the retained metadata.
    if let Some(error) = html_error {
        return Ok(missing_ids
            .into_iter()
            .map(|id| (id, error.to_string()))
            .collect());
    }
    populate_missing_types(
        details,
        missing_ids,
        tokio::time::sleep(TYPE_LOOKUP_BUDGET),
        |id| {
            let client = client.clone();
            async move {
                lookup_coordinator()
                    .resolve(&id, || fetch_file_type(&client, &id, preference))
                    .await
            }
        },
    )
    .await
}

async fn populate_missing_types<F, Fut, Deadline>(
    details: &mut HashMap<String, Value>,
    missing_ids: Vec<String>,
    deadline: Deadline,
    fetch: F,
) -> Result<HashMap<String, String>, String>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<u32, String>> + Send + 'static,
    Deadline: Future<Output = ()>,
{
    // GetPublishedFileDetails can omit file_type. Bound the whole optional HTML
    // phase, including time queued behind other calls, and retain useful metadata.
    // Workers belong to this call; OnceCell lets waiters resume cancelled work.
    let mut pending = missing_ids.iter().cloned().collect::<HashSet<_>>();
    let mut ids = missing_ids.into_iter();
    let mut workers = tokio::task::JoinSet::new();
    let mut errors = HashMap::new();
    for _ in 0..TYPE_LOOKUP_CONCURRENCY {
        if let Some(id) = ids.next() {
            spawn_type_lookup(&mut workers, id, &fetch);
        }
    }
    tokio::pin!(deadline);
    let timed_out = loop {
        let result = tokio::select! {
            biased;
            () = &mut deadline => break true,
            result = workers.join_next() => result,
        };
        let Some(result) = result else { break false };
        let (id, result) =
            result.map_err(|error| format!("Workshop type lookup worker failed: {error}"))?;
        pending.remove(&id);
        apply_type_result(details, &mut errors, id, result);
        if let Some(id) = ids.next() {
            spawn_type_lookup(&mut workers, id, &fetch);
        }
    };
    if timed_out {
        workers.abort_all();
        while let Some(result) = workers.join_next().await {
            match result {
                // Preserve completed responses that had not yet been consumed.
                Ok((id, result)) => {
                    pending.remove(&id);
                    apply_type_result(details, &mut errors, id, result);
                }
                Err(error) if error.is_cancelled() => {}
                Err(error) => {
                    return Err(format!("Workshop type lookup worker failed: {error}"));
                }
            }
        }
        let error = super::network_error::workshop_network_error(
            "item_type",
            &app_network::NetworkError::Deadline {
                attempts: 0,
                origin: String::from("https://steamcommunity.com"),
            },
        );
        for id in pending {
            errors.insert(id, error.clone());
        }
    }
    Ok(errors)
}

fn apply_type_result(
    details: &mut HashMap<String, Value>,
    errors: &mut HashMap<String, String>,
    id: String,
    result: Result<u32, String>,
) {
    match result {
        Ok(file_type) => {
            if let Some(detail) = details.get_mut(&id) {
                detail["file_type"] = Value::from(file_type);
            }
        }
        Err(error) => {
            errors.insert(id, error);
        }
    }
}

fn spawn_type_lookup<F, Fut>(
    workers: &mut tokio::task::JoinSet<(String, Result<u32, String>)>,
    id: String,
    fetch: &F,
) where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Result<u32, String>> + Send + 'static,
{
    let request = fetch(id.clone());
    workers.spawn(async move {
        let result = request.await;
        (id, result)
    });
}

async fn fetch_file_type(
    client: &reqwest::Client,
    id: &str,
    preference: app_network::SourcePreference,
) -> Result<u32, String> {
    let request = client
        .get("https://steamcommunity.com/sharedfiles/filedetails/")
        .query(&[("id", id), ("l", "english")])
        .build()
        .map_err(|error| format!("failed to prepare Workshop type request: {error}"))?;
    let response = read_workshop_response(client, request, preference).await?;
    let html = std::str::from_utf8(&response.bytes)
        .map_err(|error| format!("Workshop item type response was not UTF-8: {error}"))?;
    let file_type = parse_file_type(html, id)?;
    app_network::record_success(response.url.as_str());
    Ok(file_type)
}

pub(super) fn parse_file_type(html: &str, id: &str) -> Result<u32, String> {
    // Only read a native action attribute for this exact item. Description text
    // containing the function's name is not type evidence and is never evaluated.
    for marker in [
        "onClick=\"PublishedFileAward(",
        "onclick=\"PublishedFileAward(",
    ] {
        for rest in html.split(marker).skip(1) {
            let Some((action_id, arguments)) = rest.split_once(',') else {
                continue;
            };
            if action_id.trim().trim_matches('\'') != id {
                continue;
            }
            let digits = arguments
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            if let Ok(file_type) = digits.parse::<u32>() {
                return Ok(file_type);
            }
        }
    }
    Err(format!(
        "Steam did not provide a verifiable Workshop item type for {id}. Please retry or inspect the item in Steam."
    ))
}

#[cfg(test)]
#[path = "file_types_tests.rs"]
mod tests;
