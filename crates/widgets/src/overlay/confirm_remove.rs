use kernel::domain::{geometry::Cells, server::ServerName};

use crate::{
    overlay::modal::prompt::{PromptBody, PromptWidget},
    primitive::glyphs,
    theme::active_theme::ActiveTheme,
};

const MIN_WIDTH: Cells = Cells(24);

#[must_use]
fn sentence(server_name: &ServerName) -> [&str; 5] {
    [
        "Remove ",
        glyphs::quote::QUOTE_OPEN,
        server_name.as_str(),
        glyphs::quote::QUOTE_CLOSE,
        " from sifr",
    ]
}

pub(crate) fn prompt<'a>(
    server_name: &'a ServerName,
    active_theme: ActiveTheme<'a>,
) -> PromptWidget<'a> {
    PromptWidget::new(PromptBody::Sentence(sentence(server_name)), active_theme)
        .title(glyphs::confirm_remove::TITLE_WORD)
        .hint(glyphs::confirm_remove::HINT)
        .min_width(MIN_WIDTH)
}

#[cfg(test)]
mod tests {
    use kernel::domain::server::ServerName;

    use crate::{
        overlay::confirm_remove::prompt,
        test_support::{noir, rendered},
        theme::{active_theme::ActiveTheme, rgb::ColorDepth},
    };

    #[test]
    fn confirm_remove_names_the_server_in_quotes() {
        let theme = noir();
        let server_name = ServerName::new("home");
        let prompt = prompt(
            &server_name,
            ActiveTheme::new(&theme, ColorDepth::TrueColor),
        );
        insta::assert_snapshot!(
            rendered(60, 12, |frame| frame.render_widget(&prompt, frame.area()))
                .to_string()
        );
    }
}
