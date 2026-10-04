use std::path::Path;

use config::AppearancePatch;
use kernel::ConfigPatch;

use crate::error::SaveError;

#[derive(Debug)]
pub(crate) struct Written {
    pub text: String,
}

pub(crate) type SaveResult = Result<Written, SaveError>;

pub(crate) fn save_config(
    path: &Path,
    patch: ConfigPatch,
) -> Result<Written, SaveError> {
    write_text(path, |existing| config::patch_config_text(existing, patch))
}

pub(crate) fn save_appearance(
    path: &Path,
    patch: AppearancePatch,
) -> Result<Written, SaveError> {
    write_text(path, |existing| {
        config::patch_appearance_text(existing, patch)
    })
}

fn write_text(
    path: &Path,
    produce: impl FnOnce(&str) -> Result<String, config::Error>,
) -> Result<Written, SaveError> {
    let existing =
        library::files::read_if_present(path).map_err(|source| SaveError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    let text = produce(existing.as_deref().unwrap_or("")).map_err(|source| {
        SaveError::Parse {
            path: path.to_path_buf(),
            source,
        }
    })?;
    library::files::create_parent_dir(path).map_err(|source| SaveError::Write {
        path: path.to_path_buf(),
        source,
    })?;
    library::files::write_atomic(path, text.as_bytes()).map_err(|source| {
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

    use config::{AppearancePatch, CoverBrackets};
    use kernel::{
        Bounded,
        ConfigPatch,
        domain::{Crossfade, ThemeName},
    };
    use rstest::{fixture, rstest};

    use crate::{
        config::write::{Written, save_appearance, save_config},
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
        "# keep me\n[cover]\nmode = \"vinyl\"\nbrackets = false\n";
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
        save_config(
            path,
            ConfigPatch::builder()
                .theme(ThemeName::from_static("dark"))
                .build(),
        )
    }

    fn save_crossfade(path: &Path) -> Result<Written, SaveError> {
        save_config(path, ConfigPatch::builder().crossfade(crossfade(5)).build())
    }

    struct InstallParts {
        name: &'static str,
        existing: &'static str,
        save: Save,
    }

    #[rstest]
    #[case::appearance_creates_a_minimal_file(InstallParts {
        name: "appearance_missing",
        existing: "",
        save: save_cover_brackets,
    })]
    #[case::appearance_updates_one_key_of_an_existing_file(InstallParts {
        name: "appearance_existing",
        existing: EXISTING_UI,
        save: save_cover_brackets,
    })]
    #[case::config_creates_a_minimal_file(InstallParts {
        name: "config_missing",
        existing: "",
        save: save_theme,
    })]
    #[case::config_updates_one_key_of_an_existing_file(InstallParts {
        name: "config_existing",
        existing: EXISTING_CONFIG,
        save: save_crossfade,
    })]
    fn a_save_lands_on_disk(
        #[case] landing: InstallParts,
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
                source: config::Error::NotATable { ref key },
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
