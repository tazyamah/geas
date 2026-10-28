#!/bin/sh
# Geas の仕様（overseas_shop.geas）から Rust・画面・拡張・サーバーの実行部を作り、サーバーをビルドする。
# 使い方: ./build.sh   →   ./generated/server/target/release/geas_server --pwa generated/ui --dev generated/dev --seed seed.json
set -e
cd "$(dirname "$0")/../../../.."
python -m geas rust tests/fixtures/overseas_shop/overseas_shop.geas -o tests/fixtures/overseas_shop/app/generated --host --runtime --app tests/fixtures/overseas_shop/app/app.rs --reader tests/fixtures/overseas_shop/app/extension/reader.js
cd tests/fixtures/overseas_shop/app/generated/server
cargo build --release
