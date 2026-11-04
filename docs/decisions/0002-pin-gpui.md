# 0002 固定 GPUI 与组件库版本

日期:2026-11-04 · 状态:已采纳

## 背景

GPUI 随 Zed 开发,没有 1.0,版本之间常有破坏性更新;文档少,排障主要靠读源码。

## 决定

- 依赖 `gpui-kit = "=0.9.0"`(在 workspace 中命名为 `gpui`),它固定了 `gpui-pre = 0.3.11` 及配套的
  `gpui-component` / `gpui-base` / 平台层,应用只需这一个依赖。
- Rust 工具链用 `rust-toolchain.toml` 固定为 1.99.0(gpui-pre 0.3.11 要求 ≥ 1.93)。
- GPUI 相关代码只出现在 `app` 与 `ui` 两个 crate;`gitcore`、`graph` 完全不依赖 GPUI。
- 升级单独排期:升级时先跑 `cargo test --workspace`(含无头 UI 测试),再走 testing.md 的手工清单。

## 已知问题

gpui-kit 的可选依赖 gpui-fast 精确固定了 taffy 等版本。Cargo 生成 lockfile 时会把弱依赖特性(`gpui-fast?/…`)
引用的可选依赖也纳入解析,增量 `cargo update` 容易无解。处理方式:删除 `Cargo.lock` 后重新生成。
