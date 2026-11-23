# AGENTS.md — RustAegis 多 Agent 协作常令

> 唯一常令来源，每个 session 开工先读。规则只写「永远成立的约束 + 链接」，细节不放这里。
> `CLAUDE.md` 只是 `@AGENTS.md` 的指针；其他 harness 同样只加薄适配，不复述本文件。

## 权威阶梯（冲突时谁赢）

当前代码 > 本文件 > `README.md` / `DEVELOPMENT.md` > `docs/adr/accepted/` > `docs/adr/draft/`。
文档与代码冲突时不许默默择一：先判「实现错 / 文档过期 / 设计已改」，再改对应那一处，并在结论里点名改的是哪一类。

## 不变量（红线）

- **proc-macro 与运行时是一对**：`aegis_vm_macro`（crates.io 外部 crate）在编译期加密字节码并做 opcode 置换，本仓运行时负责解密执行；改 `build.rs` 产出的 `build_config.rs` 契约（`BUILD_SEED` / `OPCODE_ENCODE` / `OPCODE_DECODE` / 熵池 / SMC 域哈希）等于同时改两端握手，必须同轮升级 macro 版本并写决议。
- **whitebox 是编译期硬约束**：`src/lib.rs` 的 `compile_error!` 要求 `whitebox` 或 `whitebox_lite`；`--no-default-features` 只能搭配 `whitebox_lite`（WASM 构建即此组合）。删掉这条检查等于关掉加密字节码。
- **`.anticheat_build_seed` 不入库**：构建时生成，决定每个构建的 opcode 置换与密钥。可复现构建用 `ANTICHEAT_BUILD_KEY`，时间戳用 `SOURCE_DATE_EPOCH`；换机器重建请先 `cargo clean`。
- **no_std 路径不得引入 std**：`no_std` 下只用 `alloc` / `core` 与既有 `spin`；新依赖必须 `default-features = false`，并在决议里写明理由。
- **性能口径是设计取舍**：VM 比原生慢 10–100 倍属预期，不是缺陷；`#[vm_protect]` 只用于敏感函数。不许为「提速」改 handler 语义；性能结论必须标「实测 / 推断」。

## 质量门禁

- **单命令自查 = CI**：`./scripts/check.sh`（clippy `--all-targets -D warnings` → `cargo test` → 文档门禁）。纯文档改动跑 `./scripts/check.sh --docs-only`；门禁自证跑 `python3 scripts/verify-gates.py`。
- **clippy 必须带 `--all-targets`**：只查 lib/bin 会漏掉 `tests/` 与 examples 的告警；本仓 CI 曾经如此，已修正，见 [docs/adr/accepted/2026-11-23-gate-baseline.md](docs/adr/accepted/2026-11-23-gate-baseline.md)。
- **`cargo fmt --check` 不进主门禁**：基线有 723 处差异，全仓重排会淹没功能 diff，且本仓未钉 `rust-toolchain.toml`。只格式化自己改过的文件。
- **先写红测试**：新 opcode / handler 行为先写 `tests/*.rs` 用例并确认因「功能缺失」而红，再实现；`cargo test --quiet` 要逐 target 看 `test result:`，别只看最后一块。

## 文档与决议

- 有真实取舍（选 A 弃 B/C）且理由不会自然出现在代码里 → `docs/adr/draft|accepted|rejected/` 单篇决议，格式由 `python3 scripts/validate_adr.py docs/adr` 强制，**不建 INDEX**。
- 正文只写当前状态：演进历史归 git 与决议，事故叙事单独成篇，不混进 README / DEVELOPMENT。

## 子代理协作

- 派活时把 [docs/subagents.md](docs/subagents.md) 的五段输出契约（SUMMARY / EVIDENCE / CHANGES / RISKS / BLOCKERS）原样写进 brief；只读角色用同文件的封闭命令语法。
- 子代理自报不是质量门：合并前父代理亲自看 diff、跑门禁、读回改动；`path:LINE` 引用不算「已编辑」声明。
- 并行写者只许不相交的文件范围；`git status` 里的陌生改动先查 mtime 与进程列表，不删、不改、不据为己有。

## 拒收清单（做得再好也不收）

- 没有宏/运行时对接方案的新 opcode；没有真实消费者的新 feature flag。
- 为格式化或重排产生的大 diff；把 `.anticheat_build_seed`、`target/`、`.cargo/` 入库。
- 未经验证的「性能优化」；把推断写成实测的结论；以「清理」为名删掉 `compile_error!` 等安全护栏。
- 没有测试的行为改动；把没跑过的检查报成通过。