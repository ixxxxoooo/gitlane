//! Git 核心:仓库读写、命令与撤销记录。不依赖 GPUI,可独立测试。
//!
//! 读写分路(见 docs/architecture.md):
//! - 读路径走 git2(进程内 libgit2):历史、引用、status、diff;
//! - 写路径走系统 git 命令行:与用户已有的配置、凭据、hooks、签名完全一致。
//!
//! 本 crate 的函数都是同步阻塞的,由应用层放到后台执行器里调用。

pub mod change_context;
pub mod cli;
pub mod config;
pub mod diff;
mod error;
pub mod highlight;
pub mod ops;
pub mod patch;
pub mod repo;
pub mod scan;
pub mod types;
pub mod undo;
pub mod watch;

pub use cli::{Cancel, Git, GitVersion};
pub use error::{Error, Result};
pub use git2::Oid;
pub use repo::Repo;
pub use types::*;
