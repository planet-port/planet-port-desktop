# homebrew 公式模板,由 release.yml 的 suite job 渲染后推送
# planet-port/homebrew-tap;dist 公式的 checksum 是套装注入前的,故模板化。
# 渲染规则:sed 注入版本号占位,envsubst 注入 REPO/TAG 与四个 SHA 占位。
# 两个禁忌:注释不能含美元符大括号(envsubst 连注释一起替换)、不能含版本
# 占位字面量(sed 全文替换),否则提交进 tap 的文件头注释失真。
# 套装归档已由 suite job 剥掉 dist 顶层目录平铺,bin.install 直接取根部。
class PlanetPort < Formula
  desc "Multi-planet ZeroTier client (cli + daemon suite)"
  homepage "https://github.com/${REPO}"
  version "__VERSION__"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/${REPO}/releases/download/${TAG}/planet-port-x86_64-apple-darwin.tar.xz"
      sha256 "${X64_MAC_SHA}"
    else
      url "https://github.com/${REPO}/releases/download/${TAG}/planet-port-aarch64-apple-darwin.tar.xz"
      sha256 "${AARCH64_MAC_SHA}"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/${REPO}/releases/download/${TAG}/planet-port-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "${X64_LINUX_SHA}"
    else
      url "https://github.com/${REPO}/releases/download/${TAG}/planet-port-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "${ARM64_LINUX_SHA}"
    end
  end

  def install
    bin.install "planet-port", "planet-portd"
  end

  def caveats
    <<~EOS
      首次使用：sudo planet-port install（安装 daemon 服务与 PATH 入口）
    EOS
  end
end
