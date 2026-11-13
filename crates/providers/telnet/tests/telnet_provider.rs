//! Telnet Provider 的离线集成测试：用本机假设备验证登录、执行、分页与超时。

use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use umt_core::{Capability, ConnectionConfig, ExecutionState};
use umt_protocol_api::{
    ConnectOptions, ExecRequest, ProtocolProvider, ProviderError, SecretValues,
};
use umt_provider_telnet::TelnetProvider;

struct FakeDevice {
    port: u16,
    handle: tokio::task::JoinHandle<()>,
}

/// 在本机随机端口启动一个只接受一次连接的假 Telnet 设备。
async fn spawn_device<F, Fut>(handler: F) -> FakeDevice
where
    F: FnOnce(TcpStream) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send,
{
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("绑定测试端口失败");
    let address = listener.local_addr().expect("获取测试地址失败");
    let handle = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            handler(stream).await;
        }
    });
    FakeDevice {
        port: address.port(),
        handle,
    }
}

fn base_config(port: u16) -> ConnectionConfig {
    let mut config = ConnectionConfig {
        protocol_id: "telnet".into(),
        values: Default::default(),
    };
    config.values["host"] = "127.0.0.1".into();
    config.values["port"] = u64::from(port).into();
    config.values["connection_timeout_secs"] = 2u64.into();
    config
}

fn options_with_password(password: &str) -> ConnectOptions {
    let mut secrets = BTreeMap::new();
    secrets.insert("password".to_string(), password.to_string());
    ConnectOptions {
        secrets: SecretValues::new(secrets),
        ..ConnectOptions::default()
    }
}

