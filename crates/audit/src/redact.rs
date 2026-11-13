use regex::Regex;

/// 脱敏结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionOutcome {
    pub text: String,
    pub redacted: bool,
}

/// 审计脱敏器。
///
/// 规则覆盖：`password=...` 之类的键值对、Bearer Token、`umt_` Token、
/// PEM 私钥块、SSH 私钥内容。宁可多脱敏，也不能漏。
pub struct Redactor {
    rules: Vec<(Regex, &'static str)>,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new()
    }
}

impl Redactor {
    pub fn new() -> Self {
        let rules = vec![
            (
                Regex::new(
                    r#"(?i)\b(password|passwd|pwd|passphrase|secret|token|api[_-]?key)\b\s*[=:]\s*("[^"]*"|'[^']*'|\S+)"#,
                )
                .expect("脱敏规则编译失败"),
                "${1}=***",
            ),
            (
                Regex::new(r"(?i)\bbearer\s+[A-Za-z0-9._\-]+").expect("脱敏规则编译失败"),
                "Bearer ***",
            ),
            (
                Regex::new(r"umt_[0-9a-fA-F]{8}_[0-9a-fA-F]{64}").expect("脱敏规则编译失败"),
                "umt_***",
            ),
            (
                Regex::new(
                    r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
                )
                .expect("脱敏规则编译失败"),
                "-----BEGIN PRIVATE KEY-----***-----END PRIVATE KEY-----",
            ),
            (
                Regex::new(r"(?m)^(ssh-(rsa|ed25519)|ecdsa-sha2-\S+)\s+[A-Za-z0-9+/=]+").expect(
                    "脱敏规则编译失败",
                ),
                "$1 ***",
            ),
        ];
        Self { rules }
    }

    /// 对任意文本执行脱敏，并返回是否发生过替换。
    pub fn redact(&self, input: &str) -> RedactionOutcome {
        let mut text = input.to_string();
        let mut redacted = false;
        for (rule, replacement) in &self.rules {
            if rule.is_match(&text) {
                text = rule.replace_all(&text, *replacement).into_owned();
                redacted = true;
            }
        }
        RedactionOutcome { text, redacted }
    }

    /// 对可选文本脱敏。
    pub fn redact_opt(&self, input: Option<&str>) -> (Option<String>, bool) {
        match input {
            None => (None, false),
            Some(text) => {
                let outcome = self.redact(text);
                (Some(outcome.text), outcome.redacted)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_password_assignments() {
        let outcome = Redactor::new().redact("mysql -u root --password=Sup3rS3cret -h db");
        assert!(outcome.redacted);
        assert!(!outcome.text.contains("Sup3rS3cret"));
        assert!(outcome.text.contains("password=***"));
    }

    #[test]
    fn masks_bearer_and_generated_tokens() {
        let redactor = Redactor::new();
        let token = format!("umt_{}_{}", "a".repeat(8), "b".repeat(64));
        let outcome = redactor.redact(&format!("Authorization: Bearer {token}"));
        assert!(outcome.redacted);
        assert!(!outcome.text.contains(&token));
    }

    #[test]
    fn masks_private_key_blocks() {
        let text = "-----BEGIN OPENSSH PRIVATE KEY-----\nAAAA\n-----END OPENSSH PRIVATE KEY-----";
        let outcome = Redactor::new().redact(text);
        assert!(outcome.redacted);
        assert!(!outcome.text.contains("AAAA"));
    }

    #[test]
    fn leaves_normal_commands_untouched() {
        let outcome = Redactor::new().redact("systemctl status nginx");
        assert!(!outcome.redacted);
        assert_eq!(outcome.text, "systemctl status nginx");
    }
}
