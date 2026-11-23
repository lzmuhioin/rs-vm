#!/usr/bin/env python3
"""门禁自证：在临时副本里注入缺陷，逐条要求门禁变红；干净副本必须是绿。

用法: python3 scripts/verify-gates.py
退出码 0=全部符合预期，1=有门禁没拦住（或副本本身就红）。stdlib-only。
"""
from __future__ import annotations

import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
COPY = ["AGENTS.md", "CLAUDE.md", "DEVELOPMENT.md", "docs", "scripts/validate_adr.py", "scripts/check-docs.py"]
MIN_CASES = 8  # 注入用例下限，只许上调


def make_tree() -> Path:
    missing = [n for n in COPY if not (ROOT / n).exists()]
    if missing:
        raise FileNotFoundError(f"缺件，未做验红: {', '.join(missing)}")
    dst = Path(tempfile.mkdtemp(prefix="aegis-gate-"))
    for name in COPY:
        src = ROOT / name
        (dst / name).parent.mkdir(parents=True, exist_ok=True)
        if src.is_dir():
            shutil.copytree(src, dst / name)
        else:
            shutil.copy2(src, dst / name)
    return dst


def run(tree: Path, script: str, *args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, str(tree / "scripts" / script), *args],
        cwd=tree,
        capture_output=True,
        text=True,
    )


def append(tree: Path, name: str, text: str) -> None:
    p = tree / name
    p.write_text(p.read_text(encoding="utf-8") + text, encoding="utf-8")


def first_adr(tree: Path) -> Path:
    return sorted((tree / "docs/adr/accepted").glob("*.md"))[0]


def main() -> int:
    cases = [
        ("干净副本", None, "check-docs.py", (), 0, ""),
        ("AGENTS 超预算", lambda t: append(t, "AGENTS.md", "x" * 600 + "\n"), "check-docs.py", (), 1, "超出预算"),
        ("AGENTS 条目超 3 行", lambda t: append(t, "AGENTS.md", "\n- **探针**：第 1 行\n  第 2 行\n  第 3 行\n  第 4 行\n"), "check-docs.py", (), 1, "3 行内放不下"),
        ("CLAUDE 缺导入行", lambda t: (t / "CLAUDE.md").write_text((t / "CLAUDE.md").read_text(encoding="utf-8").replace("@AGENTS.md", "AGENTS.md"), encoding="utf-8"), "check-docs.py", (), 1, "缺少一行原样的"),
        ("CLAUDE 复述规则", lambda t: append(t, "CLAUDE.md", "\n## 规则\n"), "check-docs.py", (), 1, "二级标题"),
        ("文档死链", lambda t: append(t, "AGENTS.md", "\n[缺失](docs/nope.md)\n"), "check-docs.py", (), 1, "死链"),
        ("子代理契约缺段", lambda t: (t / "docs/subagents.md").write_text("\n".join(ln for ln in (t / "docs/subagents.md").read_text(encoding="utf-8").splitlines() if not ln.strip().startswith("### BLOCKERS")) + "\n", encoding="utf-8"), "check-docs.py", (), 1, "BLOCKERS"),
        ("ADR Status 错位", lambda t: first_adr(t).write_text(first_adr(t).read_text(encoding="utf-8").replace("Status: accepted", "Status: draft", 1), encoding="utf-8"), "validate_adr.py", ("docs/adr",), 1, "不一致"),
        ("ADR 散落文件", lambda t: (t / "docs/adr/notes.md").write_text("# 笔记\n", encoding="utf-8"), "validate_adr.py", ("docs/adr",), 1, "散落"),
    ]
    if len(cases) - 1 < MIN_CASES:
        print(f"用例数 {len(cases) - 1} 低于下限 {MIN_CASES}")
        return 1

    failures = 0
    for name, mutate, script, args, want_rc, expect in cases:
        try:
            tree = make_tree()
        except FileNotFoundError as exc:
            print(f"BLOCKED: {exc}")
            return 2
        try:
            if mutate is not None:
                mutate(tree)
            res = run(tree, script, *args)
            out = res.stdout + res.stderr
            rc_ok = (res.returncode == want_rc) if want_rc == 0 else (res.returncode != 0)
            msg_ok = expect in out
            if rc_ok and msg_ok:
                print(f"PASS {name}")
            else:
                failures += 1
                print(f"FAIL {name}: rc={res.returncode} 期望 {'0' if want_rc == 0 else '非0'}"
                      f"，输出{'含' if msg_ok else '不含'} '{expect}'\n{out.strip()[:400]}")
        finally:
            shutil.rmtree(tree, ignore_errors=True)

    total = len(cases) - 1
    if failures:
        print(f"\n{failures}/{total} 条注入未被拦住")
        return 1
    print(f"\n通过: 干净副本为绿，{total} 条注入全部变红（下限 {MIN_CASES}）")
    return 0


if __name__ == "__main__":
    sys.exit(main())