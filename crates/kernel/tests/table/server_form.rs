use kernel::{
    cmd::{Effect, RemoteCmd},
    domain::{
        io_error::IoError,
        key::{Key, KeyCode, KeyPress, Modifiers},
        model::Model,
        overlay::{Field, Overlay, ServerPrompt, TextEntry},
        server::{
            Account,
            ApiCode,
            Connection,
            Credential,
            Endpoint,
            EndpointError,
            RemoteError,
            Secret,
            SecretError,
            ServerName,
            ServerStatus,
            UserName,
            UserNameError,
        },
        time::Moment,
    },
    message::{Message, RemoteEvent},
    update::{machine::Unhandled, update},
};
use rstest::rstest;

use crate::table::server_tab::session;

fn model_with(overlay: Overlay) -> Model {
    let mut model = Model::default();
    model.workspace.overlay = Some(overlay);
    model
}

fn press_key(model: &mut Model, key: Key) -> Result<Vec<Effect>, Unhandled> {
    update(
        model,
        Message::Key(KeyPress { key, typed: key }),
        Moment::default(),
    )
}

fn press(model: &mut Model, code: KeyCode) -> Result<Vec<Effect>, Unhandled> {
    press_key(model, Key::plain(code))
}

fn shift_tab() -> Key {
    Key {
        code: KeyCode::Tab,
        modifiers: Modifiers::SHIFT,
    }
}

fn form(field: Field, link: &str, user: &str) -> ServerPrompt {
    ServerPrompt {
        link_text_entry: TextEntry {
            input: link.to_owned(),
            error: None,
        },
        user_text_entry: TextEntry {
            input: user.to_owned(),
            error: None,
        },
        field,
        reached_field: field,
        ..ServerPrompt::default()
    }
}

fn add_server(field: Field, link: &str, user: &str) -> Overlay {
    Overlay::AddServer(form(field, link, user))
}

fn filled(user: &str) -> ServerPrompt {
    ServerPrompt {
        password_text_entry: TextEntry {
            input: "secret".to_owned(),
            error: None,
        },
        ..form(Field::Password, "https://beats.example.com", user)
    }
}

#[rstest]
#[case::tab_moves_from_the_link_to_the_user(
    add_server(Field::Link, "https://other.example.com", ""),
    Key::plain(KeyCode::Tab),
    add_server(Field::User, "https://other.example.com", "")
)]
#[case::down_moves_from_the_user_to_the_password(
    add_server(Field::User, "https://other.example.com", "carol"),
    Key::plain(KeyCode::Down),
    add_server(Field::Password, "https://other.example.com", "carol")
)]
#[case::shift_tab_moves_from_the_password_back_to_the_user_with_its_error(
    add_server(Field::Password, "https://other.example.com", "carol"),
    shift_tab(),
    Overlay::AddServer(ServerPrompt {
        password_text_entry: TextEntry {
            input: String::new(),
            error: Some(SecretError::Empty),
        },
        reached_field: Field::Password,
        ..form(Field::User, "https://other.example.com", "carol")
    })
)]
#[case::up_moves_from_the_user_back_to_the_link(
    add_server(Field::User, "https://other.example.com", "carol"),
    Key::plain(KeyCode::Up),
    Overlay::AddServer(ServerPrompt {
        reached_field: Field::User,
        ..form(Field::Link, "https://other.example.com", "carol")
    })
)]
#[case::enter_on_the_user_moves_on_to_the_password(
    add_server(Field::User, "https://other.example.com", "carol"),
    Key::plain(KeyCode::Enter),
    add_server(Field::Password, "https://other.example.com", "carol")
)]
#[case::leaving_a_bad_link_shows_its_error_under_it(
    add_server(Field::Link, "music.example.com", ""),
    Key::plain(KeyCode::Tab),
    Overlay::AddServer(ServerPrompt {
        link_text_entry: TextEntry {
            input: "music.example.com".to_owned(),
            error: Some(EndpointError::Scheme),
        },
        ..form(Field::User, "", "")
    })
)]
fn a_key_in_the_server_form_moves_between_its_fields(
    #[case] overlay: Overlay,
    #[case] key: Key,
    #[case] expected: Overlay,
) {
    let mut model = model_with(overlay);

    press_key(&mut model, key).unwrap();

    assert_eq!(model.workspace.overlay, Some(expected));
}

