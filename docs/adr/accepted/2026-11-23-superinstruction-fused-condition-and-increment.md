# 决策记录: 超级指令 `CMP_REG_REG_JGE` / `INC_REG`（宏与运行时同轮升级）

Status: accepted — `aegis_vm_macro` 0.2.52 已 vendor 进仓并新增两条融合指令，
实测循环密集场景降 **30%**、`fib(20)` 降 **29%**；旧字节码保持兼容。

## 问题

两轮解释器开销优化（去掉每次 `execute()` 重建的 256 项 `NativeRegistry`、去掉 `VmState`
里热路径用不到的 4 KiB 堆/空闲链表/调用栈预分配）之后，实测（`taskset -c 6`、release、
best-of-5）得到两条结论：

- 每次调用固定成本 **441 → 47 ns**，已接近下限；
- 每指令成本 **dispatch 2.30 ns + handler 3.19–3.31 ns**（NOP / 算术两类实测），
  已经贴近这套设计的物理下界，**handler 侧没有剩余油水**。

于是倍率全部落在「指令条数」上：宏为 `for` 循环每轮发射 14–18 条 VM 指令
（条件 `PUSH_REG;PUSH_REG;CMP;DROP;DROP;JGE` 占 6 条，增量 `PUSH_REG;INC;POP_REG` 占 3 条），
而原生只要 1–2 条。`sum(2000)` 稳定在 ~100x、`fib(20)` ~530x、小算术函数 ~100–160x。

只改运行时无法再降：`CMP` 的 pop/pop/push/push 是 ISA 契约的一部分（文档写明
"CMP doesn't consume"），改成原地读栈顶实测无收益（89 384 ns vs 88 852 ns，噪声内），
且属于「为提速改 handler 语义」，按红线退回。

## 决策

1. **把 `aegis_vm_macro` 收进仓库**（`vendor/aegis_vm_macro`，0.2.51 → 0.2.52），
   根 `Cargo.toml` 改为 path 依赖。理由：新 opcode 必须宏（发射端）与运行时（解码/执行端）
   同轮升级，否则两端 opcode 置换表不一致，所有受保护函数全解错。
2. **新增两条融合指令**（base 值 `0x9B` / `0x9C`）：
   - `CMP_REG_REG_JGE <a:u8> <b:u8> <offset:i16>`：等价于
     `PUSH_REG a; PUSH_REG b; CMP; DROP; DROP; JGE offset`（同标志位、栈中性、有符号 `a >= b`），
     循环条件省 5 条 dispatch；
   - `INC_REG <r:u8>`：等价于 `PUSH_REG r; INC; POP_REG r`（就地自增 + 只置 ZF），
     循环增量省 2 条 dispatch。
3. **第二轮：4 条寄存器算术融合指令**（base `0x9D`–`0xA0`，同一份「只能末尾追加」约束）：
   - `ADD_REG_IMM <dst:u8> <imm:u32>` ≡ `PUSH_REG dst; PUSH_IMM32 imm; ADD; POP_REG dst`
   - `ADD_REG_REG <dst:u8> <a:u8> <b:u8>` ≡ `PUSH_REG a; PUSH_REG b; ADD; POP_REG dst`
   - `MUL_REG_IMM` / `MUL_REG_REG`：同上，算子换成 `MUL`
   宏侧在赋值处走「寄存器目的地」编译：`can_expr_to_reg` 先做纯判断（只支持 int 的
   `+`/`*`，叶为变量或 `u32` 内的整数字面量，深度 ≤8），可编译则 `compile_expr_to_reg`
   直接发射寄存器指令，否则回退原栈序列 —— 保证不产生半截字节码。
4. **第三轮：通用比较-跳转 `CMP_REG_REG_JCC <a> <b> <cond> <off:i16>`**（base `0xA1`）：
   一条指令替代 `while`/`if` 条件原有的 `PUSH;PUSH;比较;DUP;ZERO;XOR;DROP;DROP;JZ`（约 10 条）。
   `cond` 选择子：0=eq 1=ne 2=gt 3=lt 4=ge 5=le（有符号），宏侧在「条件两边都是寄存器」时发射。
   **已知覆盖限制**：函数参数编译成 `InputOffset`（每次使用走 `NATIVE_READ`），不是寄存器，
   所以 `while i < n`（`n` 是参数）用不上这条融合；只有本地变量作边界时才生效。
5. **参数序言提升**：函数参数原本编译成 `InputOffset`，每次使用发一条 `NATIVE_READ`
   （一次 native 调用）。因为 Rust 函数参数不可变，改为在序言里一次性 `NATIVE_READ + POP_REG`
   读进寄存器，之后所有使用点走 `PUSH_REG`。只提升 8 字节标量（整数 / bool）；结构体、切片、
   引用、浮点参数保持 `InputOffset` 原路径。寄存器区余量不足（`next_local_reg >= 240`）时放弃提升。
   收益有两处叠加：循环里每轮少一次 native 调用；条件两端都成了寄存器，第三轮的
   `CMP_REG_REG_JCC` 才真正落地。
