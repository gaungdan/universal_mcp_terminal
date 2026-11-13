use serde::{Deserialize, Serialize};

/// 连接测试结果。必须来自真实握手/认证，禁止只探测端口就返回成功。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    pub ok: bool,
    /// 面向用户的结论，例如"SSH 认证成功"。
    pub message: String,
    pub latency_ms: Option<u64>,
    /// 分步结果，用于前端展示失败发生在哪一步。
    pub steps: Vec<TestStep>,
    /// 失败时的排查建议。
    pub suggestion: Option<String>,
    /// 握手阶段观察到的 Host Key。存在时前端可以引导用户确认并固定指纹。
    #[serde(default)]
    pub host_key: Option<HostKeyInfo>,
}

/// 握手时观察到的服务端 Host Key。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostKeyInfo {
    pub fingerprint: String,
    #[serde(default)]
    pub algorithm: Option<String>,
}

impl ConnectionTestResult {
    pub fn success(message: impl Into<String>, latency_ms: u64, steps: Vec<TestStep>) -> Self {
        Self {
            ok: true,
            message: message.into(),
            latency_ms: Some(latency_ms),
            steps,
            suggestion: None,
            host_key: None,
        }
    }

    pub fn failure(
        message: impl Into<String>,
        suggestion: Option<String>,
        steps: Vec<TestStep>,
    ) -> Self {
        Self {
            ok: false,
            message: message.into(),
            latency_ms: None,
            steps,
            suggestion,
            host_key: None,
        }
    }

    /// 记录观察到的 Host Key，供 UI 引导用户信任。
    pub fn with_host_key(mut self, host_key: Option<HostKeyInfo>) -> Self {
        self.host_key = host_key;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestStep {
    pub name: String,
    pub status: TestStepStatus,
    pub message: Option<String>,
    pub duration_ms: Option<u64>,
}

impl TestStep {
    pub fn passed(name: impl Into<String>, duration_ms: Option<u64>) -> Self {
        Self {
            name: name.into(),
            status: TestStepStatus::Passed,
            message: None,
            duration_ms,
        }
    }

    pub fn failed(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: TestStepStatus::Failed,
            message: Some(message.into()),
            duration_ms: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestStepStatus {
    Passed,
    Failed,
    Skipped,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_result_carries_suggestion() {
        let result = ConnectionTestResult::failure(
            "SSH 认证失败",
            Some("检查用户名或密钥".into()),
            vec![TestStep::failed("authentication", "permission denied")],
        );
        assert!(!result.ok);
        assert_eq!(result.steps.len(), 1);
        assert!(result.suggestion.is_some());
    }
}
