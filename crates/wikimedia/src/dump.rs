//! Operations on Wikimedia article dump archives.

pub mod download;
pub mod local;

mod types;
pub use types::*;

pub fn dump_name_to_wikimedia_url_base(dump: &DumpName) -> Option<String> {
    match &*dump.0 {
        "enwiki" => Some("https://en.wikipedia.org/wiki".to_string()),
        "simplewiki" => Some("https://simple.wikipedia.org/wiki".to_string()),
        _ => None,
    }
}
