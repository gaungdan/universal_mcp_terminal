-- Schema v2：把审计结果（outcome）从 status 的「outcome|state」联合编码中拆出来。
-- 只新增列并回填，不修改 0001 的已发布内容；sqlx 通过迁移记录保证本文件只执行一次。

ALTER TABLE audit_events ADD COLUMN outcome TEXT NOT NULL DEFAULT 'allowed';

-- 旧数据形如 status = 'allowed|succeeded'：分隔符前是 outcome，分隔符后是执行状态。
-- 不含分隔符（或分隔符位置异常）的行保持 status 原值、outcome 使用默认值。
UPDATE audit_events
SET outcome = substr(status, 1, instr(status, '|') - 1),
    status  = substr(status, instr(status, '|') + 1)
WHERE instr(status, '|') > 1
  AND instr(status, '|') < length(status);

CREATE INDEX IF NOT EXISTS idx_audit_outcome ON audit_events(outcome);
