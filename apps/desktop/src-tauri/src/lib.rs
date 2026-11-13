//! 桌面外壳：初始化应用服务并把窗口交给 `umt-app` 编排层。

#![deny(unsafe_code)]

use std::time::Duration;

use tauri::Manager;
use umt_app::{AppState, default_data_dir};

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .setup(|app| {
            let data_dir = default_data_dir();
            tracing::info!(dir = %data_dir.display(), "初始化应用数据目录");

            let state = tauri::async_runtime::block_on(AppState::initialize(&data_dir))?;
            spawn_idle_reaper(&state);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            umt_app::ipc::commands::get_overview,
            umt_app::ipc::commands::list_protocols,
            umt_app::ipc::commands::list_connections,
            umt_app::ipc::commands::get_connection,
            umt_app::ipc::commands::create_connection,
            umt_app::ipc::commands::update_connection,
            umt_app::ipc::commands::delete_connection,
            umt_app::ipc::commands::set_connection_enabled,
            umt_app::ipc::commands::duplicate_connection,
            umt_app::ipc::commands::test_connection,
            umt_app::ipc::commands::test_connection_draft,
            umt_app::ipc::commands::list_host_keys,
            umt_app::ipc::commands::trust_host_key,
            umt_app::ipc::commands::delete_host_key,
            umt_app::ipc::commands::list_tokens,
            umt_app::ipc::commands::create_token,
            umt_app::ipc::commands::get_token_mcp_config,
            umt_app::ipc::commands::update_token,
            umt_app::ipc::commands::set_token_status,
            umt_app::ipc::commands::delete_token,
            umt_app::ipc::commands::list_token_grants,
            umt_app::ipc::commands::set_token_grant,
            umt_app::ipc::commands::list_active_sessions,
            umt_app::ipc::commands::list_session_history,
            umt_app::ipc::commands::close_session,
            umt_app::ipc::commands::query_audit,
            umt_app::ipc::commands::get_audit,
            umt_app::ipc::commands::export_audit,
            umt_app::ipc::commands::get_mcp_status,
            umt_app::ipc::commands::start_mcp,
            umt_app::ipc::commands::stop_mcp,
            umt_app::ipc::commands::update_mcp_settings,
            umt_app::ipc::commands::get_settings,
            umt_app::ipc::commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}

/// 每 60 秒回收一次空闲会话；失败只记日志，不影响主进程。
fn spawn_idle_reaper(state: &AppState) {
    let sessions = state.sessions.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let closed = sessions.reap_idle().await;
            if closed > 0 {
                tracing::info!(closed, "已回收空闲会话");
            }
        }
    });
}
