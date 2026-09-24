use std::path::PathBuf;

use kernel::{
    LoadedRequest,
    Message,
    WorkspaceRequest,
    domain::{ConfigFailure, ConfigSource},
};

use crate::{
    config::{
        driver::{KeysSighting, Outbound},
        reload::{appearance_reload, keymap_reload, theme_reload},
        watch::ConfigChange,
    },
    shell::Reload,
};

pub(crate) fn react(
    change: ConfigChange,
    outbound: &Outbound<'_>,
    keys_sighting: &mut KeysSighting,
) {
    match change {
        ConfigChange::Appearance(text) => appearance_changed(text.as_deref(), outbound),
        ConfigChange::Keymap(text) => {
            keymap_changed(text.as_deref(), outbound, keys_sighting);
        }
        ConfigChange::Theme { name, text } => {
            theme_changed(&name, text.as_deref(), outbound);
        }
        ConfigChange::Themes(names) => {
            let loaded = LoadedRequest::ThemesLoaded(embedded_and_user(names));
            send(outbound, Message::Loaded(loaded));
        }
        ConfigChange::Unreadable { file, detail } => {
            send(
                outbound,
                Message::Workspace(WorkspaceRequest::ConfigFailed(
                    ConfigFailure::Unreadable { file, detail },
                )),
            );
        }
    }
}

fn appearance_changed(text: Option<&str>, outbound: &Outbound<'_>) {
    match appearance_reload(text) {
        Ok(file) => {
            let rows = config::custom_rows(&file);
            let _ = outbound.reloads.send(Reload::Appearance(file));
            send(
                outbound,
                Message::Loaded(LoadedRequest::CustomRowsReloaded(rows)),
            );
            source(ConfigSource::Appearance, None, outbound);
        }
        Err(error) => {
            source(ConfigSource::Appearance, Some(error.to_string()), outbound);
        }
    }
}

fn theme_changed(name: &str, text: Option<&str>, outbound: &Outbound<'_>) {
    match theme_reload(name, text) {
        Ok(file) => {
            let _ = outbound.reloads.send(Reload::Theme(file));
            source(ConfigSource::Theme, None, outbound);
        }
        Err(error) => source(ConfigSource::Theme, Some(error.to_string()), outbound),
    }
}

fn keymap_changed(
    text: Option<&str>,
    outbound: &Outbound<'_>,
    keys_sighting: &mut KeysSighting,
) {
    match keymap_reload(text) {
        Ok(parsed) => {
            let reloaded = WorkspaceRequest::KeymapReloaded(Box::new(parsed.keymap));
            send(outbound, Message::Workspace(reloaded));
            music_dir_changed(parsed.music_dir, outbound, keys_sighting);
        }
        Err(error) => source(ConfigSource::Keymap, Some(error.to_string()), outbound),
    }
}

fn music_dir_changed(
    music_dir: Option<PathBuf>,
    outbound: &Outbound<'_>,
    keys_sighting: &mut KeysSighting,
) {
    let sighting = std::mem::replace(keys_sighting, KeysSighting::Repeat);
    if let (KeysSighting::Repeat, Some(music_dir)) = (sighting, music_dir) {
        send(
            outbound,
            Message::Loaded(LoadedRequest::MusicDirReloaded(music_dir)),
        );
    }
}

fn source(source: ConfigSource, text: Option<String>, outbound: &Outbound<'_>) {
    let request = text.map_or(WorkspaceRequest::SourceRecovered(source), |text| {
        WorkspaceRequest::SourceFailed { source, text }
    });
    send(outbound, Message::Workspace(request));
}

fn send(outbound: &Outbound<'_>, message: Message) {
    let _ = outbound.mailbox.send(message);
}

fn embedded_and_user(user: Vec<String>) -> Vec<String> {
    config::EMBEDDED_THEMES
        .iter()
        .map(|name| (*name).to_string())
        .chain(user)
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}
