# 0006 应用内更新与发版

状态:采用 · 2026-11-04

## 背景

原计划“不做签名公证、自动更新”,自己构建自己用。现在要在多台机器上用,希望发一个版本后其它机器能自动拿到。
参考同一作者的 LoomCode(`~/ucode`)已经跑通的方案。

## 决定

- **发布源**:GitHub Release 附带 `appcast.json`(仓库公开,无需鉴权)。应用读
  `releases/latest/download/appcast.json`,内容含 `version`、`build`、`url`、`sha256`、`size`、`notes`。
- **版本比较**:先比 `Cargo.toml` 的版本号;版本号相同再比构建号(`git rev-list --count HEAD`,打包时经环境变量
  `GITLANE_BUILD` 编进二进制)。开发构建的构建号为 0,不自动检查。
- **流程**:检查 → 后台下载 dmg → 校验 sha256 → 挂载解出 app → `codesign --verify` → 通知“重启更新”
  → 用户确认后退出,由脚本等进程退出后替换 `.app`、去除隔离属性并重新打开。替换失败会还原并在下次启动提示。
- **不新增依赖**:联网用系统 `curl`,校验用 `shasum`,挂载用 `hdiutil`,与“写操作走系统 git”同一思路;
  下载进度通过轮询目标文件大小得到(`size` 字段提供总长度)。
- **签名**:缺省临时签名(`codesign -s -`),满足自校验;设置 `DEVELOPER_ID` 才用开发者证书。不公证。
  更新包由应用自己下载,不带隔离属性,所以临时签名下自动更新不会触发 Gatekeeper;
  首次手动安装的 dmg 需右键打开。
- **发版两条路,产物一致**:
  1. 本机 `scripts/publish.sh`:打包 → appcast → 打 tag 并推送 → `gh release create`;
  2. 只推 tag(`v<版本>`),由 `.github/workflows/release.yml` 在 macOS runner 上构建并上传。
  本机先发布时云端检测到同名 Release 就整体跳过,避免 appcast 与 dmg 来自两次构建、sha256 对不上。
- 暂存目录 `~/Library/Application Support/Gitlane/updates`,安装函数都接受目录参数,测试用临时目录。

## 后果

- 发版流程:改 `Cargo.toml` 版本号 → 提交推送 → `scripts/publish.sh`(或推 tag)。
- tag 版本必须与 `Cargo.toml` 一致,`scripts/appcast.sh` 找不到对应 dmg 会报错终止。
- 没有公证,换成 Developer ID 签名只需设环境变量,不改代码。
