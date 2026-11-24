# 决策记录: 超级指令 `CMP_REG_REG_JGE`（宏与运行时同轮升级）

Status: draft — 已在 `vendor/aegis_vm_macro` 落地 tracer bullet，待实测收益后转 accepted。

## 问题

两轮解释器开销优化（去掉每次 `execute()` 重建的 256 项 `NativeRegistry`、去掉 `VmState`
里热路径用不到的 4 KiB 堆/空闲链表/调用栈预分配）之后，实测（`taskset -c 6`、release、
best-of-5）得到两条结论：

- 每次调用固定成本 **441 → 47 ns**，已接近下限；
- 每指令成本 **dispatch 2.30 ns + handler 3.19–3.31 ns**（NOP / 算术两类实测），
  已经贴近这套设计的物理下界，**handler 侧没有剩余油水**。

于是倍率全部落在「指令条数」上：宏为 `for` 循环每轮发射 14–18 条 VM 指令
（条件 `PUSH_REG;PUSH_REG;CMP;DROP;DROP;JGE` 占 6 条，增量 `PUSH_REG;INC;POP_REG;JMP`
占 4 条，其余是循环体），而原生只要 1–2 条。`sum(2000)` 稳定在 ~100x、
`fib(20)` ~530x、小算术函数 ~100–160x，都受此支配。

只改运行时无法再降：`CMP` 的 pop/pop/push/push 是 ISA 契约的一部分（文档写明
"CMP doesn't consume"），改成原地读栈顶虽然实测无收益（89 384 ns vs 88 852 ns，噪声内），
也属于「为提速改 handler 语义」，按红线退回。

## 提案

1. **把 `aegis_vm_macro` 收进仓库**（`vendor/aegis_vm_macro`，0.2.51 → 0.2.52），
   根 `Cargo.toml` 改为 path 依赖。理由：新 opcode 必须宏（发射端）与运行时（解码/执行端）
   同轮升级，否则两端 opcode 置换表不一致，所有受保护函数全解错。
2. **新增一条融合指令 `CMP_REG_REG_JGE <a:u8> <b:u8> <offset:i16>`（base `0x9B`）**，
   等价于 `PUSH_REG a; PUSH_REG b; CMP; DROP; DROP; JGE offset`：同样的标志位、栈中性、
   `a >= b`（有符号）时相对跳转。运行时侧：`src/opcodes.rs` 常量、`handlers/control.rs`
   handler、`handlers/dispatch.rs` 包装与 `HANDLER_TABLE[0x9B]`。
3. **opcode 表只允许「追加」**：`build.rs` 的 `BASE_OPCODES` 与宏的 `BASE_OPCODES`
   都按列表顺序分配置换槽位，把新条目放在**最后**可保证既有 opcode 的置换值一字不变 ——
   旧字节码仍然有效，升级是向后兼容的。这条必须写死在两处注释里，防止后人插到中间。
4. 宏侧只在「循环条件两端都是寄存器、比较符为 `<`（编译成 `JGE` 跳出）」时发射融合指令，
   其余情形保持原序列；`#[vm_protect]` 的保护级别语义不变。

## 备选方案

- **只做宏侧 peephole（常量折叠 / 删除冗余栈搬运），不新增 opcode**：被否。改动最小，
  但省不掉循环条件那 6 条指令，预计收益 <10%，无法回答「指令条数是不是杠杆」这个问题。
- **改 `CMP` 语义（原地读栈顶，不再回压）**：被否。违反已文档化的 ISA 契约
  （宏固定发射 `CMP;DROP;DROP`），且实测无收益。
- **JIT / AOT 本机编译**：暂缓。这是唯一能把倍率压到个位数的路线，但等于换架构
  （需重做字节码→IR→机器码、平台适配、保护模型），不是本轮「优化」的范围。
- **quickening（加载时把置换 opcode 解码成基础 opcode 缓存）**：被否。省一次查表
  （~1 ns/指令），却把明文基础 opcode 留在内存里，等于用保护强度换 5% 性能。

## 风险

- **两端置换表不一致 = 全量失效**：`build.rs` 与 `vendor/aegis_vm_macro/src/crypto.rs`
  各有一份 `BASE_OPCODES`，顺序必须逐项相同。已用「只在末尾追加 + 两处注释写死」约束，
  但未来任何人插到中间都会让既有字节码整体解错，且报错形态是 `InvalidOpcode` 而非编译错。
- **宏版本被 vendor 后不再跟随上游**：上游修 bug / 加保护时不会自动进来，
  需要人工合并；这是换来「能改发射端」的代价，明知放弃自动更新。
- **收益若不足就白改**：融合只覆盖「两端都是寄存器」的循环条件，覆盖不到
  `i < n` 里立即数一类的条件；若实测收益 <10%，本决议转 rejected 并回退 `vendor/`。
- **保护强度不变但不增强**：新 opcode 同样经过置换与加密，未引入明文路径；
  但多一条指令也就多一个可被静态识别的模式，属可接受的信息泄露。

## 验收标准

- `tests/fused_opcodes.rs` 四个用例：`>`、`=`、`<` 三条分支语义 + 栈中性；
  实现前应因「未知 opcode（`InvalidOpcode`）」而红，实现后转绿（已按此顺序验证）。
- 全量 `cargo test` 与 `cargo clippy --all-targets -D warnings` 零告警；
  `./scripts/check.sh --docs-only` 通过。
- 旧字节码兼容性：升级前后 `BASE_OPCODES` 前 96 项的置换值必须完全一致
  （追加式修改的直接推论，用一次构建的 `OPCODE_ENCODE` diff 佐证）。
- 实测收益：`sum(2000)` / `fib(20)` 的倍率下降幅度，必须标「实测」并给出
  绑核、release、best-of-5 的数字；若收益 <10%，本条决议转 rejected 并回退 `vendor/`。