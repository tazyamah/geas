#!/bin/sh
# Geas の仕様（overseas_shop.geas）から Rust を作り、サーバーをビルドする。
# 使い方: ./build.sh   →   cd server && cargo run --release
set -e
cd "$(dirname "$0")/../../../.."
python -m geas rust tests/fixtures/overseas_shop/overseas_shop.geas -o tests/fixtures/overseas_shop/app/server/generated --host --reader tests/fixtures/overseas_shop/app/extension/reader.js
cd tests/fixtures/overseas_shop/app/server
cargo build --release
