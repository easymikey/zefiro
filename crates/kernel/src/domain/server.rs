use std::{
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use strum::EnumIter;

use crate::domain::{
    config::Diagnostic,
    io_error::IoError,
    revision::Revision,
    time::Moment,
};

const SCHEMES: [&str; 2] = ["https://", "http://"];

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerName(Arc<str>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scrobble {
    NowPlaying,
    Played(Moment),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayReport {
    pub server_name: ServerName,
    pub server_track_id: ServerTrackId,
    pub scrobble: Scrobble,
}

impl ServerName {
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self(Arc::from(name.trim()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerTrackId(Arc<str>);

impl ServerTrackId {
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(Arc::from(id))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ServerTrackId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserName(Arc<str>);

impl UserName {
    pub fn new(name: &str) -> Result<Self, UserNameError> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(UserNameError::Empty);
        }
        Ok(Self(Arc::from(trimmed)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UserName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UserNameError {
    #[error("Type the user name")]
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint(Arc<str>);

impl Endpoint {
    pub fn parse(link: &str) -> Result<Self, EndpointError> {
        let trimmed = link.trim();
        if trimmed.is_empty() {
            return Err(EndpointError::Empty);
        }
        let (scheme, rest) = SCHEMES
            .into_iter()
            .find_map(|scheme| trimmed.strip_prefix(scheme).map(|rest| (scheme, rest)))
            .ok_or(EndpointError::Scheme)?;
        if rest.contains(['?', '#']) {
            return Err(EndpointError::Query);
        }
        let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
        if authority.contains('@') {
            return Err(EndpointError::UserInfo);
        }
        let (host, port) = authority_parts(authority);
        let port_valid = port.is_none_or(|digits| {
            digits.bytes().all(|byte| byte.is_ascii_digit())
                && digits.parse::<u16>().is_ok_and(|number| number != 0)
        });
        if host.is_empty() || !port_valid {
            return Err(EndpointError::Host);
        }
        let path_prefix = path.trim_end_matches('/');
        let separator = if path_prefix.is_empty() { "" } else { "/" };
        Ok(Self(Arc::from(format!(
            "{scheme}{authority}{separator}{path_prefix}"
        ))))
    }

    #[must_use]
    pub fn host(&self) -> &str {
        authority_parts(self.authority()).0
    }

    #[must_use]
    pub fn authority(&self) -> &str {
        let rest = SCHEMES
            .into_iter()
            .find_map(|scheme| self.0.strip_prefix(scheme))
            .unwrap_or(&self.0);
        rest.split_once('/')
            .map_or(rest, |(authority, _)| authority)
    }

    #[must_use]
    pub fn is_https(&self) -> bool {
        self.0.starts_with(SCHEMES[0])
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn authority_parts(authority: &str) -> (&str, Option<&str>) {
    match authority.rsplit_once(':') {
        Some((host, port)) if !port.contains(']') => (host, Some(port)),
        Some(_) | None => (authority, None),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EndpointError {
    #[error("Type the link of the server")]
    Empty,
    #[error("Start the link with https:// or http://")]
    Scheme,
    #[error("Add the server host after the scheme, with a port from 1 to 65535 if any")]
    Host,
    #[error("Type the user in the next step, not in the link")]
    UserInfo,
    #[error("Leave ? and # out of the link")]
    Query,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Secret(Box<str>);

impl Secret {
    pub fn new(password: &str) -> Result<Self, SecretError> {
        if password.is_empty() {
            return Err(SecretError::Empty);
        }
        Ok(Self(Box::from(password)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(…)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SecretError {
    #[error("Type the password")]
    Empty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    Typed(Secret),
    Stored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub server_name: ServerName,
    pub endpoint: Endpoint,
    pub user_name: UserName,
}

impl Account {
    #[must_use]
    pub fn keychain_account(&self) -> String {
        format!("{}@{}", self.user_name, self.endpoint.authority())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub account: Account,
    pub credential: Credential,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Session {
    pub endpoint: Endpoint,
    pub query: Arc<str>,
}

impl Session {
    #[must_use]
    pub fn new(endpoint: Endpoint, query: &str) -> Self {
        Self {
            endpoint,
            query: Arc::from(query),
        }
    }
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("endpoint", &self.endpoint)
            .field("query", &format_args!("…"))
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(Arc<str>);

impl CacheKey {
    #[must_use]
    pub fn new(
        server_name: &ServerName,
        server_track_id: &ServerTrackId,
        suffix: &str,
    ) -> Self {
        Self(Arc::from(format!(
            "{}/{}.{}",
            file_name(server_name.as_str()),
            file_name(server_track_id.as_str()),
            file_name(suffix)
        )))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn file_name(text: &str) -> String {
    let replaced = text.replace(['/', '\\'], "_");
    if matches!(replaced.as_str(), "" | "." | "..") {
        format!("_{replaced}")
    } else {
        replaced
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFetch {
    pub server_name: ServerName,
    pub server_track_id: ServerTrackId,
    pub cache_key: CacheKey,
    pub session: Session,
    pub first_byte: u64,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    pub media_path: PathBuf,
    pub downloaded: u64,
    pub byte_len: u64,
}

impl Fetched {
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.downloaded == self.byte_len
    }
}

pub const START_MARGIN: u64 = 512 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub media_fetch: MediaFetch,
    pub fetched: Option<Fetched>,
}

impl Download {
    #[must_use]
    pub fn ready(&self) -> bool {
        self.fetched.as_ref().is_some_and(|fetched| {
            fetched.is_complete()
                || (fetched.downloaded >= START_MARGIN
                    && !Path::new(self.media_fetch.cache_key.as_str())
                        .extension()
                        .and_then(|suffix| suffix.to_str())
                        .is_some_and(|suffix| {
                            ["m4a", "m4b", "mp4"]
                                .iter()
                                .any(|known| suffix.eq_ignore_ascii_case(known))
                        }))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatus {
    Connecting,
    Online(Session),
    Offline(RemoteError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Server {
    pub account: Account,
    pub server_status: ServerStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HttpStatus(pub u16);

impl fmt::Display for HttpStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ApiCode(pub u16);

impl fmt::Display for ApiCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

const CREDENTIAL_CODES: [ApiCode; 3] = [ApiCode(40), ApiCode(41), ApiCode(44)];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemoteError {
    #[error("{server_name} is out of reach: {source}")]
    Unreachable {
        server_name: ServerName,
        source: IoError,
    },
    #[error("{server_name} answered with HTTP status {http_status}")]
    Status {
        server_name: ServerName,
        http_status: HttpStatus,
    },
    #[error("{server_name} refused the request with code {api_code}")]
    Api {
        server_name: ServerName,
        api_code: ApiCode,
    },
    #[error("{server_name} moved to another address; update its link")]
    Moved { server_name: ServerName },
    #[error("{server_name} sent an answer zefiro cannot read: {diagnostic}")]
    Parse {
        server_name: ServerName,
        diagnostic: Diagnostic,
    },
    #[error("No password is saved for {server_name}")]
    NoPassword { server_name: ServerName },
    #[error("The keychain failed for {server_name}: {source}")]
    Keychain {
        server_name: ServerName,
        source: IoError,
    },
    #[error("The media cache failed for {server_name}: {source}")]
    Cache {
        server_name: ServerName,
        source: IoError,
    },
}

impl RemoteError {
    #[must_use]
    pub fn server_name(&self) -> &ServerName {
        match self {
            Self::Unreachable { server_name, .. }
            | Self::Keychain { server_name, .. }
            | Self::Cache { server_name, .. }
            | Self::Status { server_name, .. }
            | Self::Api { server_name, .. }
            | Self::Parse { server_name, .. }
            | Self::Moved { server_name }
            | Self::NoPassword { server_name } => server_name,
        }
    }

    #[must_use]
    pub fn is_credentials(&self) -> bool {
        match self {
            Self::Api { api_code, .. } => CREDENTIAL_CODES.contains(api_code),
            Self::NoPassword { .. } => true,
            Self::Unreachable { .. }
            | Self::Status { .. }
            | Self::Moved { .. }
            | Self::Parse { .. }
            | Self::Keychain { .. }
            | Self::Cache { .. } => false,
        }
    }

    #[must_use]
    pub fn is_refusal(&self) -> bool {
        match self {
            Self::Status { http_status, .. } => (400..500).contains(&http_status.0),
            Self::Api { .. } | Self::Parse { .. } => true,
            Self::Unreachable { .. }
            | Self::Moved { .. }
            | Self::NoPassword { .. }
            | Self::Keychain { .. }
            | Self::Cache { .. } => false,
        }
    }
}

pub const PAGE_ROWS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AlbumId(Arc<str>);

impl AlbumId {
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(Arc::from(id))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub enum AlbumOrder {
    Newest,
    Recent,
    Frequent,
    Starred,
    Alphabetical,
    Random,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Listing {
    Albums(AlbumOrder),
    Album(AlbumId),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Page(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAlbum {
    pub album_id: AlbumId,
    pub title: Arc<str>,
    pub artist: Arc<str>,
    pub year: Option<u16>,
    pub track_count: usize,
    pub duration: Duration,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::domain::{
        config::Diagnostic,
        io_error::IoError,
        server::{
            Account,
            ApiCode,
            CacheKey,
            Endpoint,
            EndpointError,
            HttpStatus,
            RemoteError,
            Secret,
            SecretError,
            ServerName,
            ServerTrackId,
            Session,
            UserName,
            UserNameError,
        },
    };

    #[rstest]
    #[case(
        "https://example.com/navidrome/",
        "https://example.com/navidrome",
        "example.com"
    )]
    #[case("  http://[::1]:4533/  ", "http://[::1]:4533", "[::1]")]
    fn endpoint_parse_keeps_the_link_without_its_trailing_slash(
        #[case] link: &str,
        #[case] kept: &str,
        #[case] host: &str,
    ) {
        let endpoint = Endpoint::parse(link).map_err(|error| error.to_string());
        assert_eq!(endpoint.as_ref().map(Endpoint::as_str), Ok(kept));
        assert_eq!(endpoint.as_ref().map(Endpoint::host), Ok(host));
    }

    #[rstest]
    #[case("", EndpointError::Empty)]
    #[case("https://", EndpointError::Host)]
    #[case("https://x:0", EndpointError::Host)]
    #[case("https://x:99999", EndpointError::Host)]
    #[case("https://x:+80", EndpointError::Host)]
    #[case("https://alice@x", EndpointError::UserInfo)]
    #[case("https://x/?a", EndpointError::Query)]
    #[case("https://x/#a", EndpointError::Query)]
    fn endpoint_parse_refuses_a_link_it_cannot_sign(
        #[case] link: &str,
        #[case] endpoint_error: EndpointError,
    ) {
        assert_eq!(Endpoint::parse(link), Err(endpoint_error));
    }

    #[rstest]
    #[case("http://[::1]:4533/", "alice@[::1]:4533")]
    fn keychain_account_is_user_at_host_with_the_port_if_any_and_survives_a_rename(
        #[case] link: &str,
        #[case] keychain_account: &str,
    ) {
        let accounts = ["home", "renamed"].map(|name| {
            Endpoint::parse(link)
                .ok()
                .zip(UserName::new("alice").ok())
                .map(|(endpoint, user_name)| Account {
                    server_name: ServerName::new(name),
                    endpoint,
                    user_name,
                })
        });
        let [Some(home), Some(renamed)] = accounts else {
            panic!("valid accounts");
        };
        assert_eq!(home.keychain_account(), keychain_account);
        assert_eq!(renamed.keychain_account(), keychain_account);
    }

    #[test]
    fn user_name_is_trimmed_and_never_empty() {
        assert_eq!(
            UserName::new("  alice ").map(|user| user.to_string()),
            Ok("alice".to_owned())
        );
        assert_eq!(UserName::new("   "), Err(UserNameError::Empty));
    }

    #[test]
    fn secret_and_session_debug_hold_no_secret_text() {
        let secret = Secret::new("hunter2");
        let session = Session::new(
            Endpoint::parse("https://music.example").unwrap(),
            "u=alice&t=deadbeef&s=0123456789ab",
        );
        assert!(
            secret
                .as_ref()
                .is_ok_and(|secret| !format!("{secret:?}").contains("hunter2"))
        );
        assert!(!format!("{session:?}").contains("deadbeef"));
        assert_eq!(Secret::new(""), Err(SecretError::Empty));
    }

    fn server_name() -> ServerName {
        ServerName::new("home")
    }

    #[rstest]
    #[case(RemoteError::Api { server_name: server_name(), api_code: ApiCode(40) }, true)]
    #[case(RemoteError::Api { server_name: server_name(), api_code: ApiCode(41) }, true)]
    #[case(RemoteError::Api { server_name: server_name(), api_code: ApiCode(44) }, true)]
    #[case(RemoteError::NoPassword { server_name: server_name() }, true)]
    #[case(RemoteError::Api { server_name: server_name(), api_code: ApiCode(70) }, false)]
    #[case(RemoteError::Status { server_name: server_name(), http_status: HttpStatus(401) }, false)]
    #[case(RemoteError::Unreachable { server_name: server_name(), source: IoError::Other }, false)]
    #[case(RemoteError::Moved { server_name: server_name() }, false)]
    #[case(
        RemoteError::Parse { server_name: server_name(), diagnostic: Diagnostic::from_error(&std::fmt::Error) },
        false
    )]
    #[case(RemoteError::Keychain { server_name: server_name(), source: IoError::Denied }, false)]
    fn is_credentials_holds_for_wrong_login_codes_and_a_missing_password(
        #[case] error: RemoteError,
        #[case] credentials: bool,
    ) {
        assert_eq!(error.is_credentials(), credentials);
        assert_eq!(error.server_name(), &server_name());
    }

    #[rstest]
    #[case(RemoteError::Status { server_name: server_name(), http_status: HttpStatus(404) }, true)]
    #[case(
        RemoteError::Parse { server_name: server_name(), diagnostic: Diagnostic::from_error(&std::fmt::Error) },
        true
    )]
    #[case(RemoteError::Status { server_name: server_name(), http_status: HttpStatus(503) }, false)]
    fn is_refusal_holds_for_api_codes_parse_failures_and_client_statuses(
        #[case] error: RemoteError,
        #[case] refusal: bool,
    ) {
        assert_eq!(error.is_refusal(), refusal);
    }

    #[test]
    fn a_parse_error_says_zefiro_cannot_read_the_answer() {
        let error = RemoteError::Parse {
            server_name: server_name(),
            diagnostic: Diagnostic::from_error(&std::fmt::Error),
        };
        assert_eq!(
            error.to_string(),
            "home sent an answer zefiro cannot read: an error occurred when formatting an argument"
        );
    }

    #[rstest]
    #[case(["a/b", "c\\d", "mp3"], "a_b/c_d.mp3")]
    #[case(["..", "..", ""], "_../_..._")]
    fn cache_key_is_server_slash_id_dot_suffix_with_separators_replaced(
        #[case] parts: [&str; 3],
        #[case] key: &str,
    ) {
        let [name, id, suffix] = parts;
        let cache_key =
            CacheKey::new(&ServerName::new(name), &ServerTrackId::new(id), suffix);

        assert_eq!(cache_key.as_str(), key);
    }
}
