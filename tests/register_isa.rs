//! 寄存器式 ISA 阶段一：`*_REG_REG` 算术指令的行为契约。
//!
//! 每条指令等价于 `PUSH_REG a; PUSH_REG b; <OP>; POP_REG dst`：栈中性、只写目的寄存器、
//! 标志位与栈式版本一致（只置 ZF）。除零/除零取模按栈式语义返回 0（不是错误）。
//!
//! 先于实现写（TDD）：没有 handler 时引擎把它当未知 opcode，返回 `InvalidOpcode`，测试即红。

use aegis_vm::build_config::opcodes::{arithmetic, exec, fused, stack};
use aegis_vm::execute;

/// 把 (寄存器号, 值) 写成 `PUSH_IMM <u64>; POP_REG <r>` 序列
fn load(regs: &[(u8, u64)]) -> Vec<u8> {
    let mut code = Vec::new();
    for &(r, v) in regs {
        code.push(stack::PUSH_IMM);
        code.extend_from_slice(&v.to_le_bytes());
        code.push(stack::POP_REG);
        code.push(r);
    }
    code
}

/// 跑一段程序，返回 `HALT` 时栈顶的值
fn run(code: Vec<u8>) -> u64 {
    let mut code = code;
    code.push(exec::HALT);
    execute(&code, &[]).expect("程序应当正常结束")
}

/// r0 = a, r1 = b, 目标寄存器 r2 = 融合指令的结果，最后 PUSH_REG 2 取值
fn fused_case(op: u8, a: u64, b: u64) -> u64 {
    let mut code = load(&[(0, a), (1, b)]);
    code.extend_from_slice(&[op, 2, 0, 1]);
    code.extend_from_slice(&[stack::PUSH_REG, 2]);
    run(code)
}

/// 同一组操作数的栈式写法，作为等价基准
fn stack_case(op: u8, a: u64, b: u64) -> u64 {
    let mut code = load(&[(0, a), (1, b)]);
    code.extend_from_slice(&[stack::PUSH_REG, 0, stack::PUSH_REG, 1, op, stack::POP_REG, 2]);
    code.extend_from_slice(&[stack::PUSH_REG, 2]);
    run(code)
}

#[test]
fn sub_reg_reg_wraps_on_underflow() {
    assert_eq!(fused_case(fused::SUB_REG_REG, 10, 3), 7);
    assert_eq!(
        fused_case(fused::SUB_REG_REG, 0, 1),
        u64::MAX,
        "减法下溢必须与栈式一致地回绕"
    );
}

#[test]
fn div_reg_reg_returns_zero_on_division_by_zero() {
    assert_eq!(fused_case(fused::DIV_REG_REG, 20, 6), 3);
    assert_eq!(
        fused_case(fused::DIV_REG_REG, 20, 0),
        0,
        "除零返回 0，与 handle_div 的 checked_div(..).unwrap_or(0) 一致"
    );
}

#[test]
fn mod_reg_reg_returns_zero_on_zero_divisor() {
    assert_eq!(fused_case(fused::MOD_REG_REG, 20, 6), 2);
    assert_eq!(fused_case(fused::MOD_REG_REG, 20, 0), 0);
}

#[test]
fn bitwise_reg_reg_matches_stack_ops() {
    assert_eq!(fused_case(fused::AND_REG_REG, 0b1100, 0b1010), 0b1000);
    assert_eq!(fused_case(fused::OR_REG_REG, 0b1100, 0b1010), 0b1110);
    assert_eq!(fused_case(fused::XOR_REG_REG, 0b1100, 0b1010), 0b0110);
}

#[test]
fn shift_reg_reg_masks_shift_count() {
    assert_eq!(fused_case(fused::SHL_REG_REG, 1, 4), 16);
    assert_eq!(fused_case(fused::SHR_REG_REG, 16, 4), 1);
    assert_eq!(
        fused_case(fused::SHL_REG_REG, 1, 64),
        1,
        "移位量 64 回绕为 0（wrapping_shl），与栈式 SHL 一致"
    );
}

#[test]
fn reg_reg_ops_are_stack_neutral() {
    // 栈上先放一个哨兵，跑完所有新指令后它必须还在栈顶
    let mut code = load(&[(0, 0b1100), (1, 0b1010)]);
    code.extend_from_slice(&[stack::PUSH_IMM8, 42]);
    for op in [
        fused::SUB_REG_REG,
        fused::DIV_REG_REG,
        fused::MOD_REG_REG,
        fused::AND_REG_REG,
        fused::OR_REG_REG,
        fused::XOR_REG_REG,
        fused::SHL_REG_REG,
        fused::SHR_REG_REG,
    ] {
        code.extend_from_slice(&[op, 2, 0, 1]);
    }
    assert_eq!(run(code), 42, "寄存器式指令不得动栈");
}

