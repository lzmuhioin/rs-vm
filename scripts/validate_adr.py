#!/usr/bin/env python3
"""决策记录格式门禁：python3 validate_adr.py <adr_root> [--legacy-max N]

两种命名制都支持（本 skill 规定「顺序号 / 日期+标题」二选一、选定不换）：
    docs/adr/accepted/2026-01-02-topic-slug.md     # 日期制
    docs/adr/accepted/r128-topic-slug.md           # 顺序号制（含 r023b 这类插入号）

检查：目录结构、文件名、标题与标题内编号、Status 行与目录一致性、按状态的必需/禁止章节、
备选方案非空、顺序号唯一、`--legacy-max` 以上禁用「未记录」占位（棘轮）。
stdlib-only，可移植。退出码 0=通过，1=有错误。
"""
from __future__ import annotations

import re
import sys
from datetime import date
from pathlib import Path

DIRS = {"draft", "accepted", "rejected"}
DATE_RE = re.compile(r"^(\d{4})-(\d{2})-(\d{2})-(.+)\.md$")
SEQ_RE = re.compile(r"^(r|ar)(\d{3})(b?)-(.+)\.md$")
TITLE_RE = re.compile(r"^# (决策记录|Decision Record): (.+?)\s*$")
TITLE_SEQ_RE = re.compile(r"^(R|AR)(\d{1,3})(b?)\s+\S")
PLACEHOLDER = "<!-- 备选方案未记录 -->"

# 章节别名 -> 归一化键（中文标题优先，英文别名兼容）
ALIASES = {
    "问题": "problem", "problem": "problem",
    "提案": "proposal", "proposal": "proposal",
    "决策": "decision", "decision": "decision",
    "备选方案": "alternatives", "alternatives considered": "alternatives",
    "alternatives": "alternatives",
    "后果": "consequences", "consequences": "consequences",
    "验收标准": "acceptance", "acceptance criteria": "acceptance",
    "风险": "risks", "risks": "risks",
    "计划": "plan", "plan": "plan",
    "迁移计划": "plan", "migration plan": "plan",
}
REQUIRED = {
    "draft": ["problem", "proposal", "alternatives", "acceptance"],
    "accepted": ["problem", "decision", "alternatives", "consequences"],
    "rejected": ["problem", "proposal", "alternatives"],
}
# accepted 禁用 spec 口吻章节
ACCEPTED_FORBIDDEN = ["proposal", "plan", "acceptance"]
HEAD_RE = re.compile(r"^##\s+(.+?)\s*$")


