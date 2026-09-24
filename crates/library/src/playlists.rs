use std::{borrow::Borrow, sync::Arc};

use kernel::{Playlist, Track, playlist::PlaylistFileName};

use crate::{
    error::{LibraryError, Subject},
    paths::LibraryPaths,
};

fn playlist_filename(name: &PlaylistFileName) -> String {
    format!("{}.m3u8", name.as_str())
}

pub fn load(
    paths: &LibraryPaths,
    name: &PlaylistFileName,
) -> Result<Playlist, LibraryError> {
    let path = paths.playlists.join(playlist_filename(name));
    let content =
        std::fs::read_to_string(&path).map_err(|source| LibraryError::Read {
            subject: Subject::Playlist,
            path: path.clone(),
            source,
        })?;
    let track_paths = crate::m3u::parse(&content);
    let tracks: Vec<Arc<Track>> = track_paths
        .iter()
        .map(|track_path| {
            crate::tags::read_track(track_path)
                .unwrap_or_else(|_| Track::listed(track_path))
        })
        .map(Arc::new)
        .collect();
    Ok(Playlist::from_tracks(tracks))
}

pub(crate) fn save<Item: Borrow<Track>>(
    paths: &LibraryPaths,
    name: &PlaylistFileName,
    tracks: &[Item],
) -> Result<(), LibraryError> {
    let path = paths.playlists.join(playlist_filename(name));
    crate::files::create_parent(&path).map_err(|source| LibraryError::Write {
        subject: Subject::Playlist,
        path: paths.playlists.clone(),
        source,
    })?;
    std::fs::write(&path, crate::m3u::to_string(tracks)).map_err(|source| {
        LibraryError::Write {
            subject: Subject::Playlist,
            path: path.clone(),
            source,
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use kernel::{Track, playlist::PlaylistFileName};

    use crate::{
        paths,
        playlists::{self, playlist_filename},
    };

    fn name(input: &str) -> PlaylistFileName {
        PlaylistFileName::new(input).unwrap()
    }

    #[test]
    fn playlist_filename_appends_the_extension() {
        assert_eq!(playlist_filename(&name("My Mix")), "My Mix.m3u8");
    }

    #[test]
    fn named_playlist_load_computes_track_display() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());
        std::fs::create_dir_all(&library_paths.playlists).unwrap();
        let media_path = directory.path().join("loaded-from-m3u.mp3");
        std::fs::write(
            library_paths.playlists.join("display.m3u8"),
            format!("#EXTM3U\n{}\n", media_path.display()),
        )
        .unwrap();

        let playlist = playlists::load(&library_paths, &name("display")).unwrap();
        let track = playlist.tracks.first().unwrap();
        let expected_track = Track::listed(&media_path);
        let expected = expected_track.display();

        assert!(!track.display().is_empty());
        assert_eq!(track.display(), expected);
    }

    #[test]
    fn save_then_load_round_trips_under_the_validated_name() {
        let directory = tempfile::tempdir().unwrap();
        let library_paths = paths::stub(directory.path());
        let media_path = directory.path().join("roundtrip.mp3");
        let track = Track::listed(&media_path);

        playlists::save(&library_paths, &name("My Mix"), &[track]).unwrap();
        let saved = library_paths.playlists.join("My Mix.m3u8");
        assert!(saved.is_file());

        let playlist = playlists::load(&library_paths, &name("My Mix")).unwrap();
        assert_eq!(playlist.tracks.len(), 1);
    }
}
