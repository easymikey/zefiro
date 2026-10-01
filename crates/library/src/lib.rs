#![forbid(unsafe_code)]

mod cache;
mod dirs;
mod error;
mod execute;
mod favorites;
pub mod files;
mod history;
mod playlists;
mod scan;
mod tags;
#[cfg(test)] mod test_support;
mod trash;

pub use crate::{
    dirs::LibraryDirs,
    error::Error,
    execute::execute,
    playlists::load as load_playlist,
    tags::embedded_cover,
};
