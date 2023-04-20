#![feature(
    async_closure,
    iterator_try_collect,
    iterator_try_reduce,
)]

// These sub-modules are imported first to import their macros.
#[macro_use]
mod lazy_regex;
#[macro_use]
pub mod util;

// The rest of these sub-modules are in alphabetical order.
mod progress_reader;
pub mod dump;
pub mod http;
pub mod slug;
mod temp_dir;
mod user_regex;
pub mod wikitext;

pub use progress_reader::ProgressReader;
pub use temp_dir::TempDir;
pub use user_regex::UserRegex;

pub type Error = anyhow::Error;
pub type Result<T> = std::result::Result<T, Error>;
