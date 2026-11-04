#!/usr/bin/env python3
"""生成 Gitlane App 图标的 SVG 源文件(浅色 / 深色)。

设计:提交图的一小段——一条竖直主线(深色)上有三个提交,一条彩色分支从第一个提交分出、
沿右侧泳道走一个提交后合并回主线;合并提交画成空心点,与应用里提交图的画法一致。
外框沿用 macOS 图标网格(824 圆角方块居中)。

AppKit 的 SVG 渲染不支持遮罩,“线从节点下面穿过”靠节点外圈的背景色描边留缝实现。
"""
import pathlib

OUT = pathlib.Path(__file__).parent
MAIN_X, BRANCH_X = 396, 628         # 主线与分支泳道的 x(两者居中于 512)
TOP, MID, BOTTOM = 268, 512, 756    # 三个主线提交的 y
LANE = 60                           # 泳道线宽
NODE = 62                           # 提交点半径
RING = 18                           # 节点外圈留缝宽度
MERGE_RING = 26                     # 合并提交(空心点)的环宽
CURVE = 210                         # 分支离开 / 回到主线所用的竖直距离


def graph(ink, accent, gap):
    """提交图图形。参数:ink 主线色;accent 分支色;gap 背景色(留缝用)。返回值:SVG 片段。"""
    # 竖直出发、竖直到达的 S 形弯,和提交图里的分支走线一样;中间的直线段藏在分支提交下面
    k = CURVE * 0.6
    branch = (
        f"M {MAIN_X} {TOP} "
        f"C {MAIN_X} {TOP + k}, {BRANCH_X} {TOP + CURVE - k}, {BRANCH_X} {TOP + CURVE} "
        f"L {BRANCH_X} {BOTTOM - CURVE} "
        f"C {BRANCH_X} {BOTTOM - CURVE + k}, {MAIN_X} {BOTTOM - k}, {MAIN_X} {BOTTOM}"
    )
    out = [
        # 分支先画,主线压在上面
        f'<path d="{branch}" fill="none" stroke="{accent}" stroke-width="{LANE}" stroke-linecap="round"/>',
        f'<rect x="{MAIN_X - LANE / 2}" y="{TOP}" width="{LANE}" height="{BOTTOM - TOP}" fill="{ink}"/>',
    ]
    nodes = [(MAIN_X, TOP, ink, False), (MAIN_X, MID, ink, False), (BRANCH_X, MID, accent, False), (MAIN_X, BOTTOM, ink, True)]
    for x, y, color, merge in nodes:
        out.append(f'<circle cx="{x}" cy="{y}" r="{NODE + RING}" fill="{gap}"/>')
        out.append(f'<circle cx="{x}" cy="{y}" r="{NODE}" fill="{color}"/>')
        if merge:
            out.append(f'<circle cx="{x}" cy="{y}" r="{NODE - MERGE_RING}" fill="{gap}"/>')
    return "\n  ".join(out)


def app_icon(dark):
    """1024×1024 App 图标。参数:dark 是否深色。返回值:SVG。"""
    if dark:
        bg0, bg1, ink, a0, a1, rim_op, shade = "#2A2D33", "#14161A", "#ECEEF2", "#5EE0C1", "#1FB892", "0.16", "0.5"
    else:
        bg0, bg1, ink, a0, a1, rim_op, shade = "#FEFFFE", "#E4E9EC", "#1F242B", "#2FD3AE", "#0C9A78", "0.95", "0.2"
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<defs>
  <linearGradient id="bg" x1="0" y1="100" x2="0" y2="924" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="{bg0}"/><stop offset="1" stop-color="{bg1}"/></linearGradient>
  <linearGradient id="ac" x1="{MAIN_X}" y1="{TOP}" x2="{BRANCH_X}" y2="{BOTTOM}" gradientUnits="userSpaceOnUse"><stop offset="0" stop-color="{a0}"/><stop offset="1" stop-color="{a1}"/></linearGradient>
  <linearGradient id="rim" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity="{rim_op}"/><stop offset="0.5" stop-color="#fff" stop-opacity="0"/></linearGradient>
  <filter id="sh" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="12" stdDeviation="16" flood-color="#000" flood-opacity="{shade}"/></filter>
</defs>
<rect x="100" y="100" width="824" height="824" rx="186" fill="url(#bg)" filter="url(#sh)"/>
<rect x="102" y="102" width="820" height="820" rx="184" fill="none" stroke="url(#rim)" stroke-width="4"/>
  {graph(ink, "url(#ac)", "url(#bg)")}
</svg>
'''


if __name__ == "__main__":
    (OUT / "app-light.svg").write_text(app_icon(False))
    (OUT / "app-dark.svg").write_text(app_icon(True))
