#!/usr/bin/env bash
# 新能源车位占用举证系统 —— 一条命令启动
set -euo pipefail
cd "$(dirname "$0")"

export PATH="$HOME/.cargo/bin:$PATH"

echo "==> 构建前端 (Svelte + Vite)"
(cd frontend && npm install --no-fund --no-audit && npm run build)

echo "==> 启动后端 (Rust + Axum + SQLite)，首次运行需编译依赖"
echo "    访问 http://localhost:3000"
exec cargo run
