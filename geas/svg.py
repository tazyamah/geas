# SPDX-License-Identifier: GPL-3.0-or-later
"""正本から図（SVG）を描く。色は CSS のクラスで指定し、表示側のテーマに合わせる。

配置は単純な層別レイアウト：最初のノードから何手で届くかで列を決め、
前向きの矢印は右へ、戻る矢印は下に弧を描く。
"""

from __future__ import annotations

from dataclasses import dataclass
from html import escape

SAME, ADDED, REMOVED = "same", "added", "removed"

FONT = 13
NODE_H = 38
ROW_GAP = 34
COL_GAP = 96
PAD = 24


def text_width(s: str, size: float = FONT) -> float:
    return sum(size if ord(ch) > 0xFF else size * 0.6 for ch in s)


@dataclass
class Edge:
    src: str
    label: str
    dst: str
    kind: str = SAME


def flow(nodes: list[str], edges: list[Edge], start: str, finals: list[str] = (),
         node_kinds: dict[str, str] | None = None, title: str = "", outside: list[str] = ()) -> str:
    """outside は流れの外のノード（通知メールなどの入口）。入る先の画面の1つ左の列に置く。"""
    node_kinds = node_kinds or {}
    # 同じ2点の矢印はラベルをまとめる（種類が違うものは分ける）
    merged: dict[tuple, Edge] = {}
    for e in edges:
        key = (e.src, e.dst, e.kind)
        if key in merged:
            merged[key].label += "・" + e.label
        else:
            merged[key] = Edge(e.src, e.label, e.dst, e.kind)
    edges = list(merged.values())

    layer = {start: 0}
    queue = [start]
    while queue:
        cur = queue.pop(0)
        for e in edges:
            if e.src == cur and e.dst not in layer and e.kind != REMOVED:
                layer[e.dst] = layer[cur] + 1
                queue.append(e.dst)
    # 入口から入る画面も、入口の右に並ぶよう層を決める
    for o in outside:
        for e in edges:
            if e.src == o and e.dst not in layer:
                layer[e.dst] = 1
                queue.append(e.dst)
    while queue:
        cur = queue.pop(0)
        for e in edges:
            if e.src == cur and e.dst not in layer and e.kind != REMOVED:
                layer[e.dst] = layer[cur] + 1
                queue.append(e.dst)
    last = max(layer.values(), default=0)
    for n in nodes:
        if n not in layer and n not in outside:
            last += 1
            layer[n] = last
    for o in outside:
        layer[o] = min([layer[e.dst] for e in edges if e.src == o] or [1]) - 1
    cols: dict[int, list[str]] = {}
    for n in nodes:
        cols.setdefault(layer[n], []).append(n)

    width = {n: text_width(n) + 32 for n in nodes}
    col_w = {c: max(width[n] for n in ns) for c, ns in cols.items()}
    x_of_col, x = {}, PAD
    for c in sorted(cols):
        x_of_col[c] = x
        x += col_w[c] + COL_GAP
    pos = {}
    for c, ns in cols.items():
        for i, n in enumerate(ns):
            cx = x_of_col[c] + col_w[c] / 2
            pos[n] = (cx, PAD + 28 + i * (NODE_H + ROW_GAP) + NODE_H / 2)
    max_rows = max(len(ns) for ns in cols.values())
    body_h = PAD + 28 + max_rows * (NODE_H + ROW_GAP)
    total_w = x - COL_GAP + PAD

    parts = []
    loops_below = 0
    for e in edges:
        (x1, y1), (x2, y2) = pos[e.src], pos[e.dst]
        cls = f"edge {e.kind}"
        if e.src == e.dst:
            w = width[e.src] / 2
            d = f"M{x1 - 10} {y1 - NODE_H / 2} C{x1 - 30} {y1 - NODE_H - 22} {x1 + 30} {y1 - NODE_H - 22} {x1 + 10} {y1 - NODE_H / 2}"
            lx, ly = x1, y1 - NODE_H - 16
            del w
        elif layer[e.dst] > layer[e.src]:
            sx, ex = x1 + width[e.src] / 2, x2 - width[e.dst] / 2
            mid = (sx + ex) / 2
            d = f"M{sx} {y1} C{mid} {y1} {mid} {y2} {ex - 2} {y2}"
            lx, ly = mid, (y1 + y2) / 2 - 6
        elif layer[e.dst] == layer[e.src]:
            # 同じ列の上下：列の右側を回る
            sx = x1 + width[e.src] / 2
            ex = x2 + width[e.dst] / 2
            out = max(sx, ex) + 44
            d = f"M{sx} {y1} C{out} {y1} {out} {y2} {ex + 2} {y2}"
            lx, ly = out - 4, (y1 + y2) / 2 + 4
        else:
            # 戻る矢印：あいだの列のどの箱よりも下を通る
            loops_below += 1
            lo, hi = sorted((layer[e.src], layer[e.dst]))
            bottom = max(pos[n][1] for n in nodes if lo <= layer[n] <= hi) + NODE_H / 2
            low = bottom + 22 + 18 * loops_below
            sy, ey = y1 + NODE_H / 2, y2 + NODE_H / 2
            d = f"M{x1} {sy} C{x1} {low} {x2} {low} {x2} {ey + 2}"
            lx, ly = (x1 + x2) / 2, low - 4 - (low - max(sy, ey)) * 0.25
            body_h = max(body_h, low + 12)
        parts.append(f'<path class="{cls}" d="{d}" marker-end="url(#arrow-{e.kind})"/>')
        lw = text_width(e.label, 11.5) + 10
        parts.append(
            f'<g class="elabel {e.kind}"><rect x="{lx - lw / 2:.1f}" y="{ly - 13:.1f}" width="{lw:.1f}" height="17" rx="4"/>'
            f'<text x="{lx:.1f}" y="{ly:.1f}" text-anchor="middle">{escape(e.label)}</text></g>'
        )

    for n in nodes:
        cx, cy = pos[n]
        w = width[n]
        classes = ["node", node_kinds.get(n, SAME)] + (["outside"] if n in outside else [])
        if n == start:
            classes.append("start")
        if n in finals:
            classes.append("final")
        inner = ""
        if n in finals:
            inner = f'<rect class="inner" x="{cx - w / 2 + 3:.1f}" y="{cy - NODE_H / 2 + 3:.1f}" width="{w - 6:.1f}" height="{NODE_H - 6}" rx="6"/>'
        parts.append(
            f'<g class="{" ".join(classes)}"><rect x="{cx - w / 2:.1f}" y="{cy - NODE_H / 2:.1f}" width="{w:.1f}" height="{NODE_H}" rx="8"/>{inner}'
            f'<text x="{cx:.1f}" y="{cy + FONT / 3 + 1:.1f}" text-anchor="middle">{escape(n)}</text></g>'
        )
        if n == start:
            parts.append(f'<path class="edge same" d="M{cx - w / 2 - 22} {cy} H{cx - w / 2 - 2}" marker-end="url(#arrow-same)"/>')

    height = body_h + PAD
    return _svg(total_w + 12, height, parts, title)


