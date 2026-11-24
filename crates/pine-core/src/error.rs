use thiserror::Error;

/// Errors returned by the StreamElements and Twitch clients.
#[derive(Debug, Error)]
pub enum Error {
    /// The token was rejected (HTTP 401 or 403).
    #[error("the token was rejected ({status}): {body}")]
    Unauthorized { status: u16, body: String },

    /// Any other non-success HTTP status.
    #[error("request failed with HTTP {status}: {body}")]
    Http { status: u16, body: String },

    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("could not read the response: {0}")]
    Decode(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Error::Unauthorized { .. })
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Turn a response into `Ok(response)` or a typed error that keeps the body for diagnosis.
pub(crate) async fn check(resp: reqwest::Response) -> Result<reqwest::Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let code = status.as_u16();
    let body = resp.text().await.unwrap_or_default();
    let body: String = body.chars().take(500).collect();
    if code == 401 || code == 403 {
        Err(Error::Unauthorized { status: code, body })
    } else {
        Err(Error::Http { status: code, body })
    }
}
