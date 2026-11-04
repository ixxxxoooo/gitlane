//! 重启替换:退出后由脚本换掉 `.app` 并重新打开;失败时留下标记,下次启动提示。
//!
//! @author ixxxxoooo

use super::install::StagedUpdate;
use std::path::Path;
use std::process::{Command, Stdio};

/// 「等旧进程退出 → 替换 → 重启」脚本。路径一律作为位置参数传入,脚本里不拼接路径。
/// 替换失败时恢复原应用并写下失败标记,下次启动提示。参数:$1 旧 PID、$2 目标 app、$3 暂存 app、$4 暂存根目录。
const REPLACE_SCRIPT: &str = r#"#!/bin/sh
PID="$1"; TARGET="$2"; STAGED="$3"; ROOT="$4"
LOG="$ROOT/replace.log"
i=0; while kill -0 "$PID" 2>/dev/null && [ $i -lt 120 ]; do sleep 0.5; i=$((i+1)); done
if kill -0 "$PID" 2>/dev/null; then exit 0; fi
BACKUP="$TARGET.old"
rm -rf "$BACKUP"
if mv "$TARGET" "$BACKUP" 2>>"$LOG"; then
  if /usr/bin/ditto "$STAGED" "$TARGET" 2>>"$LOG"; then
    /usr/bin/xattr -dr com.apple.quarantine "$TARGET" 2>/dev/null
    rm -rf "$BACKUP" "$ROOT"
  else
    rm -rf "$TARGET"; mv "$BACKUP" "$TARGET"; echo "copy" > "$ROOT/failed"
  fi
else
  echo "move" > "$ROOT/failed"
fi
/usr/bin/open "$TARGET"
"#;

/// 生成替换脚本并在后台启动;调用方随后应退出应用。
///
/// 参数:`root` 暂存根目录;`staged` 暂存的更新;`target_app` 要被替换的应用(当前 bundle)。
pub fn launch_replacer(
    root: &Path,
    staged: &StagedUpdate,
    target_app: &Path,
) -> Result<(), String> {
    let script = root.join("replace.sh");
    std::fs::write(&script, REPLACE_SCRIPT).map_err(|e| format!("无法写入替换脚本:{e}"))?;
    Command::new("/bin/sh")
        .arg(&script)
        .arg(std::process::id().to_string())
        .arg(target_app)
        .arg(&staged.app_path)
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("无法启动替换脚本:{e}"))
}

/// 读取并清理上次安装失败的记录(常见原因是应用所在目录不可写)。上次没有失败为 `None`。
pub fn take_install_failure(root: &Path) -> Option<String> {
    let step = std::fs::read_to_string(root.join("failed")).ok()?;
    let log = std::fs::read_to_string(root.join("replace.log")).unwrap_or_default();
    let _ = std::fs::remove_dir_all(root);
    let tail: String = log
        .chars()
        .rev()
        .take(300)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let tail = tail.trim();
    Some(format!(
        "{}:{}",
        step.trim(),
        if tail.is_empty() { "-" } else { tail }
    ))
}
