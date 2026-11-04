#!/usr/bin/env bash
# 构建并重启 Gitlane 开发实例(target/debug/gitlane)。
#
#   scripts/dev-restart.sh              无条件构建并重启
#   scripts/dev-restart.sh --if-changed 仅当源码比上次启动新时才构建并重启(Claude Code Stop hook 使用)
#
# 不带仓库参数启动,应用会恢复上次打开的仓库标签。构建失败时不杀旧实例,
# 错误写到 stderr;hook 模式下以退出码 2 返回,让 Claude 继续修复。
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/gitlane"
STAMP="$ROOT/target/.dev-restart.stamp"
BUILD_LOG="$ROOT/target/dev-build.log"
RUN_LOG="$ROOT/target/dev-run.log"
LOCK="$ROOT/target/.dev-restart.lock"
export PATH="$HOME/.cargo/bin:$PATH"

hook=false
stop_hook_active=false
if [[ "${1:-}" == "--if-changed" ]]; then
    hook=true
    shift
    # Stop hook 的 stdin 是 JSON;连续因构建失败被拦下时不再拦截,避免死循环
    if [[ ! -t 0 ]]; then
        stop_hook_active=$(jq -r '.stop_hook_active // false' 2>/dev/null || echo false)
    fi
fi

mkdir -p "$ROOT/target"

changed() {
    [[ -f "$STAMP" ]] || return 0
    [[ -n "$(find "$ROOT/crates" "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$ROOT/rust-toolchain.toml" \
        -newer "$STAMP" -type f \( -name '*.rs' -o -name '*.toml' -o -name '*.lock' \) \
        -not -path '*/target/*' 2>/dev/null | head -1)" ]]
}

if $hook && ! changed; then
    exit 0
fi

# 同一时间只允许一个构建/重启
if ! mkdir "$LOCK" 2>/dev/null; then
    echo "另一个 dev-restart 正在运行,跳过" >&2
    exit 0
fi
trap 'rmdir "$LOCK" 2>/dev/null' EXIT

# 构建开始前记下时间点,构建期间的新改动会在下一轮被发现
NEXT_STAMP="$STAMP.next"
touch "$NEXT_STAMP"

if ! (cd "$ROOT" && cargo build -p ui >"$BUILD_LOG" 2>&1); then
    rm -f "$NEXT_STAMP"
    {
        echo "Gitlane 构建失败,旧实例保持运行。错误摘要(完整日志 target/dev-build.log):"
        grep -E '^(error|warning: unused)' -A6 "$BUILD_LOG" | head -60
    } >&2
    if $hook && [[ "$stop_hook_active" != "true" ]]; then
        exit 2
    fi
    exit 1
fi

# 结束旧的开发实例(只匹配本仓库 target/debug 下的二进制,不影响 /Applications 里的版本)
if pgrep -f "^$BIN" >/dev/null; then
    pkill -TERM -f "^$BIN"
    for _ in $(seq 1 30); do
        pgrep -f "^$BIN" >/dev/null || break
        sleep 0.1
    done
    pkill -KILL -f "^$BIN" 2>/dev/null || true
fi

nohup "$BIN" "$@" >"$RUN_LOG" 2>&1 </dev/null &
pid=$!
disown "$pid" 2>/dev/null || true
mv -f "$NEXT_STAMP" "$STAMP"

sleep 1
if ! kill -0 "$pid" 2>/dev/null; then
    {
        echo "Gitlane 启动后立即退出,日志 target/dev-run.log:"
        tail -30 "$RUN_LOG"
    } >&2
    if $hook && [[ "$stop_hook_active" != "true" ]]; then
        exit 2
    fi
    exit 1
fi

msg="已重启 Gitlane 开发实例(pid $pid,日志 target/dev-run.log)"
if $hook; then
    printf '{"systemMessage": "%s"}\n' "$msg"
else
    echo "$msg"
fi
