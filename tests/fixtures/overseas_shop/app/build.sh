#!/bin/sh
# Geas の仕様（overseas_shop.geas）から Rust・画面・拡張・サーバーの実行部を作り、サーバーをビルドする。
# 使い方: ./build.sh   →   cd generated/server && ./target/release/geas_server
set -e
cd "$(dirname "$0")/../../../.."
python -m geas rust tests/fixtures/overseas_shop/overseas_shop.geas -o tests/fixtures/overseas_shop/app/generated --host --runtime --seed tests/fixtures/overseas_shop/app/seed.json
cd tests/fixtures/overseas_shop/app/generated/server
cargo build --release
