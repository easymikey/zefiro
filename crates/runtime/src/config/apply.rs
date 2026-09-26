use std::path::PathBuf;

use kernel::{
    ConfigFact,
    Delivery,
    Outbox,
    domain::{ConfigFailure, ConfigSource, ThemeName},
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
            send(outbound, ConfigFact::ThemesLoaded(embedded_and_user(names)));
        }
        ConfigChange::Unreadable { file, detail } => {
            send(
                outbound,
                ConfigFact::Failed(ConfigFailure::Unreadable { file, detail }),
            );
        }
    }
}

fn appearance_changed(text: Option<&str>, outbound: &Outbound<'_>) {
    match appearance_reload(text) {
        Ok(file) => {
            let rows = config::custom_rows(&file);
            let _ = outbound.reloads.send(Reload::Appearance(file));
            send(outbound, ConfigFact::CustomRowsReloaded(rows));
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
            let reloaded = ConfigFact::KeymapReloaded(Box::new(parsed.keymap));
            send(outbound, reloaded);
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
        send(outbound, ConfigFact::MusicDirReloaded(music_dir));
    }
}

fn source(source: ConfigSource, text: Option<String>, outbound: &Outbound<'_>) {
    let fact = text.map_or(ConfigFact::SourceRecovered(source), |text| {
        ConfigFact::SourceFailed { source, text }
    });
    send(outbound, fact);
}

fn send(outbound: &Outbound<'_>, fact: ConfigFact) {
    if let Delivery::Closed = outbound.mailbox.send(fact) {}
}

fn embedded_and_user(user: Vec<String>) -> Vec<ThemeName> {
    config::EMBEDDED_THEMES
        .iter()
        .map(|name| (*name).to_string())
        .chain(user)
        .filter_map(|name| ThemeName::new(name).ok())
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}
