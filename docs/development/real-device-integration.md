# 真实设备联调记录

本轮完成了 SSH 的真实服务验证，以及 Telnet 的进程级真实链路验证。
真实物理网络设备（交换机/路由器）仍需要用户提供可达地址与账号后复跑。

## 一、SSH：WSL sshd（真实服务）

环境：

- WSL2 Ubuntu，`sshd` 监听 `0.0.0.0:22`
- WSL 地址 `172.27.39.33`，用户 `guang`
- 使用 Ed25519 密钥认证（不污染用户密码）

步骤：

```powershell
# 1. 生成联调密钥并写入 WSL authorized_keys（脚本可重复执行）
powershell -ExecutionPolicy Bypass -File scripts/setup-ssh-wsl-integration.ps1

# 2. 运行被忽略的真实连接测试
$env:UMT_TEST_SSH_HOST="172.27.39.33"
$env:UMT_TEST_SSH_USER="guang"
$env:UMT_TEST_SSH_PRIVATE_KEY="d:\develop\GitHub\universal_mcp_terminal\backup\integration-ssh\id_ed25519"
cargo test -p umt-provider-ssh -- --ignored
```

结果：**通过**。覆盖内容：

1. `test_connection`：TCP → SSH 握手 → 返回 Host Key 指纹（`hostKey` 字段）→ 公钥认证成功
2. `connect`：使用上一步指纹做严格校验后建立会话
3. `resize(30,120)`
4. `exec("echo umt-ssh-ok")` 返回预期 stdout
5. 交互式 shell 写入 `echo umt-interactive-ok` 并读到回显
6. `close` 正常释放

联调完成后已执行清理，删除 `authorized_keys` 中的 `umt-integration` 标记行；
`backup/integration-ssh/` 中的私钥已在 `.gitignore` 排除范围（`backup/`）。

## 二、Telnet：进程级设备模拟器（真实 TCP 链路）

本机没有 Telnet 服务端（未安装 `telnetd`，Docker 未运行），因此使用独立进程模拟一台网络设备，
验证的是**真实 TCP + IAC 协商 + 登录交互 + 命令提示符 + 分页**，仅设备实现是模拟的。

```powershell
# 1. 启动模拟设备（另开终端）
python scripts/telnet_device_sim.py --port 2324 --user admin --password admin

# 2. 运行真实链路测试
$env:UMT_TEST_TELNET_HOST="127.0.0.1"
$env:UMT_TEST_TELNET_PORT="2324"
$env:UMT_TEST_TELNET_USER="admin"
$env:UMT_TEST_TELNET_PASSWORD="admin"
cargo test -p umt-provider-telnet -- --ignored
```

结果：**通过**。覆盖内容：

1. `test_connection`：TCP + 登录（`Username:` / `Password:` / `admin> `）成功
2. `exec("show version")` 返回设备版本信息
3. `exec("show running-config")`：设备每 20 行发送 `--More--`，Provider 自动发送空格翻页，
   最终收齐 60 行配置（断言末尾包含 `GigabitEthernet0/60`）
4. `close` 正常断开

## 三、真实物理设备仍需人工复跑

拿到设备后按以下清单验证，任一项失败请附 `test_connection` 的 `steps` 输出：

| 设备类型 | 检查项 |
|---|---|
| Cisco IOS | 登录提示 `Username:`、提示符 `#`、`--More--` 分页、`terminal length 0` 是否可用 |
| Huawei VRP | 提示符 `<Huawei>` / `[Huawei]`、`---- More ----` 分页 |
| H3C Comware | 提示符 `<H3C>`、`---- More ----` 分页 |
| 老旧设备 | 是否需要先发回车唤醒、登录提示是否需要自定义正则、是否禁用了回显 |

配置建议：

- `login_prompt` / `password_prompt` / `command_prompt` 都支持正则，按设备实际提示修改
- 设备若支持 `terminal length 0`，建议在远端账号初始化脚本中关闭分页，减少交互
- Telnet 无加密，仅应在可信网络或跳板机内使用
