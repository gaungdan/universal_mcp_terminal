//! 真实 SSH 连接测试。
//!
//! 默认忽略。两种认证方式任选其一：
//!
//! - 密码：设置 `UMT_TEST_SSH_HOST/PORT/USER/PASSWORD`
//! - 私钥：设置 `UMT_TEST_SSH_HOST/PORT/USER/PRIVATE_KEY`（可选 `PRIVATE_KEY_PASSPHRASE`）
//!
//! 运行：`cargo test -p umt-provider-ssh -- --ignored`
//!
//! 本机联调示例（WSL sshd）：
//! ```powershell
//! $env:UMT_TEST_SSH_HOST="172.27.39.33"
//! $env:UMT_TEST_SSH_USER="guang"
//! $env:UMT_TEST_SSH_PRIVATE_KEY="d:\...\backup\integration-ssh\id_ed25519"
//! cargo test -p umt-provider-ssh -- --ignored
//! ```

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::json;
use umt_core::ConnectionConfig;
use umt_protocol_api::{ConnectOptions, ExecRequest, ProtocolProvider, SecretValues};
use umt_provider_ssh::SshProvider;

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 认证材料。私钥从文件读取，只放在 `SecretValues` 与测试配置中，不写入日志。
enum AuthMaterial {
    Password(String),
    PrivateKey {
        key: String,
        passphrase: Option<String>,
    },
}

fn load_auth() -> Option<AuthMaterial> {
    if let Some(path) = env("UMT_TEST_SSH_PRIVATE_KEY") {
        let key = std::fs::read_to_string(&path).expect("读取私钥文件失败");
        return Some(AuthMaterial::PrivateKey {
            key,
            passphrase: std::env::var("UMT_TEST_SSH_PRIVATE_KEY_PASSPHRASE")
                .ok()
                .filter(|value| !value.is_empty()),
        });
    }
    std::env::var("UMT_TEST_SSH_PASSWORD")
        .ok()
        .map(AuthMaterial::Password)
}

#[tokio::test]
#[ignore = "需要真实 SSH 服务器：设置 UMT_TEST_SSH_HOST/PORT/USER 与 PASSWORD 或 PRIVATE_KEY 后以 --ignored 运行"]
async fn real_ssh_test_connect_exec_and_interactive_shell() {
    let Some(host) = env("UMT_TEST_SSH_HOST") else {
        eprintln!("未设置 UMT_TEST_SSH_HOST，跳过真实连接测试");
        return;
    };
    let port = env("UMT_TEST_SSH_PORT").unwrap_or_else(|| "22".into());
    let username = env("UMT_TEST_SSH_USER").unwrap_or_else(|| "root".into());
    let Some(auth) = load_auth() else {
        eprintln!("未设置 UMT_TEST_SSH_PASSWORD 或 UMT_TEST_SSH_PRIVATE_KEY，跳过真实连接测试");
        return;
    };

    let (values, secrets) = match &auth {
        AuthMaterial::Password(password) => (
            json!({
                "host": host,
                "port": port.parse::<u16>().expect("UMT_TEST_SSH_PORT 非法"),
                "username": username,
                "auth_type": "password",
                "password": password,
            }),
            BTreeMap::from([("password".to_string(), password.clone())]),
        ),
        AuthMaterial::PrivateKey { key, passphrase } => {
            let mut secrets = BTreeMap::from([("private_key".to_string(), key.clone())]);
            if let Some(passphrase) = passphrase {
                secrets.insert("private_key_passphrase".to_string(), passphrase.clone());
            }
            (
                json!({
                    "host": host,
                    "port": port.parse::<u16>().expect("UMT_TEST_SSH_PORT 非法"),
                    "username": username,
                    "auth_type": "private_key",
                    "private_key": key,
                    "private_key_passphrase": passphrase,
                }),
                secrets,
            )
        }
    };

    let config = ConnectionConfig::new("ssh", values);
    let provider = SshProvider::new();

    let test = provider
        .test_connection(&config)
        .await
        .expect("连接测试调用失败");
    assert!(test.ok, "连接测试失败：{} {:?}", test.message, test.steps);

    let fingerprint = test
        .host_key
        .as_ref()
        .map(|host_key| host_key.fingerprint.clone())
        .or_else(|| {
            test.steps
                .iter()
                .find(|step| step.name == "Host Key 校验")
                .and_then(|step| step.message.clone())
        })
        .expect("连接测试未返回 Host Key 指纹");

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
