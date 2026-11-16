-- Token 权限等级与交互式终端开关。
-- 老 Token 迁移后默认只读，且交互式终端关闭。
ALTER TABLE tokens ADD COLUMN permission TEXT NOT NULL DEFAULT 'read_only'
    CHECK (permission IN ('read_only', 'write_access'));
ALTER TABLE tokens ADD COLUMN allow_interactive INTEGER NOT NULL DEFAULT 0;
