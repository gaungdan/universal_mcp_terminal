//! 真实 Telnet 链路测试（进程级设备模拟器）。
//!
//! 默认忽略。先启动模拟设备：
//! ```powershell
//! python scripts/telnet_device_sim.py --port 2323 --user admin --password admin
//! ```
//! 再设置环境变量运行：
//! ```powershell
//! $env:UMT_TEST_TELNET_HOST="127.0.0.1"; $env:UMT_TEST_TELNET_PORT="2323"
//! $env:UMT_TEST_TELNET_USER="admin"; $env:UMT_TEST_TELNET_PASSWORD="admin"
//! cargo test -p umt-provider-telnet -- --ignored
//! ```
//!
//! 该测试走真实 TCP、真实 IAC 协商与提示符识别，仅设备本身是模拟器。

use std::collections::BTreeMap;

use serde_json::json;
use umt_core::ConnectionConfig;
use umt_protocol_api::{ConnectOptions, ExecRequest, ProtocolProvider, SecretValues};
use umt_provider_telnet::TelnetProvider;

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[tokio::test]
#[ignore = "需要真实 Telnet 目标：先启动 scripts/telnet_device_sim.py 并设置 UMT_TEST_TELNET_* 环境变量"]
async fn real_telnet_login_exec_and_paging() {
    let Some(host) = env("UMT_TEST_TELNET_HOST") else {
        eprintln!("未设置 UMT_TEST_TELNET_HOST，跳过 Telnet 真实链路测试");
        return;
    };
    let port = env("UMT_TEST_TELNET_PORT").unwrap_or_else(|| "23".into());
    let username = env("UMT_TEST_TELNET_USER").unwrap_or_else(|| "admin".into());
    let Some(password) = env("UMT_TEST_TELNET_PASSWORD") else {
        eprintln!("未设置 UMT_TEST_TELNET_PASSWORD，跳过 Telnet 真实链路测试");
        return;
    };

    let config = ConnectionConfig::new(
        "telnet",
        json!({
            "host": host,
            "port": port.parse::<u16>().expect("UMT_TEST_TELNET_PORT 非法"),
            "username": username,
            "password": password,
            "command_prompt": "[#$>] ?$",
        }),
    );

    let provider = TelnetProvider::new();
    let test = provider
        .test_connection(&config)
        .await
        .expect("连接测试调用失败");
    assert!(
        test.ok,
        "Telnet 连接测试失败：{} {:?}",
        test.message, test.steps
    );

    let mut secrets = BTreeMap::new();
    secrets.insert("password".to_string(), password);
    let options = ConnectOptions {
        secrets: SecretValues::new(secrets),
        ..Default::default()
    };
    let mut session = provider
        .connect(&config, options)
        .await
        .expect("建立 Telnet 会话失败");

    let version = session
        .exec(ExecRequest::new("show version"))
        .await
        .expect("执行 show version 失败");
    assert!(
        version.stdout.contains("UMT Simulator OS version"),
        "show version 输出不符合预期：{:?}",
        version.stdout
    );

    // 分页输出：设备每 20 行发送 --More--，Provider 应自动翻页并收齐内容。
    let config_dump = session
        .exec(ExecRequest::new("show running-config"))
        .await
        .expect("执行 show running-config 失败");
    assert!(
        config_dump.stdout.contains("GigabitEthernet0/60"),
        "分页输出不完整，长度={}，尾部={:?}",
        config_dump.stdout.len(),
        config_dump
            .stdout
            .chars()
            .rev()
            .take(80)
            .collect::<String>()
    );

    session.close().await.expect("关闭 Telnet 会话失败");
}
