#!/usr/bin/env bash
# 打包 macOS 应用:scripts/bundle.sh [--dmg]
# 产物 dist/Gitlane.app;加 --dmg 同时生成 dist/Gitlane-<版本>.dmg(应用内更新读取的安装包)。
# 签名:设置 DEVELOPER_ID="Developer ID Application: 名字 (TEAMID)" 用开发者证书,缺省为临时签名。不公证。
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)"/\1/')
# 构建号取提交数,单调递增;编进二进制,「检查更新」用它区分同版本号的构建
BUILD="${GITLANE_BUILD:-$(git rev-list --count HEAD 2>/dev/null || echo 0)}"
export GITLANE_BUILD="$BUILD"
echo "==> 编译发布版 (v$VERSION, build $BUILD)"
cargo build --release -p ui

APP=dist/Gitlane.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/gitlane "$APP/Contents/MacOS/gitlane"
# 应用图标(浅色版;运行中 Dock 图标随主题切换)。重新生成图标:assets/icon/build.sh
cp assets/icon/Gitlane.icns "$APP/Contents/Resources/Gitlane.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Gitlane</string>
  <key>CFBundleDisplayName</key><string>Gitlane</string>
  <key>CFBundleIdentifier</key><string>dev.local.gitlane</string>
  <key>CFBundleVersion</key><string>${BUILD}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleExecutable</key><string>gitlane</string>
  <key>CFBundleIconFile</key><string>Gitlane</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST

# 签名必须在写完 Info.plist 之后;应用内更新会用 codesign --verify 校验解包出的 app
IDENTITY="${DEVELOPER_ID:--}"
if [ "$IDENTITY" = "-" ]; then
  codesign --force --sign - "$APP" && echo "==> 已做临时签名"
else
  codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP" && echo "==> 已用开发者证书签名"
fi

if [ "${1:-}" = "--dmg" ]; then
  # 暂存目录放 app 与「Applications」快捷方式,用户可直接拖动安装
  STAGE=$(mktemp -d "${TMPDIR:-/tmp}/gitlane-dmg.XXXXXX")
  trap 'rm -rf "$STAGE"' EXIT
  cp -R "$APP" "$STAGE/Gitlane.app"
  ln -s /Applications "$STAGE/Applications"
  rm -f dist/Gitlane*.dmg
  hdiutil create -volname "Gitlane $VERSION" -srcfolder "$STAGE" -ov -format UDZO "dist/Gitlane-$VERSION.dmg" >/dev/null
  echo "==> 已生成 dist/Gitlane-$VERSION.dmg"
fi

echo "已生成 $APP($VERSION,build $BUILD)"
echo "安装:cp -R $APP /Applications/"