def check_note(path: Path, status: str, legacy_max: int | None) -> tuple[list[str], str | None]:
    """Return (errors, sequence-key). sequence-key is '<nnn><suffix>' for numbering schemes."""
    errs: list[str] = []
    date_named, seq_named = DATE_RE.match(path.name), SEQ_RE.match(path.name)
    seq_key = (seq_named.group(1) + seq_named.group(2) + seq_named.group(3)) if seq_named else None
    if not (date_named or seq_named):
        errs.append(f"{path}: 文件名须为 yyyy-mm-dd-topic.md 或 r<NNN>[b]-topic.md（二选一，不混用）")
    elif date_named:
        try:
            date.fromisoformat("-".join(date_named.groups()[:3]))
        except ValueError:
            errs.append(f"{path}: 文件名日期非法")

    lines = path.read_text(encoding="utf-8").splitlines()
    title = TITLE_RE.match(lines[0]) if lines else None
    if not title:
        errs.append(f"{path}: 首行须为 '# 决策记录: <标题>' 或 '# Decision Record: <标题>'")
    elif seq_named:
        m = TITLE_SEQ_RE.match(title.group(2))
        want = seq_named.group(1).upper() + str(int(seq_named.group(2))) + seq_named.group(3)
        if not m:
            errs.append(f"{path}: 顺序号制的标题须写成 '# 决策记录: R<n>/AR<n> <标题>'")
        elif m.group(1) + str(int(m.group(2))) + m.group(3) != want:
            errs.append(f"{path}: 标题里的编号 {m.group(1)}{m.group(2)}{m.group(3)} 与文件名 "
                        f"{seq_named.group(1)}{seq_named.group(2)}{seq_named.group(3)} 不一致")

    # Status 行（前 6 行内）
    sm = next((re.match(r"^Status:\s*(\S+)(?:\s*—\s*(.+))?$", ln)
               for ln in lines[:6] if ln.startswith("Status:")), None)
    if sm is None:
        errs.append(f"{path}: 前 6 行内缺少 Status: 行")
    else:
        got, reason = sm.group(1), sm.group(2)
        if got != status:
            errs.append(f"{path}: Status '{got}' 与目录 '{status}' 不一致")
        if status == "rejected" and not reason:
            errs.append(f"{path}: rejected 的 Status 须带一行理由")

    # 章节收集
    sections: dict[str, int] = {}
    order: list[int] = []
    for i, ln in enumerate(lines):
        hm = HEAD_RE.match(ln)
        if hm:
            key = ALIASES.get(hm.group(1).strip().lower())
            if key and key not in sections:
                sections[key] = i
            order.append(i)

    for key in REQUIRED[status]:
        if key not in sections:
            errs.append(f"{path}: 缺少必需章节 '{key}'（接受中英文别名）")

    if status == "accepted":
        for key in ACCEPTED_FORBIDDEN:
            if key in sections:
                errs.append(f"{path}: accepted 禁止 spec 口吻章节 '{key}'，须改写为现在时")

    if "alternatives" in sections:
        h = sections["alternatives"]
        end = next((o for o in order if o > h), len(lines))
        body = "".join(lines[h + 1:end]).strip()
        if body == PLACEHOLDER:
            if legacy_max is not None and seq_key is not None and int(re.sub(r"^[a-z]+", "", seq_key).rstrip("b")) > legacy_max:
                errs.append(f"{path}: 新决议（{seq_key.upper()}）不得用「未记录」占位，须写真实备选与落选理由")
        elif len(body) < 20:
            errs.append(f"{path}: 备选方案章节为空（须写每个落选方案为何落选）")

    if status == "draft" and "risks" not in sections:
        errs.append(f"{path}: draft 必需 '## 风险'（会出什么错 + 明知放弃了什么）")
    return errs, seq_key


def main(root: Path, legacy_max: int | None) -> int:
    errs: list[str] = []
    if not root.is_dir():
        print(f"目录不存在: {root}")
        return 1
    for p in sorted(root.iterdir()):
        if p.name.startswith("."):
            continue
        if p.name == "INDEX.md":
            errs.append(f"{p}: 禁止索引文件，目录树本身就是清单")
        elif p.is_file():
            errs.append(f"{p}: 根下不允许散落文件（须在 draft/accepted/rejected 内）")
        elif p.name not in DIRS:
            errs.append(f"{p}: 未知顶层目录（仅允许 {sorted(DIRS)}）")
    seen: dict[str, Path] = {}
    notes = 0
    for d in sorted(root.iterdir()):
        if not (d.is_dir() and d.name in DIRS):
            continue
        for f in sorted(d.iterdir()):
            if f.name.startswith("."):
                continue
            if f.suffix != ".md":
                errs.append(f"{f}: 非 .md 文件")
                continue
            notes += 1
            note_errs, seq_key = check_note(f, d.name, legacy_max)
            errs.extend(note_errs)
            if seq_key:
                if seq_key in seen:
                    errs.append(f"{f}: 顺序号 {seq_key} 重复（另一篇 {seen[seq_key]}）")
                else:
                    seen[seq_key] = f
    if errs:
        print("\n".join(errs))
        print(f"\n{len(errs)} 处错误")
        return 1
    print(f"通过: {notes} 篇决策记录")
    return 0


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    legacy = next((int(a.split("=", 1)[1]) for a in sys.argv[1:] if a.startswith("--legacy-max=")), None)
    sys.exit(main(Path(args[0]) if args else Path("docs/adr"), legacy))
