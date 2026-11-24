//! 超级指令 `CMP_REG_REG_JGE` 的行为契约。
//!
//! 该指令等价于 `PUSH_REG a; PUSH_REG b; CMP; DROP; DROP; JGE offset`：
//! 设置同样的标志位、不动栈，`a >= b`（有符号）时相对跳转。
//!
//! 这些用例先于实现写（TDD）：没有 handler 时，引擎会把它当未知 opcode 返回
//! `InvalidOpcode`，测试即红。

use aegis_vm::build_config::opcodes::{exec, fused, stack};
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
