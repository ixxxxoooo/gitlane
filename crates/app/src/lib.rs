//! 应用层:仓库状态、命令分发、任务调度与设置。
//!
//! - `command`:写操作的统一描述与同步执行(可脱离 GPUI 测试);
//! - `repo_model`:单仓库状态 Entity,负责后台加载、命令排队、撤销栈、文件监听防抖;
//! - `workspace`:多仓库标签、设置、自动 fetch 与全局 git 配置。

pub mod command;
pub mod repo_model;
pub mod search;
pub mod settings;
pub mod workspace;

pub use command::Command;
pub use repo_model::{DiffState, NoticeKind, RepoData, RepoEvent, RepoModel, Selection};
pub use settings::{Settings, TerminalApp, ThemePreference};
pub use workspace::Workspace;
