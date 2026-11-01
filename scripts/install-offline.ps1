# planet-port Windows 离线安装脚本(随发布 zip 分发)
#
# 用法:右键开始菜单 -> 终端(管理员),cd 到 zip 解压目录后执行:
#   powershell -ExecutionPolicy Bypass -File .\install-offline.ps1
#
# 前提:本脚本必须与 planet-port.exe / planet-portd.exe / wintun.dll 同目录
# (即 zip 解压后的原样目录),依赖项缺失会报错退出。
#
# 职责切分(与 mac/linux 的 install.sh 对齐):落盘/sc create/ACL/失败自拉起
# 全部由 `planet-port service install` 自带(crates/cli/src/cmd/service/
# windows.rs),本脚本只负责完整性预检与调起,不重复 sc.exe 逻辑。顶层
# `planet-port install` 在 Windows 下因 .exe 后缀定位问题不可用,必须走
# `service install` 子命令。
#
# 落点:
#   程序 C:\Program Files\planet-port\
#   数据 C:\ProgramData\planet-port\(服务安装时自动收敛 ACL)
#   服务 planetportd(开机自启 + 5s 失败自拉起)

$ErrorActionPreference = "Stop"

# 1. 管理员检查(service install 内部同样校验,提前拦给明确提示)
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "需要管理员权限:请在管理员 PowerShell 中运行(右键开始菜单 -> 终端(管理员))"
}

# 2. 同目录三件套完整性预检(收集齐后一次性给出全量缺件清单)
$missing = @()
foreach ($f in @("planet-port.exe", "planet-portd.exe", "wintun.dll")) {
    if (-not (Test-Path (Join-Path $PSScriptRoot $f))) {
        $missing += $f
    }
}
if ($missing.Count -gt 0) {
    Write-Error "包内容不完整:当前目录缺少 $($missing -join ', ')(请解压完整 zip 后重试)"
}

# 3. 调起服务安装:同目录运行,service install 负责三件套拷贝到
#    C:\Program Files\planet-port、数据目录 ACL、sc create/failure/start
& (Join-Path $PSScriptRoot "planet-port.exe") service install
if ($LASTEXITCODE -ne 0) {
    Write-Error "service install 失败(退出码 $LASTEXITCODE)"
}

Write-Host ""
Write-Host "安装完成(服务 planetportd 已启动)"
Write-Host "  CLI: C:\Program Files\planet-port\planet-port.exe(建议加入 PATH)"
Write-Host "  卸载: & 'C:\Program Files\planet-port\planet-port.exe' service uninstall"
