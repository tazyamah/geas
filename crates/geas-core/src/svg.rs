// SPDX-License-Identifier: GPL-3.0-or-later
//! 正本から図（SVG）を描く（Python 版 svg.py の移し）。座標の文字は、Python の int／float の使い分けまで合わせる（pynum::N）。
use crate::pynum::*;
use indexmap::IndexMap;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const SAME: &str = "same";
pub const ADDED: &str = "added";
pub const REMOVED: &str = "removed";

const FONT: i64 = 13;
const NODE_H: i64 = 38;
const ROW_GAP: i64 = 34;
const COL_GAP: i64 = 96;
const PAD: i64 = 24;

pub fn text_width(s: &str, size: N) -> N {
    pysum(s.chars().map(|ch| if (ch as u32) > 0xFF { size } else { size * 0.6 }))
}
fn tw(s: &str) -> N {
    text_width(s, N::I(FONT))
}

#[derive(Clone, Debug)]
pub struct Edge {
    pub src: String,
    pub label: String,
    pub dst: String,
    pub kind: String,
}

impl Edge {
    pub fn new(src: &str, label: &str, dst: &str) -> Edge {
        Edge { src: src.into(), label: label.into(), dst: dst.into(), kind: SAME.into() }
    }
}

static SERIAL: AtomicUsize = AtomicUsize::new(0);

fn svg(w: N, h: N, parts: &[String], title: &str) -> String {
    let n = SERIAL.fetch_add(1, Ordering::SeqCst) + 1;
    svg_raw(w, h, parts, title).replace("arrow-", &format!("arrow{}-", n))
}

fn svg_raw(w: N, h: N, parts: &[String], title: &str) -> String {
    let markers: String = [SAME, ADDED, REMOVED]
        .iter()
        .map(|k| format!("<marker id=\"arrow-{k}\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path class=\"arrowhead {k}\" d=\"M0 0L10 5L0 10z\"/></marker>", k = k))
        .collect();
    format!(
        "<svg class=\"diagram\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\" role=\"img\" aria-label=\"{}\"><defs>{}</defs>{}</svg>",
        w.d0(),
        h.d0(),
        w.d0(),
        h.d0(),
        escape(title),
        markers,
        parts.concat()
    )
}

