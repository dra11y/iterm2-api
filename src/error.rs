use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Error, Debug)]
pub enum Error {
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Protobuf error: {0}")]
    Protobuf(#[from] protobuf::Error),

    #[error("URL error: {0}")]
    Url(#[from] url::ParseError),

    #[error("UTF-8 error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Connection failed: {0}")]
    Connection(String),

    #[error("API error: {0}")]
    Api(String),
}
