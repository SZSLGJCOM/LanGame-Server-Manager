//! Bounded requests and equivalent, publisher-operated public download sources.
//! Credentials and game-server control requests do not belong in this module.

pub mod media_cache;
mod policy;
mod request;

pub use policy::{SourcePreference, official_url_candidates, record_failure, record_success};
pub use request::{
    NetworkError, PublicBytes, read_public_bytes, read_public_bytes_from_source, send_read_only,
    send_with_retry,
};
