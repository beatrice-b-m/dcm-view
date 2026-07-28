use thiserror::Error;

#[derive(Debug, Error)]
pub enum PixelError {
    #[error("no pixel data")]
    NoPixelData,
    #[error("frame out of range")]
    FrameOutOfRange,
    #[error("unsupported transfer syntax: {0}")]
    UnsupportedTransferSyntax(String),
    #[error("unsupported pixel layout: {0}")]
    UnsupportedLayout(String),
    #[error("invalid window request: {0}")]
    InvalidWindow(String),
    #[error("{context}: {source}")]
    Decode {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

impl PixelError {
    pub(crate) fn frame_decode(source: anyhow::Error) -> Self {
        Self::Decode {
            context: "frame decode failed",
            source,
        }
    }

    pub(crate) fn raw_decode(source: anyhow::Error) -> Self {
        Self::Decode {
            context: "raw frame decode failed",
            source,
        }
    }
}

pub type PixelResult<T> = std::result::Result<T, PixelError>;
