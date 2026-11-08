use std::{error::Error, fmt, io};

#[derive(Debug)]
pub enum RepositoryError {
    Read(io::Error),
    Decode(serde_json::Error),
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(err) => write!(f, "read users: {err}"),
            Self::Decode(err) => write!(f, "decode users: {err}"),
        }
    }
}

impl Error for RepositoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Read(err) => Some(err),
            Self::Decode(err) => Some(err),
        }
    }
}

// ? 在传播错误时，使用这些 From 实现完成错误类型转换。
impl From<io::Error> for RepositoryError {
    fn from(err: io::Error) -> Self {
        Self::Read(err)
    }
}

impl From<serde_json::Error> for RepositoryError {
    fn from(err: serde_json::Error) -> Self {
        Self::Decode(err)
    }
}