#[rstest]
#[case::before_the_link_was_left_typing_shows_no_error(Field::Link, None)]
#[case::once_the_link_was_left_typing_checks_it_live(
    Field::User,
    Some(EndpointError::Scheme)
)]
fn typing_in_the_link_checks_it_live_once_the_link_was_left(
    #[case] reached_field: Field,
    #[case] error: Option<EndpointError>,
) {
    let mut model = model_with(Overlay::AddServer(ServerPrompt {
        reached_field,
        ..form(Field::Link, "", "")
    }));

    press(&mut model, KeyCode::Char('m')).unwrap();

    assert_eq!(
        model.workspace.overlay,
        Some(Overlay::AddServer(ServerPrompt {
            link_text_entry: TextEntry {
                input: "m".to_owned(),
                error,
            },
            reached_field,
            ..form(Field::Link, "", "")
        }))
    );
}

#[rstest]
#[case::up_on_the_link(Field::Link, Key::plain(KeyCode::Up))]
#[case::tab_on_the_password(Field::Password, Key::plain(KeyCode::Tab))]
fn the_server_form_refuses_to_move_past_its_first_and_last_field(
    #[case] field: Field,
    #[case] key: Key,
) {
    let mut model = model_with(add_server(field, "https://other.example.com", "carol"));
    let before = model.clone();

    assert_eq!(press_key(&mut model, key), Err(Unhandled));
    assert_eq!(model, before);
}

#[test]
fn a_repeated_enter_on_the_password_with_the_same_empty_password_is_refused() {
    let mut model = model_with(Overlay::AddServer(ServerPrompt {
        password_text_entry: TextEntry {
            input: String::new(),
            error: Some(SecretError::Empty),
        },
        ..form(Field::Password, "https://beats.example.com", "carol")
    }));
    let before = model.clone();

    assert_eq!(press(&mut model, KeyCode::Enter), Err(Unhandled));
    assert_eq!(model, before);
}

#[test]
fn enter_on_the_typed_user_moves_to_the_password_with_no_error_on_the_password() {
    let mut model = model_with(Overlay::AddServer(ServerPrompt::default()));

    for character in "https://beats.example.com".chars() {
        press(&mut model, KeyCode::Char(character)).unwrap();
    }
    press(&mut model, KeyCode::Enter).unwrap();
    for character in "carol".chars() {
        press(&mut model, KeyCode::Char(character)).unwrap();
    }
    press(&mut model, KeyCode::Enter).unwrap();

    assert_eq!(
        model.workspace.overlay,
        Some(add_server(
            Field::Password,
            "https://beats.example.com",
            "carol"
        ))
    );
}

#[test]
fn enter_on_the_password_connects_with_every_field_and_keeps_the_form_open() {
    let mut model = model_with(Overlay::AddServer(filled("carol")));

    let effects = press(&mut model, KeyCode::Enter).unwrap();

    let endpoint = Endpoint::parse("https://beats.example.com").unwrap();
    assert!(
        effects.contains(&Effect::Remote(RemoteCmd::Connect(Connection {
            account: Account {
                server_name: ServerName::new("beats.example.com"),
                endpoint,
                user_name: UserName::new("carol").unwrap(),
            },
            credential: Credential::Typed(Secret::new("secret").unwrap()),
        })))
    );
    assert_eq!(
        model.workspace.overlay,
        Some(Overlay::AddServer(ServerPrompt {
            origin_server_name: Some(ServerName::new("beats.example.com")),
            server_status: Some(ServerStatus::Connecting),
            ..filled("carol")
        }))
    );
}

#[test]
fn enter_on_the_password_with_an_empty_user_focuses_the_user_with_its_error() {
    let mut model = model_with(Overlay::AddServer(filled("")));

    press(&mut model, KeyCode::Enter).unwrap();

    assert_eq!(model.servers, Vec::new());
    assert_eq!(
        model.workspace.overlay,
        Some(Overlay::AddServer(ServerPrompt {
            field: Field::User,
            user_text_entry: TextEntry {
                input: String::new(),
                error: Some(UserNameError::Empty),
            },
            ..filled("")
        }))
    );
}

