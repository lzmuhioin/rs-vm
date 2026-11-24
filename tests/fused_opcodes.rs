//! 超级指令 `CMP_REG_REG_JGE` 的行为契约。
//!
//! 该指令等价于 `PUSH_REG a; PUSH_REG b; CMP; DROP; DROP; JGE offset`：
//! 设置同样的标志位、不动栈，`a >= b`（有符号）时相对跳转。
//!
//! 这些用例先于实现写（TDD）：没有 handler 时，引擎会把它当未知 opcode 返回
//! `InvalidOpcode`，测试即红。

use aegis_vm::build_config::opcodes::{control, exec, fused, stack};
use aegis_vm::execute;

/// 构造：r0 = a, r1 = b, 然后 CMP_REG_REG_JGE r0, r1, +3
/// 不跳转分支 push 9；跳转目标 push 7。
fn program(a: u8, b: u8) -> [u8; 18] {
    [
        stack::PUSH_IMM8, a, stack::POP_REG, 0,
        stack::PUSH_IMM8, b, stack::POP_REG, 1,
        fused::CMP_REG_REG_JGE, 0, 1, 0x03, 0x00,
        stack::PUSH_IMM8, 9, exec::HALT,
        stack::PUSH_IMM8, 7,
    ]
}

#[test]
fn jumps_when_a_greater_than_b() {
    let mut code = program(5, 3).to_vec();
    code.push(exec::HALT);
    assert_eq!(execute(&code, &[]).unwrap(), 7, "5 >= 3 应当跳转");
}

#[test]
fn jumps_when_a_equals_b() {
    let mut code = program(5, 5).to_vec();
    code.push(exec::HALT);
    assert_eq!(execute(&code, &[]).unwrap(), 7, "5 >= 5 应当跳转（JGE 含等号）");
}

#[test]
fn falls_through_when_a_less_than_b() {
    let mut code = program(3, 5).to_vec();
    code.push(exec::HALT);
    assert_eq!(execute(&code, &[]).unwrap(), 9, "3 >= 5 不成立，应当顺序执行");
}

#[test]
fn leaves_stack_untouched() {
    // 融合指令是栈中性的：前后栈深度一致，不能像 CMP 那样回压两个操作数。
    let code = [
        stack::PUSH_IMM8, 42,
        stack::PUSH_IMM8, 5, stack::POP_REG, 0,
        stack::PUSH_IMM8, 3, stack::POP_REG, 1,
        fused::CMP_REG_REG_JGE, 0, 1, 0x00, 0x00,
        exec::HALT,
    ];
    assert_eq!(execute(&code, &[]).unwrap(), 42, "融合指令不该动栈");
}
#[test]
fn inc_reg_increments_in_place_and_sets_zero_flag() {
    // INC_REG 等价于 `PUSH_REG r; INC; POP_REG r`：寄存器 +1，零标志随结果更新。
    let code = [
        stack::PUSH_IMM8, 41, stack::POP_REG, 0,
        fused::INC_REG, 0,
        stack::PUSH_IMM8, 42, stack::POP_REG, 1,
        fused::INC_REG, 1,          // 41+1 = 42 → ZF=0
        fused::INC_REG, 1,          // 再自增一次不影响 ZF=0（42→43）
        stack::PUSH_REG, 0,
        exec::HALT,
    ];
    assert_eq!(execute(&code, &[]).unwrap(), 42, "INC_REG 应当就地自增 r0");
}

#[test]
fn inc_reg_zero_flag_reflects_result() {
    // 只依赖 ZF 的分支：INC_REG 之后 JNZ 的行为必须与 PUSH_REG;INC;POP_REG 一致。
    let code = [
        stack::PUSH_IMM8, 0xFF, stack::POP_REG, 0,
        fused::INC_REG, 0,                      // 0xFF + 1 = 0x100 → 非零
        fused::INC_REG, 0,                      // ...
        stack::PUSH_IMM8, 1, stack::POP_REG, 0,
        fused::INC_REG, 0,                      // 1+1 = 2 → 非零
        stack::PUSH_IMM8, 0, stack::POP_REG, 0,
        fused::INC_REG, 0,                      // 0+1 = 1 → 非零
        stack::PUSH_IMM8, 0, stack::POP_REG, 0,
        stack::PUSH_IMM8, 0xFF, stack::POP_REG, 0,
        fused::INC_REG, 0,                      // 0xFF+1 = 0x100，非零
        stack::PUSH_IMM8, 77,
        exec::HALT,
    ];
    assert_eq!(execute(&code, &[]).unwrap(), 77);
}

