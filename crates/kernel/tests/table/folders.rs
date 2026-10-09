use std::path::PathBuf;

use kernel::{
    cmd::{Cmd, Effect, LibraryCmd},
    domain::{
        direction::Direction,
        model::Model,
        overlay::{Overlay, OverlayName, Subfolder, Subfolders, Verdict},
        revision::Revision,
        time::Moment,
    },
    message::{LibraryEvent, Message, OverlayRequest, TextRequest},
    update::machine::Unhandled,
};
use rstest::rstest;

use crate::support::update::{send, update};

fn listings(cmd: &Cmd) -> Vec<(PathBuf, Revision)> {
    cmd.effects()
        .filter_map(|effect| {
            if let Effect::Library(LibraryCmd::Subfolders { path, revision }) = effect {
                Some((path.clone(), *revision))
            } else {
                None
            }
        })
        .collect()
}

fn asked(model: &mut Model, request: OverlayRequest) -> Vec<(PathBuf, Revision)> {
    listings(&update(model, Message::Overlay(request), Moment::default()).unwrap())
}

fn opened(input: &str) -> (Model, Revision) {
    let mut model = Model {
        music_dir: input.into(),
        ..Model::default()
    };
    let listed = asked(&mut model, OverlayRequest::Open(OverlayName::MusicDir));
    assert_eq!(listed.len(), 1, "opening the prompt lists one folder");
    let (_, revision) = listed[0].clone();
    (model, revision)
}

fn subfolders(verdict: Verdict, names: &[&str]) -> Subfolders {
    Subfolders {
        path: PathBuf::from("/m"),
        verdict,
        subfolders: names
            .iter()
            .map(|name| Subfolder::Plain((*name).to_owned()))
            .collect(),
    }
}

const NAMES: [&str; 5] = ["Ambient", "Jazz", "progrock", "Rock", "Rockabilly"];

fn answered(
    model: &mut Model,
    subfolders: Subfolders,
    revision: Revision,
) -> Result<Cmd, Unhandled> {
    update(
        model,
        Message::Library(LibraryEvent::Subfolders {
            subfolders,
            revision,
        }),
        Moment::default(),
    )
}

fn listed(input: &str) -> Model {
    let (mut model, revision) = opened(input);
    assert_eq!(
        answered(&mut model, subfolders(Verdict::Readable, &NAMES), revision),
        Ok(Cmd::none())
    );
    model
}

fn shown(model: &Model) -> Vec<&str> {
    let Some(Overlay::MusicDir { folders, .. }) = &model.workspace.overlay else {
        return Vec::new();
    };
    let Some(listing) = &folders.content.subfolders else {
        return Vec::new();
    };
    folders
        .content
        .matches
        .iter()
        .map(|index| listing.subfolders[*index].name())
        .collect()
}

fn selected(model: &Model) -> Option<&str> {
    let Some(Overlay::MusicDir { folders, .. }) = &model.workspace.overlay else {
        return None;
    };
    let index = folders.cursor.get(&folders.content.matches)?;
    Some(
        folders
            .content
            .subfolders
            .as_ref()?
            .subfolders
            .get(*index)?
            .name(),
    )
}

fn input(model: &Model) -> Option<&str> {
    if let Some(Overlay::MusicDir { text_entry, .. }) = &model.workspace.overlay {
        Some(text_entry.input.as_str())
    } else {
        None
    }
}

fn folders_of(listed: &[(PathBuf, Revision)]) -> Vec<PathBuf> {
    listed.iter().map(|(folder, _)| folder.clone()).collect()
}

#[rstest]
#[case::every_subfolder_after_a_slash("/m/", &NAMES)]
#[case::prefix_matches_first_then_substring_matches(
    "/m/ro",
    &["Rock", "Rockabilly", "progrock"]
)]
#[case::the_filter_ignores_case("/m/RO", &["Rock", "Rockabilly", "progrock"])]
#[case::a_substring_match_alone("/m/bil", &["Rockabilly"])]
#[case::nothing_matches("/m/xyz", &[])]
fn the_typed_segment_filters_the_listed_subfolders(
    #[case] typed: &str,
    #[case] expected: &[&str],
) {
    assert_eq!(shown(&listed(typed)), expected);
}

#[test]
fn typing_in_the_last_segment_refilters_without_listing_the_folder_again() {
    let mut model = listed("/m/r");
    let listed = asked(&mut model, OverlayRequest::Text(TextRequest::Char('o')));
    assert_eq!(folders_of(&listed), Vec::<PathBuf>::new());
    assert_eq!(shown(&model), ["Rock", "Rockabilly", "progrock"]);
    assert_eq!(
        asked(&mut model, OverlayRequest::Text(TextRequest::Backspace)),
        Vec::new()
    );
    assert_eq!(input(&model), Some("/m/r"));
}

