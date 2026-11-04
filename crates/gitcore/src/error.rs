use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Git2(#[from] git2::Error),

    #[error("git {args} 失败(退出码 {code:?}):{stderr}")]
    Cli {
        args: String,
        code: Option<i32>,
        stderr: String,
    },

    #[error("找不到 git 命令,请安装 Xcode 命令行工具(xcode-select --install)")]
    GitNotFound,

    #[error("操作已取消")]
    Cancelled,

    #[error("不是 Git 仓库:{}", .0.display())]
    NotARepo(PathBuf),

    #[error("{0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(msg: impl Into<String>) -> Self {
        Error::Other(msg.into())
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