pub fn flow(nodes: &[String], edges: &[Edge], start: &str, finals: &[String], title: &str, outside: &[String]) -> String {
    // 同じ2点の矢印はラベルをまとめる（種類が違うものは分ける）
    let mut merged: IndexMap<(String, String, String), Edge> = IndexMap::new();
    for e in edges {
        let key = (e.src.clone(), e.dst.clone(), e.kind.clone());
        match merged.get_mut(&key) {
            Some(m) => m.label += &format!("・{}", e.label),
            None => {
                merged.insert(key, e.clone());
            }
        }
    }
    let edges: Vec<Edge> = merged.into_values().collect();
    let mut layer: IndexMap<String, i64> = IndexMap::new();
    layer.insert(start.to_string(), 0);
    let mut queue: Vec<String> = vec![start.to_string()];
    let bfs = |layer: &mut IndexMap<String, i64>, queue: &mut Vec<String>| {
        while !queue.is_empty() {
            let cur = queue.remove(0);
            for e in &edges {
                if e.src == cur && !layer.contains_key(&e.dst) && e.kind != REMOVED {
                    let l = layer[&cur] + 1;
                    layer.insert(e.dst.clone(), l);
                    queue.push(e.dst.clone());
                }
            }
        }
    };
    bfs(&mut layer, &mut queue);
    for o in outside {
        for e in &edges {
            if &e.src == o && !layer.contains_key(&e.dst) {
                layer.insert(e.dst.clone(), 1);
                queue.push(e.dst.clone());
            }
        }
    }
    bfs(&mut layer, &mut queue);
    let mut last = layer.values().copied().max().unwrap_or(0);
    for n in nodes {
        if !layer.contains_key(n) && !outside.contains(n) {
            last += 1;
            layer.insert(n.clone(), last);
        }
    }
    for o in outside {
        let m = edges.iter().filter(|e| &e.src == o).map(|e| layer[&e.dst]).min().unwrap_or(1);
        layer.insert(o.clone(), m - 1);
    }
    let mut cols: IndexMap<i64, Vec<String>> = IndexMap::new();
    for n in nodes {
        cols.entry(layer[n]).or_default().push(n.clone());
    }
    let width: IndexMap<&String, N> = nodes.iter().map(|n| (n, tw(n) + 32)).collect();
    let col_w: IndexMap<i64, N> = cols.iter().map(|(c, ns)| (*c, pymax_of(ns.iter().map(|n| width[n])))).collect();
    let mut x_of_col: IndexMap<i64, N> = IndexMap::new();
    let mut x = N::I(PAD);
    let mut keys: Vec<i64> = cols.keys().copied().collect();
    keys.sort();
    for c in &keys {
        x_of_col.insert(*c, x);
        x = x + col_w[c] + COL_GAP;
    }
    let mut pos: IndexMap<String, (N, N)> = IndexMap::new();
    for (c, ns) in &cols {
        for (i, n) in ns.iter().enumerate() {
            let cx = x_of_col[c] + col_w[c] / 2;
            pos.insert(n.clone(), (cx, N::I(PAD + 28 + i as i64 * (NODE_H + ROW_GAP)) + N::I(NODE_H) / 2));
        }
    }
    let max_rows = cols.values().map(|v| v.len()).max().unwrap_or(0) as i64;
    let mut body_h = N::I(PAD + 28 + max_rows * (NODE_H + ROW_GAP));
    let total_w = x - COL_GAP + PAD;
    let mut parts: Vec<String> = vec![];
    let mut loops_below = 0i64;
    for e in &edges {
        let ((x1, y1), (x2, y2)) = (pos[&e.src], pos[&e.dst]);
        let cls = format!("edge {}", e.kind);
        let (d, lx, ly);
        if e.src == e.dst {
            d = format!(
                "M{} {} C{} {} {} {} {} {}",
                x1 - 10,
                y1 - N::I(NODE_H) / 2,
                x1 - 30,
                y1 - NODE_H - 22,
                x1 + 30,
                y1 - NODE_H - 22,
                x1 + 10,
                y1 - N::I(NODE_H) / 2
            );
            lx = x1;
            ly = y1 - NODE_H - 16;
        } else if layer[&e.dst] > layer[&e.src] {
            let sx = x1 + width[&e.src] / 2;
            let ex = x2 - width[&e.dst] / 2;
            let mid = (sx + ex) / 2;
            d = format!("M{} {} C{} {} {} {} {} {}", sx, y1, mid, y1, mid, y2, ex - 2, y2);
            lx = mid;
            ly = (y1 + y2) / 2 - 6;
        } else if layer[&e.dst] == layer[&e.src] {
            let sx = x1 + width[&e.src] / 2;
            let ex = x2 + width[&e.dst] / 2;
            let out = pymax(sx, ex) + 44;
            d = format!("M{} {} C{} {} {} {} {} {}", sx, y1, out, y1, out, y2, ex + 2, y2);
            lx = out - 4;
            ly = (y1 + y2) / 2 + 4;
        } else {
            loops_below += 1;
            let (a, b) = (layer[&e.src], layer[&e.dst]);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let bottom = pymax_of(nodes.iter().filter(|n| lo <= layer[*n] && layer[*n] <= hi).map(|n| pos[n].1)) + N::I(NODE_H) / 2;
            let low = bottom + 22 + 18 * loops_below;
            let (sy, ey) = (y1 + N::I(NODE_H) / 2, y2 + N::I(NODE_H) / 2);
            d = format!("M{} {} C{} {} {} {} {} {}", x1, sy, x1, low, x2, low, x2, ey + 2);
            lx = (x1 + x2) / 2;
            ly = low - 4 - (low - pymax(sy, ey)) * 0.25;
            body_h = pymax(body_h, low + 12);
        }
        parts.push(format!("<path class=\"{}\" d=\"{}\" marker-end=\"url(#arrow-{})\"/>", cls, d, e.kind));
        let lw = text_width(&e.label, N::F(11.5)) + 10;
        parts.push(format!(
            "<g class=\"elabel {}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"17\" rx=\"4\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>",
            e.kind,
            (lx - lw / 2).d1(),
            (ly - 13).d1(),
            lw.d1(),
            lx.d1(),
            ly.d1(),
            escape(&e.label)
        ));
    }
    for n in nodes {
        let (cx, cy) = pos[n];
        let w = width[n];
        let mut classes: Vec<&str> = vec!["node", SAME];
        if outside.contains(n) {
            classes.push("outside");
        }
        if n == start {
            classes.push("start");
        }
        if finals.contains(n) {
            classes.push("final");
        }
        let mut inner = String::new();
        if finals.contains(n) {
            inner = format!(
                "<rect class=\"inner\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"6\"/>",
                (cx - w / 2 + 3).d1(),
                (cy - N::I(NODE_H) / 2 + 3).d1(),
                (w - 6).d1(),
                NODE_H - 6
            );
        }
        parts.push(format!(
            "<g class=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\"/>{}<text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>",
            classes.join(" "),
            (cx - w / 2).d1(),
            (cy - N::I(NODE_H) / 2).d1(),
            w.d1(),
            NODE_H,
            inner,
            cx.d1(),
            (cy + N::I(FONT) / 3 + 1).d1(),
            escape(n)
        ));
        if n == start {
            parts.push(format!("<path class=\"edge same\" d=\"M{} {} H{}\" marker-end=\"url(#arrow-same)\"/>", cx - w / 2 - 22, cy, cx - w / 2 - 2));
        }
    }
    let height = body_h + PAD;
    svg(total_w + 12, height, &parts, title)
}

