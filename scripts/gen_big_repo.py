#!/usr/bin/env python3
"""生成用于性能测试的大仓库:N 个提交,定期分出侧分支再合并回主线。

用法:python3 scripts/gen_big_repo.py /tmp/big-repo 50000
"""
import os
import subprocess
import sys


def main():
    path = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 50_000
    os.makedirs(path, exist_ok=True)
    subprocess.run(["git", "init", "-q", "-b", "main", path], check=True)
    p = subprocess.Popen(["git", "fast-import", "--quiet"], cwd=path, stdin=subprocess.PIPE)
    w = p.stdin.write
    t = 1_600_000_000
    mark = 0
    main_tip = None
    side = []  # 正在进行的侧分支:(名称, tip mark, 剩余长度)
    for i in range(n):
        t += 60
        mark += 1
        # 每 20 个提交开一个侧分支,长度 3-8
        if i % 20 == 5 and main_tip:
            side.append((f"feature/f{i}", main_tip, 3 + i % 6))
        if side and i % 3 == 0:
            name, tip, left = side[0]
            msg = f"{name}: step {left}".encode()
            w(f"commit refs/heads/{name}\nmark :{mark}\n".encode())
            w(f"author Dev{i % 7} <dev{i % 7}@example.com> {t} +0800\n".encode())
            w(f"committer Dev{i % 7} <dev{i % 7}@example.com> {t} +0800\n".encode())
            w(f"data {len(msg)}\n".encode() + msg + b"\n")
            w(f"from :{tip}\n".encode())
            w(f"M 644 inline {name}.txt\ndata {len(str(i))}\n{i}\n".encode())
            if left <= 1:
                side.pop(0)
                # 合并回主线
                t += 60
                mark_merge = mark + 1
                msg = f"Merge {name}".encode()
                w(f"commit refs/heads/main\nmark :{mark_merge}\n".encode())
                w(f"author Dev0 <dev0@example.com> {t} +0800\n".encode())
                w(f"committer Dev0 <dev0@example.com> {t} +0800\n".encode())
                w(f"data {len(msg)}\n".encode() + msg + b"\n")
                w(f"from :{main_tip}\nmerge :{mark}\n".encode())
                mark = mark_merge
                main_tip = mark
            else:
                side[0] = (name, mark, left - 1)
            continue
        msg = f"commit {i}: 修改 file{i % 50}".encode()
        w(f"commit refs/heads/main\nmark :{mark}\n".encode())
        w(f"author Dev{i % 7} <dev{i % 7}@example.com> {t} +0800\n".encode())
        w(f"committer Dev{i % 7} <dev{i % 7}@example.com> {t} +0800\n".encode())
        w(f"data {len(msg)}\n".encode() + msg + b"\n")
        if main_tip:
            w(f"from :{main_tip}\n".encode())
        w(f"M 644 inline src/file{i % 50}.txt\ndata {len(str(i)) + 1}\n{i}\n\n".encode())
        main_tip = mark
        if i % 1000 == 0:
            w(f"reset refs/tags/v{i // 1000}\nfrom :{mark}\n\n".encode())
    p.stdin.close()
    p.wait()
    subprocess.run(["git", "checkout", "-q", "-f", "main"], cwd=path, check=True)
    count = subprocess.run(["git", "rev-list", "--all", "--count"], cwd=path, capture_output=True, text=True).stdout.strip()
    print(f"{path}: {count} 个提交")


if __name__ == "__main__":
    main()
