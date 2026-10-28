# geas を扱う AI へ

最初に [docs/ai-guide.md](docs/ai-guide.md) を読んでください。入れ方（Python 3.10 以上、外部ライブラリなし）、コマンド、作業の流れ、守ること、ソースの地図があります。

- 言語の書き方：[docs/language-reference.md](docs/language-reference.md)
- 動く全体の例：[tests/fixtures/overseas_shop/](tests/fixtures/overseas_shop/)
- 計画書（会話で作る仕様書）の作り方：[skills/plan-interview/SKILL.md](skills/plan-interview/SKILL.md)
- 検査：`python -m geas check ファイル.geas`（書き方の説明と食い違ったら、検査器が正本）
- テスト：`python -m pytest tests`
- 返答・コメント・説明文は日本語で書く。