#[test]
fn a_second_tab_before_the_new_listing_answers_is_refused() {
    let mut model = listed("/m/ro");
    asked(&mut model, OverlayRequest::Step(Direction::Next));
    assert_eq!(input(&model), Some("/m/Rock/"));
    assert_eq!(shown(&model), Vec::<&str>::new());
    assert_eq!(
        update(
            &mut model,
            Message::Overlay(OverlayRequest::Step(Direction::Next)),
            Moment::default(),
        ),
        Err(Unhandled)
    );
    assert_eq!(input(&model), Some("/m/Rock/"));
}

#[test]
fn down_and_up_move_the_selection() {
    let mut model = listed("/m/");
    assert_eq!(selected(&model), Some("Ambient"));
    send(
        &mut model,
        Message::Overlay(OverlayRequest::Navigate(Direction::Next)),
    );
    send(
        &mut model,
        Message::Overlay(OverlayRequest::Navigate(Direction::Next)),
    );
    assert_eq!(selected(&model), Some("progrock"));
    send(
        &mut model,
        Message::Overlay(OverlayRequest::Navigate(Direction::Previous)),
    );
    assert_eq!(selected(&model), Some("Jazz"));
}

#[rstest]
#[case::the_first_match(0, "/m/Rock/")]
#[case::the_match_under_the_cursor(1, "/m/Rockabilly/")]
fn tab_completes_the_selected_subfolder_with_a_trailing_slash_and_lists_it(
    #[case] downs: usize,
    #[case] expected: &str,
) {
    let mut model = listed("/m/ro");
    for _ in 0..downs {
        send(
            &mut model,
            Message::Overlay(OverlayRequest::Navigate(Direction::Next)),
        );
    }
    let listed = asked(&mut model, OverlayRequest::Step(Direction::Next));
    assert_eq!(input(&model), Some(expected));
    assert_eq!(
        folders_of(&listed),
        vec![PathBuf::from(expected.trim_end_matches('/'))]
    );
}

#[rstest]
#[case::without_a_listing(None)]
#[case::without_a_match(Some(Verdict::Readable))]
#[case::in_an_unreadable_folder(Some(Verdict::Denied))]
fn tab_without_a_selected_subfolder_is_refused(
    #[case] listing_verdict: Option<Verdict>,
) {
    let (mut model, revision) = opened("/m/xyz");
    if let Some(verdict) = listing_verdict {
        let names: &[&str] = match verdict {
            Verdict::Readable => &NAMES,
            Verdict::Missing
            | Verdict::NotADirectory
            | Verdict::Denied
            | Verdict::Unreadable(_) => &[],
        };
        assert!(answered(&mut model, subfolders(verdict, names), revision).is_ok());
    }
    assert_eq!(
        update(
            &mut model,
            Message::Overlay(OverlayRequest::Step(Direction::Next)),
            Moment::default(),
        ),
        Err(Unhandled)
    );
    assert_eq!(input(&model), Some("/m/xyz"));
}

#[rstest]
#[case::left_from_a_completed_folder(
    OverlayRequest::Step(Direction::Previous),
    "/m/Rock/",
    "/m/"
)]
#[case::left_while_typing_a_name(
    OverlayRequest::Step(Direction::Previous),
    "/m/Rock/ja",
    "/m/"
)]
#[case::left_up_to_the_root(OverlayRequest::Step(Direction::Previous), "/m/", "/")]
#[case::backspace_right_after_a_slash(
    OverlayRequest::Text(TextRequest::Backspace),
    "/m/Rock/",
    "/m/"
)]
fn going_up_one_level_lists_the_parent(
    #[case] request: OverlayRequest,
    #[case] typed: &str,
    #[case] expected: &str,
) {
    let (mut model, _) = opened(typed);
    let listed = asked(&mut model, request);
    assert_eq!(input(&model), Some(expected));
    let (folder, _) = expected.rsplit_once('/').unwrap();
    assert_eq!(
        folders_of(&listed),
        vec![PathBuf::from(if folder.is_empty() { "/" } else { folder })]
    );
}

#[test]
fn left_at_the_root_is_refused() {
    let (mut model, _) = opened("/");
    assert_eq!(
        update(
            &mut model,
            Message::Overlay(OverlayRequest::Step(Direction::Previous)),
            Moment::default(),
        ),
        Err(Unhandled)
    );
}

#[test]
fn a_stale_listing_is_dropped_and_the_fresh_one_is_shown() {
    let (mut model, stale) = opened("/m");
    let fresh = asked(&mut model, OverlayRequest::Text(TextRequest::Char('/')));
    assert_eq!(
        answered(&mut model, subfolders(Verdict::Readable, &NAMES), stale),
        Err(Unhandled)
    );
    assert_eq!(shown(&model), Vec::<&str>::new());
    assert_eq!(
        answered(
            &mut model,
            subfolders(Verdict::Readable, &NAMES),
            fresh[0].1
        ),
        Ok(Cmd::none())
    );
    assert_eq!(shown(&model), NAMES);
}

#[test]
fn a_listing_without_the_prompt_is_refused() {
    let mut model = Model::default();
    assert_eq!(
        answered(
            &mut model,
            subfolders(Verdict::Readable, &NAMES),
            Revision::default()
        ),
        Err(Unhandled)
    );
}
