use std::time::Duration;

use crate::domain::time::Moment;

pub const TOAST_SECONDS: u64 = 5;
pub const TOAST_LIFETIME: Duration = Duration::from_secs(TOAST_SECONDS);
pub const TOAST_STACK: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub kind: ToastKind,
    pub title: String,
    pub text: Option<String>,
    pub raised_at: Moment,
}

impl Toast {
    fn of(kind: ToastKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            text: None,
            raised_at: Moment::default(),
        }
    }

    #[must_use]
    pub fn info(title: impl Into<String>) -> Self {
        Self::of(ToastKind::Info, title)
    }

    #[must_use]
    pub fn success(title: impl Into<String>) -> Self {
        Self::of(ToastKind::Success, title)
    }

    #[must_use]
    pub fn warning(title: impl Into<String>) -> Self {
        Self::of(ToastKind::Warning, title)
    }

    #[must_use]
    pub fn error(title: impl Into<String>) -> Self {
        Self::of(ToastKind::Error, title)
    }

    #[must_use]
    pub fn with_text(self, text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..self
        }
    }
}
