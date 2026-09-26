use std::path::Path;

use config::{AppearancePatch, ConfigError};
use kernel::ConfigPatch;

use crate::error::SaveError;

#[derive(Debug)]
pub(crate) struct Written {
    pub text: String,
}

pub(crate) fn save(path: &Path, patch: ConfigPatch) -> Result<Written, SaveError> {
    write_text(path, |existing| config::patched(existing, patch))
}

pub(crate) fn save_appearance(
    path: &Path,
    patch: AppearancePatch,
) -> Result<Written, SaveError> {
    write_text(path, |existing| config::appearance_patched(existing, patch))
}

fn write_text(
    path: &Path,
    produce: impl FnOnce(&str) -> Result<String, ConfigError>,
) -> Result<Written, SaveError> {
    let existing = library::files::read_if_present(path)
        .map_err(|source| SaveError::Read {
            path: path.to_path_buf(),
            source,
        })?
        .unwrap_or_default();
    let text = produce(&existing).map_err(|source| SaveError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    library::files::create_parent(path).map_err(|source| SaveError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    library::files::persist(path, text.as_bytes()).map_err(|source| {
        SaveError::Write {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Ok(Written { text })
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        time::Duration,
    };

    use config::{AppearancePatch, ConfigError, CoverBrackets};
    use kernel::{
        Bounded,
        ConfigPatch,
        domain::{Crossfade, ThemeName},
    };
    use rstest::{fixture, rstest};

    use crate::{
        config::write::{Written, save, save_appearance},
        error::SaveError,
    };

    type Save = fn(&Path) -> Result<Written, SaveError>;

    fn crossfade(seconds: u64) -> Crossfade {
        Crossfade::clamped(Duration::from_secs(seconds))
    }

    fn temporary_config() -> std::io::Result<(tempfile::TempDir, PathBuf)> {
        tempfile::tempdir().map(|directory| {
            let path = directory.path().join("config.toml");
            (directory, path)
        })
    }

    #[fixture]
    fn config_file() -> std::io::Result<(tempfile::TempDir, PathBuf)> {
        temporary_config()
    }

    const EXISTING_UI: &str =
        "# keep me\n[cover]\nstyle = \"vinyl\"\nbrackets = false\n";
    const EXISTING_CONFIG: &str =
        "# keep me\ntheme = \"auto\"\n\n[audio]\ncrossfade = \"0s\"\n";

    fn save_cover_brackets(path: &Path) -> Result<Written, SaveError> {
        save_appearance(
            path,
            AppearancePatch::builder()
                .cover_brackets(CoverBrackets::Shown)
                .build(),
        )
    }

    fn save_theme(path: &Path) -> Result<Written, SaveError> {
        save(
            path,
            ConfigPatch::builder()
                .theme(ThemeName::from_static("dark"))
                .build(),
        )
    }

    fn save_crossfade(path: &Path) -> Result<Written, SaveError> {
        save(path, ConfigPatch::builder().crossfade(crossfade(5)).build())
    }

    struct Landing {
        name: &'static str,
        existing: &'static str,
        save: Save,
    }

    #[rstest]
    #[case::appearance_creates_a_minimal_file(Landing {
        name: "appearance_missing",
        existing: "",
        save: save_cover_brackets,
    })]
    #[case::appearance_updates_one_key_of_an_existing_file(Landing {
        name: "appearance_existing",
        existing: EXISTING_UI,
        save: save_cover_brackets,
    })]
    #[case::config_creates_a_minimal_file(Landing {
        name: "config_missing",
        existing: "",
        save: save_theme,
    })]
    #[case::config_updates_one_key_of_an_existing_file(Landing {
        name: "config_existing",
        existing: EXISTING_CONFIG,
        save: save_crossfade,
    })]
    fn a_save_lands_on_disk(
        #[case] landing: Landing,
        config_file: std::io::Result<(tempfile::TempDir, PathBuf)>,
    ) {
        let (_directory, path) = config_file.unwrap();
        if !landing.existing.is_empty() {
            std::fs::write(&path, landing.existing).unwrap();
        }

        let written = (landing.save)(&path).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, written.text, "the reported text is the text on disk");
        let _parsed: toml::Value = toml::from_str(&text).unwrap();
        let leftover = path.with_extension(format!("toml.tmp.{}", std::process::id()));
        assert!(
            !leftover.exists(),
            "an atomic write leaves no tmp file behind"
        );
        insta::with_settings!({ snapshot_suffix => landing.name }, {
            insta::assert_snapshot!(text);
        });
    }

    #[rstest]
    fn a_failed_save_reports_not_a_table_and_leaves_the_file_unchanged(
        config_file: std::io::Result<(tempfile::TempDir, PathBuf)>,
    ) {
        let (_directory, path) = config_file.unwrap();
        std::fs::write(&path, "audio = 1\n").unwrap();

        let refused = save_crossfade(&path).unwrap_err();

        assert!(matches!(
            refused,
            SaveError::Parse {
                source: ConfigError::NotATable { ref key },
                ..
            } if key == "audio"
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "audio = 1\n",
            "a failed save must leave the file untouched"
        );
    }
}
