# 决策记录: SMC 执行状态改为 O(1) 搬运，不再按指令克隆

Status: accepted — `SmcExecState` 与临时 `VmState` 之间用 `mem::swap` 双向搬运状态，删除 `apply_to`/`copy_from`。

## 问题

`execute_smc_with_natives`（`src/smc.rs`）要在执行期间原地解密/重加密字节码，而 `VmState<'a>` 以 `&'a [u8]` 借用代码，两者冲突。因此每条指令都新建一个临时 `VmState`，把执行状态搬进去再搬出来。

原实现用 `apply_to`/`copy_from`（内部是 `Vec::clone_from`）完成这次搬运，代价与**堆大小成正比、与指令数成正比**：

- 单次 `execute_smc`（512 KiB 堆 + 200 条指令）实测让全局分配器记账 **742,749,344 字节（708 MiB）**；
- 相对普通 `execute()` 的耗时倍数随堆增长：0 B → 21.7×，64 KiB → 100.7×，512 KiB → **174.2×**。

这不是「慢一点」：SMC 的使用场景恰恰是「堆里放着大结构体，然后反复执行受保护逻辑」，成本随堆线性放大等于让 SMC 无法用于真实负载。

## 决策

把 `copy_from` / `apply_to` 合并成 `SmcExecState::swap_with(&mut VmState)`：对 5 个 `Vec` 字段（`regs`/`heap`/`free_list`/`stack`/`call_stack`/`output`）和 10 个标量字段各做一次 `core::mem::swap`。交换只动三个指针字或一个标量，O(1) 且不分配。

语义等价性：交换是双向的，临时 `state` 执行完再交换回去，`exec_state` 仍然持有原对象（已被本次指令修改）。错误路径上原本也不调用 `copy_from`，行为不变 —— 状态留在临时 `state` 里随它一起 drop。

## 备选方案

- **保持克隆**：被否。这是问题本身；708 MiB 单次分配足够让目标进程 OOM 或触发反作弊自身的时间检测。
- **把 `VmState::code` 从 `&'a [u8]` 改成裸指针**，这样能复用同一个 `VmState`、把每指令开销压到零：被否。会在运行时的核心结构里引入 `unsafe` 与别名可变借用，SMC 的收益换不来这个代价；本仓目前 `src/` 下 `unsafe` 只出现在 executor 的两处自证安全的代码里。
- **执行前把整份字节码解密到 `Vec<u8>`**，然后当普通字节码跑：被否。SMC 的全部意义就是任何时刻内存里只出现当前一条明文指令，这和设计目标直接冲突。
- **复用同一个 `VmState` + 每指令换 `code` 引用**：被否，语言层面不允许 —— `&'a [u8]` 借用期间无法 `&mut code`，这正是当初要建临时 `VmState` 的原因。

## 后果

- 收益（实测）：单次执行分配 708 MiB → **1.54 MiB**（`tests/smc_alloc_budget.rs`，512 KiB 堆 / 200 条指令）；耗时倍数 174.2× → **1.5×**，且**不再随堆大小恶化**（0 B → 9.3×，64 KiB → 3.8×，512 KiB → 1.5×）。
- 残留成本：每指令仍要新建一个临时 `VmState`（`VmState::new` 预分配 256 个寄存器 + 64 格值栈，约 2.5 KiB）。这是当前 1.54 MiB 的主要来源，也是 heap=0 时仍有 9.3× 的原因。
- 已知可选后续（本轮不做）：给 `VmState` 加一个「延迟分配」构造，专供 `swap_with` 之前使用。能把这 2.5 KiB 也省掉，但会引入一个「必须先 swap 才能用」的隐式不变式，而收益只在固定开销上；等 SMC 真成为热路径时再评估。
- 回归护栏：`tests/smc_alloc_budget.rs` 断言单次执行分配 < 4 MiB（克隆路径的下界约 100 MB，两个数量级余量），并同时断言结果与普通执行一致；`tests/smc_tests.rs` 19 个既有语义用例全绿。
