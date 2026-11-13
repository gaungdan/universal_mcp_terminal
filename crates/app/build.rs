//! 为测试可执行文件内嵌 Common Controls v6 manifest。
//!
//! 背景：`umt-app` 依赖 `tauri`，其 Windows 后端静态导入
//! `comctl32.dll!TaskDialogIndirect`。该符号只在 Common Controls v6 中存在，
//! 而 cargo 生成的测试可执行文件默认没有 manifest，运行时会以
//! `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)` 退出。正式桌面程序由
//! `tauri-build` 写入 manifest，这里只为 `cargo test` 的测试二进制补齐。

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*" />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "gnu" {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("缺少 OUT_DIR"));
    let manifest = out_dir.join("umt-test.manifest");
    let resource = out_dir.join("umt-test.manifest.rc");
    let object = out_dir.join("umt-test.manifest.o");

    if fs::write(&manifest, MANIFEST).is_err() {
        println!("cargo:warning=无法写入测试 manifest，跳过内嵌");
        return;
    }
    let rc = format!(
        "1 24 \"{}\"\n",
        manifest.display().to_string().replace('\\', "/")
    );
    if fs::write(&resource, rc).is_err() {
        println!("cargo:warning=无法写入测试 manifest 资源脚本，跳过内嵌");
        return;
    }

    let status = Command::new("windres")
        .arg("-i")
        .arg(&resource)
        .arg("-o")
        .arg(&object)
        .status();

    match status {
        Ok(status) if status.success() => {
            // `rustc-link-arg-tests` 在部分 cargo 版本不会传递给 lib 单元测试目标，
            // 这里使用全目标的 link-arg；对 rlib 编译无副作用，只会影响最终链接。
            println!("cargo:rustc-link-arg={}", object.display());
        }
        _ => {
            println!(
                "cargo:warning=windres 不可用，测试可执行文件将缺少 Common Controls v6 manifest"
            );
        }
    }
}
