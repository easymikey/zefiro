use config::{AppearanceFile, ConfigReload, ThemeFile};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ThemeReloadError {
    #[error(transparent)]
    Parse(#[from] config::Error),
    #[error("no theme named `{name}`")]
    Unknown { name: String },
}

pub(crate) fn appearance_reload(
    text: Option<&str>,
) -> Result<AppearanceFile, config::Error> {
    text.map_or_else(|| Ok(AppearanceFile::default()), config::parse_appearance)
}

pub(crate) fn config_reload(text: Option<&str>) -> Result<ConfigReload, config::Error> {
    config::parse_config_reload(text.unwrap_or(""))
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

    use crate::config::reload::{
        ThemeReloadError,
        appearance_reload,
        config_reload,
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
            Err(config::Error::Parse { .. })
        ));
    }

    #[test]
    fn a_missing_keys_file_reloads_with_no_music_dir() {
        let parsed = config_reload(None).unwrap();

        assert_eq!(parsed.music_dir, None);
    }

    #[test]
    fn a_user_theme_file_is_parsed_over_the_embedded_one() {
        let theme = theme_reload("noir", Some("name = \"mine\"\n[colors]\nbg = \"#000000\"\nfg = \"#000000\"\nbright_fg = \"#000000\"\naccent = \"#000000\"\ngreen = \"#000000\"\nyellow = \"#000000\"\nred = \"#000000\"\n")).unwrap();

        assert_eq!(theme.name.as_str(), "mine");
    }

    #[test]
    fn an_embedded_theme_resolves_when_no_user_file_exists() {
        let theme = theme_reload("noir", None).unwrap();

        assert_eq!(theme.name.as_str(), "noir");
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
