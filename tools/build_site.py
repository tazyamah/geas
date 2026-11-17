"""文書サイトを作る：docs/・skills/・見本の計画書の Markdown を、左サイドバー付きの HTML にする（GitHub Pages 用）。

使い方：python tools/build_site.py 出力先
Markdown は `markdown` パッケージで変換する（CI だけで使う。geas 本体の依存ではない）。
リンクは、変換する Markdown どうしなら .html に直し、それ以外のリポジトリ内のファイルは GitHub 上の場所にする。
"""
import html
import posixpath
import re
import sys
from pathlib import Path

import markdown

ROOT = Path(__file__).resolve().parent.parent
REPO = "https://github.com/tazyamah/geas"
FIX = "tests/fixtures/overseas_shop"

# サイドバーの並び（グループ名, [(Markdown のパス, 出力名)]）。載せない Markdown は変換しない。
NAV = [
    ("はじめに", [("docs/README.md", "index")]),
    ("使う", [
        ("docs/ai-guide.md", "ai-guide"),
        ("docs/language-reference.md", "language-reference"),
        ("docs/language-concepts.md", "language-concepts"),
        ("docs/modules.md", "modules"),
        ("docs/rest.md", "rest"),
        ("docs/datetime.md", "datetime"),
    ]),
    ("動かす", [
        ("docs/server.md", "server"),
        ("docs/standalone.md", "standalone"),
    ]),
    ("信頼の根拠", [
        ("docs/why-spec-and-code-dont-drift.md", "why-spec-and-code-dont-drift"),
        ("docs/language-test-map.md", "language-test-map"),
    ]),
    ("AI の作業手順", [
        ("skills/plan-interview/SKILL.md", "skill-plan-interview"),
        ("skills/geas-develop/SKILL.md", "skill-geas-develop"),
    ]),
    ("見本（お店の例）", [
        (f"{FIX}/overseas_shop_plan.md", "sample-plan"),
        (f"{FIX}/overseas_shop_spec.md", "sample-spec-md"),
        (f"{FIX}/overseas_shop_mutation.md", "sample-mutation"),
    ]),
    ("これからのこと", [
        ("docs/roadmap.md", "roadmap"),
        ("docs/handoff.md", "handoff"),
        ("docs/open-questions.md", "open-questions"),
    ]),
    ("経緯・記録", [
        ("docs/naming.md", "naming"),
        ("docs/license.md", "license"),
        ("docs/v0.1.0-scope.md", "v0.1.0-scope"),
        ("docs/records/review-2026-11.md", "review-2026-11"),
        ("docs/records/review-2026-11-constraints.md", "review-2026-11-constraints"),
    ]),
]
# 仕様書の HTML（別に生成する）。サイドバーの先頭に出す。
SPEC_LINKS = [("仕様書の全体", "../overseas_shop.html"), ("前の版からの変更", "../overseas_shop_changes.html")]

PAGES = {p: out for _, items in NAV for p, out in items}


def slugify(value, sep):
    """見出しの id。GitHub と同じく、日本語を残し、記号を消して、空白を - にする"""
    value = re.sub(r"[^\w\s-]", "", value.strip().lower())
    return re.sub(r"[\s]+", sep, value)


def title_of(text, fallback):
    m = re.search(r"^#\s+(.+)$", text, re.M)
    return m.group(1).strip() if m else fallback


def fix_links(body, src):
    """相対リンクを直す。src は変換元のリポジトリ内のパス"""
    def sub(m):
        url = m.group(2)
        if re.match(r"^(https?:|mailto:|#)", url):
            return m.group(0)
        path, _, frag = url.partition("#")
        target = posixpath.normpath(posixpath.join(posixpath.dirname(src), path))
        if target in PAGES:
            new = PAGES[target] + ".html" + ("#" + frag if frag else "")
        elif (ROOT / target).is_dir():
            new = f"{REPO}/tree/main/{target}"
        else:
            new = f"{REPO}/blob/main/{target}" + ("#" + frag if frag else "")
        return f'{m.group(1)}"{new}"'
    return re.sub(r'(<a href=)"([^"]+)"', sub, body)