/// cases は (アクター, ユースケース名, 種類)。externals は (外部システム名, 添える行, 使うユースケース名の並び)
pub fn usecases(actors: &[String], cases: &[(String, String, String)], title: &str, externals: &[(String, String, Vec<String>)]) -> String {
    let actor_w = pymax_of(actors.iter().map(|a| tw(a)).chain(std::iter::once(N::I(60)))) + 16;
    let case_w = pymax_of(cases.iter().map(|c| tw(&c.1)).chain(std::iter::once(N::I(80)))) + 48;
    let ax = N::I(PAD) + actor_w / 2;
    let ux = N::I(PAD) + actor_w + 120 + case_w / 2;
    let mut parts: Vec<String> = vec![];
    let mut y = N::I(PAD);
    let mut ycase: IndexMap<String, N> = IndexMap::new();
    for a in actors {
        let by_actor: Vec<&(String, String, String)> = cases.iter().filter(|c| &c.0 == a).collect();
        let n_items = by_actor.len().max(1) as i64;
        let block_h = N::I(n_items * (NODE_H + 14) - 14);
        let ay = y + block_h / 2;
        for (i, (_, name, kind)) in by_actor.iter().map(|c| (&c.0, &c.1, &c.2)).enumerate() {
            let cy = y + N::I(i as i64 * (NODE_H + 14)) + N::I(NODE_H) / 2;
            ycase.insert(name.clone(), cy);
            parts.push(format!(
                "<path class=\"edge {}\" d=\"M{} {} C{} {} {} {} {} {}\"/>",
                kind,
                ax + actor_w / 2 - 4,
                ay,
                ax + actor_w / 2 + 60,
                ay,
                ux - case_w / 2 - 60,
                cy,
                ux - case_w / 2,
                cy
            ));
            parts.push(format!(
                "<g class=\"usecase {}\"><ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>",
                kind,
                ux,
                cy,
                case_w / 2,
                N::I(NODE_H) / 2,
                ux,
                (cy + N::I(FONT) / 3 + 1).d1(),
                escape(name)
            ));
        }
        parts.push(format!(
            "<g class=\"actor\"><circle cx=\"{}\" cy=\"{}\" r=\"7\"/><path d=\"M{} {} V{} M{} {} H{} M{} {} L{} {} M{} {} L{} {}\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>",
            ax,
            ay - 22,
            ax,
            ay - 15,
            ay - 1,
            ax - 10,
            ay - 10,
            ax + 10,
            ax,
            ay - 1,
            ax - 8,
            ay + 10,
            ax,
            ay - 1,
            ax + 8,
            ay + 10,
            ax,
            ay + 27,
            escape(a)
        ));
        y = y + pymax(block_h, N::I(64)) + 28;
    }
    let mut width = ux + case_w / 2 + PAD;
    let mut height = y - 28 + PAD + 12;
    if !externals.is_empty() {
        let box_h = N::I(NODE_H + 16);
        let sys_w = pymax_of(externals.iter().map(|(n, _, _)| tw(n)).chain(externals.iter().map(|(_, t, _)| text_width(t, N::I(11)))).chain(std::iter::once(N::I(80)))) + 40;
        let sx = ux + case_w / 2 + 90 + sys_w / 2;
        let mut want: Vec<(N, &String, &String, &Vec<String>)> = externals
            .iter()
            .map(|(n, t, us)| {
                let hit: Vec<N> = us.iter().filter_map(|u| ycase.get(u).copied()).collect();
                let s = pysum(hit.iter().copied());
                (s / N::I(hit.len().max(1) as i64), n, t, us)
            })
            .collect();
        want.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut last = N::I(PAD) - box_h;
        for (cy0, n, t, us) in &want {
            let by = pymax(*cy0, last + box_h + 18);
            last = by;
            for u in us.iter() {
                if let Some(cu) = ycase.get(u) {
                    parts.push(format!(
                        "<path class=\"edge same\" d=\"M{} {} C{} {} {} {} {} {}\" marker-end=\"url(#arrow-same)\"/>",
                        ux + case_w / 2,
                        cu,
                        ux + case_w / 2 + 70,
                        cu,
                        sx - sys_w / 2 - 70,
                        by,
                        sx - sys_w / 2 - 2,
                        by
                    ));
                }
            }
            parts.push(format!(
                "<g class=\"node\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text><text class=\"sub\" x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>",
                sx - sys_w / 2,
                by - box_h / 2,
                sys_w,
                box_h,
                sx,
                by - 2,
                escape(n),
                sx,
                by + 14,
                escape(t)
            ));
        }
        width = sx + sys_w / 2 + PAD;
        height = pymax(height, last + box_h / 2 + PAD);
    }
    svg(width, height, &parts, title)
}

