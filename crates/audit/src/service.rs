use crate::redact::Redactor;
use std::sync::Arc;
use umt_core::{AuditId, AuditQuery, AuditRecord, Page, UmtError};
use umt_persistence::{AuditRepo, NewAuditEntry};

/// 单条输出保留的最大字符数，避免审计表被超大输出撑爆。
pub const MAX_EXCERPT_CHARS: usize = 8_192;

/// 审计服务：所有写入都会先脱敏，所有读取都只读。
pub struct AuditService {
    repo: Arc<dyn AuditRepo>,
    redactor: Redactor,
}

impl AuditService {
    pub fn new(repo: Arc<dyn AuditRepo>) -> Self {
        Self {
            repo,
            redactor: Redactor::new(),
        }
    }

    /// 追加审计记录。命令、工作目录、输出、错误信息统一脱敏后再落库。
    pub async fn record(&self, mut entry: NewAuditEntry) -> Result<AuditId, UmtError> {
        let mut redacted = entry.redacted;

        if let (Some(text), did) = self.redactor.redact_opt(entry.command.as_deref()) {
            entry.command = Some(text);
            redacted |= did;
        }
        if let (Some(text), did) = self.redactor.redact_opt(entry.working_dir.as_deref()) {
            entry.working_dir = Some(text);
            redacted |= did;
        }
        if let (Some(text), did) = self.redactor.redact_opt(entry.stdout_excerpt.as_deref()) {
            entry.stdout_excerpt = Some(truncate(text));
            redacted |= did;
        }
        if let (Some(text), did) = self.redactor.redact_opt(entry.stderr_excerpt.as_deref()) {
            entry.stderr_excerpt = Some(truncate(text));
            redacted |= did;
        }
        if let (Some(text), did) = self.redactor.redact_opt(entry.error.as_deref()) {
            entry.error = Some(text);
            redacted |= did;
        }
        entry.redacted = redacted;

        Ok(self.repo.append(entry).await?)
    }

    pub async fn query(&self, query: &AuditQuery) -> Result<Page<AuditRecord>, UmtError> {
        Ok(self.repo.query(query).await?)
    }

    pub async fn get(&self, id: AuditId) -> Result<AuditRecord, UmtError> {
        self.repo.get(id).await?.ok_or_else(|| UmtError::NotFound {
            entity: "审计记录",
            id: id.to_string(),
        })
    }

    pub async fn count(&self) -> Result<u64, UmtError> {
        Ok(self.repo.count().await?)
    }

    /// 导出为 CSV 文本，调用方负责写文件。内容已脱敏。
    pub async fn export_csv(
        &self,
        query: &AuditQuery,
        limit: u32,
    ) -> Result<(String, u64), UmtError> {
        let records = self
            .repo
            .query(&AuditQuery {
                limit: limit.min(10_000),
                ..query.clone()
            })
            .await?
            .items;
        let mut csv = String::from(
            "id,started_at,kind,outcome,state,session_id,token_id,connection_id,command,exit_code,error\n",
        );
        for record in &records {
            csv.push_str(&format!(
                "{},{},{:?},{:?},{:?},{},{},{},{},{},{}\n",
                record.id.0,
                record.started_at.0,
                record.kind,
                record.outcome,
                record.state,
                optional(&record.session_id.map(|v| v.to_string())),
                optional(&record.token_id.map(|v| v.to_string())),
                optional(&record.connection_id.map(|v| v.to_string())),
                escape_csv(record.command.as_deref()),
                record.exit_code.map(|v| v.to_string()).unwrap_or_default(),
                escape_csv(record.error.as_deref()),
            ));
        }
        Ok((csv, records.len() as u64))
    }
}

fn optional(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("")
}

fn escape_csv(value: Option<&str>) -> String {
    match value {
        None => String::new(),
        Some(text) => format!("\"{}\"", text.replace('"', "\"\"")),
    }
}

fn truncate(text: String) -> String {
    if text.chars().count() <= MAX_EXCERPT_CHARS {
        return text;
    }
    let mut truncated: String = text.chars().take(MAX_EXCERPT_CHARS).collect();
    truncated.push_str("…(已截断)");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use umt_core::{AuditKind, AuditOutcome, ExecutionState, UnixMillis};
    use umt_persistence::InMemoryRepos;

    fn entry(stdout: Option<&str>) -> NewAuditEntry {
        NewAuditEntry {
            session_id: None,
            token_id: None,
            connection_id: None,
            kind: AuditKind::CommandExecuted,
            outcome: AuditOutcome::Allowed,
            command: Some("mysql -u root --password=topsecret".into()),
            working_dir: None,
            started_at: UnixMillis::now(),
            duration_ms: Some(12),
            state: ExecutionState::Succeeded,
            exit_code: Some(0),
            stdout_excerpt: stdout.map(str::to_string),
            stderr_excerpt: None,
            error: None,
            redacted: false,
        }
    }

    #[tokio::test]
    async fn record_redacts_before_persisting() {
        let repos = InMemoryRepos::new();
        let service = AuditService::new(repos.audit.clone());
        let id = service
            .record(entry(Some("token=abcdef123456")))
            .await
            .expect("写入失败");
        let record = service.get(id).await.expect("读取失败");
        assert!(record.redacted);
        assert!(!record.command.as_deref().unwrap().contains("topsecret"));
        assert!(
            !record
                .stdout_excerpt
                .as_deref()
                .unwrap()
                .contains("abcdef123456")
        );
    }

    #[tokio::test]
    async fn export_csv_contains_header_and_rows() {
        let repos = InMemoryRepos::new();
        let service = AuditService::new(repos.audit.clone());
        service.record(entry(None)).await.expect("写入失败");
        let (csv, rows) = service
            .export_csv(&AuditQuery::default(), 100)
            .await
            .expect("导出失败");
        assert_eq!(rows, 1);
        assert!(csv.starts_with("id,started_at,kind"));
        assert!(!csv.contains("topsecret"));
    }
}
