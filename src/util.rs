mod collections;
pub use collections::{IteratorExt, IteratorExtLocal, IteratorExtSend};

pub mod fmt;

pub mod rand;

#[macro_use]
mod try_macros;
