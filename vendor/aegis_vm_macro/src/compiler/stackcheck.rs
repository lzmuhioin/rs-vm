//! 编译期栈平衡断言。
//!
//! 为什么需要它：`while` 循环体里每个 `if` 每转一圈多留一格栈，运行时表现成
//! 「转够圈数就 E03 StackOverflow」。这类错误读反汇编看不出来——**每条指令单看都合规**，
//! 错的是「一条路径整体的收支」。只有把每条发射路径的栈账累计起来，再去比
//! 「同一个标签的两条到达路径深度是否一致」，才能让它在编译期显形。
//!
//! ## 记账放在哪
//!
//! 主体放在 [`opcode_stack_effect`]（由 `emit_op` 调用）。为什么不逐个 `emit_*` 助手加：
//! 助手绝大多数只是「一条 `emit_op(op)` + 操作数」，逐个加最容易在新助手出现时漏掉，
//! 而 `emit_op` 是所有**逻辑指令**的唯一出口，既不会漏也不会重复计。
//! 真正绕开 `emit_op` 直写字节的只有替换/加密层（`substitution.rs` 的 Zero/Constant/
//! Inc/Dec/Not/And/Or/Add/Sub 替换、`mba.rs`、`value_cryptor.rs`），它们由 `emit.rs`
//! 里各自的调用点补记 `stack_effect(...)`——这些补记就是「直写段」与「逻辑指令」的分界。
//!
//! 注入层（NOP、死代码、不透明谓词假分支）不需要补记：它们的每个模式都是自配平的
//! （`PUSH x, DROP` / `PUSH x, PUSH y, DROP, DROP` / NOP），这一点在 `substitution.rs`
//! 里逐条核过；`AEGIS_NOJUNK=1` 关掉注入后栈深行为不变（实测），也印证了这一点。
//!
//! ## 开关
//!
//! `AEGIS_STKCHECK=1` 时把失衡报成编译错误（信息带标签名、字节码偏移、期望/实际深度）。
//! 编译期读取，改环境变量后必须 touch 源文件强制重展开。
//! 关掉时**仍然记账**（每条指令一次 i32 加法），只是不把结果当错误——这样开关不会改变
//! 生成的字节码，避免「打开诊断就换了一份产物」这种最难受的调试处境。

use super::Compiler;
use super::CompileError;
use crate::opcodes::{arithmetic, control, convert, exec, heap, memory, native, register, special, stack, string, vector};

impl Compiler {
    /// 记一条指令对栈的影响。`delta` 是「压 − 弹」：压栈为正，弹栈为负。
    pub(crate) fn stack_effect(&mut self, delta: i32) {
        self.stack_depth += delta;
    }

    /// 无条件跳转/停机之后，后面的线性代码不会被执行：栈账从这一刻起没有意义，
    /// 直到下一个标签落点重新对齐（否则 `if` 的两个分支会被算成同一条路径）。
    pub(crate) fn stack_end_path(&mut self) {
        self.stack_reachable = false;
    }

    /// 登记一次跳转：目标标签处栈上应该有几格。
    ///
    /// 前向跳转此时还没落点，先存期望；后向跳转（循环回边）标签已经落过点，
    /// 直接拿落点深度比。比出差异就是失衡——「循环每转一圈漏一格」正是在这里现形。
    pub(crate) fn stack_expect_at(&mut self, label: &str) {
        // 死代码里的跳转永远不会执行，登记了反而会把后面真正到达的路径带偏。
        if !self.stack_reachable {
            return;
        }
        let from = self.pos();
        match self.label_depth.get(label).copied() {
            Some(expected) if expected != self.stack_depth => {
                let landed = self
                    .label_at
                    .get(label)
                    .map(|at| at.to_string())
                    .unwrap_or_else(|| "?".to_string());
                self.stack_report_fault(format!(
                    "跳转到 `{label}`（字节码偏移 {from}）时栈深是 {}，而该标签落点（偏移 {landed}）要求 {expected}（差 {}）",
                    self.stack_depth,
                    self.stack_depth - expected
                ));
            }
            Some(_) => {}
            None => {
                self.label_depth.insert(label.to_string(), self.stack_depth);
            }
        }
    }