#[rstest]
#[case::a_wrong_password_focuses_the_password(
    RemoteError::Api {
        server_name: ServerName::new("beats.example.com"),
        api_code: ApiCode(40),
    },
    Field::Password
)]
#[case::an_unreachable_server_focuses_the_link(
    RemoteError::Unreachable {
        server_name: ServerName::new("beats.example.com"),
        source: IoError::Other,
    },
    Field::Link
)]
fn a_refusal_from_the_server_keeps_the_fields_and_focuses_the_one_it_concerns(
    #[case] remote_error: RemoteError,
    #[case] field: Field,
) {
    let mut model = model_with(Overlay::AddServer(filled("carol")));
    press(&mut model, KeyCode::Enter).unwrap();

    update(
        &mut model,
        Message::Remote(RemoteEvent::Error(remote_error.clone())),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(
        model.workspace.overlay,
        Some(Overlay::AddServer(ServerPrompt {
            field,
            origin_server_name: Some(ServerName::new("beats.example.com")),
            server_status: Some(ServerStatus::Offline(remote_error)),
            ..filled("carol")
        }))
    );
    assert_eq!(model.workspace.toasts, Vec::new());
}

#[test]
fn a_link_fixed_after_a_refusal_replaces_the_refused_server() {
    let mut model = model_with(Overlay::AddServer(ServerPrompt {
        link_text_entry: TextEntry {
            input: "https://bad.example.com".to_owned(),
            error: None,
        },
        ..filled("carol")
    }));
    press(&mut model, KeyCode::Enter).unwrap();
    update(
        &mut model,
        Message::Remote(RemoteEvent::Error(RemoteError::Unreachable {
            server_name: ServerName::new("bad.example.com"),
            source: IoError::Other,
        })),
        Moment::default(),
    )
    .unwrap();
    let Some(Overlay::AddServer(server_prompt)) = &mut model.workspace.overlay else {
        panic!("the refusal closed the server form");
    };
    server_prompt.link_text_entry.input = "https://good.example.com".to_owned();

    for _ in [Field::Link, Field::User, Field::Password] {
        press(&mut model, KeyCode::Enter).unwrap();
    }

    let server_names = model
        .servers
        .iter()
        .map(|server| server.account.server_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(server_names, vec!["good.example.com"]);
}

#[test]
fn the_form_server_answering_online_closes_the_form() {
    let mut model = model_with(Overlay::AddServer(filled("carol")));
    press(&mut model, KeyCode::Enter).unwrap();

    update(
        &mut model,
        Message::Remote(RemoteEvent::Connected {
            server_name: ServerName::new("beats.example.com"),
            session: session(),
        }),
        Moment::default(),
    )
    .unwrap();

    assert_eq!(model.workspace.overlay, None);
}

#[rstest]
#[case::online(RemoteEvent::Connected {
    server_name: ServerName::new("other.example.com"),
    session: session(),
})]
#[case::refused(RemoteEvent::Error(RemoteError::Unreachable {
    server_name: ServerName::new("other.example.com"),
    source: IoError::Other,
}))]
fn an_answer_for_another_server_leaves_the_form_connecting(
    #[case] remote_event: RemoteEvent,
) {
    let mut model = model_with(Overlay::AddServer(ServerPrompt {
        link_text_entry: TextEntry {
            input: "https://other.example.com".to_owned(),
            error: None,
        },
        ..filled("carol")
    }));
    press(&mut model, KeyCode::Enter).unwrap();
    model.workspace.overlay = Some(Overlay::AddServer(filled("carol")));
    press(&mut model, KeyCode::Enter).unwrap();
    let before = model.workspace.overlay.clone();

    update(&mut model, Message::Remote(remote_event), Moment::default()).unwrap();

    assert_eq!(model.workspace.overlay, before);
}

#[test]
fn two_adds_on_one_host_with_two_ports_keep_two_servers() {
    let mut model = Model::default();
    for link in [
        "https://music.example.com:4533",
        "https://music.example.com:4534",
    ] {
        model.workspace.overlay = Some(Overlay::AddServer(ServerPrompt {
            link_text_entry: TextEntry {
                input: link.to_owned(),
                error: None,
            },
            ..filled("alice")
        }));
        press(&mut model, KeyCode::Enter).unwrap();
    }
    let server_names = model
        .servers
        .iter()
        .map(|server| server.account.server_name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        server_names,
        vec!["music.example.com:4533", "music.example.com:4534"]
    );
}

#[test]
fn the_model_debug_in_the_password_field_holds_no_typed_character() {
    let model = model_with(Overlay::AddServer(ServerPrompt {
        password_text_entry: TextEntry {
            input: "zq7#xv".to_owned(),
            error: None,
        },
        ..filled("alice")
    }));
    assert!(!format!("{model:?}").contains("zq7#xv"));
}