#[test]
fn every_reg_reg_op_matches_its_stack_equivalent() {
    let pairs = [(0u64, 0u64), (1, 1), (7, 3), (255, 8), (u64::MAX, 2), (123_456, 789)];
    let cases: [(u8, u8); 8] = [
        (fused::SUB_REG_REG, 0),
        (fused::DIV_REG_REG, 0),
        (fused::MOD_REG_REG, 0),
        (fused::AND_REG_REG, 0),
        (fused::OR_REG_REG, 0),
        (fused::XOR_REG_REG, 0),
        (fused::SHL_REG_REG, 0),
        (fused::SHR_REG_REG, 0),
    ];
    // 栈式对应算子（只用于本测试的对照，取 opcodes::exec 里的同名算子）
    let stack_ops: [u8; 8] = [
        arithmetic::SUB,
        arithmetic::DIV,
        arithmetic::MOD,
        arithmetic::AND,
        arithmetic::OR,
        arithmetic::XOR,
        arithmetic::SHL,
        arithmetic::SHR,
    ];
    for (i, (fused_op, _)) in cases.iter().enumerate() {
        for (a, b) in pairs {
            assert_eq!(
                fused_case(*fused_op, a, b),
                stack_case(stack_ops[i], a, b),
                "op 0x{fused_op:02X} 在 ({a}, {b}) 上与栈式不一致"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// 立即数形态：`r = r OP imm`，等价 `PUSH_REG r; PUSH_IMM32 imm; OP; POP_REG r`
// ---------------------------------------------------------------------------

/// r0 = a，然后 <op> r0, imm，最后取值
fn fused_imm_case(op: u8, a: u64, imm: u32) -> u64 {
    let mut code = load(&[(0, a)]);
    code.push(op);
    code.push(0);
    code.extend_from_slice(&imm.to_le_bytes());
    code.extend_from_slice(&[stack::PUSH_REG, 0]);
    run(code)
}

/// 同一组操作数的栈式写法
fn stack_imm_case(op: u8, a: u64, imm: u32) -> u64 {
    let mut code = load(&[(0, a)]);
    code.extend_from_slice(&[stack::PUSH_REG, 0, stack::PUSH_IMM32]);
    code.extend_from_slice(&imm.to_le_bytes());
    code.extend_from_slice(&[op, stack::POP_REG, 0]);
    code.extend_from_slice(&[stack::PUSH_REG, 0]);
    run(code)
}

#[test]
fn sub_reg_imm_subtracts_in_place() {
    assert_eq!(fused_imm_case(fused::SUB_REG_IMM, 100, 42), 58);
    assert_eq!(fused_imm_case(fused::SUB_REG_IMM, 0, 1), u64::MAX);
}

#[test]
fn div_and_mod_reg_imm_are_zero_safe() {
    assert_eq!(fused_imm_case(fused::DIV_REG_IMM, 100, 7), 14);
    assert_eq!(fused_imm_case(fused::MOD_REG_IMM, 100, 7), 2);
    assert_eq!(fused_imm_case(fused::DIV_REG_IMM, 100, 0), 0);
    assert_eq!(fused_imm_case(fused::MOD_REG_IMM, 100, 0), 0);
}

#[test]
fn bitwise_and_shift_reg_imm() {
    assert_eq!(fused_imm_case(fused::AND_REG_IMM, 0xABCD, 0x00FF), 0x00CD);
    assert_eq!(fused_imm_case(fused::OR_REG_IMM, 0xAB00, 0x00FF), 0xABFF);
    assert_eq!(fused_imm_case(fused::XOR_REG_IMM, 0xFFFF, 0x00FF), 0xFF00);
    assert_eq!(fused_imm_case(fused::SHL_REG_IMM, 1, 8), 256);
    assert_eq!(fused_imm_case(fused::SHR_REG_IMM, 256, 8), 1);
    assert_eq!(
        fused_imm_case(fused::SHL_REG_IMM, 1, 64),
        1,
        "移位量 64 回绕为 0，与栈式 SHL 一致"
    );
}

#[test]
fn every_reg_imm_op_matches_its_stack_equivalent() {
    let pairs: [(u64, u32); 6] = [
        (0, 1),
        (1, 1),
        (255, 8),
        (123_456, 789),
        (u64::MAX, 3),
        (1_000_000, 64),
    ];
    let fused_ops = [
        fused::SUB_REG_IMM,
        fused::DIV_REG_IMM,
        fused::MOD_REG_IMM,
        fused::AND_REG_IMM,
        fused::OR_REG_IMM,
        fused::XOR_REG_IMM,
        fused::SHL_REG_IMM,
        fused::SHR_REG_IMM,
    ];
    let stack_ops = [
        arithmetic::SUB,
        arithmetic::DIV,
        arithmetic::MOD,
        arithmetic::AND,
        arithmetic::OR,
        arithmetic::XOR,
        arithmetic::SHL,
        arithmetic::SHR,
    ];
    for (i, fused_op) in fused_ops.iter().enumerate() {
        for (a, imm) in pairs {
            assert_eq!(
                fused_imm_case(*fused_op, a, imm),
                stack_imm_case(stack_ops[i], a, imm),
                "op 0x{fused_op:02X} 在 ({a}, {imm}) 上与栈式不一致"
            );
        }
    }
}