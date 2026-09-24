use config::{AppearanceFile, ConfigError, ParsedKeymap, ThemeFile};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ThemeReloadError {
    #[error(transparent)]
    Parse(#[from] ConfigError),
    #[error("no theme named `{name}`")]
    Unknown { name: String },
}

pub(crate) fn appearance_reload(
    text: Option<&str>,
) -> Result<AppearanceFile, ConfigError> {
    config::parse_appearance(text.unwrap_or_default())
}

pub(crate) fn keymap_reload(text: Option<&str>) -> Result<ParsedKeymap, ConfigError> {
    config::parse_keymap(text.unwrap_or_default())
}

pub(crate) fn theme_reload(
    name: &str,
    text: Option<&str>,
) -> Result<ThemeFile, ThemeReloadError> {
    if let Some(source) = text {
        return Ok(config::parse_theme(source, name)?);
    }
    let embedded =
        config::embedded_theme(name).ok_or_else(|| ThemeReloadError::Unknown {
            name: name.to_string(),
        })?;
    Ok(config::parse_theme(embedded, name)?)
}

#[cfg(test)]
mod tests {
    use config::ConfigError;

    use crate::config::reload::{
        ThemeReloadError,
        appearance_reload,
        keymap_reload,
        theme_reload,
    };

    #[test]
    fn a_missing_appearance_file_reloads_as_the_stock_defaults() {
        let file = appearance_reload(None).unwrap();

        assert_eq!(file, config::AppearanceFile::default());
    }

    #[test]
    fn a_broken_appearance_file_reports_a_parse_fault() {
        assert!(matches!(
            appearance_reload(Some("[cover\nnot toml")),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn a_missing_keys_file_reloads_with_no_music_dir() {
        let parsed = keymap_reload(None).unwrap();

        assert_eq!(parsed.music_dir, None);
    }

    #[test]
    fn a_user_theme_file_is_parsed_over_the_embedded_one() {
        let theme = theme_reload("noir", Some("name = \"mine\"\n[colors]\nbg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n")).unwrap();

        assert_eq!(theme.name, "mine");
    }

    #[test]
    fn an_embedded_theme_resolves_when_no_user_file_exists() {
        let theme = theme_reload("noir", None).unwrap();

        assert_eq!(theme.name, "noir");
    }

    #[test]
    fn an_unknown_theme_with_no_user_file_is_reported() {
        let error = theme_reload("not-a-theme", None).unwrap_err();

        assert_eq!(
            error,
            ThemeReloadError::Unknown {
                name: "not-a-theme".to_string(),
            }
        );
    }
}
