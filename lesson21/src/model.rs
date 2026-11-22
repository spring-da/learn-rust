use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Error { InvalidText, InvalidChannel }

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidText => write!(formatter, "message must contain 1 to 120 UTF-8 bytes after trimming"),
            Self::InvalidChannel => write!(formatter, "channel is empty"),
        }
    }
}
impl std::error::Error for Error {}

pub struct Message { text: String }

impl Message {
    pub fn new(text: &str) -> Result<Self, Error> {
        let text = text.trim();
        if text.is_empty() || text.len() > 120 { return Err(Error::InvalidText); }
        Ok(Self { text: text.into() })
    }
    pub fn text(&self) -> &str { &self.text }
}

#[derive(Debug)]
pub struct Receipt {
    pub channel: String,
    pub sequence: usize,
    pub text: String,
}