/// 读取一行输入（兼容 CRLF），测试设备使用。
async fn read_line(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                if byte[0] != b'\r' {
                    bytes.push(byte[0]);
                }
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn descriptor_declares_required_capabilities_only() {
    let provider = TelnetProvider::new();
    let descriptor = provider.descriptor();
    assert_eq!(descriptor.protocol_id, "telnet");
    assert_eq!(descriptor.display_name, "Telnet");
    assert_eq!(descriptor.version, "1");
    assert!(descriptor.capabilities.contains(Capability::Shell));
    assert!(descriptor.capabilities.contains(Capability::Exec));
    assert!(
        descriptor
            .capabilities
            .contains(Capability::InteractiveTerminal)
    );
    assert!(!descriptor.capabilities.contains(Capability::TerminalResize));
    assert!(!descriptor.capabilities.contains(Capability::PortForward));
}

#[test]
fn config_schema_has_expected_fields() {
    let provider = TelnetProvider::new();
    let schema = provider.config_schema();
    for key in [
        "host",
        "port",
        "username",
        "password",
        "connection_timeout_secs",
        "login_prompt",
        "password_prompt",
        "command_prompt",
        "terminal_type",
    ] {
        assert!(schema.field(key).is_some(), "缺少字段 {key}");
    }
    let password = schema.field("password").expect("password 字段应存在");
    assert!(password.secret);
    assert!(!password.required);
}

#[tokio::test]
async fn schema_rejects_unknown_field() {
    let provider = TelnetProvider::new();
    let mut config = base_config(23);
    config.values["inject"] = true.into();
    let error = provider
        .validate_config(&config)
        .await
        .expect_err("未知字段应被拒绝");
    assert!(matches!(error, ProviderError::ConfigInvalid { field, .. } if field == "inject"));
}

#[tokio::test]
async fn schema_rejects_out_of_range_port() {
    let provider = TelnetProvider::new();
    let mut config = base_config(23);
    config.values["port"] = 70_000u64.into();
    let error = provider
        .validate_config(&config)
        .await
        .expect_err("端口越界应被拒绝");
    assert!(matches!(error, ProviderError::ConfigInvalid { field, .. } if field == "port"));
}

#[tokio::test]
async fn schema_rejects_invalid_prompt_regex() {
    let provider = TelnetProvider::new();
    let mut config = base_config(23);
    config.values["command_prompt"] = "[unclosed".into();
    let error = provider
        .validate_config(&config)
        .await
        .expect_err("非法正则应被拒绝");
    assert!(
        matches!(error, ProviderError::ConfigInvalid { field, .. } if field == "command_prompt")
    );
}

#[tokio::test]
async fn login_then_exec_returns_stdout() {
    let device = spawn_device(|mut stream| async move {
        stream
            .write_all(b"Router login: ")
            .await
            .expect("发送登录提示失败");
        let username = read_line(&mut stream).await;
        if username != "admin" {
            stream
                .write_all(b"Login invalid\r\nRouter login: ")
                .await
                .expect("发送拒绝提示失败");
            return;
        }
        stream
            .write_all(b"Password: ")
            .await
            .expect("发送密码提示失败");
        let password = read_line(&mut stream).await;
        if password != "secret" {
            stream
                .write_all(b"Login invalid\r\nRouter login: ")
                .await
                .expect("发送拒绝提示失败");
            return;
        }
        stream
            .write_all(b"\r\nWelcome to Router\r\nRouter# ")
            .await
            .expect("发送命令提示符失败");
        let command = read_line(&mut stream).await;
        if command == "show clock" {
            stream
                .write_all(b"12:00:00 UTC\r\nRouter# ")
                .await
                .expect("发送命令输出失败");
        }
    })
    .await;

    let provider = TelnetProvider::new();
    let mut config = base_config(device.port);
    config.values["username"] = "admin".into();
    let mut session = provider
        .connect(&config, options_with_password("secret"))
        .await
        .expect("登录应成功");

    let banner = session.read(0, 4096).await.expect("读取登录输出失败");
    assert!(banner.text.contains("Welcome to Router"));
    assert!(banner.text.contains("Router#"));
    assert_eq!(banner.cursor, banner.text.len() as u64);

    let mut request = ExecRequest::new("show clock");
    request.timeout_ms = 2_000;
    let result = session.exec(request).await.expect("命令执行失败");
    assert_eq!(result.state, ExecutionState::Succeeded);
    assert!(result.stdout.contains("12:00:00 UTC"));
    assert!(!result.stdout.contains("Router#"));

    session.close().await.expect("关闭会话失败");
}

#[tokio::test]
async fn pager_prompt_triggers_space_and_is_recorded() {
    let device = spawn_device(|mut stream| async move {
        stream
            .write_all(b"Router# ")
            .await
            .expect("发送命令提示符失败");
        let command = read_line(&mut stream).await;
        if command != "show run" {
            return;
        }
        stream
            .write_all(b"interface GigabitEthernet0/1\r\n--More--")
            .await
            .expect("发送第一页失败");

        // 等待会话自动发送的空格。
        let mut byte = [0u8; 1];
        loop {
            match stream.read(&mut byte).await {
                Ok(0) | Err(_) => return,
                Ok(_) if byte[0] == b' ' => break,
                Ok(_) => {}
            }
        }

        stream
            .write_all(b"\rinterface GigabitEthernet0/2\r\nRouter# ")
            .await
            .expect("发送第二页失败");
    })
    .await;

    let provider = TelnetProvider::new();
    let config = base_config(device.port);
    let mut session = provider
        .connect(&config, ConnectOptions::default())
        .await
        .expect("无需登录时应直接成功");

    let mut request = ExecRequest::new("show run");
    request.timeout_ms = 3_000;
    let result = session.exec(request).await.expect("命令执行失败");
    assert_eq!(result.state, ExecutionState::Succeeded);
    assert!(result.stdout.contains("GigabitEthernet0/1"));
    assert!(result.stdout.contains("GigabitEthernet0/2"));
    assert!(
        result.stdout.contains("[umt:"),
        "自动翻页应记录到输出：{}",
        result.stdout
    );
}

#[tokio::test]
async fn login_timeout_maps_to_provider_timeout() {
    let device = spawn_device(|mut stream| async move {
        // 保持连接但始终不发送任何提示。
        tokio::time::sleep(Duration::from_secs(5)).await;
        let _ = stream.write_all(b"late banner").await;
    })
    .await;

    let provider = TelnetProvider::new();
    let mut config = base_config(device.port);
    config.values["connection_timeout_secs"] = 1u64.into();

    let result = provider.connect(&config, ConnectOptions::default()).await;
    assert!(matches!(result, Err(ProviderError::Timeout(_))));
    device.handle.abort();
}

#[tokio::test]
async fn exec_timeout_returns_timed_out_state() {
    let device = spawn_device(|mut stream| async move {
        stream.write_all(b"# ").await.expect("发送命令提示符失败");
        tokio::time::sleep(Duration::from_secs(3)).await;
    })
    .await;

    let provider = TelnetProvider::new();
    let config = base_config(device.port);
    let mut session = provider
        .connect(&config, ConnectOptions::default())
        .await
        .expect("无需登录时应直接成功");

    let mut request = ExecRequest::new("uname -a");
    request.timeout_ms = 200;
    let result = session.exec(request).await.expect("命令执行应返回结果");
    assert_eq!(result.state, ExecutionState::TimedOut);
    device.handle.abort();
}

#[tokio::test]
async fn resize_is_unsupported_and_close_releases_socket() {
    let device = spawn_device(|mut stream| async move {
        stream.write_all(b"# ").await.expect("发送命令提示符失败");
        let mut buffer = [0u8; 64];
        while let Ok(received) = stream.read(&mut buffer).await {
            if received == 0 {
                break;
            }
        }
    })
    .await;

    let provider = TelnetProvider::new();
    let config = base_config(device.port);
    let mut session = provider
        .connect(&config, ConnectOptions::default())
        .await
        .expect("无需登录时应直接成功");

    let error = session.resize(40, 120).await.expect_err("resize 应失败");
    assert!(matches!(
        error,
        umt_protocol_api::SessionError::UnsupportedCapability(Capability::TerminalResize)
    ));
    session.close().await.expect("关闭会话失败");
    device.handle.abort();
}

#[tokio::test]
async fn test_connection_reports_step_results() {
    let device = spawn_device(|mut stream| async move {
        stream
            .write_all(b"login: ")
            .await
            .expect("发送登录提示失败");
        let _ = read_line(&mut stream).await;
        stream.write_all(b"# ").await.expect("发送命令提示符失败");
        tokio::time::sleep(Duration::from_secs(1)).await;
    })
    .await;

    let provider = TelnetProvider::new();
    let mut config = base_config(device.port);
    config.values["username"] = "admin".into();

    let result = provider
        .test_connection(&config)
        .await
        .expect("测试连接不应返回内部错误");
    assert!(result.ok);
    assert_eq!(result.steps.len(), 2);
    assert!(
        result
            .steps
            .iter()
            .all(|step| step.status == umt_protocol_api::TestStepStatus::Passed)
    );
    assert!(
        result
            .suggestion
            .as_deref()
            .is_some_and(|text| text.contains("无加密"))
    );
    device.handle.abort();
}

#[tokio::test]
async fn test_connection_failure_is_not_reported_as_ok() {
    let device = spawn_device(|_stream| async move {
        // 立即断开，模拟设备拒绝连接后的登录失败。
    })
    .await;

    let provider = TelnetProvider::new();
    let config = base_config(device.port);

    let result = provider
        .test_connection(&config)
        .await
        .expect("测试连接不应返回内部错误");
    assert!(!result.ok);
    assert!(
        result
            .steps
            .iter()
            .any(|step| step.status == umt_protocol_api::TestStepStatus::Failed)
    );
    assert!(result.suggestion.is_some());
}
