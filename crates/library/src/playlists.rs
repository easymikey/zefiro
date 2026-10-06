use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    domain::{
        playlist::{Playlist, PlaylistFileName},
        track::Track,
    },
    message::LibrarySubject,
};

use crate::{dirs::LibraryDirs, error::Error};

fn playlist_path(dirs: &LibraryDirs, name: &PlaylistFileName) -> PathBuf {
    dirs.playlists_dir.join(format!("{}.m3u8", name.as_str()))
}

pub fn load(dirs: &LibraryDirs, name: &PlaylistFileName) -> Result<Playlist, Error> {
    let path = playlist_path(dirs, name);
    let content = std::fs::read_to_string(&path)
        .map_err(Error::io(LibrarySubject::Playlist, &path))?;
    let track_paths = parse(&content, &dirs.playlists_dir);
    let tracks: Vec<Arc<Track>> = track_paths
        .iter()
        .map(|track_path| crate::tags::read_or_list(track_path))
        .collect();
    Ok(Playlist::from_tracks(tracks))
}

pub(crate) fn save(
    dirs: &LibraryDirs,
    name: &PlaylistFileName,
    tracks: &[Arc<Track>],
) -> Result<(), Error> {
    let path = playlist_path(dirs, name);
    crate::files::create_parent_dir(&path)
        .map_err(Error::io(LibrarySubject::Playlist, &dirs.playlists_dir))?;
    crate::files::write_atomic(&path, to_m3u(tracks).as_bytes())
        .map_err(Error::io(LibrarySubject::Playlist, &path))?;
    Ok(())
}

fn to_m3u(tracks: &[Arc<Track>]) -> String {
    std::iter::once("#EXTM3U\n".to_string())
        .chain(tracks.iter().map(|track| entry_line(track)))
        .collect()
}

fn entry_line(track: &Track) -> String {
    let seconds = track.duration().map_or(0, |duration| duration.as_secs());
    let label = match (&track.tags().artist, &track.tags().title) {
        (Some(artist), Some(title)) => format!("{artist} - {title}"),
        _ => track.display().to_string(),
    };
    format!("#EXTINF:{seconds},{label}\n{}\n", track.path().display())
}

#[must_use]
fn parse(text: &str, base_dir: &Path) -> Vec<PathBuf> {
    text.trim_start_matches('\u{FEFF}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('#'))
        .map(|line| base_dir.join(line))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        time::Duration,
    };

    use kernel::domain::{
        playlist::PlaylistFileName,
        track::{Tags, Track},
    };
    use rstest::rstest;

    use crate::{
        dirs::LibraryDirs,
        playlists::{self, parse, to_m3u},
        test_support,
    };

    fn name(input: &str) -> PlaylistFileName {
        PlaylistFileName::new(input).unwrap()
    }

    fn track(path: &str, seconds: f64, artist_title: (&str, &str)) -> Arc<Track> {
        let (artist, title) = artist_title;
        Arc::new(test_support::track_lasting(
            path,
            Duration::from_secs_f64(seconds),
            Tags {
                artist: Some(artist.to_string()),
                title: Some(title.to_string()),
                ..Tags::default()
            },
        ))
    }

    #[test]
    fn a_written_playlist_parses_back_to_the_same_tracks() {
        let tracks = vec![
            track("/music/a.flac", 123.0, ("Artist A", "Title A")),
            track("/music/b.mp3", 45.0, ("Artist B", "Title B")),
        ];
        let text = to_m3u(&tracks);
        insta::assert_snapshot!(text);
        insta::assert_debug_snapshot!(parse(&text, Path::new("/music")));
    }

    #[rstest]
    #[case::missing_header(
        "missing_header",
        include_str!("../tests/fixtures/no_header.m3u")
    )]
    #[case::comments_and_blank_lines(
        "comments_and_blank_lines",
        include_str!("../tests/fixtures/comments_and_blanks.m3u")
    )]
    fn parsing_reads_the_tracks_each_line_layout_names(
        #[case] name: &str,
        #[case] text: &str,
    ) {
        insta::assert_debug_snapshot!(name, parse(text, Path::new("/music")));
    }

    #[rstest]
    #[case::byte_order_mark("\u{FEFF}#EXTM3U\n/a.flac\n", "/a.flac")]
    #[case::relative_entry("#EXTM3U\nsub/b.flac\n", "/lists/sub/b.flac")]
    #[case::absolute_entry("#EXTM3U\n/c.flac\n", "/c.flac")]
    fn an_entry_resolves_onto_the_playlist_folder(
        #[case] text: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(
            parse(text, Path::new("/lists")),
            vec![PathBuf::from(expected)]
        );
    }

    #[test]
    fn saving_replaces_a_read_only_playlist_file() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        let track = Arc::new(Track::listed(&directory.path().join("song.mp3")));
        playlists::save(&dirs, &name("Locked"), std::slice::from_ref(&track)).unwrap();
        let saved = dirs.playlists_dir.join("Locked.m3u8");
        let mut permissions = std::fs::metadata(&saved).unwrap().permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(&saved, permissions).unwrap();

        playlists::save(&dirs, &name("Locked"), &[track]).unwrap();
    }

    #[test]
    fn an_untagged_track_is_written_under_its_display_title() {
        let tracks = vec![Arc::new(test_support::track_lasting(
            "/no/tags.flac",
            Duration::from_secs(7),
            Tags::default(),
        ))];
        insta::assert_snapshot!(to_m3u(&tracks));
    }

    #[test]
    fn named_playlist_load_computes_track_display() {
        let directory = tempfile::tempdir().unwrap();
        let dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
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
        let dirs = LibraryDirs {
            cache_dir: directory.path().join("cache"),
            data_dir: directory.path().join("data"),
            playlists_dir: directory.path().join("playlists"),
        };
        let media_path = directory.path().join("roundtrip.mp3");
        let track = Track::listed(&media_path);

        playlists::save(&dirs, &name("My Mix"), &[Arc::new(track)]).unwrap();
        let saved = dirs.playlists_dir.join("My Mix.m3u8");
        assert!(saved.is_file());

        let playlist = playlists::load(&dirs, &name("My Mix")).unwrap();
        assert_eq!(playlist.tracks.len(), 1);
    }
}
