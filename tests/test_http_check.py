"""外部作用の実通信（over http）の検査：通信先は https の文字列だけ、認証は needs secret の宣言と結ぶ、返事の状態は結果の種類に漏れなく結ぶ。"""
from pathlib import Path

from geas.check import check
from geas.parser import parse

SRC = (Path(__file__).parent / "fixtures" / "solo_tweet" / "solo_tweet.geas").read_text(encoding="utf-8")


def errors(src):
    return [d.msg for d in check(parse(src)).diags if d.level == "エラー"]


def mutate(old, new):
    assert old in SRC, old
    return SRC.replace(old, new, 1)


def test_見本は通る():
    assert errors(SRC) == []


def test_通信先は_https_のホスト名だけ():
    for url in ("http://api.x.com/2/tweets", "https://127.0.0.1/x", "https://user@api.x.com/x", "https://api.x.com/x?a=1", "ftp://api.x.com/x"):
        assert any("通信先" in m for m in errors(mutate("https://api.x.com/2/tweets", url))), url


def test_認証は_needs_secret_で宣言したものだけ_同じ外部のものだけ():
    assert any("宣言していません" in m for m in errors(mutate("bearer Twitter.token", "bearer Twitter.other")))
    assert any("同じ外部" in m for m in errors(mutate("bearer Twitter.token", "bearer Other.token")))


def test_返事の状態は漏れなく結ぶ():
    assert any("status other" in m for m in errors(mutate("  status other → Rejected\n", "")))  # 書いていない状態の行き先が無い
    assert any("重なって" in m for m in errors(mutate("status 400..499 → Rejected", "status 200..499 → Rejected")))
    assert any("結果の種類「Nope」" in m for m in errors(mutate("status 400..499 → Rejected", "status 400..499 → Nope")))
    assert any("take" in m and "post_id" in m for m in errors(mutate(' take post_id from "data.id"', "")))  # gives の中身を取り出さない
    assert any("道すじ" in m for m in errors(mutate('"data.id"', '"data..id"')))


def test_本文は外部作用の引数だけ_JSONにできる型だけ():
    assert any("body" in m and "引数にありません" in m for m in errors(mutate('body "text" from content', 'body "text" from nothing')))
    assert any("重なって" in m for m in errors(mutate('body "text" from content', 'body "text" from content, "text" from tweet_id')))


def test_返事を結ぶ応答が要る():
    assert any("replies" in m for m in errors(mutate(" replies PostSettled over http", " over http")))
