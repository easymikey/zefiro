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
mod scan;
mod tags;
mod trash;

pub use crate::{
    error::{LibraryError, Subject},
    execute::execute,
    paths::LibraryPaths,
    playlists::load as load_playlist,
    tags::embedded_cover,
};
