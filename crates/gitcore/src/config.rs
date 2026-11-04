//! 全局 git 配置(`~/.gitconfig`):设置窗口里的身份、pull 方式、提交签名、默认分支名。
//!
//! 它不属于某个仓库,所以不走命令层与撤销栈;读写都用系统 git(见 docs/decisions/0004-global-git-config.md),
//! include、XDG 路径与转义规则都交给 `git config` 自己处理。仓库内的 `.git/config` 仍然优先。

use crate::{Error, Git, Result};
use std::path::PathBuf;

/// `git pull` 的默认合并方式,对应 `pull.rebase` / `pull.ff`。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PullMode {
    /// 合并(`pull.rebase=false`),也是两个键都没设置时 git 的行为。
    #[default]
    Merge,
    /// 变基(`pull.rebase=true`)。
    Rebase,
    /// 只允许快进(`pull.ff=only`)。
    FastForwardOnly,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlobalConfig {
    pub user_name: String,
    pub user_email: String,
    pub pull: PullMode,
    /// `commit.gpgsign`
    pub gpg_sign: bool,
    /// `init.defaultBranch`,空串表示未设置(git 默认 master)。
    pub default_branch: String,
}

/// 设置窗口发出的单项修改。字符串为空时删除该键,回到 git 的默认值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigChange {
    UserName(String),
    UserEmail(String),
    Pull(PullMode),
    GpgSign(bool),
    DefaultBranch(String),
}

/// 要读写的配置文件:默认是 `--global`,测试里指向临时文件。
#[derive(Clone, Debug, Default)]
pub struct ConfigFile {
    file: Option<PathBuf>,
}

impl ConfigFile {
    pub fn global() -> Self {
        Self::default()
    }

    /// 读写指定文件(`git config --file`),供测试使用,不碰用户真实的全局配置。
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self {
            file: Some(path.into()),
        }
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        let mut full = vec!["config"];
        let file = self.file.as_ref().map(|p| p.to_string_lossy().into_owned());
        match &file {
            Some(f) => full.extend_from_slice(&["--file", f]),
            None => full.push("--global"),
        }
        full.extend_from_slice(args);
        // 全局配置与工作目录无关;放在临时目录执行,避免当前目录恰好是仓库时读到仓库配置
        Git::new(std::env::temp_dir()).run(&full)
    }

    /// 读一个键;未设置(退出码 1)时返回 `None`。
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        match self.run(&["--get", key]) {
            Ok(v) => Ok(Some(v.trim_end_matches('\n').to_string())),
            Err(Error::Cli { code: Some(1), .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// 按 git 的布尔规则(yes/on/1…)读一个键。
    pub fn get_bool(&self, key: &str) -> Result<Option<bool>> {
        match self.run(&["--type=bool", "--get", key]) {
            Ok(v) => Ok(Some(v.trim() == "true")),
            Err(Error::Cli { code: Some(1), .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn set(&self, key: &str, value: &str) -> Result<()> {
        self.run(&[key, value]).map(drop)
    }

    /// 删除一个键;本来就没有(退出码 5)不算错误。
    pub fn unset(&self, key: &str) -> Result<()> {
        match self.run(&["--unset-all", key]) {
            Ok(_) | Err(Error::Cli { code: Some(5), .. }) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn load(&self) -> Result<GlobalConfig> {
        let pull = if self.get("pull.ff")?.as_deref() == Some("only") {
            PullMode::FastForwardOnly
        } else {
            // pull.rebase 还可以是 merges / interactive,都算变基
            match self.get("pull.rebase")?.as_deref() {
                Some("false") | None => PullMode::Merge,
                Some(_) => PullMode::Rebase,
            }
        };
        Ok(GlobalConfig {
            user_name: self.get("user.name")?.unwrap_or_default(),
            user_email: self.get("user.email")?.unwrap_or_default(),
            pull,
            gpg_sign: self.get_bool("commit.gpgsign")?.unwrap_or(false),
            default_branch: self.get("init.defaultBranch")?.unwrap_or_default(),
        })
    }

    pub fn apply(&self, change: &ConfigChange) -> Result<()> {
        let set_or_unset = |key: &str, value: &str| {
            let value = value.trim();
            if value.is_empty() {
                self.unset(key)
            } else {
                self.set(key, value)
            }
        };
        match change {
            ConfigChange::UserName(v) => set_or_unset("user.name", v),
            ConfigChange::UserEmail(v) => set_or_unset("user.email", v),
            ConfigChange::DefaultBranch(v) => set_or_unset("init.defaultBranch", v),
            ConfigChange::GpgSign(on) => {
                self.set("commit.gpgsign", if *on { "true" } else { "false" })
            }
            // 两个键互相影响(pull.ff=only 时 pull.rebase 也会参与),切换时把另一个清掉
            ConfigChange::Pull(PullMode::Merge) => {
                self.unset("pull.ff")?;
                self.set("pull.rebase", "false")
            }
            ConfigChange::Pull(PullMode::Rebase) => {
                self.unset("pull.ff")?;
                self.set("pull.rebase", "true")
            }
            ConfigChange::Pull(PullMode::FastForwardOnly) => {
                self.unset("pull.rebase")?;
                self.set("pull.ff", "only")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_reads_as_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ConfigFile::at(dir.path().join("gitconfig"));
        assert_eq!(cfg.load().unwrap(), GlobalConfig::default());
        // 删除不存在的键不报错
        cfg.unset("user.name").unwrap();
    }

    #[test]
    fn changes_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ConfigFile::at(dir.path().join("gitconfig"));
        for change in [
            ConfigChange::UserName("张三".into()),
            ConfigChange::UserEmail(" zs@example.com ".into()),
            ConfigChange::GpgSign(true),
            ConfigChange::DefaultBranch("main".into()),
            ConfigChange::Pull(PullMode::FastForwardOnly),
        ] {
            cfg.apply(&change).unwrap();
        }
        let loaded = cfg.load().unwrap();
        assert_eq!(
            loaded,
            GlobalConfig {
                user_name: "张三".into(),
                user_email: "zs@example.com".into(),
                pull: PullMode::FastForwardOnly,
                gpg_sign: true,
                default_branch: "main".into(),
            }
        );

        cfg.apply(&ConfigChange::Pull(PullMode::Rebase)).unwrap();
        assert_eq!(cfg.get("pull.ff").unwrap(), None);
        assert_eq!(cfg.load().unwrap().pull, PullMode::Rebase);
        cfg.apply(&ConfigChange::Pull(PullMode::Merge)).unwrap();
        assert_eq!(cfg.load().unwrap().pull, PullMode::Merge);

        // 清空即删除
        cfg.apply(&ConfigChange::UserName("  ".into())).unwrap();
        assert_eq!(cfg.get("user.name").unwrap(), None);
    }

    #[test]
    fn git_boolean_spellings_are_understood() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = ConfigFile::at(dir.path().join("gitconfig"));
        cfg.set("commit.gpgsign", "yes").unwrap();
        assert!(cfg.load().unwrap().gpg_sign);
        cfg.set("pull.rebase", "merges").unwrap();
        assert_eq!(cfg.load().unwrap().pull, PullMode::Rebase);
    }
}
