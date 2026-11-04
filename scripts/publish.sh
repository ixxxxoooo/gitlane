#!/usr/bin/env bash
# 发布正式版:打包 dmg → 生成 appcast.json → 打 tag → 推送 → 创建 GitHub Release(附 dmg 与 appcast.json)。
# 应用内「检查更新」读取 releases/latest/download/appcast.json,发布后自动生效。
#
# 用法:先在根 Cargo.toml 的 [workspace.package] 把 version 改成新版本并提交,再运行 scripts/publish.sh
#   GITLANE_NOTES="更新说明"   可选,缺省取上一个 tag 以来的提交标题
#   DEVELOPER_ID="…"          可选,缺省为临时签名
# 如果只想让云端发版:手动 git tag vX.Y.Z && git push origin vX.Y.Z,由 .github/workflows/release.yml 构建上传。
set -euo pipefail
cd "$(dirname "$0")/.."

REPO="ixxxxoooo/gitlane"
VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)"/\1/')
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "错误:版本号格式不正确:$VERSION"; exit 1; }
TAG="v$VERSION"
[ -z "$(git status --porcelain)" ] || { echo "错误:工作区有未提交改动,请先提交"; exit 1; }
git rev-parse "$TAG" >/dev/null 2>&1 && { echo "错误:tag $TAG 已存在,请先在 Cargo.toml 里提升版本号"; exit 1; }
command -v gh >/dev/null || { echo "错误:需要 GitHub CLI(gh)并已登录"; exit 1; }

PREV=$(git describe --tags --abbrev=0 2>/dev/null || true)
NOTES="${GITLANE_NOTES:-$(git log "${PREV:+$PREV..}HEAD" -n 30 --pretty='%s' | paste -sd ';' -)}"

scripts/bundle.sh --dmg
scripts/appcast.sh "$VERSION" "$TAG" "$REPO" "$NOTES"

git tag -a "$TAG" -m "Gitlane $VERSION"
git push origin "$TAG"
gh release create "$TAG" --repo "$REPO" --title "Gitlane $VERSION" --latest \
  --notes "$(printf '%s\n' "$NOTES" | tr ';' '\n' | sed 's/^/- /')" \
  "dist/Gitlane-$VERSION.dmg" dist/appcast.json
echo "已发布:https://github.com/$REPO/releases/tag/$TAG"
