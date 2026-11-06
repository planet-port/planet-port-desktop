# planet-port Windows 线上安装脚本
#
# 用法:
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Version 0.1.3]
#   (iex 管道无法向 param 块传 -Version,亦无法落地校验,故采用 -File 执行)
#
# 职责切分(与 mac/linux 的 install.sh 对齐):本脚本只负责「下载 + 解压 +
# 调起」,落盘/sc create/ACL/失败自拉起全部由 `planet-port service install`
# 自带(Windows 实现在 crates/cli/src/cmd/service/windows.rs),本脚本不重复
# sc.exe 逻辑(顶层 `planet-port install` 在 Windows 亦可用,本脚本选
# `service install` 是因为下载解压后无需 root 二次确认、错误提示更聚焦)。
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

# 0. 强制 TLS 1.2:Windows 10 旧版自带的 .NET 默认安全协议不含 TLS1.2,
#    直连 GitHub 会以难排查的连接错误失败;显式指定幂等无害
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# 1. 参数校验(-Version 格式与 release.sh 的 TAG 正则对齐,拦截手滑版本号)
if ($Version -ne "latest" -and $Version -notmatch '^v?[0-9]+\.[0-9]+\.[0-9]+(-beta\.[0-9]+)?$') {
    Write-Error "-Version 格式不合法:'$Version'。示例:0.1.3 / v0.1.3 / 0.1.3-beta.1(或省略该参数装 latest)"
}
# 容忍 v 前缀:下载 URL 的 tag 统一用不带 v 的纯版本号;校验用的 -notmatch 不区分
# 大小写,`-Version V0.1.3` 能过校验,这里两种前缀都得剥,否则拼下载地址 404
$Version = $Version.TrimStart('v', 'V')

# 2. 管理员检查(service install 内部同样校验,提前拦给明确提示)
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
if (-not (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error "需要管理员权限:请在管理员 PowerShell 中运行(右键开始菜单 -> 终端(管理员))"
}

# 3. 解析版本(GitHub API;latest 取最新非 prerelease,beta 不在此列)
if ($Version -eq "latest") {
    $rel = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
    $Version = $rel.tag_name.TrimStart('v', 'V')
}
$asset = "planet-port-x86_64-pc-windows-msvc.zip"
$url = "https://github.com/$repo/releases/download/v${Version}/${asset}"

# 4. 下载解压
$tmp = Join-Path $env:TEMP "planet-port-install"
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $tmp | Out-Null
Write-Host "下载 $url"
Invoke-WebRequest -UseBasicParsing $url -OutFile (Join-Path $tmp $asset)
Expand-Archive (Join-Path $tmp $asset) (Join-Path $tmp "pkg")
# dist 归档可能带顶层目录(planet-port-x86_64-pc-windows-msvc/),也可能顶层直放,
# 两者都兼容(与 mac/linux 的 install.sh 同一思路)
$pkg = Join-Path $tmp "pkg"
if (-not (Test-Path (Join-Path $pkg "planet-port.exe"))) {
    $found = Get-ChildItem $pkg -Recurse -Depth 2 -Filter "planet-port.exe" -File |
        Select-Object -First 1
    if ($found) { $pkg = $found.DirectoryName }
}

# 5. 包内容完整性预检(缺件时 service install 也会报,这里收集齐后给全量提示)
$missing = @()
foreach ($f in @("planet-port.exe", "planet-portd.exe", "wintun.dll")) {
    if (-not (Test-Path (Join-Path $pkg $f))) {
        $missing += $f
    }
}
if ($missing.Count -gt 0) {
    Write-Error "包内容不完整:缺少 $($missing -join ', ')(Release 资产可能未上传齐)"
}

# 6. 调起服务安装:从解压目录运行,service install 负责三件套拷贝到
#    C:\Program Files\planet-port、数据目录 ACL、sc create/failure/start
& (Join-Path $pkg "planet-port.exe") service install
if ($LASTEXITCODE -ne 0) {
    Write-Error "service install 失败(退出码 $LASTEXITCODE)"
}

Write-Host ""
Write-Host "安装完成:$Version(服务 planetportd 已启动)"
Write-Host "  CLI: C:\Program Files\planet-port\planet-port.exe(已加入 PATH,重开终端后 planet-port 可直接使用)"
Write-Host "  卸载: & 'C:\Program Files\planet-port\planet-port.exe' service uninstall"
