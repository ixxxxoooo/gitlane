#!/bin/sh
# 重新生成 App 图标:SVG 源文件 → PNG → .icns。需要 python3 + PyObjC(macOS 自带 AppKit 渲染)。
# 产物都提交进仓库,打包与编译不依赖本脚本:
#   app-light.png / app-dark.png  512 像素,运行时按主题设置 Dock 图标
#   Gitlane.icns                  打包用(浅色版;深色版只在运行时切换,不需要 icns)
set -eu
cd "$(dirname "$0")"
python3 gen.py
for v in light dark; do
  python3 render.py "app-$v.svg" "app-$v.png" 512
done
SET=$(mktemp -d)/icon.iconset
mkdir -p "$SET"
for s in 16 32 128 256 512; do
  python3 render.py app-light.svg "$SET/icon_${s}x${s}.png" "$s"
  python3 render.py app-light.svg "$SET/icon_${s}x${s}@2x.png" "$((s * 2))"
done
iconutil -c icns "$SET" -o Gitlane.icns
ls -lh *.png *.icns