def usecases(actors: list[str], cases: list[tuple[str, str, str]], title: str = "") -> str:
    """cases は (アクター, ユースケース名, 種類) の並び。"""
    by_actor = {a: [c for c in cases if c[0] == a] for a in actors}
    actor_w = max([text_width(a) for a in actors] + [60]) + 16
    case_w = max([text_width(c[1]) for c in cases] + [80]) + 48
    ax, ux = PAD + actor_w / 2, PAD + actor_w + 120 + case_w / 2
    parts, y = [], PAD
    for a in actors:
        items = by_actor[a] or [(a, "", SAME)]
        block_h = len(items) * (NODE_H + 14) - 14
        ay = y + block_h / 2
        for i, (_, name, kind) in enumerate(by_actor[a]):
            cy = y + i * (NODE_H + 14) + NODE_H / 2
            parts.append(f'<path class="edge {kind}" d="M{ax + actor_w / 2 - 4} {ay} C{ax + actor_w / 2 + 60} {ay} {ux - case_w / 2 - 60} {cy} {ux - case_w / 2} {cy}"/>')
            parts.append(
                f'<g class="usecase {kind}"><ellipse cx="{ux}" cy="{cy}" rx="{case_w / 2}" ry="{NODE_H / 2}"/>'
                f'<text x="{ux}" y="{cy + FONT / 3 + 1:.1f}" text-anchor="middle">{escape(name)}</text></g>'
            )
        # 人の形
        parts.append(
            f'<g class="actor"><circle cx="{ax}" cy="{ay - 22}" r="7"/>'
            f'<path d="M{ax} {ay - 15} V{ay - 1} M{ax - 10} {ay - 10} H{ax + 10} M{ax} {ay - 1} L{ax - 8} {ay + 10} M{ax} {ay - 1} L{ax + 8} {ay + 10}"/>'
            f'<text x="{ax}" y="{ay + 27}" text-anchor="middle">{escape(a)}</text></g>'
        )
        y += max(block_h, 64) + 28
    return _svg(ux + case_w / 2 + PAD, y - 28 + PAD + 12, parts, title)


def _svg(w: float, h: float, parts: list[str], title: str) -> str:
    markers = "".join(
        f'<marker id="arrow-{k}" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">'
        f'<path class="arrowhead {k}" d="M0 0L10 5L0 10z"/></marker>'
        for k in (SAME, ADDED, REMOVED)
    )
    return (
        f'<svg class="diagram" viewBox="0 0 {w:.0f} {h:.0f}" width="{w:.0f}" height="{h:.0f}" role="img" aria-label="{escape(title)}">'
        f"<defs>{markers}</defs>{''.join(parts)}</svg>"
    )