6. **opcode 表只允许「追加」**：`build.rs` 与 `vendor/aegis_vm_macro/src/crypto.rs` 各有一份
   `BASE_OPCODES`，置换槽位按列表顺序分配，新条目放在**最后**可保证既有 opcode 的置换值一字不变 ——
   旧字节码仍然有效，升级向后兼容。两处注释都写死了这条约束。
5. 宏侧只在可证明等价的位置发射：`for` 循环（非闭区间）的条件与增量；其余情形保持原序列。
   `#[vm_protect]` 的保护级别语义不变。

## 备选方案

- **只做宏侧 peephole（常量折叠 / 删除冗余栈搬运），不新增 opcode**：被否。改动最小，
  但省不掉循环条件那 6 条指令，无法回答「指令条数是不是杠杆」这个问题。
- **改 `CMP` 语义（原地读栈顶，不再回压）**：被否。违反已文档化的 ISA 契约
  （宏固定发射 `CMP;DROP;DROP`），且实测无收益。
- **JIT / AOT 本机编译**：暂缓。这是唯一能把倍率压到个位数的路线，但等于换架构
  （重做字节码→IR→机器码、平台适配、保护模型），不是本轮「优化」的范围。
- **quickening（加载时把置换 opcode 解码成基础 opcode 缓存）**：被否。省一次查表
  （~1 ns/指令），却把明文基础 opcode 留在内存里，等于用保护强度换 5% 性能。

## 后果

- 实测（同一台机、同一 build seed、两版二进制交替跑、`taskset -c 6`、release、各 3–5 轮取最小）：
  - `sum(2000)`：**192.7–197.2 µs → 134.8–135.5 µs（−30%）**
  - `fib(20)`：**2610–2791 ns → 1845–1976 ns（−29%）**
- 每轮循环少 7 条 dispatch（条件 6→1、增量 3→1），与实测降幅吻合，说明「指令条数」确实是主杠杆；
  剩余倍率（sum ~64x、fib ~390x）仍由循环体自身的指令条数决定，要再降只能动循环体
  或换寄存器式 ISA。
- 旧字节码兼容：追加式修改的直接推论，前 96 项置换值不变；`tests/fused_opcodes.rs` 6 个用例
  （`>`/`=`/`<` 分支、栈中性、`INC_REG` 就地自增、ZF 语义）与 `encrypted_execution` 全绿。
- 第二轮 A/B 实测（同一探针二进制、同 build seed、交替跑 5 轮、`taskset -c 6`）：
  - `sum(2000)`（`while` + `s = s*9 + i*i`）：**305–351x → 201–209x（−33%）**
  - 迭代 `fib(20)`：**222–238x → 180–184x（−20%）**
  - 小算术函数：278x → 261x（−6%，其指令本来就少）
  绝对倍率随机器负载可摆动 2–4 倍，故只引用同一轮交替跑的相对降幅。
- 第三轮 A/B 实测（同一探针、同 seed、交替跑 3 轮）：
  - **边界是本地变量的循环（`let m = n; while i < m`）：192 000x → 97 000x（−49%，时间减半）**
  - 边界是函数参数的循环（`while i < n`）与递归 `fib`：无变化 —— 条件里含 `NATIVE_READ`，
    融合指令的「两边都是寄存器」前提不成立。
  这正是下一块要动的：把函数参数在序言里一次性搬进寄存器，既省掉循环里的 `NATIVE_READ`，
  又让这条融合指令覆盖到参数作边界的循环。
- 第四轮 A/B 实测（同一探针、同 seed、交替跑 3 轮）：
  - 参数作边界的循环 `sum(2000)`：**220.7–226.3x → 116.5–117.8x（−48%）**
  - 参数作边界的迭代 `fib(20)`：**737–757x → 386–478x（约 −40%，噪声较大）**
  - 无循环的小算术函数：282x → 283x（无变化，符合预期）
  - 本地边界循环 `sum_local`：98 000x → 91 000x（小幅，条件已在第三轮融合）
  结构断言（宏 crate 单测，机器无关）：`while i < n` 的字节码从 **52 字节压到 ≤45 字节**，
  关掉提升即回退到 52 → 该测试是真正的红/绿判据。
- 维护成本：宏不再跟随上游，需要人工合并；`handler_mutation` 特性尚未覆盖融合指令
  （仍经置换+加密，但跳过变异）。

## 风险

- **两端置换表不一致 = 全量失效**：`build.rs` 与 `vendor/aegis_vm_macro/src/crypto.rs`
  各有一份 `BASE_OPCODES`，顺序必须逐项相同；插到中间会让既有字节码整体解错，
  报错形态是 `InvalidOpcode` 而非编译错。已在两处写死「只能追加」。
- **宏被 vendor 后不跟随上游**：上游修 bug / 加保护不会自动进来，明知放弃自动更新。
- **保护强度局部下降**：融合指令跳过了 `IncSubstitution`（指令替换）与 `handler_mutation`，
  相当于这两条指令少了一层变形；opcode 置换、字节码加密、白盒派生仍然覆盖。
  若要补回，需要在宏侧为融合指令再写一套替换变体。
- **覆盖范围有限**：只覆盖「条件两端都是寄存器 + 非闭区间」的 `for`；`while`、
  `..=` 与立即数条件仍走原序列，收益会随代码形态波动。