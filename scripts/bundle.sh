#!/usr/bin/env bash
# 本机打包:cargo build --release 后组装成 dist/Gitlane.app。不签名、不公证。
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)"/\1/')
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
  <key>CFBundleVersion</key><string>${VERSION}</string>
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

echo "已生成 $APP($VERSION)"
echo "安装:cp -R $APP /Applications/"
