use std::{sync::LazyLock, time::Duration};

use ureq::{
    Agent,
    tls::{RootCerts, TlsConfig},
};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub const API_TIMEOUT: Duration = Duration::from_secs(15);

pub const API_BYTES: u64 = 8 * 1024 * 1024;

pub const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

pub const FETCH_CHUNK: u64 = 4 * 1024 * 1024;

pub const CACHE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

static AGENT: LazyLock<Agent> = LazyLock::new(|| {
    Agent::config_builder()
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(API_TIMEOUT))
        .user_agent(concat!("zefiro/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
});

#[must_use]
pub fn agent() -> Agent {
    AGENT.clone()
}
