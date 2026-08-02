<#
Stitch 一键构建脚本（Windows · PowerShell）

用途：自动检测环境 → 安装缺失依赖 → 配置国内镜像 → 构建 → 启动
产物：target/release/stitch-desktop.exe（绿色版，双击即用）

用法：
  powershell -ExecutionPolicy Bypass -File install.ps1
或远程执行：
  irm https://raw.githubusercontent.com/iokcloud/stitch/main/scripts/install.ps1 | iex
#>

$ErrorActionPreference = "Stop"
$Repo = $PSScriptRoot

function Step($msg) { Write-Host "== $msg" -ForegroundColor Cyan }
function Ok($msg)   { Write-Host "  ok: $msg" -ForegroundColor Green }
function Warn($msg){ Write-Host "  !! $msg" -ForegroundColor Yellow }

Step "检查环境"

# 1. Rust
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    Ok "Rust 已安装 ($(cargo --version))"
} else {
    Warn "未找到 Rust，正在安装 rustup..."
    winget install Rustlang.Rustup --silent
    $env:Path = "$env:USERPROFILE\.cargo\bin;" + $env:Path
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Error "cargo 安装后未出现在 PATH，请重开终端后重跑本脚本"
    }
    Ok "Rust 已安装"
}

# 2. MSVC Build Tools（cargo 链接必需）
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vcInstalled = $false
if (Test-Path $vswhere) {
    $vcInstalled = & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Select-Object -First 1
}
if ($vcInstalled) {
    Ok "MSVC Build Tools 已安装"
} else {
    Warn "未找到 MSVC C++ 工具集——请手动安装 Visual Studio Build Tools（勾选 C++ 桌面开发工作负载）："
    Warn "  https://visualstudio.microsoft.com/visual-cpp-build-tools/"
    Write-Error "MSVC 是必需依赖，装好后重跑本脚本"
}

# 3. Node（前端构建）
if (Get-Command node -ErrorAction SilentlyContinue) {
    Ok "Node 已安装 ($(node --version))"
} else {
    Warn "未找到 Node，正在安装 LTS..."
    winget install OpenJS.NodeJS.LTS --silent
    $env:Path = "$env:ProgramFiles\nodejs;" + $env:Path
    if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
        Write-Error "Node 安装后未出现在 PATH，请重开终端后重跑本脚本"
    }
    Ok "Node 已安装"
}

# 4. WebView2（Win10/11 自带）
if (Test-Path "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}") {
    Ok "WebView2 Runtime 已安装"
} else {
    Warn "未检测到 WebView2 Runtime，下载安装（约 1 分钟）..."
    winget install Microsoft.EdgeWebView2Runtime --silent
}

# 5. 国内镜像（可选，网络慢时推荐）
$mirror = Read-Host "配置国内镜像（crates.io 清华 + npm 淘宝）？[y/N]"
if ($mirror -match "^[yY]") {
    $cfgDir = Join-Path $env:USERPROFILE ".cargo"
    $cfgFile = Join-Path $cfgDir "config.toml"
    New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null
    if (-not (Test-Path $cfgFile)) {
        @"
[source.crates-io]
replace-with = "tuna"
[source.tuna]
registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"
"@ | Set-Content -Path $cfgFile -Encoding UTF8
        Ok "crates.io 镜像已配置"
    }
    npm config set registry https://registry.npmmirror.com
    Ok "npm 镜像已配置"
}

Step "构建前端"
Set-Location (Join-Path $Repo "crates/stitch-desktop/frontend")
npm install
npm run build
Ok "前端构建完成"

Step "编译 Stitch（首次 5-15 分钟，请耐心）"
Set-Location $Repo
cargo build --release -p stitch-desktop
Ok "编译完成"

Step "完成"
$exe = Join-Path $Repo "target/release/stitch-desktop.exe"
Write-Host ""
Write-Host "Stitch 已就绪：$exe"
Write-Host "  绿色版，双击即可运行；数据保存在 %USERPROFILE%\.stitch"
Write-Host "  安装版（NSIS）在 target/release/bundle/nsis/"
Write-Host ""
$run = Read-Host "现在启动 Stitch？[Y/n]"
if ($run -notmatch "^[nN]") { Start-Process $exe }