/// システム構成図。systems は (名前, 種類, 矢印に添える行, 差分の種類)
pub fn context(center: &str, systems: &[(String, String, Vec<String>, String)], title: &str) -> String {
    let line_h = 16i64;
    let box_h = N::I(NODE_H + 16);
    let labels_w = pymax_of(systems.iter().flat_map(|(_, _, ls, _)| ls.iter().map(|t| text_width(t, N::F(11.5)))).chain(std::iter::once(N::I(80))));
    let sys_w = pymax_of(systems.iter().map(|(n, _, _, _)| tw(n)).chain(std::iter::once(N::I(80)))) + 40;
    let center_w = tw(center) + 48;
    let blocks: Vec<N> = systems.iter().map(|(_, _, ls, _)| pymax(box_h, N::I(ls.len() as i64 * line_h) + box_h / 2 + 14)).collect();
    let sum = pysum(blocks.iter().copied());
    let total_h = sum + 20 * (systems.len() as i64 - 1) + 2 * PAD;
    let cx = N::I(PAD) + center_w / 2;
    let cy = total_h / 2;
    let x0 = N::I(PAD) + center_w;
    let xh = x0 + 120;
    let x1 = xh + labels_w + 24;
    let sx = x1 + sys_w / 2;
    let mut parts: Vec<String> = vec![format!(
        "<g class=\"node start\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"60\" rx=\"8\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-weight=\"600\">{}</text></g>",
        PAD,
        cy - 30,
        center_w,
        cx,
        cy + 5,
        escape(center)
    )];
    let mut y = N::I(PAD);
    for ((name, kind, lines, ekind), bh) in systems.iter().zip(blocks.iter()) {
        let my = y + *bh - box_h / 2;
        parts.push(format!("<path class=\"edge {}\" d=\"M{} {} C{} {} {} {} {} {} H{}\" marker-end=\"url(#arrow-{})\"/>", ekind, x0, cy, x0 + 70, cy, xh - 50, my, xh, my, x1 - 2, ekind));
        for (i, t) in lines.iter().rev().enumerate() {
            parts.push(format!("<g class=\"elabel {}\"><text x=\"{}\" y=\"{}\" text-anchor=\"end\">{}</text></g>", ekind, x1 - 14, (my - 7 - N::I(i as i64 * line_h)).d1(), escape(t)));
        }
        parts.push(format!(
            "<g class=\"node {}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text><text class=\"sub\" x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>",
            ekind,
            x1,
            my - box_h / 2,
            sys_w,
            box_h,
            sx,
            my - 2,
            escape(name),
            sx,
            my + 14,
            escape(kind)
        ));
        y = y + *bh + 20;
    }
    svg(x1 + sys_w + PAD, total_h, &parts, title)
}

