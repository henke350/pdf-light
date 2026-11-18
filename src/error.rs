//! The one error type shared by the whole library.

use thiserror::Error;

/// Everything that can go wrong in pdf-light.
///
/// `thiserror` writes the `Display` impl from each `#[error(...)]` message,
/// and `#[from]` lets `?` turn a `std::io::Error` into `PdfError::Io` automatically.
#[derive(Debug, Error)]
pub enum PdfError {
    #[error("couldn't read or write the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("this PDF is password-protected, which pdf-light doesn't support yet")]
    Encrypted,
    #[error("this file isn't a PDF pdf-light can read")]
    Invalid,
    #[error("the page is too large to render at this size ({width} × {height} px)")]
    TooLarge { width: u64, height: u64 },
    #[error("a PDF needs at least one page")]
    NoPagesLeft,
    #[error("couldn't save the PDF: {0}")]
    Save(String),
    #[error("couldn't write the image: {0}")]
    Image(String),
    #[error("cancelled")]
    Cancelled,
}
