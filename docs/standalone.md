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
- 外へ出る通信は、`effect` に `over http { … }` を宣言した外部作用だけが、宣言した先に Rust 側から出します（リダイレクトには従いません。書き方は [language-reference.md](language-reference.md) の「外とのやりとり」）。宣言がないアプリには、通信の部品（ureq）が依存にも実行ファイルにも入りません。通信しうる先は、仕様書とレビュー画面の「外へ出る通信」の表と、「動かす前提」の到達性の行に出ます。見本は `tests/fixtures/solo_tweet`（X への投稿。試験は手元の模擬の X）。

## 常駐

`resident { … }` を宣言すると、タスクトレイ（macOS はメニューバー）に居続けます。窓を閉じても終了せず隠し、アイコンのクリックか、右クリックのメニューの「開く」で出します。右クリックのメニューには、画面を開く項目、出来事を起こす項目、区切り線を宣言できます。書き方は [language-reference.md](language-reference.md) の「常駐（resident）」。

- 確かめたこと：Linux（xvfb）で、トレイのアイコンを作れること、窓が開いて画面が描かれること、同じ保存先の 2 つ目の起動を断ること。
- **確かめていないこと：** 実際のトレイに出たアイコンのクリック・メニュー、窓を閉じて隠れる動き（仮想ディスプレイにはトレイが無い）、Windows・macOS の動作。macOS で Dock のアイコンを隠す設定（アクセサリのアクティベーションポリシー）は、まだ入れていません。
- 組むには、Linux では libayatana-appindicator3-dev が要ります（実行時にも、トレイを表示する環境が要ります）。
- ログイン時の自動起動と、OS の通知は、まだありません。

## 保存先

`.geas` の `data_folder`（OS ごとのパスの書き方。[language-reference.md](language-reference.md) の「データ用フォルダ」）で、DB・ログ・シークレットを置くフォルダを宣言できます。

（Windows のログ：`log.txt` が同じフォルダに出ます。`--console` で窓に出せます。）

`--db ファイル` で決めます。省くと、利用者のデータ用フォルダの下のアプリ名のフォルダ（Windows: `%APPDATA%`、macOS: `~/Library/Application Support`、それ以外: `$XDG_DATA_HOME` か `~/.local/share`）に `app.sqlite` を置きます。`--seed 最初のデータ.json` も使えます（サーバーと同じ）。`--secrets ファイル` か、データ用フォルダの `secrets.json`（`{"外部.名前": "値"}`）か、環境変数 `GEAS_SECRET_外部_名前`（大文字）で、外部作用の認証に使う値を渡します。`--selftest` は、窓を開いて画面が描かれるのを確かめて終わる試験用の起動です。

## 作り

- `server/` は lib（`geas_server`：経路の処理 `http::handle`、保存、処理を動かす層）と、HTTP の待ち受け（`main.rs`）に分かれています。窓版は lib だけを使い、HTTP の待ち受けは使いません。
- `http::handle` は要求（`Req`）から答え（`Resp`）を返す関数で、運び方を知りません。サーバーは tiny_http、窓版は IPC から同じものを呼びます。
- 窓版の `--test` の口（時計・模擬の操作）は開けません。

## 前提と未確認

- Windows 用の exe は、Linux から `--target x86_64-pc-windows-gnu`（mingw-w64）で組めます。手順と確認のしかたは下の「Windows の確認用」。
- 組むには、Linux では libwebkit2gtk-4.1-dev と GTK の開発ファイルが要ります。
- Windows の exe は、この開発環境では組んでいません（WebView2 ランタイムが要ります。Windows 11 は標準、10 は別に入れる必要があります）。`wry` と `tao` は Windows でも組める部品ですが、実際の確認は未了です。

## Windows の確認用

Linux から Windows 用の exe を組めます（実機で動かして確かめるためのもの。Windows 上の動作は、この開発環境では確かめていません）。

```
rustup target add x86_64-pc-windows-gnu          # Debian/Ubuntu は gcc-mingw-w64-x86-64 も要る
geas rust アプリ.geas -o 出力先 --host --runtime --standalone
cd 出力先/window
CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc cargo build --release --target x86_64-pc-windows-gnu
# できる物：target/x86_64-pc-windows-gnu/release/geas_window.exe
# mingw で組むと WebView2Loader.dll が別ファイルで要る。exe と同じフォルダに置く：
#   target/x86_64-pc-windows-gnu/release/build/webview2-com-sys-*/out/x64/WebView2Loader.dll
```

Windows 用の exe は、ターミナルの窓を出さずに起動します（窓だけのアプリ）。ログ（標準エラー）は、データ用フォルダの `log.txt` に追記します。ターミナルの窓にログを出したいときは `--console` を付けます（ターミナルから起動していればそこへ、そうでなければ新しい窓を開きます。`--selftest` は `--console` を含みます）。

`WebView2Loader.dll` が exe の隣になければ、起動時に「WebView2Loader.dll が見つからない」と出ます（Windows 実機で確認済み）。MSVC で組めば dll は exe に静的に入ります。署名のない exe なので、「不明な発行元」の警告が出ます。

Windows 側では、WebView2 ランタイム（Windows 11 は標準）が要ります。確認の順は、`geas_window.exe --selftest`（窓が開いて画面が描かれたら終了コード 0）、通常の起動、（常駐を宣言したアプリは）トレイのアイコンのクリック・右クリックのメニュー・窓を閉じて隠れる動き。保存先は `%APPDATA%\アプリ名\app.sqlite`。
