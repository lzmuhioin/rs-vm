# 决策记录: 门禁基线（clippy all-targets / fmt 排除 / CI 同源）

Status: accepted — 以 `./scripts/check.sh` 作为本地与 CI 的唯一自查入口，fmt 不进主门禁。

## 问题

2026-11-23 在干净工作树（HEAD `3f43dcd`）实测：

- `cargo clippy --all-targets -- -D warnings` → 退出码 101，共 17 处告警（lib 2 / example 1 / tests 14）：`manual_checked_ops` 1、`manual_range_contains` 2、`get_first` 3、`assertions_on_constants` 3、`clone_on_copy` 1、`unusual_byte_groupings` 1、`identity_op` 4、`same_item_push` 1、`len_zero` 1（rustc 1.99.1）。其中 **15 处只有 `--all-targets` 才看得见**。
- CI 的 clippy job 跑的是 `cargo clippy -- -D warnings`，不带 `--all-targets` ⇒ 测试与 example 里的告警永远漏检。上一轮 CI 全绿并不证明「零告警」。
- `cargo fmt --check` → 723 处差异（11851 行输出）。仓库未钉 `rust-toolchain.toml`，不同 rustfmt 版本输出还会变。

## 决策

- 修掉那 17 处告警（等价改写：`checked_div().unwrap_or(0)`、`(30..=100).contains(..)`、`args.first()`、`const { assert!(…) }`、`!is_empty()`、`extend([..; N])`、等宽十六进制分组、去掉 `0 ^`/`0 |` 恒等项），让基线真的绿。
- 主门禁固定为：`cargo clippy --all-targets -- -D warnings` → `cargo test --quiet` → 文档门禁（ADR 格式 + 预算/死链/入口指针）。入口是 `scripts/check.sh`，`--docs-only` 供纯文档改动与 CI 文档 job 使用。
- CI 的 clippy job 补 `--all-targets`；新增 `docs` job 跑 `./scripts/check.sh --docs-only`，与本地同一条命令。
- `cargo fmt --check` 不进主门禁：只格式化自己动过的文件；将来若钉死 `rust-toolchain.toml`，再单独评估。
- 门禁本身要有验红证据：`scripts/verify-gates.py` 在临时副本里注入 8 类缺陷（预算超限、死链、入口指针缺失、入口出现二级标题、ADR Status 错位、ADR 散落文件、子代理契约缺段、ADR 备选为空），逐条要求变红，并断言干净副本为绿。

## 备选方案

- **把 fmt 一起进门禁**：被否。需要先提交一次 700+ 处的全仓重排，功能 PR 的 diff 会被格式化噪音淹没；且没有 toolchain 钉版时，本机绿不代表 CI 绿（不同 rustfmt 版本换行策略不同）。
- **在 `Cargo.toml` / 源码里 `-A clippy::manual_checked_ops` 抑制**：被否。抑制等于让门禁对同类新代码永久失明，而正确修法只有两行；用户的要求是「零告警」，不是「零报错」。
- **维持 CI 只查 lib/bin**：被否。这正是缺口本身；测试里的断言逻辑出错时，编译期告警是最便宜的一道拦截。
- **改用 `cargo nextest` 等外部测试运行器**：被否。当前 `cargo test` 在本机全绿（33 个集成测试文件），换运行器要先引入新依赖与 CI 缓存策略，收益只在并行度，不在正确性。

## 后果

- 基线现在可复现：任何人在干净 checkout 上跑 `./scripts/check.sh` 得到与 CI 相同的判定。
- 代价一：升级 rustc 后可能出现新的默认 lint，提交前必须先修——这是「零告警」的固有成本。
- 代价二：`fmt` 漂移仍在，只能靠「只格式化触碰文件」的人工纪律，门禁不兜底。
- 已知限制：`Cargo.lock` 未入库（库 crate 惯例），依赖版本漂移只在 CI 首次构建暴露；`rust-toolchain.toml` 仍未钉——本机已升到 1.100.0 与 runner 对齐，但下次 runner 升级会重演「本机绿、CI 红」，详见下面的验证一节。
- 回归指针：`scripts/check.sh`（主门禁）、`scripts/validate_adr.py`（决议格式）、`scripts/verify-gates.py`（门禁自证，含用例数下限 8）。

## 验证

- 2026-11-23 把本机 clippy 从 1.99.1 升到 1.100.0（与 CI runner 同版本）后，CI 的红在本机复现：`needless_range_loop` 在 `build.rs` 11 处、`src/whitebox/{generator,tables}.rs` 19 处，另有 1 条 `unused dependency 'hex'`。这批全部按 clippy 建议改成迭代器，**未留任何 `#[allow]`**；`hex` 依赖已删（全仓无 `hex::` 引用）。
- 等价性用一次性探针证（跑完即删）：`MixingBijection32::apply/apply_inverse` 在 500 组随机矩阵 × 1024 个输入上新旧写法逐值相同；8-bit / 4-bit 编码表的填充顺序在 200 个种子下逐项相同。
- 由此得到一条运行事实：`ANTICHEAT_BUILD_KEY` 只固定 build seed，**熵池与增量仍取 `/dev/urandom`**，所以 `build_config.rs` 不能逐字节复现——比较两份构建产物时不能拿它当判据（实测同一 env 下两次构建，文件第一处差异在第 7 行的熵池）。