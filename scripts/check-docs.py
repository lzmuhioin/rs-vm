#!/usr/bin/env python3
"""文档门禁：AGENTS 预算/条目行数、CLAUDE 指针、相对链接可达、子代理契约五段。

用法: python3 scripts/check-docs.py [--root DIR]
退出码 0=通过，1=有错误。stdlib-only，可在任意 checkout 上跑。
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

# 预算：首版正文实测 2361 字符（2026-11-23），留约 14% 余量。加节前先量余量。
AGENTS_BUDGET = 2700
CLAUDE_BUDGET = 400
MAX_BULLET_LINES = 3
CONTRACT_HEADINGS = ["### SUMMARY", "### EVIDENCE", "### CHANGES", "### RISKS", "### BLOCKERS"]
LINK_RE = re.compile(r"\[[^\]]*\]\(([^)\s]+)\)")


def check_agents(root: Path, errs: list[str]) -> int:
    p = root / "AGENTS.md"
    if not p.is_file():
        errs.append("AGENTS.md: 常令文件不存在")
        return 0
    text = p.read_text(encoding="utf-8")
    if len(text) > AGENTS_BUDGET:
        errs.append(f"AGENTS.md: 超出预算 {len(text)}/{AGENTS_BUDGET} 字符（先搬层再压，别直接提预算）")
    block: list[str] = []
    blocks: list[list[str]] = []
    for ln in text.splitlines():
        if ln.startswith("- "):
            if block:
                blocks.append(block)
            block = [ln]
        elif block and ln.strip():
            block.append(ln)
        elif block:
            blocks.append(block)
            block = []
    if block:
        blocks.append(block)
    for b in blocks:
        if len(b) > MAX_BULLET_LINES:
            errs.append(f"AGENTS.md: 常令条目前 {MAX_BULLET_LINES} 行内放不下: {b[0][:32]}…")
    return len(blocks)


def check_claude(root: Path, errs: list[str]) -> None:
    p = root / "CLAUDE.md"
    if not p.is_file():
        errs.append("CLAUDE.md: 入口文件不存在")
        return
    text = p.read_text(encoding="utf-8")
    if not any(ln.strip() == "@AGENTS.md" for ln in text.splitlines()):
        errs.append("CLAUDE.md: 缺少一行原样的 '@AGENTS.md' 导入")
    if any(ln.startswith("##") for ln in text.splitlines()):
        errs.append("CLAUDE.md: 出现二级标题，说明开始复述规则（只许指针）")
    if len(text) > CLAUDE_BUDGET:
        errs.append(f"CLAUDE.md: 超出指针预算 {len(text)}/{CLAUDE_BUDGET} 字符")


def markdown_files(root: Path) -> list[Path]:
    files = [root / n for n in ("AGENTS.md", "CLAUDE.md", "DEVELOPMENT.md")]
    docs = root / "docs"
    if docs.is_dir():
        files.extend(sorted(docs.rglob("*.md")))
    return [f for f in files if f.is_file()]


def check_links(files: list[Path], errs: list[str]) -> int:
    total = 0
    for f in files:
        in_fence = False
        for i, ln in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
            if ln.lstrip().startswith(("```", "~~~")):
                in_fence = not in_fence
                continue
            if in_fence:
                continue
            for target in LINK_RE.findall(ln):
                if target.startswith(("#", "http://", "https://", "mailto:")):
                    continue
                total += 1
                rel = target.split("#", 1)[0]
                if not rel:
                    continue
                if not (f.parent / rel).exists():
                    errs.append(f"{f.relative_to(f.parent.parent)}:{i}: 死链 {target}")
    return total


def check_contract(root: Path, errs: list[str]) -> None:
    p = root / "docs/subagents.md"
    if not p.is_file():
        errs.append("docs/subagents.md: 子代理契约文件不存在")
        return
    lines = [ln.strip() for ln in p.read_text(encoding="utf-8").splitlines()]
    pos = []
    for h in CONTRACT_HEADINGS:
        hit = next((i for i, ln in enumerate(lines) if ln.startswith(h)), None)
        if hit is None:
            errs.append(f"docs/subagents.md: 输出契约缺少 '{h}'")
            continue
        pos.append(hit)
    if pos != sorted(pos):
        errs.append("docs/subagents.md: 输出契约五段顺序被打乱")


def check_adr_dirs(root: Path, errs: list[str]) -> None:
    adr = root / "docs/adr"
    if not adr.is_dir():
        errs.append("docs/adr: 决议目录不存在")
    elif not any(adr.glob("*/*.md")):
        errs.append("docs/adr: 没有任何决议记录")


def main(root: Path) -> int:
    errs: list[str] = []
    bullets = check_agents(root, errs)
    check_claude(root, errs)
    check_contract(root, errs)
    check_adr_dirs(root, errs)
    files = markdown_files(root)
    links = check_links(files, errs)
    if errs:
        print("\n".join(errs))
        print(f"\n{len(errs)} 处错误")
        return 1
    print(f"通过: {len(files)} 份文档 / {links} 条相对链接 / AGENTS {bullets} 条常令")
    return 0


if __name__ == "__main__":
    args = sys.argv[1:]
    root = Path(args[args.index("--root") + 1]) if "--root" in args else Path(".")
    sys.exit(main(root))