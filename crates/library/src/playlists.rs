use std::{borrow::Borrow, sync::Arc};

use kernel::{LibrarySubject, Playlist, Track, playlist::PlaylistFileName};

use crate::{dirs::LibraryDirs, error::Error};

fn playlist_filename(name: &PlaylistFileName) -> String {
    format!("{}.m3u8", name.as_str())
}

pub fn load(dirs: &LibraryDirs, name: &PlaylistFileName) -> Result<Playlist, Error> {
    let path = dirs.playlists_dir.join(playlist_filename(name));
    let content = std::fs::read_to_string(&path)
        .map_err(Error::read(LibrarySubject::Playlist, &path))?;
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
    dirs: &LibraryDirs,
    name: &PlaylistFileName,
    tracks: &[Item],
) -> Result<(), Error> {
    let path = dirs.playlists_dir.join(playlist_filename(name));
    crate::files::create_parent_dir(&path)
        .map_err(Error::write(LibrarySubject::Playlist, &dirs.playlists_dir))?;
    std::fs::write(&path, crate::m3u::to_string(tracks))
        .map_err(Error::write(LibrarySubject::Playlist, &path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use kernel::{Track, playlist::PlaylistFileName};

    use crate::{
        dirs::LibraryDirs,
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
        let dirs = LibraryDirs::under(directory.path());
        std::fs::create_dir_all(&dirs.playlists_dir).unwrap();
        let media_path = directory.path().join("loaded-from-m3u.mp3");
        std::fs::write(
            dirs.playlists_dir.join("display.m3u8"),
            format!("#EXTM3U\n{}\n", media_path.display()),
        )
        .unwrap();

        let playlist = playlists::load(&dirs, &name("display")).unwrap();
        let track = playlist.tracks.first().unwrap();
        let expected_track = Track::listed(&media_path);
        let expected = expected_track.display();

        assert!(!track.display().is_empty());
        assert_eq!(track.display(), expected);
    }

    #[test]
    fn save_then_load_round_trips_under_the_validated_name() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs::under(directory.path());
        let media_path = directory.path().join("roundtrip.mp3");
        let track = Track::listed(&media_path);

        playlists::save(&dirs, &name("My Mix"), &[track]).unwrap();
        let saved = dirs.playlists_dir.join("My Mix.m3u8");
        assert!(saved.is_file());

        let playlist = playlists::load(&dirs, &name("My Mix")).unwrap();
        assert_eq!(playlist.tracks.len(), 1);
    }
}