    /// 标签落点：和已登记的期望深度比对，并从落点起按该深度接着记账。
    pub(crate) fn stack_land_label(&mut self, name: &str) {
        let at = self.pos();
        match self.label_depth.get(name).copied() {
            Some(expected) => {
                if self.stack_reachable && self.stack_depth != expected {
                    self.stack_report_fault(format!(
                        "标签 `{name}`（字节码偏移 {at}）汇合点栈深不一致：应为 {expected}，实际 {}（差 {}）",
                        self.stack_depth,
                        self.stack_depth - expected
                    ));
                }
                // 不管线性前驱是否可达，落点往后一律按期望深度算：
                // 走到这里的是那条跳转，而不是上一段线性代码。
                self.stack_depth = expected;
                self.stack_reachable = true;
            }
            None if self.stack_reachable => {
                // 没有跳转指向它，但线性顺序能走到：深度就是当下的深度。
                self.label_depth.insert(name.to_string(), self.stack_depth);
                self.label_at.insert(name.to_string(), at);
            }
            None => {
                // 既没有跳转指向它、线性前驱又已经是死代码——这个标签本身不可达
                // （典型例子：`match` 最后一条臂之后那个「兜底」标签）。
                // 这里若拿死代码的深度去记账，会凭空给后面造出 +1 的假失衡，
                // 所以什么都不做，继续保持「不可达」。
            }
        }
    }

    /// 函数体编译完的调用约定：栈上正好留 1 个返回值。
    ///
    /// 以 `return` 收尾的函数最后一段线性代码不可达，此时栈账本身没有意义，跳过。
    pub(crate) fn stack_expect_return_value(&mut self) {
        if !self.stack_reachable {
            return;
        }
        if self.stack_depth != 1 {
            self.stack_report_fault(format!(
                "函数体编译完栈深是 {}，调用约定要求正好 1 个返回值（差 {}）",
                self.stack_depth,
                self.stack_depth - 1
            ));
        }
    }

    /// 只记第一处失衡：一处失衡会在后面引出一串连锁的「差 1」，刷屏反而看不出根因。
    pub(crate) fn stack_report_fault(&mut self, msg: String) {
        if self.stack_fault.is_none() {
            self.stack_fault = Some(msg);
        }
    }

    /// 收尾：开关打开时把记下的第一处失衡变成编译错误。
    pub(crate) fn stack_finish(&mut self, func_name: &str) -> Result<(), CompileError> {
        if !self.stack_check {
            return Ok(());
        }
        match self.stack_fault.take() {
            Some(msg) => Err(CompileError(format!(
                "[栈平衡断言] fn {func_name}: {msg}"
            ))),
            None => Ok(()),
        }
    }

    /// 表达式层的约定：`compile_expr` 一定「净 +1」（给调用方留一个值）。
    ///
    /// 为什么表达式这一层也要单独查：`if` 两个分支汇合时即使一起差 1，标签比对也看不出来；
    /// 而一旦某个表达式多留一格，它所在的循环就会逐轮涨栈。这里直接点名是哪个表达式坏了，
    /// 比事后从标签倒推省事得多。
    pub(crate) fn stack_check_expr(&mut self, expr: &syn::Expr, before: i32) {
        if self.stack_depth != before + 1 {
            let brief = brief_expr(expr);
            self.stack_report_fault(format!(
                "表达式 `{brief}` 偏离调用约定：应当净 +1，实际 {}（差 {} 格栈）",
                self.stack_depth - before,
                self.stack_depth - before - 1
            ));
        }
    }
}

/// 把表达式折成一行短文本放进报错里；太长就截断，避免一条报错刷满屏。
fn brief_expr(expr: &syn::Expr) -> String {
    use quote::ToTokens;
    let text = expr.to_token_stream().to_string();
    let limit = 60;
    if text.chars().count() > limit {
        let head: String = text.chars().take(limit).collect();
        format!("{head}…")
    } else {
        text
    }
}