#[derive(Clone, Debug)]
pub struct Message {
    pub src: String,
    pub dst: String,
    pub label: String,
    pub style: String,
    pub flag: String,
}

impl Message {
    pub fn new(src: &str, dst: &str, label: &str) -> Message {
        Message { src: src.into(), dst: dst.into(), label: label.into(), style: "solid".into(), flag: String::new() }
    }
    pub fn dashed(src: &str, dst: &str, label: &str) -> Message {
        Message { style: "dashed".into(), ..Message::new(src, dst, label) }
    }
}

pub fn sequence(participants: &[String], messages: &[Message], title: &str) -> String {
    let f115 = N::F(11.5);
    let widest = pymax_of(messages.iter().filter(|m| m.src != m.dst).map(|m| text_width(&m.label, f115)).chain(std::iter::once(N::I(60))));
    let notes_w = pymax_of(messages.iter().filter(|m| m.src == m.dst).map(|m| text_width(&m.label, f115) + 20).chain(std::iter::once(N::I(0))));
    let col = pymax(pymax(N::I(160), widest + 40), notes_w + 16);
    let head_w: IndexMap<&String, N> = participants.iter().map(|p| (p, tw(p) + 28)).collect();
    let half = pymax(pymax_of(head_w.values().copied()), notes_w) / 2;
    let xs: IndexMap<&String, N> = participants.iter().enumerate().map(|(i, p)| (p, N::I(PAD) + half + col * (i as i64))).collect();
    let (row, top) = (46i64, PAD + NODE_H + 30);
    let total_w = pymax_of(xs.values().copied()) + half + PAD;
    let total_h = N::I(top + messages.len() as i64 * row + PAD);
    let mut parts: Vec<String> = vec![];
    for p in participants {
        let x = xs[p];
        parts.push(format!("<path class=\"lifeline\" d=\"M{} {} V{}\"/>", x, PAD + NODE_H, total_h - PAD + 4));
        parts.push(format!(
            "<g class=\"node\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>",
            x - head_w[p] / 2,
            PAD,
            head_w[p],
            NODE_H,
            x,
            (N::I(PAD) + N::I(NODE_H) / 2 + N::I(FONT) / 3 + 1).d1(),
            escape(p)
        ));
    }
    for (i, m) in messages.iter().enumerate() {
        let y = N::I(top + i as i64 * row);
        let flag = if m.flag.is_empty() { String::new() } else { format!(" {}", m.flag) };
        if m.src == m.dst {
            let w = text_width(&m.label, f115) + 20;
            let x = xs[&m.src];
            parts.push(format!(
                "<g class=\"note{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"24\" rx=\"4\"/><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>",
                flag,
                (x - w / 2).d1(),
                y - 14,
                w.d1(),
                x,
                y + 2,
                escape(&m.label)
            ));
            continue;
        }
        let (x1, x2) = (xs[&m.src], xs[&m.dst]);
        let end = if x2 > x1 { x2 - 2 } else { x2 + 2 };
        let dash = if m.style == "dashed" { " stroke-dasharray=\"5 4\"" } else { "" };
        parts.push(format!("<path class=\"msg{}\" d=\"M{} {} H{}\"{} marker-end=\"url(#arrow-same)\"/>", flag, x1, y, end, dash));
        parts.push(format!("<g class=\"mlabel{}\"><text x=\"{}\" y=\"{}\" text-anchor=\"middle\">{}</text></g>", flag, ((x1 + x2) / 2).d1(), y - 7, escape(&m.label)));
    }
    svg(total_w, total_h, &parts, title)
}

