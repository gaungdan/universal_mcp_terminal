-- 审计补充 MCP 工具名与 Token 权限上下文。
ALTER TABLE audit_events ADD COLUMN tool_name TEXT;
ALTER TABLE audit_events ADD COLUMN permission TEXT;
