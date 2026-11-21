// SPDX-License-Identifier: GPL-3.0-or-later
//! 文書サイトを作る：docs/・skills/・見本の計画書の Markdown を、左サイドバー付きの HTML にする（GitHub Pages 用）。
//! リンクは、変換する Markdown どうしなら .html に直し、それ以外のリポジトリ内のファイルは GitHub 上の場所にする。
use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};
use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const REPO: &str = "https://github.com/tazyamah/geas";
const FIX: &str = "tests/fixtures/overseas_shop";
const CSS: &str = include_str!("site.css");
const FONTS: &str = "<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\"><link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans+JP:wght@400;500;600&family=Zen+Kaku+Gothic+New:wght@700&display=swap\">";

type Nav = Vec<(String, Vec<(String, String)>)>;

fn nav(groups: &[(&str, &[(&str, &str)])]) -> Nav {
    groups.iter().map(|(g, items)| (g.to_string(), items.iter().map(|(p, o)| (p.to_string(), o.to_string())).collect())).collect()
}

struct Lang {
    code: &'static str,
    nav: Nav,
    sub: &'static str,
    spec: Vec<(&'static str, &'static str)>,
    spec_h: &'static str,
    other: &'static str,
    other_label: &'static str,
    root: &'static str,
    menu: &'static str,
    brand: &'static str,
    src: &'static str,
}

impl Lang {
    fn pages(&self) -> HashMap<String, String> {
        self.nav.iter().flat_map(|(_, items)| items.iter().cloned()).collect()
    }
}

fn langs() -> Vec<Lang> {
    let ja = nav(&[
        ("はじめに", &[("docs/getting-started.md", "getting-started"), ("docs/how-it-works.md", "how-it-works"), ("docs/README.md", "index")]),
        (
            "使う",
            &[
                ("docs/ai-guide.md", "ai-guide"),
                ("docs/language-reference.md", "language-reference"),
                ("docs/language-concepts.md", "language-concepts"),
                ("docs/modules.md", "modules"),
                ("docs/rest.md", "rest"),
                ("docs/datetime.md", "datetime"),
                ("docs/i18n.md", "i18n"),
            ],
        ),
        ("動かす", &[("docs/server.md", "server"), ("docs/standalone.md", "standalone")]),
        ("信頼の根拠", &[("docs/why-spec-and-code-dont-drift.md", "why-spec-and-code-dont-drift"), ("docs/language-test-map.md", "language-test-map")]),
        ("AI の作業手順", &[("skills/plan-interview/SKILL.md", "skill-plan-interview"), ("skills/geas-develop/SKILL.md", "skill-geas-develop")]),
        (
            "見本（お店の例）",
            &[
                ("tests/fixtures/overseas_shop/overseas_shop_plan.md", "sample-plan"),
                ("tests/fixtures/overseas_shop/overseas_shop_spec.md", "sample-spec-md"),
                ("tests/fixtures/overseas_shop/overseas_shop_mutation.md", "sample-mutation"),
            ],
        ),
        ("これからのこと・ライセンス", &[("docs/roadmap.md", "roadmap"), ("docs/handoff.md", "handoff"), ("docs/license.md", "license")]),
        (
            "経緯・記録（現在の仕様ではない）",
            &[
                ("docs/history/README.md", "history"),
                ("docs/history/v0.1.0-scope.md", "v0.1.0-scope"),
                ("docs/history/naming.md", "naming"),
                ("docs/history/rest-decisions.md", "rest-decisions"),
                ("docs/history/modules-history.md", "modules-history"),
                ("docs/history/proof-ideas.md", "proof-ideas"),
                ("docs/history/roadmap-done.md", "roadmap-done"),
                ("docs/history/open-questions.md", "open-questions"),
                ("docs/history/reviews/review-2026-11.md", "review-2026-11"),
                ("docs/history/reviews/review-2026-11-constraints.md", "review-2026-11-constraints"),
            ],
        ),
    ]);
    let en = nav(&[
        ("Start here", &[("docs/en/getting-started.md", "getting-started"), ("docs/en/how-it-works.md", "how-it-works"), ("docs/en/README.md", "index")]),
        (
            "Use",
            &[
                ("docs/en/ai-guide.md", "ai-guide"),
                ("docs/en/language-reference.md", "language-reference"),
                ("docs/en/language-concepts.md", "language-concepts"),
                ("docs/en/modules.md", "modules"),
                ("docs/en/rest.md", "rest"),
                ("docs/en/datetime.md", "datetime"),
                ("docs/en/i18n.md", "i18n"),
            ],
        ),
        ("Run", &[("docs/en/server.md", "server"), ("docs/en/standalone.md", "standalone")]),
        ("Why you can trust it", &[("docs/en/why-spec-and-code-dont-drift.md", "why-spec-and-code-dont-drift"), ("docs/en/language-test-map.md", "language-test-map")]),
        ("Working procedures for AI", &[("docs/en/skill-plan-interview.md", "skill-plan-interview"), ("docs/en/skill-geas-develop.md", "skill-geas-develop")]),
        ("What is next, license", &[("docs/en/roadmap.md", "roadmap"), ("docs/en/handoff.md", "handoff"), ("docs/en/license.md", "license")]),
    ]);
    debug_assert!(FIX == "tests/fixtures/overseas_shop");
    vec![
        Lang {
            code: "ja",
            nav: ja,
            sub: "docs",
            spec: vec![("仕様書の全体", "../overseas_shop.html"), ("前の版からの変更", "../overseas_shop_changes.html")],
            spec_h: "仕様書の見本",
            other: "en",
            other_label: "English",
            root: "../",
            menu: "メニュー",
            brand: "<b>Geas</b> ギアス",
            src: "元の Markdown：",
        },
        Lang {
            code: "en",
            nav: en,
            sub: "en/docs",
            spec: vec![("Sample specification (Japanese)", "../../overseas_shop.html"), ("Changes since the previous version (Japanese)", "../../overseas_shop_changes.html")],
            spec_h: "Sample specification",
            other: "ja",
            other_label: "日本語",
            root: "../",
            menu: "Menu",
            brand: "<b>Geas</b>",
            src: "Source Markdown: ",
        },
    ]
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// 見出しの id。GitHub と同じく、日本語を残し、記号を消して、空白を - にする
fn slugify(value: &str) -> String {
    let lower = value.trim().to_lowercase();
    let kept: String = lower.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || c.is_whitespace()).collect();
    Regex::new(r"\s+").unwrap().replace_all(&kept, "-").into_owned()
}

