use kernel::{Message, Toast, WorkspaceRequest};
use terminal::UnknownThemeError;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ShellFailure {
    Theme(UnknownThemeError),
    Cover(String),
}

pub(crate) fn wording(failure: &ShellFailure) -> String {
    match failure {
        ShellFailure::Theme(error) => error.to_string(),
        ShellFailure::Cover(reason) => format!("cover art: {reason}"),
    }
}

pub(crate) fn toast_message(failure: &ShellFailure) -> Message {
    Message::Workspace(WorkspaceRequest::ShowToast(Toast::error(wording(failure))))
}

#[cfg(test)]
mod tests {
    use kernel::{Message, Toast, ToastLevel, WorkspaceRequest};
    use rstest::rstest;
    use terminal::UnknownThemeError;

    use crate::toast::{ShellFailure, toast_message, wording};

    #[rstest]
    fn a_theme_failure_reads_as_the_underlying_error() {
        let failure = ShellFailure::Theme(UnknownThemeError {
            name: "gone".to_string(),
        });

        assert_eq!(wording(&failure), "no such theme: gone");
    }

    #[rstest]
    fn a_cover_failure_names_the_cover_and_the_reason() {
        let failure = ShellFailure::Cover("decode error".to_string());

        assert_eq!(wording(&failure), "cover art: decode error");
    }

    #[rstest]
    fn a_failure_becomes_an_error_toast_message() {
        let failure = ShellFailure::Cover("broken".to_string());

        assert_eq!(
            toast_message(&failure),
            Message::Workspace(WorkspaceRequest::ShowToast(Toast {
                level: ToastLevel::Error,
                text: wording(&failure),
            }))
        );
    }
}
