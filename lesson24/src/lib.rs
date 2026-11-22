mod store;
pub use store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum Error { Invalid, NotFound, Conflict, Full, Stale, Owned(String), Database(rusqlite::Error), Io(std::io::Error) }
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid => write!(f, "invalid task, key, time or capacity"),
            Self::NotFound => write!(f, "task not found"), Self::Conflict => write!(f, "idempotency key has different parameters"),
            Self::Full => write!(f, "unfinished task capacity is full"), Self::Stale => write!(f, "attempt is no longer current"),
            Self::Owned(error) => write!(f, "task database already has an owner: {error}"),
            Self::Database(error) => write!(f, "{error}"), Self::Io(error) => write!(f, "{error}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error { fn from(error: rusqlite::Error) -> Self { Self::Database(error) } }
impl From<std::io::Error> for Error { fn from(error: std::io::Error) -> Self { Self::Io(error) } }

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub kind: String, pub payload: String, pub max_attempts: i64,
    #[serde(default)] pub fail_until: i64,
    #[serde(default)] pub work_ms: i64,
    #[serde(default)] pub delay_ms: i64,
}
impl Spec {
    pub fn validate(&self) -> Result<(), Error> {
        if !matches!(self.kind.as_str(), "uppercase" | "flaky") || self.payload.is_empty() || self.payload.len() > 512 || !(1..=8).contains(&self.max_attempts) || !(0..=8).contains(&self.fail_until) || (self.kind == "uppercase" && self.fail_until != 0) || !(0..=2000).contains(&self.work_ms) || !(0..=60000).contains(&self.delay_ms) { return Err(Error::Invalid); }
        Ok(())
    }
}
pub fn validate_key(key: &str) -> Result<(), Error> {
    if key.is_empty() || key.len() > 64 || !key.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_') { return Err(Error::Invalid); }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct Task {
    pub id: i64, pub request_key: String,
    #[serde(flatten)] pub spec: Spec,
    pub status: String, pub attempts: i64, pub run_token: i64, pub next_run_at: i64,
    pub cancel_requested: bool, pub result: String, pub last_error: String,
}
impl Task { pub fn terminal(&self) -> bool { matches!(self.status.as_str(), "succeeded" | "failed" | "cancelled") } }
