#![forbid(unsafe_code)]

mod cache;
mod dirs;
mod error;
mod execute;
mod favorites;
pub mod files;
mod history;
mod m3u;
mod playlists;
mod record;
mod scan;
mod tags;
#[cfg(any(test, feature = "fixtures"))] pub mod test_support;
mod trash;

pub use crate::{
    cache::CacheMiss,
    dirs::LibraryDirs,
    error::Error,
    execute::{Executed, LibraryWarning, execute},
    playlists::load as load_playlist,
    tags::embedded_cover,
};
