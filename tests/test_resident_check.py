"""常駐（resident）の検査・仕様書：メニューは入口の画面（引数なし）だけを開く。終了と開くは言語が足す。"""
from pathlib import Path

from geas.check import check
from geas.parser import ParseError, parse
from geas.spec import build

SRC = (Path(__file__).parent / "fixtures" / "solo_tweet" / "solo_tweet.geas").read_text(encoding="utf-8")


def errors(src):
    return [d.msg for d in check(parse(src)).diags if d.level == "エラー"]


def mutate(old, new):
    assert old in SRC, old
    return SRC.replace(old, new, 1)


def test_見本は通り_仕様書に常駐が出る():
    ck = check(parse(SRC))
    assert [d for d in ck.diags if d.level == "エラー"] == []
    rows = dict(build(ck)["常駐"])
    assert rows["右クリックのメニュー「つぶやきを書く」"] == "画面 TweetList を開く" and "終了" in "".join(rows)
    assert rows["右クリックのメニュー「未投稿をまとめて X へ」"] == "出来事 未投稿をまとめて X へ を起こす" and rows["左クリック"] == "窓を開閉する"


def test_常駐を書かなければ_仕様書に常駐は出ない():
    src = SRC[: SRC.index("// 常駐")] + SRC[SRC.index("needs secret"):]
    assert "resident" not in src and build(check(parse(src)))["常駐"] == []


def test_メニューが開けるのは_入口の画面で_引数を持たないものだけ():
    assert any("開けません" in m for m in errors(mutate('menu "つぶやきを書く" → TweetList', 'menu "つぶやきを書く" → TweetDetail')))
    assert any("がありません" in m for m in errors(mutate('menu "つぶやきを書く" → TweetList', 'menu "つぶやきを書く" → Nothing')))


def test_終了の項目は言語が足す_ラベルは重ねない():
    assert any("終了" in m for m in errors(mutate('menu "つぶやきを書く" → TweetList', 'menu "終了" → TweetList')))
    assert any("重なって" in m for m in errors(mutate('menu "つぶやきを書く" → TweetList', 'menu "a" → TweetList\n  menu "a" → TweetList')))


def test_startはshownかhiddenだけ_residentは1つだけ():
    for bad in ('start now', 'start'):
        try:
            parse(mutate('tooltip "ひとりつぶやき"', bad))
            assert False, bad
        except ParseError:
            pass
    try:
        parse(SRC + "\nresident {\n}\n")
        assert False
    except ParseError as e:
        assert "1 つだけ" in str(e)


def test_メニューから起こせる出来事は_外から入力を受けないものだけ():
    assert any("がありません" in m for m in errors(mutate("fire PostSaved", "fire Nothing")))
    bad = mutate("each saved_tweets → post_to_x", "external → write_tweet")
    assert any("external" in m for m in errors(bad))


def test_区切り線は_先頭や末尾や連続に置けない_clickはtoggleかmenuだけ():
    assert any("先頭や末尾" in m for m in errors(mutate('  menu "つぶやきを書く" → TweetList\n', '  separator\n  menu "つぶやきを書く" → TweetList\n')))
    assert any("続いて" in m for m in errors(mutate("  separator\n", "  separator\n  separator\n")))
    try:
        parse(mutate("click toggle", "click right"))
        assert False
    except ParseError:
        pass