// ---- 処理の中身のフローチャート ----
const STEP_W: i64 = 300;
const STEP_IN: i64 = 34;
const LINE_H: i64 = 18;

pub fn wrap(s: &str, width: N, size: N) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    let mut cur = String::new();
    let marked = s.replace(' ', " \0");
    for word in marked.split('\0') {
        let mut word: Vec<char> = word.chars().collect();
        loop {
            let probe: String = cur.chars().chain(word.iter().copied()).collect();
            if !(text_width(&probe, size) > width) {
                break;
            }
            if !cur.trim().is_empty() {
                lines.push(cur.trim_end().to_string());
                cur = String::new();
                continue;
            }
            let mut cut = 1usize;
            while cut < word.len() && text_width(&word[..cut + 1].iter().collect::<String>(), size) <= width {
                cut += 1;
            }
            lines.push(word[..cut].iter().collect());
            word = word[cut..].to_vec();
        }
        cur.extend(word.iter());
    }
    if !cur.trim().is_empty() {
        lines.push(cur.trim_end().to_string());
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// 処理の中身のフローチャートの木（step_tree の値）
#[derive(Clone, Debug)]
pub enum Step {
    Do(String),
    Result(String),
    If(String, Vec<Step>, Option<Vec<Step>>),
    Absent(String, String),
    Each(String, Vec<Step>),
}

struct Steps {
    parts: Vec<String>,
    right: N,
}

impl Steps {
    fn box_(&mut self, kind: &str, text: &str, x: N, y: N) -> (N, N) {
        let lines = wrap(text, N::I(STEP_W - 28), N::I(FONT));
        let w = pymin(N::I(STEP_W), pymax_of(lines.iter().map(|l| tw(l))) + 28);
        let h = N::I(lines.len() as i64 * LINE_H + 14);
        let shape = if kind == "分かれ道" {
            format!("<path d=\"M{} {} H{} L{} {} L{} {} H{} L{} {} Z\"/>", x + 10, y, x + w - 10, x + w, y + h / 2, x + w - 10, y + h, x + 10, x, y + h / 2)
        } else {
            format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\"/>", x.d1(), y.d1(), w.d1(), h, (if kind == "結果" { h / 2 } else { N::I(4) }).d1())
        };
        let cls = match kind {
            "結果" => "node step final",
            "分かれ道" => "node step branch",
            _ => "node step",
        };
        let text_svg: String = lines.iter().enumerate().map(|(i, ln)| format!("<text x=\"{}\" y=\"{}\">{}</text>", (x + 14).d1(), (y + 7 + (LINE_H * (i as i64 + 1)) - 5).d1(), escape(ln))).collect();
        self.parts.push(format!("<g class=\"{}\">{}{}</g>", cls, shape, text_svg));
        self.right = pymax(self.right, x + w);
        (y + h, w)
    }

    fn label(&mut self, text: &str, x: N, y: N) {
        self.parts.push(format!("<text class=\"steplabel\" x=\"{}\" y=\"{}\">{}</text>", x.d1(), y.d1(), escape(text)));
    }

    fn arrow(&mut self, d: &str) {
        self.parts.push(format!("<path class=\"edge same\" d=\"{}\" marker-end=\"url(#arrow-same)\"/>", d));
    }

    fn branch(&mut self, sub: &[Step], text: &str, x: N, y_from: N, y: N) -> (N, bool) {
        let (sx, bx) = (x + 14, x + STEP_IN);
        self.label(text, bx + 18, y + 10);
        let top = y + 16;
        self.arrow(&format!("M{} {} V{} H{} V{}", sx, y_from, top - 8, bx + 14, top - 2));
        let (bottom, ends) = self.seq(sub, bx, top);
        let mut bottom = bottom;
        if !ends {
            self.parts.push(format!("<path class=\"edge same\" d=\"M{} {} V{} H{}\"/>", bx + 14, bottom, bottom + 8, sx));
            bottom = bottom + 8;
        }
        (bottom, !ends)
    }

    fn seq(&mut self, nodes: &[Step], x: N, y: N) -> (N, bool) {
        let mut y = y;
        let sx = x + 14;
        let mut prev: Option<N> = None;
        for node in nodes {
            if let Some(p) = prev {
                self.arrow(&format!("M{} {} V{}", sx, p, y - 2));
            }
            match node {
                Step::Result(t) => {
                    let (b, _) = self.box_("結果", t, x, y);
                    return (b, true);
                }
                Step::Do(t) => {
                    let (b, _) = self.box_("する", t, x, y);
                    prev = Some(b);
                    y = b + 16;
                }
                Step::Absent(head, r) => {
                    let (bottom, _) = self.box_("する", head, x, y);
                    let (end, _) = self.branch(&[Step::Result(r.clone())], "無ければ", x, bottom, bottom + 2);
                    prev = Some(bottom);
                    y = end + 22;
                    self.label("あれば", sx + 6, y - 8);
                }
                Step::Each(head, sub) => {
                    let (bottom, _) = self.box_("分かれ道", head, x, y);
                    let (end, _) = self.branch(sub, "1 つずつ", x, bottom, bottom + 2);
                    prev = Some(bottom);
                    y = end + 16;
                }
                Step::If(cond, then, els) => {
                    let (bottom, _) = self.box_("分かれ道", cond, x, y);
                    let (end, back) = self.branch(then, "はい", x, bottom, bottom + 2);
                    if let Some(els) = els {
                        let (end2, back2) = self.branch(els, "いいえ", x, bottom, end + 6);
                        if !(back || back2) {
                            return (end2, true);
                        }
                        prev = Some(bottom);
                        y = end2 + 16;
                    } else {
                        prev = Some(bottom);
                        y = end + 22;
                        self.label("いいえ", sx + 6, y - 8);
                    }
                }
            }
        }
        (prev.unwrap_or(y), false)
    }
}

/// 処理の中身のフローチャート。上から下へ進む
pub fn steps(tree: &[Step], title: &str) -> String {
    let mut s = Steps { parts: vec![], right: N::F(0.0) };
    let start = N::I(PAD) / 2;
    let (bottom, _) = s.seq(tree, start, start);
    let (right, bottom) = (s.right, bottom);
    svg(right + N::I(PAD) / 2, bottom + N::I(PAD) / 2, &s.parts, title)
}
