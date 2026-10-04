#![forbid(unsafe_code)]

mod cache;
pub mod cover;
pub mod dirs;
pub mod driver;
pub mod error;
mod execute;
mod favorites;
pub mod files;
mod history;
pub mod job;
pub mod playlists;
mod scan;
pub mod tags;
#[cfg(test)]
#[path = "../tests/unit/fixtures.rs"]
mod test_support;
mod trash;
mod watch;