fn title_of(text: &str, fallback: &str) -> String {
    let re = Regex::new(r"(?m)^#\s+(.+)$").unwrap();
    re.captures(text).map(|c| c[1].trim().to_string()).unwrap_or_else(|| fallback.to_string())
}

/// パスの `.`・`..` をたたむ
fn normpath(p: &str) -> String {
    let mut out: Vec<&str> = vec![];
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// 相対リンクを直す。src は変換元のリポジトリ内のパス
fn fix_link(url: &str, src: &str, lang: &Lang, all: &[Lang], root: &Path) -> String {
    if Regex::new(r"^(https?:|mailto:|#)").unwrap().is_match(url) {
        return url.to_string();
    }
    let (path, frag) = match url.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (url, None),
    };
    let dir = src.rsplit_once('/').map(|x| x.0).unwrap_or("");
    let target = normpath(&format!("{}/{}", dir, path));
    let frag_s = frag.map(|f| format!("#{}", f)).unwrap_or_default();
    let own = lang.pages();
    let other = all.iter().find(|l| l.code == lang.other).unwrap();
    if let Some(n) = own.get(&target) {
        format!("{}.html{}", n, frag_s)
    } else if let Some(n) = other.pages().get(&target) {
        let up = if lang.code == "en" { "../../docs/" } else { "../en/docs/" };
        format!("{}{}.html{}", up, n, frag_s)
    } else if root.join(&target).is_dir() {
        format!("{}/tree/main/{}", REPO, target)
    } else {
        format!("{}/blob/main/{}{}", REPO, target, frag_s)
    }
}

fn sidebar(cur: &str, titles: &HashMap<String, String>, lang: &Lang) -> String {
    let mut out = vec!["<nav class=\"side\">".to_string(), format!("<h4>{}</h4>", esc(lang.spec_h))];
    for (label, href) in &lang.spec {
        out.push(format!("<a class=\"spec\" href=\"{}\">{}</a>", href, esc(label)));
    }
    for (group, items) in &lang.nav {
        out.push(format!("<h4>{}</h4>", esc(group)));
        for (p, name) in items {
            let c = if name == cur { " class=\"cur\"" } else { "" };
            out.push(format!("<a{} href=\"{}.html\">{}</a>", c, name, esc(&titles[p])));
        }
    }
    out.push("</nav>".to_string());
    out.join("\n")
}

