#!/usr/bin/env bash
# 作用：RustAegis 单命令自查（clippy 全目标、测试、文档门禁），只读不改文件
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== toolchain =="
rustc --version 2>/dev/null || echo "rustc: 不可用"
cargo --version 2>/dev/null || echo "cargo: 不可用"
cargo clippy --version 2>/dev/null || echo "clippy: 不可用"

case "${1:-}" in
  --docs-only) docs_only=1; fast=0 ;;
  --fast) docs_only=0; fast=1 ;;
  "") docs_only=0; fast=0 ;;
  *) echo "用法: $0 [--fast|--docs-only]"; exit 2 ;;
esac

if [ "$docs_only" -eq 0 ]; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "SKIP: cargo 不在 PATH（只跑文档门禁）"
    docs_only=1
  fi
fi

# 快速回路：只跑「改了代码生成就必须看」的那几组，给迭代用。
# 提交前仍须跑全量（不带参数），它不是 CI 的替代品。
if [ "$fast" -eq 1 ]; then
  echo "== fast: 编译器单测（regalloc 规格 + 编译期红灯）=="
  (cd vendor/aegis_vm_macro && cargo test --quiet)
  echo "== fast: 表达式语义对拍 + 加密执行/大常量 =="
  cargo test --quiet --test expr_semantics_parity --test value_cryptor
  echo "OK: 快速回路通过（已跳过 clippy、全量用例、文档门禁）"
  exit 0
fi

if [ "$docs_only" -eq 0 ]; then
  echo "== cargo clippy --all-targets =="
  cargo clippy --all-targets -- -D warnings
  echo "== cargo test =="
  cargo test --quiet
  echo "== cargo test（vendor/aegis_vm_macro 编译器单测）=="
  (cd vendor/aegis_vm_macro && cargo test --quiet)
fi

echo "== 文档门禁 =="
python3 scripts/validate_adr.py docs/adr
python3 scripts/check-docs.py

if [ "$docs_only" -eq 1 ]; then
  echo "OK: 文档门禁通过（构建检查未跑）"
else
  echo "OK: 全部检查通过"
fi