CSS = """
:root{--bg:#f4f6f6;--surface:#fff;--ink:#18252a;--muted:#5a6b71;--line:#d3dcde;--accent:#0d6870;--accent-soft:#dff0f1;--code:#eaf0f1;
--font-display:"Zen Kaku Gothic New","Hiragino Sans","Noto Sans JP",sans-serif;
--font-body:"IBM Plex Sans JP","Hiragino Sans","Noto Sans JP","Yu Gothic",sans-serif;
--font-mono:"IBM Plex Mono",ui-monospace,Menlo,monospace}
@media(prefers-color-scheme:dark){:root{--bg:#101618;--surface:#172024;--ink:#e2eaec;--muted:#98a9ae;--line:#2b393d;--accent:#5cc0c8;--accent-soft:#15353a;--code:#1d2b30}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font-family:var(--font-body);line-height:1.85}
a{color:var(--accent)}
.top{position:sticky;top:0;z-index:5;display:flex;align-items:center;gap:12px;padding:10px 16px;background:var(--surface);border-bottom:1px solid var(--line)}
.top .brand{font-family:var(--font-display);font-size:18px;color:var(--ink);text-decoration:none}
.top .brand b{color:var(--accent)}
.top .gh{margin-left:auto;font-size:14px}
#nav-toggle{display:none}
.burger{display:none;cursor:pointer;font-size:22px;line-height:1;padding:4px 8px;border:1px solid var(--line);border-radius:8px}
.layout{display:flex;align-items:flex-start;max-width:1200px;margin:0 auto}
nav.side{flex:0 0 270px;position:sticky;top:53px;max-height:calc(100vh - 53px);overflow:auto;padding:20px 12px 40px 16px;font-size:14px;line-height:1.6}
nav.side h4{margin:18px 0 6px;font-size:12px;letter-spacing:.08em;color:var(--muted);font-weight:600}
nav.side a{display:block;padding:5px 10px;border-radius:8px;color:var(--ink);text-decoration:none}
nav.side a:hover{background:var(--accent-soft)}
nav.side a.cur{background:var(--accent-soft);color:var(--accent);font-weight:600}
nav.side a.spec{color:var(--accent);font-weight:600}
main{flex:1;min-width:0;padding:28px 24px 80px}
article{max-width:46em;background:var(--surface);border:1px solid var(--line);border-radius:14px;padding:12px 32px 32px}
h1,h2,h3{font-family:var(--font-display);line-height:1.4}
h1{font-size:28px;margin:24px 0 16px}
h2{font-size:22px;margin:40px 0 12px;padding-bottom:6px;border-bottom:1px solid var(--line)}
h3{font-size:18px;margin:28px 0 8px}
code{font-family:var(--font-mono);font-size:.88em;background:var(--code);padding:.1em .4em;border-radius:5px}
pre{background:var(--code);padding:14px 16px;border-radius:10px;overflow:auto;line-height:1.6}
pre code{background:none;padding:0}
table{border-collapse:collapse;display:block;overflow:auto;margin:16px 0}
th,td{border:1px solid var(--line);padding:6px 12px;text-align:left;vertical-align:top}
th{background:var(--accent-soft)}
blockquote{margin:16px 0;padding:2px 16px;border-left:4px solid var(--accent);color:var(--muted)}
.src{margin-top:32px;font-size:13px;color:var(--muted)}
@media(max-width:820px){
.burger{display:block}
nav.side{display:none;position:fixed;top:53px;left:0;right:0;bottom:0;max-height:none;background:var(--surface);z-index:4;padding:12px 16px 40px}
#nav-toggle:checked~.layout nav.side{display:block}
main{padding:16px 16px 60px}
article{padding:4px 18px 24px}}
"""

FONTS = ('<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="stylesheet" '
         'href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans+JP:wght@400;500;600'
         '&family=Zen+Kaku+Gothic+New:wght@700&display=swap">')


def sidebar(cur, titles):
    out = ['<nav class="side">']
    out.append("<h4>仕様書の見本</h4>")
    for label, href in SPEC_LINKS:
        out.append(f'<a class="spec" href="{href}">{html.escape(label)}</a>')
    for group, items in NAV:
        out.append(f"<h4>{html.escape(group)}</h4>")
        for p, name in items:
            c = ' class="cur"' if name == cur else ""
            out.append(f'<a{c} href="{name}.html">{html.escape(titles[p])}</a>')
    out.append("</nav>")
    return "\n".join(out)


def page(title, nav, body, src):
    return f"""<!doctype html>
<html lang="ja"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{html.escape(title)} - Geas</title>{FONTS}<style>{CSS}</style></head>
<body><input type="checkbox" id="nav-toggle">
<div class="top"><label class="burger" for="nav-toggle" aria-label="メニュー">☰</label>
<a class="brand" href="../"><b>Geas</b> ギアス</a><a class="gh" href="{REPO}">GitHub</a></div>
<div class="layout">{nav}<main><article>{body}
<p class="src">元の Markdown：<a href="{REPO}/blob/main/{src}">{src}</a></p></article></main></div></body></html>"""


def main(out_dir):
    out = Path(out_dir) / "docs"
    out.mkdir(parents=True, exist_ok=True)
    texts = {p: (ROOT / p).read_text(encoding="utf-8") for p in PAGES}
    titles = {p: title_of(t, Path(p).stem) for p, t in texts.items()}
    nav_cache = {}
    for p, text in texts.items():
        name = PAGES[p]
        md = markdown.Markdown(extensions=["tables", "fenced_code", "toc", "sane_lists"],
                               extension_configs={"toc": {"slugify": slugify}})
        body = fix_links(md.convert(text), p)
        (out / f"{name}.html").write_text(page(titles[p], sidebar(name, titles), body, p), encoding="utf-8")
    print(f"文書 {len(texts)} 枚を {out} に作りました")


if __name__ == "__main__":
    main(sys.argv[1])
