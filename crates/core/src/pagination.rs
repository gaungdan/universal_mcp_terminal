use serde::{Deserialize, Serialize};

/// 分页请求。`limit` 为 0 时使用默认值，且被强制上限约束。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    pub offset: u32,
    pub limit: u32,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 50,
        }
    }
}

impl PageRequest {
    pub const MAX_LIMIT: u32 = 500;

    pub fn new(offset: u32, limit: u32) -> Self {
        Self { offset, limit }
    }

    pub fn normalized(&self) -> Self {
        let limit = if self.limit == 0 {
            50
        } else {
            self.limit.min(Self::MAX_LIMIT)
        };
        Self {
            offset: self.offset,
            limit,
        }
    }
}

/// 分页结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: u64,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>, total: u64) -> Self {
        Self { items, total }
    }

    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            total: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_request_is_clamped() {
        assert_eq!(PageRequest::new(0, 0).normalized().limit, 50);
        assert_eq!(PageRequest::new(0, 99_999).normalized().limit, 500);
    }
}
