# third_party/wintun

本目录存放 `wintun.dll`（wintun 0.14.1，amd64）与其许可文本 `LICENSE.txt`，
供 Windows 平台 tun 适配器（tun-rs 的 wintun 后端）运行期加载。发布打包时随
zip 分发，与两枚 exe 同目录落盘到 `C:\Program Files\planet-port\`（见
`crates/cli/src/cmd/service/windows.rs` 的 `WINTUN_DLL` 约定）。

- 文件：`wintun.dll`
- 版本：0.14.1（官方发布包 `wintun-0.14.1.zip` 中 `wintun/bin/amd64/wintun.dll`）
- 来源：https://www.wintun.net/
- SHA256：`e5da8447dc2c320edc0fc52fa01885c103de8c118481f683643cacc3220dafce`
  （`scripts/release.sh` 打包前校验，不符即中止）

## 再分发许可

wintun.dll **不是** MIT，随包分发依据官方发布物内的
`Prebuilt Binaries License`（本目录 `LICENSE.txt`，原样取自
`wintun-0.14.1.zip`）：其 3.d 条允许在「Software 与其他仅通过 Permitted API
（即 `wintun.h` 声明的接口）使用 Software 的软件一同分发」的前提下随附再分发。
本产品通过 tun-rs 的 wintun 后端按 `wintun.h` API 调用，未逆向、未修改、
未剥离声明，符合该条款；故允许原样随包分发。本产品不修改该 dll。
