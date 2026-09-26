#![forbid(unsafe_code)]

mod cache;
mod error;
mod execute;
mod favorites;
pub mod files;
mod history;
mod m3u;
mod paths;
mod playlists;
mod record;
mod scan;
mod tags;
mod trash;

pub use crate::{
    cache::CacheMiss,
    error::LibraryError,
    execute::{Executed, LibraryNote, execute},
    paths::LibraryPaths,
    playlists::load as load_playlist,
    tags::embedded_cover,
};
