use crate::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        cursor::Cursor,
        cursor_over::CursorOver,
        direction::Direction,
        overlay::{Folders, MusicDirError, Overlay, Subfolder, Subfolders, TextEntry},
        revision::Revision,
        workspace::Workspace,
    },
    message::TextRequest,
    update::machine::{Unhandled, replace},
};

pub(crate) fn listing(
    text_entry: &TextEntry<MusicDirError>,
    folders: &mut CursorOver<Folders>,
    revision: Revision,
) -> Cmd {
    let Some((path, _)) = text_entry.folder() else {
        *folders = CursorOver::default();
        return Cmd::none();
    };
    let cmd = if folders.content.path == path {
        Cmd::none()
    } else {
        folders.content.revision = Some(revision);
        folders.content.path.clone_from(&path);
        Effect::Library(LibraryCmd::Subfolders { path, revision }).into()
    };
    refilter(text_entry, folders);
    cmd
}

pub(crate) fn listed(
    workspace: &mut Workspace,
    subfolders: Subfolders,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    let Some(Overlay::MusicDir {
        text_entry,
        folders,
        ..
    }) = workspace.overlay.as_mut()
    else {
        return Err(Unhandled);
    };
    if folders.content.revision != Some(revision) {
        return Err(Unhandled);
    }
    folders.content.revision = None;
    folders.content.subfolders = Some(subfolders);
    refilter(text_entry, folders);
    Ok(Cmd::none())
}

pub(crate) fn navigate(
    folders: &mut CursorOver<Folders>,
    direction: Direction,
) -> Result<Cmd, Unhandled> {
    let moved = folders.cursor.step(direction.sign());
    replace(&mut folders.cursor, moved).map(|()| Cmd::none())
}

pub(crate) fn stepped(
    overlay: Option<&Overlay>,
    direction: Direction,
) -> Option<String> {
    let Some(Overlay::MusicDir {
        text_entry,
        folders,
        ..
    }) = overlay
    else {
        return None;
    };
    match direction {
        Direction::Next => completed(folders),
        Direction::Previous => parent(text_entry),
    }
}

pub(crate) fn backspaced(
    overlay: Option<&Overlay>,
    text_request: TextRequest,
) -> Option<String> {
    let Some(Overlay::MusicDir { text_entry, .. }) = overlay else {
        return None;
    };
    match text_request {
        TextRequest::Backspace if text_entry.input.trim_end().ends_with('/') => {
            parent(text_entry)
        }
        TextRequest::Char(_)
        | TextRequest::Backspace
        | TextRequest::DeleteWord
        | TextRequest::Clear => None,
    }
}

fn completed(folders: &CursorOver<Folders>) -> Option<String> {
    let listing = folders.content.subfolders.as_ref()?;
    let index = folders.cursor.get(&folders.content.matches)?;
    let subfolder = listing.subfolders.get(*index)?;
    let path = listing.path.join(subfolder.name());
    Some(format!("{}/", path.to_str()?))
}

fn parent(text_entry: &TextEntry<MusicDirError>) -> Option<String> {
    let (folder, _) = text_entry.folder()?;
    let parent = folder.parent()?.to_str()?;
    match parent {
        "" => None,
        "/" => Some(parent.to_owned()),
        _ => Some(format!("{parent}/")),
    }
}

fn refilter(text_entry: &TextEntry<MusicDirError>, folders: &mut CursorOver<Folders>) {
    let matches = folders
        .content
        .subfolders
        .as_ref()
        .filter(|_| folders.content.revision.is_none())
        .map_or_else(Vec::new, |listing| {
            let segment = text_entry
                .folder()
                .filter(|(path, _)| *path == listing.path)
                .map_or("", |(_, segment)| segment);
            matching(&listing.subfolders, segment)
        });
    folders.cursor = Cursor::new(matches.len());
    folders.content.matches = matches;
}

fn matching(subfolders: &[Subfolder], segment: &str) -> Vec<usize> {
    let needle = segment.to_lowercase();
    let names: Vec<(usize, String)> = subfolders
        .iter()
        .enumerate()
        .filter(|(_, subfolder)| {
            needle.starts_with('.') || !subfolder.name().starts_with('.')
        })
        .map(|(index, subfolder)| (index, subfolder.name().to_lowercase()))
        .collect();
    let prefixed = names
        .iter()
        .filter(|(_, name)| name.starts_with(&needle))
        .map(|(index, _)| *index);
    let contains = names
        .iter()
        .filter(|(_, name)| !name.starts_with(&needle) && name.contains(&needle))
        .map(|(index, _)| *index);
    prefixed.chain(contains).collect()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{domain::overlay::Subfolder, update::overlay::folders::matching};

    fn entries() -> Vec<Subfolder> {
        [
            "Ambient",
            "Jazz",
            "Rock",
            "progrock",
            "Rockabilly",
            ".cache",
        ]
        .into_iter()
        .map(|name| Subfolder::Plain(name.to_owned()))
        .collect()
    }

    #[rstest]
    #[case::no_segment_keeps_every_folder_in_order("", vec![0, 1, 2, 3, 4])]
    #[case::prefix_matches_come_before_substring_matches("rock", vec![2, 4, 3])]
    #[case::the_filter_ignores_case("JA", vec![1])]
    #[case::a_substring_match_alone("bil", vec![4])]
    #[case::nothing_matches("xyz", vec![])]
    #[case::a_hidden_folder_stays_out_of_a_substring_match("c", vec![2, 3, 4])]
    #[case::a_leading_dot_shows_the_hidden_folders(".", vec![5])]
    fn the_typed_segment_filters_the_subfolders(
        #[case] segment: &str,
        #[case] expected: Vec<usize>,
    ) {
        assert_eq!(matching(&entries(), segment), expected);
    }
}
