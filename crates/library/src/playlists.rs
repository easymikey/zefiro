use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    sync::Arc,
};

use kernel::{
    domain::{
        playlist::{PLAYLIST_EXTENSION, Playlist, PlaylistFileName},
        track::Track,
    },
    message::LibrarySubject,
};

use crate::{dirs::LibraryDirs, error::Error};

fn playlist_path(dirs: &LibraryDirs, name: &PlaylistFileName) -> PathBuf {
    dirs.playlists_dir
        .join(format!("{}{PLAYLIST_EXTENSION}", name.as_str()))
}

pub fn load(dirs: &LibraryDirs, name: &PlaylistFileName) -> Result<Playlist, Error> {
    let path = playlist_path(dirs, name);
    let content = std::fs::read_to_string(&path)
        .map_err(Error::io(LibrarySubject::Playlist, &path))?;
    let tracks = crate::scan::read_tags(&parse(&content, &dirs.playlists_dir)).tracks;
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
        .chain(tracks.iter().filter_map(|track| entry_line(track)))
        .collect()
}

fn entry_line(track: &Track) -> Option<String> {
    let path = track.local_path()?;
    let seconds = track.duration().map_or(0, |duration| duration.as_secs());
    let label = match (&track.tags().artist, &track.tags().title) {
        (Some(artist), Some(title)) => Cow::Owned(format!("{artist} - {title}")),
        _ => Cow::Borrowed(track.display()),
    };
    let label = if label.contains(['\r', '\n']) {
        Cow::Owned(label.replace(['\r', '\n'], " "))
    } else {
        label
    };
    Some(format!("#EXTINF:{seconds},{label}\n{}\n", path.display()))
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
        server::{ServerName, ServerTrackId},
        track::{Tags, Track, TrackSource},
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
    fn a_label_with_a_line_break_parses_back_to_one_track() {
        let text = to_m3u(&[track("/music/a.flac", 10.0, ("Art\r\nist", "Title"))]);

        assert_eq!(
            parse(&text, Path::new("/music")),
            vec![PathBuf::from("/music/a.flac")]
        );
    }

    #[rstest]
    #[case::byte_order_mark("\u{FEFF}#EXTM3U\n/a.flac\n", &["/a.flac"])]
    #[case::relative_entry("#EXTM3U\nsub/b.flac\n", &["/lists/sub/b.flac"])]
    #[case::missing_header(
        include_str!("../tests/fixtures/no_header.m3u"),
        &["/no/header.flac", "/plain/path.mp3"]
    )]
    #[case::comments_and_blank_lines(
        include_str!("../tests/fixtures/comments_and_blanks.m3u"),
        &["/a.flac", "/b.flac"]
    )]
    fn an_entry_resolves_onto_the_playlist_folder(
        #[case] text: &str,
        #[case] expected: &[&str],
    ) {
        assert_eq!(
            parse(text, Path::new("/lists")),
            expected.iter().map(PathBuf::from).collect::<Vec<_>>()
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

    #[rstest]
    #[case::a_server_track_is_left_out(
        vec![
            track("/music/a.flac", 123.0, ("Artist A", "Title A")),
            Arc::new(Track::from(TrackSource::Server {
                server_name: ServerName::new("home"),
                server_track_id: ServerTrackId::new("tr-1"),
            })),
        ],
        "#EXTM3U\n#EXTINF:123,Artist A - Title A\n/music/a.flac\n"
    )]
    #[case::an_untagged_track_goes_under_its_display_title(
        vec![Arc::new(test_support::track_lasting(
            "/no/tags.flac",
            Duration::from_secs(7),
            Tags::default(),
        ))],
        "#EXTM3U\n#EXTINF:7,tags.flac\n/no/tags.flac\n"
    )]
    fn an_m3u_save_writes_only_the_local_lines(
        #[case] tracks: Vec<Arc<Track>>,
        #[case] expected: &str,
    ) {
        assert_eq!(to_m3u(&tracks), expected);
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
        let track = Arc::new(Track::listed(&media_path));

        playlists::save(&dirs, &name("My Mix"), std::slice::from_ref(&track)).unwrap();
        let saved = dirs.playlists_dir.join("My Mix.m3u8");
        assert!(saved.is_file());

        let playlist = playlists::load(&dirs, &name("My Mix")).unwrap();
        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.tracks[0].display(), track.display());
    }
}
