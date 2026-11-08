use crate::errors::QueryError;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

// 教学用取消信号：一个原子标记，加一个可选的截止时间。
pub struct CancelToken {
    cancelled: AtomicBool,
    deadline: Option<Instant>,
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            deadline: None,
        }
    }

    pub fn with_timeout(timeout: Duration) -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            deadline: Some(Instant::now() + timeout),
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn check(&self) -> Result<(), QueryError> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(QueryError::Cancelled);
        }
        if let Some(deadline) = self.deadline {
            if Instant::now() >= deadline {
                return Err(QueryError::DeadlineExceeded);
            }
        }
        Ok(())
    }

    pub fn wait(&self, duration: Duration) -> Result<(), QueryError> {
        let finish = Instant::now() + duration;
        loop {
            self.check()?;
            let remaining = finish.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(());
            }

            // 把等待切成小段，以便周期性响应取消信号。
            let mut pause = remaining.min(Duration::from_millis(10));
            if let Some(deadline) = self.deadline {
                pause = pause.min(deadline.saturating_duration_since(Instant::now()));
            }
            thread::sleep(pause);
        }
    }
}