fn page(title: &str, nav: &str, body: &str, src: &str, lang: &Lang, counterpart: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"{code}\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>{title} - Geas</title>{fonts}<style>{css}</style></head>\n<body><input type=\"checkbox\" id=\"nav-toggle\">\n<div class=\"top\"><label class=\"burger\" for=\"nav-toggle\" aria-label=\"{menu}\">☰</label>\n<a class=\"brand\" href=\"{root}\">{brand}</a><a class=\"gh\" href=\"{counter}\">{other}</a><a class=\"gh2\" href=\"{repo}\">GitHub</a></div>\n<div class=\"layout\">{nav}<main><article>{body}\n<p class=\"src\">{srcl}<a href=\"{repo}/blob/main/{src}\">{src}</a></p></article></main></div></body></html>",
        code = lang.code,
        title = esc(title),
        fonts = FONTS,
        css = CSS,
        menu = lang.menu,
        root = lang.root,
        brand = lang.brand,
        counter = counterpart,
        other = lang.other_label,
        repo = REPO,
        nav = nav,
        body = body,
        srcl = lang.src,
        src = src
    )
}

/// Markdown を HTML の本文にする。見出しに id を付け、相対リンクを直す
fn render(text: &str, src: &str, lang: &Lang, all: &[Lang], root: &Path) -> String {
    let opts = Options::ENABLE_TABLES;
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut events: Vec<Event> = vec![];
    let mut in_heading: Option<usize> = None; // 見出しの開始イベントの位置
    let mut heading_text = String::new();
    for ev in Parser::new_ext(text, opts) {
        match ev {
            Event::Start(Tag::Heading { .. }) => {
                in_heading = Some(events.len());
                heading_text.clear();
                events.push(ev);
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(i) = in_heading.take() {
                    let base = slugify(&heading_text);
                    let n = used.entry(base.clone()).or_insert(0);
                    let id = if *n == 0 { base.clone() } else { format!("{}-{}", base, n) };
                    *n += 1;
                    if let Event::Start(Tag::Heading { level, classes, attrs, .. }) = events[i].clone() {
                        events[i] = Event::Start(Tag::Heading { level, id: Some(CowStr::from(id)), classes, attrs });
                    }
                }
                events.push(ev);
            }
            Event::Text(ref t) | Event::Code(ref t) if in_heading.is_some() => {
                heading_text.push_str(t);
                events.push(ev.clone());
            }
            Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
                let fixed = fix_link(&dest_url, src, lang, all, root);
                events.push(Event::Start(Tag::Link { link_type, dest_url: CowStr::from(fixed), title, id }));
            }
            e => events.push(e),
        }
    }
    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

fn build(root: &Path, out_dir: &Path, lang: &Lang, all: &[Lang]) -> Result<(), String> {
    let pages = lang.pages();
    let other = all.iter().find(|l| l.code == lang.other).unwrap();
    let out: PathBuf = out_dir.join(lang.sub);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let mut texts: Vec<(String, String)> = vec![];
    for (p, _) in lang.nav.iter().flat_map(|(_, i)| i.iter()) {
        let t = std::fs::read_to_string(root.join(p)).map_err(|e| format!("{}: {}", p, e))?;
        texts.push((p.clone(), t));
    }
    let titles: HashMap<String, String> = texts.iter().map(|(p, t)| (p.clone(), title_of(t, Path::new(p).file_stem().and_then(|s| s.to_str()).unwrap_or("")))).collect();
    // 同じ名前のページがもう一方の言語にもあれば、そこへ切り替える。無ければ、その言語の入口へ
    let other_names: std::collections::HashSet<String> = other.pages().values().cloned().collect();
    for (p, text) in &texts {
        let name = &pages[p];
        let body = render(text, p, lang, all, root);
        let target = if other_names.contains(name) { name.as_str() } else { "index" };
        let counterpart = format!("{}{}.html", if lang.code == "ja" { "../en/docs/" } else { "../../docs/" }, target);
        let html = page(&titles[p], &sidebar(name, &titles, lang), &body, p, lang, &counterpart);
        std::fs::write(out.join(format!("{}.html", name)), html).map_err(|e| e.to_string())?;
    }
    println!("{}: {} pages -> {}", lang.code, texts.len(), out.display());
    Ok(())
}

pub fn run(root: &Path, out_dir: &Path) -> Result<(), String> {
    let all = langs();
    for l in &all {
        build(root, out_dir, l, &all)?;
    }
    Ok(())
}
