//! Shared code for making HTTP requests

use crate::Result;

pub fn client() -> Result<reqwest::Client> {
    Ok(reqwest::ClientBuilder::new()
           .user_agent(concat!(
               env!("CARGO_PKG_NAME"),
               "/",
               env!("CARGO_PKG_VERSION"),
               ))
           .build()?)
}
