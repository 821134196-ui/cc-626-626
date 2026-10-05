#!/usr/bin/env bash
# 一条命令启动新能源车位占用举证系统:
#   1) 安装(如需)并构建 Svelte 前端到 frontend/dist
#   2) 编译并启动 Axum 服务,由其同时托管 API 与前端静态资源
set -euo pipefail
cd "$(dirname "$0")"

# 兼容 cargo 由 rustup 安装但未进全局 PATH 的环境
if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1090
  . "$HOME/.cargo/env"
fi

PORT="${PORT:-8080}"
DATA_DIR="${DATA_DIR:-./data}"

echo "==> [1/2] 构建前端(Svelte + Vite)"
cd frontend
if [ ! -d node_modules ]; then
  npm install
fi
npm run build
cd ..

echo "==> [2/2] 启动后端(Axum, http://localhost:${PORT})"
echo "    数据目录: ${DATA_DIR}(SQLite + 本地照片)"
export PORT DATA_DIR
exec cargo run