/// 每条基础 opcode 的净栈影响：正数压、负数弹、0 不动。
///
/// 逐条对着 `src/handlers/*.rs` 的实现核过（不是按常识猜的）：
/// - 二元算术/位运算都是「弹 2 压 1」；`NOT`/`INC`/`DEC` 是「弹 1 压 1」。
/// - `CMP` 弹出来又压回去（`handle_cmp` 明确写了 push 回两个操作数），所以净 0；
///   `JZ/JNZ/JGT/JLT/JGE/JLE` 只看标志位，不碰栈。
/// - 融合指令 `CMP_REG_REG_JGE` / `CMP_REG_REG_JCC` 等价于
///   `PUSH_REG; PUSH_REG; CMP; DROP; DROP; Jxx`，净 0。
/// - 寄存器形态（`*_REG_*`，含 `NATIVE_READ_REG`）只写寄存器，净 0。
/// - `HALT` 把栈顶取成返回值（`handle_halt` 里 `pop`），净 −1。
///
/// `NATIVE_CALL` 的弹压数取决于参数个数，而参数个数在 opcode 之后才发射，
/// 这张表看不见——所以这里记 0，由 `emit_native_call` 自己补记 `1 - arg_count`。
pub(crate) fn opcode_stack_effect(op: u8) -> i32 {
    match op {
        // 栈操作
        stack::PUSH_IMM | stack::PUSH_IMM8 | stack::PUSH_IMM16 | stack::PUSH_IMM32
        | stack::PUSH_REG | stack::DUP => 1,
        stack::POP_REG | stack::DROP => -1,
        stack::SWAP => 0,

        // 寄存器操作
        register::MOV_IMM | register::MOV_REG | register::LOAD_MEM => 0,
        register::STORE_MEM => -2,

        // 算术/位运算
        arithmetic::ADD | arithmetic::SUB | arithmetic::MUL | arithmetic::DIV
        | arithmetic::MOD | arithmetic::IDIV | arithmetic::IMOD | arithmetic::XOR
        | arithmetic::AND | arithmetic::OR | arithmetic::SHL | arithmetic::SHR
        | arithmetic::ROL | arithmetic::ROR => -1,
        arithmetic::NOT | arithmetic::INC | arithmetic::DEC => 0,

        // 比较与跳转
        control::CMP => 0,
        control::JMP | control::JZ | control::JNZ | control::JGT | control::JLT
        | control::JGE | control::JLE | control::CALL | control::RET => 0,
        control::CMP_REG_REG_JGE | control::CMP_REG_REG_JCC => 0,
        control::INC_REG | control::ADD_REG_IMM | control::ADD_REG_REG
        | control::MUL_REG_IMM | control::MUL_REG_REG | control::XOR_REG_REG
        | control::AND_REG_REG | control::AND_REG_IMM | control::XOR_REG_IMM
        | control::SHR_REG_IMM | control::NATIVE_READ_REG => 0,

        // 类型转换：都是弹 1 压 1
        convert::SEXT8 | convert::SEXT16 | convert::SEXT32 | convert::TRUNC8
        | convert::TRUNC16 | convert::TRUNC32 => 0,

        // 定长内存
        memory::LOAD8 | memory::LOAD16 | memory::LOAD32 | memory::LOAD64 => 0,
        memory::STORE8 | memory::STORE16 | memory::STORE32 | memory::STORE64 => -2,

        // 堆
        heap::HEAP_ALLOC => 0,
        heap::HEAP_FREE => -1,
        heap::HEAP_LOAD8 | heap::HEAP_LOAD16 | heap::HEAP_LOAD32 | heap::HEAP_LOAD64 => 0,
        heap::HEAP_STORE8 | heap::HEAP_STORE16 | heap::HEAP_STORE32 | heap::HEAP_STORE64 => -2,
        heap::HEAP_SIZE => 1,

        // 向量：见 handlers/vector.rs 每个函数的「Stack:」注释
        vector::VEC_NEW => -1,
        vector::VEC_LEN | vector::VEC_CAP | vector::VEC_POP => 0,
        vector::VEC_PUSH => -2,
        vector::VEC_GET => -1,
        vector::VEC_SET => -3,
        vector::VEC_REPEAT => -2,
        vector::VEC_CLEAR => -1,
        vector::VEC_RESERVE => -2,

        // 字符串：见 handlers/string.rs
        string::STR_NEW | string::STR_LEN | string::STR_HASH => 0,
        string::STR_PUSH => -2,
        string::STR_GET => -1,
        string::STR_SET => -3,
        string::STR_CMP | string::STR_EQ | string::STR_CONCAT => -1,

        // 原生调用
        native::NATIVE_CALL => 0, // 由 emit_native_call 补记（依赖参数个数）
        native::NATIVE_WRITE => -1, // 弹一个值写进输出缓冲
        native::NATIVE_READ | native::INPUT_LEN => 1,

        // 特殊/执行控制
        special::NOP | special::NOP_N | special::HASH_CHECK | special::TIMING_CHECK => 0,
        special::OPAQUE_TRUE | special::OPAQUE_FALSE => 1,
        exec::HALT => -1,
        exec::HALT_ERR => 0,

        // 未知 opcode（含只在本宏内部当伪指令用的 CONST_ENC）：不猜，按 0 处理。
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use crate::compiler::{CompileError, Compiler};
    use crate::opcodes::control;
    use quote::quote;

    /// 打开断言编译一段源码，返回编译结果。
    fn compile_checked(src: proc_macro2::TokenStream) -> Result<Vec<u8>, CompileError> {
        let func: syn::ItemFn = syn::parse2(src).expect("测试源码应能解析成函数");
        crate::compiler::compile_function_checked(&func, false, false, true).map(|(code, _)| code)
    }

    /// 机制自检：循环回边的深度比落点深 1，必须记下一条带标签名与期望/实际深度的失衡。
    #[test]
    fn back_edge_with_extra_push_is_reported() {
        let mut c = Compiler::with_options(false, false);
        c.stack_check = true;
        c.mark_label("loop_head");
        c.emit_push_reg(0); // 故意多压一格
        c.emit_jump(control::JMP, "loop_head");

        let fault = c.stack_fault.clone().expect("回边深度 +1 必须被记下来");
        assert!(fault.contains("loop_head"), "报错要带标签名：{fault}");
        assert!(fault.contains('0'), "报错要带期望深度：{fault}");
        assert!(fault.contains('1'), "报错要带实际深度：{fault}");
    }

    /// 机制自检：收支平衡的回边不许误报。
    #[test]
    fn balanced_back_edge_is_silent() {
        let mut c = Compiler::with_options(false, false);
        c.stack_check = true;
        c.mark_label("loop_head");
        c.emit_push_reg(0);
        c.emit_drop();
        c.emit_jump(control::JMP, "loop_head");

        assert!(
            c.stack_fault.is_none(),
            "平衡的回边不该报错：{:?}",
            c.stack_fault
        );
    }

    /// 机制自检：函数体编译完栈上该正好剩 1 个返回值，剩 2 个要报。
    #[test]
    fn return_value_depth_must_be_one() {
        let mut c = Compiler::with_options(false, false);
        c.stack_check = true;
        c.emit_constant(7);
        c.emit_constant(8); // 多留一个
        c.stack_expect_return_value();
        assert!(
            c.stack_fault.is_some(),
            "结束时栈深 2 应报失衡（调用约定是 1 个返回值）"
        );
    }

    /// 真源码：`==` 只该留 1 个布尔值。曾经它把 XOR 的中间结果也留在栈上，
    /// 每个 `==` 多一格——循环里就逐轮涨栈。
    #[test]
    fn eq_leaves_exactly_one_value() {
        compile_checked(quote! {
            fn eq_if(a: u64, b: u64) -> u64 {
                if a == b { 1 } else { 0 }
            }
        })
        .expect("`if a == b` 编译后栈上应只剩 1 个返回值");
    }

    /// 真源码：`!=` 同理。
    #[test]
    fn ne_leaves_exactly_one_value() {
        compile_checked(quote! {
            fn ne_if(a: u64, b: u64) -> u64 {
                if a != b { 1 } else { 0 }
            }
        })
        .expect("`if a != b` 编译后栈上应只剩 1 个返回值");
    }

    /// 真源码：实测会泄漏的那个形状——while 循环体里两路 if/else，
    /// 每转一圈多留一格栈（n=300 时栈深 303）。这里要求编译期就报绿。
    #[test]
    fn loop_body_with_eq_if_is_balanced() {
        compile_checked(quote! {
            fn b_two_way(n: u64) -> u64 {
                let mut s = 0u64;
                let mut i = 0u64;
                while i < n {
                    if i & 1 == 0 {
                        s = s + i;
                    } else {
                        s = s + 1;
                    }
                    i = i + 1;
                }
                s
            }
        })
        .expect("循环体里带 if/else 必须收支平衡");
    }

    /// 真源码：`is_empty()` 也是「XOR 只置标志位」的形状，同样只该留 1 个布尔值。
    /// 这是本断言在既有用例里查出来的第二处同类泄漏（每次调用漏 1 格栈）。
    #[test]
    fn is_empty_leaves_exactly_one_value() {
        compile_checked(quote! {
            fn empty_check() -> u64 {
                let s = "";
                if s.is_empty() { 1 } else { 0 }
            }
        })
        .expect("s.is_empty() 编译后栈上应只剩 1 个返回值");
    }

    /// 真源码：三路 if/else if/else（else 分支里嵌套一层 compile_if）。
    #[test]
    fn loop_body_with_three_way_if_is_balanced() {
        compile_checked(quote! {
            fn a_three_way(n: u64) -> u64 {
                let mut s = 0u64;
                let mut i = 0u64;
                while i < n {
                    if i & 1 == 0 {
                        s = s + i;
                    } else if i & 3 == 1 {
                        s = s + 2;
                    } else {
                        s = s + 3;
                    }
                    i = i + 1;
                }
                s
            }
        })
        .expect("三路 if 必须收支平衡");
    }
}
