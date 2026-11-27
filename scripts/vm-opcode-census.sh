#!/usr/bin/env bash
# 主指令构成普查：统计仓库内所有 #[vm_protect] 函数实际发射的 base opcode 分布。
#
# 用途：每次改 ISA 后跑一遍，看「融合指令 / 栈搬运 / 控制流」各占多少，
# 防止保护缺口（融合指令不接指令替换）无声扩大，也防止把推断当实测。
#
# 原理：宏在展开时把每条 emit_op 的 base opcode 打到 stderr（受 AEGIS_DUMP 控制），
# 本脚本强制重新展开后聚合。注意 AEGIS_DUMP 是**编译期**读取的，
# 所以必须先 touch 宏源码否则 cargo 会复用旧展开、输出为空。
#
# 用法：./scripts/vm-opcode-census.sh [--top N]
set -euo pipefail
cd "$(dirname "$0")/.."

TOP=20
if [[ "${1:-}" == "--top" && -n "${2:-}" ]]; then TOP="$2"; fi

RAW="$(mktemp)"
trap 'rm -f "$RAW"' EXIT

# 注意两点（都踩过）：
#   - 不要设 ANTICHEAT_BUILD_KEY：固定 key 会与工作树已有 seed 错位，构建直接报 E13；
#   - 不要设 SOURCE_DATE_EPOCH=1700000000：早于 2024-01-01，tests/build_config.rs 的
#     BUILD_TIMESTAMP 断言会失败。
# cargo test --no-run 即使因无关用例编译失败也照样可用：宏展开在失败前已完成，
# 指令流已经在 stderr 里，所以这里不因退出码中断（后面按条数判断是否采到）。
AEGIS_DUMP=1 bash -c 'touch vendor/aegis_vm_macro/src/compiler/emit.rs && cargo test --no-run' \
  >/dev/null 2>"$RAW" || true

TABLE="vendor/aegis_vm_macro/src/opcodes.rs"
TOTAL=$(grep -cE 'OP [0-9A-F]{2}' "$RAW" || echo 0)
if [[ "$TOTAL" -eq 0 ]]; then
  echo "没有采到任何指令：AEGIS_DUMP 未生效（cargo 复用了旧展开？）" >&2
  exit 1
fi

echo "去重后语料：仓库内全部 #[vm_protect] 函数，共 $TOTAL 条 base 指令"
echo
printf '%-6s %-18s %7s %7s  %s\n' 编号 指令 条数 占比 类别
printf '%s\n' '-----------------------------------------------------------'

grep -oE 'OP [0-9A-F]{2}' "$RAW" | awk '{print $2}' | sort | uniq -c | sort -rn | head -n "$TOP" |
while read -r n hex; do
  name=$(grep -hE "u8 = 0x${hex};" "$TABLE" | head -1 | sed 's/.*pub const //; s/: u8 = .*//' || true)
  [[ -z "$name" ]] && name='?'
  # 寄存器形态（阶段一新增的融合/寄存器指令）名字里都带 _REG_ 或以 _JCC 结尾
  case "$name" in
    *_REG_*|*_JCC) kind='融合（寄存器形态）' ;;
    PUSH_REG|POP_REG|MOV_REG) kind='栈/寄存器搬运' ;;
    *) kind='栈式/其它' ;;
  esac
  pct=$(awk -v a="$n" -v b="$TOTAL" 'BEGIN{printf "%.1f%%", a*100/b}')
  printf '0x%-4s %-18s %7s %7s  %s\n' "$hex" "$name" "$n" "$pct" "$kind"
done