def context(center: str, systems: list[tuple[str, str, list[str], str]], title: str = "") -> str:
    """システム構成図。systems は (名前, 種類, 矢印に添える行, 差分の種類) の並び。

    矢印は相手の手前で水平になり、添える行はその水平な部分の上に積む。
    """
    line_h = 16
    box_h = NODE_H + 16
    labels_w = max([text_width(t, 11.5) for _, _, lines, _ in systems for t in lines] + [80])
    sys_w = max([text_width(n) for n, _, _, _ in systems] + [80]) + 40
    center_w = text_width(center) + 48
    blocks = [max(box_h, len(lines) * line_h + box_h / 2 + 14) for _, _, lines, _ in systems]
    total_h = sum(blocks) + 20 * (len(systems) - 1) + 2 * PAD
    cx, cy = PAD + center_w / 2, total_h / 2
    x0 = PAD + center_w
    xh = x0 + 120  # ここから水平になる
    x1 = xh + labels_w + 24  # 相手の箱の左端
    sx = x1 + sys_w / 2
    parts = [
        f'<g class="node start"><rect x="{PAD}" y="{cy - 30}" width="{center_w}" height="60" rx="8"/>'
        f'<text x="{cx}" y="{cy + 5}" text-anchor="middle" font-weight="600">{escape(center)}</text></g>'
    ]
    y = PAD
    for (name, kind, lines, ekind), bh in zip(systems, blocks):
        my = y + bh - box_h / 2
        parts.append(
            f'<path class="edge {ekind}" d="M{x0} {cy} C{x0 + 70} {cy} {xh - 50} {my} {xh} {my} H{x1 - 2}" marker-end="url(#arrow-{ekind})"/>'
        )
        for i, t in enumerate(reversed(lines)):
            parts.append(f'<g class="elabel {ekind}"><text x="{x1 - 14}" y="{my - 7 - i * line_h:.1f}" text-anchor="end">{escape(t)}</text></g>')
        parts.append(
            f'<g class="node {ekind}"><rect x="{x1}" y="{my - box_h / 2}" width="{sys_w}" height="{box_h}" rx="8"/>'
            f'<text x="{sx}" y="{my - 2}" text-anchor="middle">{escape(name)}</text>'
            f'<text class="sub" x="{sx}" y="{my + 14}" text-anchor="middle">{escape(kind)}</text></g>'
        )
        y += bh + 20
    return _svg(x1 + sys_w + PAD, total_h, parts, title)


@dataclass
class Message:
    src: str
    dst: str  # src と同じなら、その参加者の上に書く覚え書き
    label: str
    style: str = "solid"  # solid / dashed
    flag: str = ""  # ask / reject なら目立たせる


def sequence(participants: list[str], messages: list[Message], title: str = "") -> str:
    """シーケンス図。上から順にメッセージを並べる。"""
    widest = max([text_width(m.label, 11.5) for m in messages if m.src != m.dst] + [60])
    notes_w = max([text_width(m.label, 11.5) + 20 for m in messages if m.src == m.dst] + [0])
    col = max(160, widest + 40, notes_w + 16)
    head_w = {p: text_width(p) + 28 for p in participants}
    half = max(max(head_w.values()), notes_w) / 2  # 両端の参加者の外側に要る幅
    xs = {p: PAD + half + i * col for i, p in enumerate(participants)}
    row, top = 46, PAD + NODE_H + 30
    total_w = max(xs.values()) + half + PAD
    total_h = top + len(messages) * row + PAD
    parts = []
    for p in participants:
        x = xs[p]
        parts.append(f'<path class="lifeline" d="M{x} {PAD + NODE_H} V{total_h - PAD + 4}"/>')
        parts.append(
            f'<g class="node"><rect x="{x - head_w[p] / 2}" y="{PAD}" width="{head_w[p]}" height="{NODE_H}" rx="8"/>'
            f'<text x="{x}" y="{PAD + NODE_H / 2 + FONT / 3 + 1:.1f}" text-anchor="middle">{escape(p)}</text></g>'
        )
    for i, m in enumerate(messages):
        y = top + i * row
        flag = f" {m.flag}" if m.flag else ""
        if m.src == m.dst:
            w = text_width(m.label, 11.5) + 20
            x = xs[m.src]
            parts.append(
                f'<g class="note{flag}"><rect x="{x - w / 2:.1f}" y="{y - 14}" width="{w:.1f}" height="24" rx="4"/>'
                f'<text x="{x}" y="{y + 2}" text-anchor="middle">{escape(m.label)}</text></g>'
            )
            continue
        x1, x2 = xs[m.src], xs[m.dst]
        end = x2 - 2 if x2 > x1 else x2 + 2
        dash = ' stroke-dasharray="5 4"' if m.style == "dashed" else ""
        kind = "same"
        parts.append(f'<path class="msg{flag}" d="M{x1} {y} H{end}"{dash} marker-end="url(#arrow-{kind})"/>')
        parts.append(
            f'<g class="mlabel{flag}"><text x="{(x1 + x2) / 2:.1f}" y="{y - 7}" text-anchor="middle">{escape(m.label)}</text></g>'
        )
    return _svg(total_w, total_h, parts, title)
