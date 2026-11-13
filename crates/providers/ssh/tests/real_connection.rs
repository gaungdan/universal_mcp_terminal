//! 真实 SSH 连接测试。
//!
//! 默认忽略；设置 `UMT_TEST_SSH_HOST/PORT/USER/PASSWORD` 后执行：
//! `cargo test -p umt-provider-ssh -- --ignored`。

use std::collections::BTreeMap;
use std::time::Duration;

use umt_core::ConnectionConfig;
use umt_protocol_api::{ConnectOptions, ExecRequest, ProtocolProvider, SecretValues};
use umt_provider_ssh::SshProvider;

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn json_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[tokio::test]
#[ignore = "需要真实 SSH 服务器：设置 UMT_TEST_SSH_HOST/PORT/USER/PASSWORD 后以 --ignored 运行"]
async fn real_ssh_test_connect_exec_and_interactive_shell() {
    let Some(host) = env("UMT_TEST_SSH_HOST") else {
        eprintln!("未设置 UMT_TEST_SSH_HOST，跳过真实连接测试");
        return;
    };
    let port = env("UMT_TEST_SSH_PORT").unwrap_or_else(|| "22".into());
    let username = env("UMT_TEST_SSH_USER").unwrap_or_else(|| "root".into());
    let Some(password) = std::env::var("UMT_TEST_SSH_PASSWORD").ok() else {
        eprintln!("未设置 UMT_TEST_SSH_PASSWORD，跳过真实连接测试");
        return;
    };

    let values = format!(
        r#"{{"host":"{}","port":{},"username":"{}","auth_type":"password","password":"{}"}}"#,
        json_escape(&host),
        port,
        json_escape(&username),
        json_escape(&password),
    );
    let config = ConnectionConfig::new("ssh", values.parse().expect("测试配置 JSON 非法"));
    let provider = SshProvider::new();

    let test = provider
        .test_connection(&config)
        .await
        .expect("连接测试调用失败");
    assert!(test.ok, "连接测试失败：{} {:?}", test.message, test.steps);

    let fingerprint = test
        .steps
        .iter()
        .find(|step| step.name == "Host Key 校验")
        .and_then(|step| step.message.clone())
        .expect("连接测试未返回 Host Key 指纹");

    let mut secrets = BTreeMap::new();
    secrets.insert("password".to_string(), password);
    let options = ConnectOptions {
        secrets: SecretValues::new(secrets),
        expected_host_key_fingerprint: Some(fingerprint),
        ..Default::default()
    };
    let mut session = provider
        .connect(&config, options)
        .await
        .expect("建立会话失败");

    session.resize(30, 120).await.expect("调整终端尺寸失败");

    let exec = session
        .exec(ExecRequest::new("echo umt-ssh-ok"))
        .await
        .expect("执行命令失败");
    assert!(
        exec.stdout.contains("umt-ssh-ok"),
        "exec 输出不符合预期：stdout={:?} stderr={:?}",
        exec.stdout,
        exec.stderr
    );

    session
        .write(b"echo umt-interactive-ok\n")
        .await
        .expect("写入交互终端失败");
    let mut cursor = 0;
    let mut output = String::new();
    for _ in 0..100 {
        let chunk = session.read(cursor, 8192).await.expect("读取交互终端失败");
        cursor = chunk.cursor;
        output.push_str(&chunk.text);
        if output.contains("umt-interactive-ok") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        output.contains("umt-interactive-ok"),
        "交互终端未回显预期内容：{output:?}"
    );

    session.close().await.expect("关闭会话失败");
}
