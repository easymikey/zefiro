use kernel::domain::{geometry::Cells, server::ServerName};

use crate::{
    overlay::modal::prompt::{PromptBody, PromptWidget},
    primitive::glyphs,
    theme::active_theme::ActiveTheme,
};

const TITLE: &str = "REMOVE SERVER?";
const HINT: &str = "Enter remove · Esc back";
const MIN_WIDTH: Cells = Cells(24);

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConfirmRemoveWidget<'a> {
    server_name: &'a ServerName,
    theme: ActiveTheme<'a>,
}

impl<'a> ConfirmRemoveWidget<'a> {
    #[must_use]
    pub(crate) fn new(
        server_name: &'a ServerName,
        active_theme: ActiveTheme<'a>,
    ) -> Self {
        Self {
            server_name,
            theme: active_theme,
        }
    }

    #[must_use]
    fn sentence(self) -> [&'a str; 5] {
        [
            "Remove ",
            glyphs::confirm_trash::QUOTE_OPEN,
            self.server_name.as_str(),
            glyphs::confirm_trash::QUOTE_CLOSE,
            " from sifr",
        ]
    }

    #[must_use]
    pub(crate) fn prompt(self) -> PromptWidget<'a> {
        PromptWidget::new(PromptBody::Sentence(self.sentence()), self.theme)
            .title(TITLE)
            .hint(HINT)
            .min_width(MIN_WIDTH)
    }
}

#[cfg(test)]
mod tests {
    use kernel::domain::server::ServerName;

    use crate::{
        overlay::confirm_remove::ConfirmRemoveWidget,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[test]
    fn confirm_remove_names_the_server_in_quotes() {
        let theme = noir();
        let server_name = ServerName::new("home");
        let prompt = ConfirmRemoveWidget::new(
            &server_name,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        )
        .prompt();
        insta::assert_snapshot!(
            rendered(60, 12, |frame| frame.render_widget(&prompt, frame.area()))
                .to_string()
        );
    }
}
