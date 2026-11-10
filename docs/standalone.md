# 窓つき exe（スタンドアロンアプリ）

`geas rust アプリ.geas -o 出力先 --host --runtime --standalone` で、出力先の `window/` に、画面と処理を 1 つにまとめた実行ファイル `geas_window` の crate が出ます。

```
cd 出力先/window && cargo build --release
./target/release/geas_window
```

OS の WebView（Windows は WebView2、macOS は WKWebView、Linux は WebKitGTK）の窓で画面を開きます。サーバーも、ブラウザも、ポートも使いません。

## 通信

- **画面から処理への呼び出しは、HTTP を通りません。** 画面（`ui.js`）は、`window.ipc` があれば、問い合わせも処理も WebView の IPC（プロセスの中）で渡します。URL（`/orders` など）は、`resource` 宣言どおりの「機能の定義としての名前」で、通信路ではありません。ふつうのブラウザでは、同じ画面が `fetch` で動きます。
- 画面のファイルの配信だけは、WebView のカスタムプロトコル（`geas://`。Windows では WebView2 が `http://geas.localhost` と見せる）を使います。読み込むのは exe に埋め込んだ HTML・JS・CSS だけで、ネットワークには出ません。
- 窓の中の画面には、通信先を持たせません（CSP の `connect-src 'none'`、窓の移動も自分の画面だけ）。
- Linux では、窓とその補助プロセスが IP のソケット（待ち受けも接続も）を 1 つも持たないことを、`tests/test_window.py` で確かめています。
- 外部作用（`effect`）で外へ出る通信は、このあと足します（宣言した先だけに、Rust 側から出す形）。いまの窓版は、通信しない部分だけです。

## 保存先

`--db ファイル` で決めます。省くと、利用者のデータ用フォルダの下のアプリ名のフォルダ（Windows: `%APPDATA%`、macOS: `~/Library/Application Support`、それ以外: `$XDG_DATA_HOME` か `~/.local/share`）に `app.sqlite` を置きます。`--seed 最初のデータ.json` も使えます（サーバーと同じ）。`--selftest` は、窓を開いて画面が描かれるのを確かめて終わる試験用の起動です。

## 作り

- `server/` は lib（`geas_server`：経路の処理 `http::handle`、保存、処理を動かす層）と、HTTP の待ち受け（`main.rs`）に分かれています。窓版は lib だけを使い、HTTP の待ち受けは使いません。
- `http::handle` は要求（`Req`）から答え（`Resp`）を返す関数で、運び方を知りません。サーバーは tiny_http、窓版は IPC から同じものを呼びます。
- 窓版の `--test` の口（時計・模擬の操作）は開けません。

## 前提と未確認

- 組むには、Linux では libwebkit2gtk-4.1-dev と GTK の開発ファイルが要ります。
- Windows の exe は、この開発環境では組んでいません（WebView2 ランタイムが要ります。Windows 11 は標準、10 は別に入れる必要があります）。`wry` と `tao` は Windows でも組める部品ですが、実際の確認は未了です。
