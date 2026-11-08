use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryError {
    Cancelled,
    DeadlineExceeded,
    BackendUnavailable,
}

impl QueryError {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cancelled => "cancelled",
            Self::DeadlineExceeded => "deadline-exceeded",
            Self::BackendUnavailable => "backend-unavailable",
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for QueryError {}
