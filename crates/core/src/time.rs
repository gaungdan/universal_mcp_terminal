use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// 统一时间戳：Unix 毫秒。
///
/// 数据库统一以 INTEGER 毫秒存储，避免秒/毫秒混用导致的时间错乱。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixMillis(pub i64);

impl UnixMillis {
    pub const ZERO: Self = Self(0);

    pub fn now() -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        Self(millis)
    }

    pub fn from_secs(secs: i64) -> Self {
        Self(secs.saturating_mul(1000))
    }

    pub fn as_secs(self) -> i64 {
        self.0 / 1000
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for UnixMillis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_after_epoch() {
        assert!(UnixMillis::now().0 > 0);
    }

    #[test]
    fn seconds_conversion_is_lossless_for_whole_seconds() {
        assert_eq!(UnixMillis::from_secs(3).as_secs(), 3);
    }
}
