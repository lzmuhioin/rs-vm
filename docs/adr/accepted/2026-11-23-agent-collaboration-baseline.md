# 决策记录: 多 Agent 协作文档与门禁体系

Status: accepted — 以单命令自查与逐篇决议承载多 agent 协作，替代无记录的口头约定。

## 问题

本仓此前没有任何 agent 指令文件（无 `AGENTS.md` / `CLAUDE.md` / `CONTRIBUTING.md`），协作信息散在 README 与代码注释里：

- 33 个集成测试文件、740 个 `#[test]`，但没有「一条命令跑全部检查」的入口；本地只跑自己改过的那一个测试文件就宣称绿。
- CI 的 clippy job 只查 lib/bin，`tests/` 与 examples 的告警漏检；这一缺口在 2026-11-23 修复前一直存在。
- 多个 agent 并行改同一仓库时，没有裁决顺序（文档说 A、代码是 B 时听谁的）、没有改动记录、没有子代理产出格式，导致同一类误解被重复解释。
- 构建契约（`build.rs` → `build_config.rs` → 外部 `aegis_vm_macro`）只在 README 里零散提及，agent 很容易单方面改一端。

## 决策

分四层落地，每类事实只有一个家：

1. `AGENTS.md`：常令层。每条规则 1–3 行 + 详情链接；含权威阶梯、不变量、门禁、拒收清单。
2. `CLAUDE.md`：`@AGENTS.md` 一行导入 + 一句 harness 专属纪律，禁止复述规则（门禁断言「无二级标题」）。
3. `docs/adr/{draft,accepted,rejected}/`：有真实取舍才写单篇决议；格式由 `scripts/validate_adr.py` 强制；不建 `INDEX.md`。
4. `docs/subagents.md`：子代理角色矩阵、五段输出契约、只读封闭语法、并发写者纪律、父代理 posture。
5. `DEVELOPMENT.md`：代码地图（构建流水线、模块表、特性矩阵、测试布局、常用任务）。
6. `scripts/check.sh`：单命令自查（clippy `--all-targets` → `cargo test` → 文档门禁），CI 与本地跑同一条；`scripts/verify-gates.py` 用注入缺陷证明门禁会红。

## 备选方案

- **单文件决议台账**（多仓用过的旧形态）：所有决策追加进一个长文件。被否：并行 agent 争抢同一文件的同一段，冲突无法自动合并；索引行复制路径元数据，会让不相关改动互相争抢；检索成本随文件增长线性上升。逐篇文件让「路径即状态」，移动文件就是状态迁移。
- **只写 `CONTRIBUTING.md`**：面向人类贡献流程（分支、issue、PR），不承载「不许做什么」的 agent 常令与子代理输出契约。被否：两者的读者与加载时机不同——常令每 session 必读，贡献流程按需查。
- **用 GitHub Issues / Wiki 承载决策**：被否：外部参考永远低于仓内代码，离线不可用，门禁无法校验（issue 不是文件，`validate_adr.py` 读不到），且 PR 评审时看不到上下文。
- **只靠 README 与代码注释**：被否：README 是产品文档，写「当前状态」；决策理由（放弃了什么、为什么）不属于产品文档，塞进去会让 README 变成时间线。

## 后果

- 收益：冲突有裁决顺序；决议格式与文档链接可机器校验；子代理产出能并集而不是各自表述；新增 agent 上手只需读两份文件（`AGENTS.md` + `DEVELOPMENT.md`）。
- 代价：新增 6 个文件与 3 个脚本，每次改动多一步 `./scripts/check.sh`；`AGENTS.md` / `CLAUDE.md` 属受保护文件，改动要人工批准，批准前文档门禁会因缺件而红（这是有意的：缺件红比静默弱化好）。
- 已知限制：文档正文为中文（读者是用户与其 agent），若要向上游 `onurkun/RustAegis` 提 PR，决议与常令需先翻英文，这一点写在 `AGENTS.md` 的文档规则里。
- 证据层级：本决策的现状描述来自 2026-11-23 在本机仓库的实测（`git status` 干净、`ls` 无 agent 文件、`cargo clippy --all-targets` 退出码 101、`cargo fmt --check` 723 处差异），不是推断。