// ---- 寄存器-立即数 / 寄存器-寄存器 算术融合 ----
// 语义等价：`ADD_REG_IMM r, imm` ≡ PUSH_REG r; PUSH_IMM32 imm; ADD; POP_REG r
//          `ADD_REG_REG d,a,b`  ≡ PUSH_REG a; PUSH_REG b; ADD; POP_REG d
// 标志位只置 ZF（与 ADD/MUL 一致）。

#[test]
fn add_reg_imm_matches_stack_sequence() {
    let code = [
        stack::PUSH_IMM8, 40, stack::POP_REG, 0,
        fused::ADD_REG_IMM, 0, 2, 0, 0, 0, // r0 = 40 + 2
        stack::PUSH_REG, 0,
        exec::HALT,
    ];
    assert_eq!(execute(&code, &[]).unwrap(), 42);
}

#[test]
fn add_reg_reg_does_not_disturb_stack() {
    let code = [
        stack::PUSH_IMM8, 7,          // 栈上留一个哨兵
        stack::PUSH_IMM8, 40, stack::POP_REG, 0,
        stack::PUSH_IMM8, 2, stack::POP_REG, 1,
        fused::ADD_REG_REG, 2, 0, 1,  // r2 = r0 + r1
        stack::PUSH_REG, 2,
        exec::HALT,
    ];
    assert_eq!(execute(&code, &[]).unwrap(), 42);
}

#[test]
fn mul_reg_imm_and_mul_reg_reg() {
    let code = [
        stack::PUSH_IMM8, 6, stack::POP_REG, 0,
        fused::MUL_REG_IMM, 0, 7, 0, 0, 0, // r0 = 6 * 7
        stack::PUSH_IMM8, 6, stack::POP_REG, 1,
        fused::MUL_REG_REG, 2, 0, 1,       // r2 = 42 * 6 = 252
        stack::PUSH_REG, 2,
        exec::HALT,
    ];
    assert_eq!(execute(&code, &[]).unwrap(), 252);
}

#[test]
fn fused_arithmetic_sets_zero_flag() {
    let code = [
        stack::PUSH_IMM8, 0, stack::POP_REG, 0,
        fused::ADD_REG_IMM, 0, 0, 0, 0, 0, // 0 + 0 = 0 → ZF=1
        control::JZ, 0x03, 0x00,
        stack::PUSH_IMM8, 1,               // ZF=0 才会走到这里
        exec::HALT,
        stack::PUSH_IMM8, 9,
        exec::HALT,
    ];
    assert_eq!(execute(&code, &[]).unwrap(), 9, "0+0 后 JZ 应跳转");
}

// ---- CMP_REG_REG_JCC：通用比较-跳转 ----
// cond: 0=eq 1=ne 2=gt 3=lt 4=ge 5=le（有符号）

#[test]
fn cmp_reg_reg_jcc_all_conditions() {
    // r0=5, r1=3 → eq:F ne:T gt:T lt:F ge:T le:F
    let cases: [(u8, bool); 6] = [
        (0, false), (1, true), (2, true), (3, false), (4, true), (5, false),
    ];
    for (cond, expect_jump) in cases {
        let code = [
            stack::PUSH_IMM8, 5, stack::POP_REG, 0,
            stack::PUSH_IMM8, 3, stack::POP_REG, 1,
            // 条件成立则跳过「push 9」分支，落到「push 7」分支
            fused::CMP_REG_REG_JCC, 0, 1, cond, 0x03, 0x00,
            stack::PUSH_IMM8, 9, exec::HALT,
            stack::PUSH_IMM8, 7, exec::HALT,
        ];
        let got = execute(&code, &[]).unwrap();
        let want = if expect_jump { 7 } else { 9 };
        assert_eq!(got, want, "cond={cond} 期望 {}", if expect_jump { "跳转" } else { "顺序" });
    }
}

#[test]
fn cmp_reg_reg_jcc_handles_negative() {
    // r0=0xFF(-1), r1=2 → lt:T
    let code = [
        // PUSH_IMM8 是零扩展，写不出负数；用 64 位字面量 -1（u64::MAX）
        stack::PUSH_IMM, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, stack::POP_REG, 0,
        stack::PUSH_IMM8, 2, stack::POP_REG, 1,
        fused::CMP_REG_REG_JCC, 0, 1, 3, 0x03, 0x00, // lt
        stack::PUSH_IMM8, 9, exec::HALT,
        stack::PUSH_IMM8, 7, exec::HALT,
    ];
    assert_eq!(execute(&code, &[]).unwrap(), 7, "-1 < 2 应为真（有符号）");
}
