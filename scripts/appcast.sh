#!/usr/bin/env bash
# 生成应用内更新读取的发布源 dist/appcast.json。
# 用法:scripts/appcast.sh <版本> <tag> <仓库 owner/name> [更新说明]
# 依赖:dist/Gitlane-<版本>.dmg 已由 scripts/bundle.sh --dmg 生成。本地发布与 GitHub Actions 共用这一份。
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="$1"; TAG="$2"; REPO="$3"; NOTES="${4:-}"
DMG="dist/Gitlane-$VERSION.dmg"
[ -f "$DMG" ] || { echo "错误:找不到 $DMG(tag 版本与 Cargo.toml 不一致?)"; exit 1; }

export VERSION TAG REPO NOTES
SHA=$(shasum -a 256 "$DMG" | awk '{print $1}'); export SHA
SIZE=$(stat -f%z "$DMG"); export SIZE
BUILD=$(git rev-list --count HEAD); export BUILD

# 用 python 写 JSON,更新说明里的引号不会破坏格式
python3 - <<'PY'
import json, os
e = os.environ
feed = {"stable": {
    "version": e["VERSION"], "build": int(e["BUILD"]),
    "url": f'https://github.com/{e["REPO"]}/releases/download/{e["TAG"]}/Gitlane-{e["VERSION"]}.dmg',
    "sha256": e["SHA"], "size": int(e["SIZE"]), "notes": e["NOTES"]}}
json.dump(feed, open("dist/appcast.json", "w"), ensure_ascii=False, indent=2)
PY
echo "已生成 dist/appcast.json(build $BUILD)"
