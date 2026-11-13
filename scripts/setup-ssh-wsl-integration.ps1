<#
.SYNOPSIS
    为本机 WSL sshd 配置一个仅用于联调的 SSH 密钥。

.DESCRIPTION
    生成独立密钥（默认放在 backup/integration-ssh，已被 .gitignore 排除），
    把公钥追加到 WSL 用户的 ~/.ssh/authorized_keys（带 # umt-integration 标记），
    并打印运行真实 SSH 测试所需的命令。

    使用 -Cleanup 可以从 authorized_keys 中移除该标记行。

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts/setup-ssh-wsl-integration.ps1
    powershell -ExecutionPolicy Bypass -File scripts/setup-ssh-wsl-integration.ps1 -Cleanup
#>
param(
    [string]$Distro = "Ubuntu",
    [string]$User = "guang",
    [string]$KeyDir = "backup/integration-ssh",
    [switch]$Cleanup
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$keyDir = Join-Path $root $KeyDir
$keyPath = Join-Path $keyDir "id_ed25519"
$marker = "umt-integration"

function Invoke-Wsl([string]$Command) {
    wsl.exe -d $Distro -- bash -lc $Command
    if ($LASTEXITCODE -ne 0) { throw "WSL 命令执行失败：$Command" }
}

if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
    throw "未找到 wsl.exe，请在 Windows 上运行本脚本。"
}

if ($Cleanup) {
    Invoke-Wsl "sed -i '/$marker/d' ~/.ssh/authorized_keys 2>/dev/null || true; grep -c '$marker' ~/.ssh/authorized_keys 2>/dev/null || true"
    Write-Host "已从 $Distro 的 authorized_keys 中移除 $marker 标记行。"
    exit 0
}

if (-not (Test-Path $keyPath)) {
    New-Item -ItemType Directory -Force $keyDir | Out-Null
    & "$env:WINDIR\System32\OpenSSH\ssh-keygen.exe" -t ed25519 -N '""' -C $marker -f $keyPath | Out-Null
    Write-Host "已生成联调密钥：$keyPath"
}

$pubKey = (Get-Content "$keyPath.pub" -Raw).Trim()
$unixKeyPath = "/mnt/" + $keyDir.Substring(0, 1).ToLower() + ($keyDir.Substring(2) -replace '\\', '/')
Invoke-Wsl "mkdir -p ~/.ssh; chmod 700 ~/.ssh; touch ~/.ssh/authorized_keys; chmod 600 ~/.ssh/authorized_keys; grep -qF '$pubKey' ~/.ssh/authorized_keys || echo '$pubKey' >> ~/.ssh/authorized_keys"

$ip = (wsl.exe -d $Distro -- bash -lc "hostname -I" | Out-String).Trim().Split(" ")[0]
Write-Host ""
Write-Host "WSL 地址：$ip"
Write-Host "运行真实 SSH 测试："
Write-Host "  `$env:UMT_TEST_SSH_HOST='$ip'"
Write-Host "  `$env:UMT_TEST_SSH_USER='$User'"
Write-Host "  `$env:UMT_TEST_SSH_PRIVATE_KEY='$keyPath'"
Write-Host "  cargo test -p umt-provider-ssh -- --ignored"
Write-Host ""
Write-Host "清理：powershell -File scripts/setup-ssh-wsl-integration.ps1 -Cleanup"
