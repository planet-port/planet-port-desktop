# planet-port Windows 线上安装脚本
#
# 用法:
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Version 0.1.3]
#   (iex 一键流因本仓库资产走 GitHub Releases,被拦网络下不可用;落地执行为准)
#
# 职责切分(与 mac/linux 的 install.sh 对齐):本脚本只负责「下载 + 解压 +
# 调起」,落盘/sc create/ACL/失败自拉起全部由 `planet-port service install`
# 自带(Windows 实现在 crates/cli/src/cmd/service/windows.rs),本脚本不重复
# sc.exe 逻辑。注意顶层 `planet-port install` 在 Windows 下因 .exe 后缀定位
# 问题不可用,必须走 `service install` 子命令。
#
# 落点(与 windows.rs 约定一致):
#   程序 C:\Program Files\planet-port\
#   数据 C:\ProgramData\planet-port\(服务安装时自动收敛 ACL)
#   服务 planetportd(开机自启 + 5s 失败自拉起)
#
# 说明:浏览器/SmartScreen 可能拦下载的 zip 与脚本,如被拦请在
# 文件属性里解除锁定(Unblock)后重试。

param(
    [string]$Version = "latest"
)
$ErrorActionPreference = "Stop"
$repo = "planet-port/planet-port-desktop"

# 1. 管理员检查(service install 内部同样校验,提前拦给明确提示)
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "需要管理员权限:请在管理员 PowerShell 中运行(右键开始菜单 -> 终端(管理员))"
}

# 2. 解析版本(GitHub API;latest 取最新非 prerelease,beta 不在此列)
if ($Version -eq "latest") {
    $rel = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
    $Version = $rel.tag_name.TrimStart('v')
}
$asset = "planet-port_${Version}_windows_x86_64.zip"
$url = "https://github.com/$repo/releases/download/v${Version}/${asset}"

# 3. 下载解压
$tmp = Join-Path $env:TEMP "planet-port-install"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $tmp | Out-Null
Write-Host "下载 $url"
Invoke-WebRequest -UseBasicParsing $url -OutFile (Join-Path $tmp $asset)
Expand-Archive (Join-Path $tmp $asset) (Join-Path $tmp "pkg")
$pkg = Join-Path $tmp "pkg"

# 4. 包内容完整性预检(缺件时 service install 也会报,这里提前给全量提示)
foreach ($f in @("planet-port.exe", "planet-portd.exe", "wintun.dll")) {
    if (-not (Test-Path (Join-Path $pkg $f))) {
        Write-Error "包内容不完整:缺少 $f(Release 资产可能未上传齐)"
    }
}

# 5. 调起服务安装:从解压目录运行,service install 负责三件套拷贝到
#    C:\Program Files\planet-port、数据目录 ACL、sc create/failure/start
& (Join-Path $pkg "planet-port.exe") service install
if ($LASTEXITCODE -ne 0) {
    Write-Error "service install 失败(退出码 $LASTEXITCODE)"
}

Write-Host ""
Write-Host "安装完成:$Version(服务 planetportd 已启动)"
Write-Host "  CLI: C:\Program Files\planet-port\planet-port.exe(建议加入 PATH)"
Write-Host "  卸载: & 'C:\Program Files\planet-port\planet-port.exe' service uninstall"
