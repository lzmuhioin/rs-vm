#!/usr/bin/env bash
# 作用：RustAegis 单命令自查（clippy 全目标、测试、文档门禁），只读不改文件
set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-}" in
  --docs-only) docs_only=1 ;;
  "") docs_only=0 ;;
  *) echo "用法: $0 [--docs-only]"; exit 2 ;;
esac

if [ "$docs_only" -eq 0 ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "SKIP: cargo 不在 PATH（只跑文档门禁）"
    docs_only=1
  fi
fi

if [ "$docs_only" -eq 0 ]; then
  echo "== cargo clippy --all-targets =="
  cargo clippy --all-targets -- -D warnings
  echo "== cargo test =="
  cargo test --quiet
fi

echo "== 文档门禁 =="
python3 scripts/validate_adr.py docs/adr
python3 scripts/check-docs.py

if [ "$docs_only" -eq 1 ]; then
  echo "OK: 文档门禁通过（构建检查未跑）"
else
  echo "OK: 全部检查通过"
fi