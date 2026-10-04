#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IoError {
    #[error("not found")]
    Missing,
    #[error("permission denied")]
    Denied,
    #[error("corrupt data")]
    Malformed,
    #[error("disk full")]
    Full,
    #[error("an unknown error")]
    Other,
}

impl From<std::io::ErrorKind> for IoError {
    fn from(kind: std::io::ErrorKind) -> Self {
        [
            (std::io::ErrorKind::NotFound, IoError::Missing),
            (std::io::ErrorKind::PermissionDenied, IoError::Denied),
            (std::io::ErrorKind::StorageFull, IoError::Full),
            (std::io::ErrorKind::InvalidData, IoError::Malformed),
            (std::io::ErrorKind::UnexpectedEof, IoError::Malformed),
        ]
        .into_iter()
        .find(|(known, _)| *known == kind)
        .map_or(IoError::Other, |(_, error)| error)
    }
}
