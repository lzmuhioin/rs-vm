//! 访存融合：`VEC_GET_*` / `VEC_SET_*`。
//!
//! 栈式契约：
//!   `[vec_addr, index] -> [value]`（VEC_GET），`[vec_addr, index, value] -> []`（VEC_SET），
//!   越界返回 `HeapOutOfBounds`（E14）。
//! 融合形态等价于「把寄存器压栈 → 原指令 → 取回寄存器」，因此必须**栈中性**，
//! 且越界行为与栈式逐字一致。
//!
//! 先于实现写（TDD）：未实现时是未知 opcode → `InvalidOpcode`，用例即红。

use aegis_vm::build_config::opcodes::{exec, fused, stack, vector};
use aegis_vm::{execute, VmError};

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

fn run(code: Vec<u8>) -> u64 {
    let mut code = code;
    code.push(exec::HALT);
    execute(&code, &[]).expect("程序应当正常结束")
}

fn run_err(code: Vec<u8>) -> VmError {
    let mut code = code;
    code.push(exec::HALT);
    execute(&code, &[]).expect_err("应当报错")
}

/// 造一个长度 `len`、元素为 `i * 3` 的向量，地址放进 r0。
///
/// 注意 `VEC_NEW` 只分配容量、**长度是 0**，所以必须用 `VEC_PUSH` 追加；
/// 用 `VEC_SET` 写会当场越界，测试会「因为别的原因」通过（第一版就踩了这个坑）。
fn make_vec(len: u64) -> Vec<u8> {
    let mut code = Vec::new();
    code.push(stack::PUSH_IMM);
    code.extend_from_slice(&len.to_le_bytes());
    code.push(stack::PUSH_IMM);
    code.extend_from_slice(&8u64.to_le_bytes());
    code.push(vector::VEC_NEW); // -> [addr]

    for i in 0..len {
        code.push(stack::DUP); // [addr, addr]
        code.push(stack::PUSH_IMM);
        code.extend_from_slice(&(i * 3).to_le_bytes());
        code.push(vector::VEC_PUSH); // -> [addr]
    }
    code.extend_from_slice(&[stack::POP_REG, 0]);
    code
}

// ---------------------------------------------------------------------------
// VEC_GET_REG <dst> <vec> <idx>  ≡  PUSH_REG vec; PUSH_REG idx; VEC_GET; POP_REG dst
// ---------------------------------------------------------------------------

fn get_fused(idx: u64, len: u64) -> u64 {
    let mut code = make_vec(len);
    code.extend_from_slice(&load(&[(1, idx)]));
    code.extend_from_slice(&[fused::VEC_GET_REG, 2, 0, 1]);
    code.extend_from_slice(&[stack::PUSH_REG, 2]);
    run(code)
}

fn get_stack(idx: u64, len: u64) -> u64 {
    let mut code = make_vec(len);
    code.extend_from_slice(&load(&[(1, idx)]));
    code.extend_from_slice(&[stack::PUSH_REG, 0, stack::PUSH_REG, 1, vector::VEC_GET]);
    code.extend_from_slice(&[stack::POP_REG, 2, stack::PUSH_REG, 2]);
    run(code)
}

#[test]
fn vec_get_fused_matches_stack() {
    for len in [1u64, 2, 7, 16] {
        for idx in 0..len {
            assert_eq!(get_fused(idx, len), get_stack(idx, len), "len={len} idx={idx}");
        }
    }
}

#[test]
fn vec_get_fused_is_stack_neutral() {
    let mut code = make_vec(4);
    code.extend_from_slice(&[stack::PUSH_IMM8, 99]);
    code.extend_from_slice(&load(&[(1, 2)]));
    code.extend_from_slice(&[fused::VEC_GET_REG, 2, 0, 1]);
    run(code); // 结尾 HALT 时栈顶应还是 99 → 用下面的断言值验证
    let value = run({
        let mut c = make_vec(4);
        c.extend_from_slice(&[stack::PUSH_IMM8, 99]);
        c.extend_from_slice(&load(&[(1, 2)]));
        c.extend_from_slice(&[fused::VEC_GET_REG, 2, 0, 1]);
        c
    });
    assert_eq!(value, 99, "融合取数不得改动栈");
}

#[test]
fn vec_get_fused_out_of_bounds_matches_stack() {
    let mut fused_code = make_vec(4);
    fused_code.extend_from_slice(&load(&[(1, 9)]));
    fused_code.extend_from_slice(&[fused::VEC_GET_REG, 2, 0, 1]);
    let fused_err = run_err(fused_code);

    let mut stack_code = make_vec(4);
    stack_code.extend_from_slice(&load(&[(1, 9)]));
    stack_code.extend_from_slice(&[stack::PUSH_REG, 0, stack::PUSH_REG, 1, vector::VEC_GET]);
    let stack_err = run_err(stack_code);

    assert_eq!(fused_err, stack_err, "越界错误必须与栈式一致");
    assert_eq!(fused_err, VmError::HeapOutOfBounds);
}

// ---------------------------------------------------------------------------
// VEC_GET_REG_IMM <dst> <vec> <idx:u32>
// ---------------------------------------------------------------------------

fn get_imm_fused(idx: u32, len: u64) -> u64 {
    let mut code = make_vec(len);
    code.extend_from_slice(&[fused::VEC_GET_REG_IMM, 2, 0]);
    code.extend_from_slice(&idx.to_le_bytes());
    code.extend_from_slice(&[stack::PUSH_REG, 2]);
    run(code)
}

fn get_imm_stack(idx: u32, len: u64) -> u64 {
    let mut code = make_vec(len);
    code.extend_from_slice(&[stack::PUSH_REG, 0, stack::PUSH_IMM32]);
    code.extend_from_slice(&idx.to_le_bytes());
    code.extend_from_slice(&[vector::VEC_GET, stack::POP_REG, 2, stack::PUSH_REG, 2]);
    run(code)
}

#[test]
fn vec_get_imm_matches_stack() {
    for len in [1u64, 5, 16] {
        for idx in 0..len as u32 {
            assert_eq!(
                get_imm_fused(idx, len),
                get_imm_stack(idx, len),
                "len={len} idx={idx}"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// VEC_SET_REG_REG_REG <vec> <idx> <src>
// ---------------------------------------------------------------------------

fn set_fused_read_back(idx: u64, value: u64, len: u64) -> u64 {
    let mut code = make_vec(len);
    code.extend_from_slice(&load(&[(1, idx), (2, value)]));
    code.extend_from_slice(&[fused::VEC_SET_REG, 0, 1, 2]);
    // 读回 r3 = vec[idx]
    code.extend_from_slice(&[fused::VEC_GET_REG, 3, 0, 1, stack::PUSH_REG, 3]);
    run(code)
}

fn set_stack_read_back(idx: u64, value: u64, len: u64) -> u64 {
    let mut code = make_vec(len);
    code.extend_from_slice(&load(&[(1, idx), (2, value)]));
    code.extend_from_slice(&[
        stack::PUSH_REG,
        0,
        stack::PUSH_REG,
        1,
        stack::PUSH_REG,
        2,
        vector::VEC_SET,
    ]);
    code.extend_from_slice(&[stack::PUSH_REG, 0, stack::PUSH_REG, 1, vector::VEC_GET, stack::POP_REG, 3]);
    code.extend_from_slice(&[stack::PUSH_REG, 3]);
    run(code)
}

#[test]
fn vec_set_fused_matches_stack() {
    for len in [1u64, 6, 16] {
        for idx in 0..len {
            for value in [0u64, 1, 77, u64::MAX] {
                assert_eq!(
                    set_fused_read_back(idx, value, len),
                    set_stack_read_back(idx, value, len),
                    "len={len} idx={idx} value={value}"
                );
            }
        }
    }
}

#[test]
fn vec_set_fused_is_stack_neutral() {
    let mut code = make_vec(4);
    code.extend_from_slice(&load(&[(1, 2), (2, 55)]));
    code.extend_from_slice(&[stack::PUSH_IMM8, 7]);
    code.extend_from_slice(&[fused::VEC_SET_REG, 0, 1, 2]);
    assert_eq!(run(code), 7, "融合存数不得改动栈");
}

// ---------------------------------------------------------------------------
// VEC_SET_REG_IMM_REG <vec> <idx:u32> <src>
// ---------------------------------------------------------------------------

fn set_imm_fused_read_back(idx: u32, value: u64, len: u64) -> u64 {
    let mut code = make_vec(len);
    code.extend_from_slice(&load(&[(2, value)]));
    code.extend_from_slice(&[fused::VEC_SET_REG_IMM, 0]);
    code.extend_from_slice(&idx.to_le_bytes());
    code.push(2);
    code.extend_from_slice(&[stack::PUSH_REG, 0, stack::PUSH_IMM32]);
    code.extend_from_slice(&idx.to_le_bytes());
    code.extend_from_slice(&[vector::VEC_GET]);
    run(code)
}

#[test]
fn vec_set_imm_writes_expected_value() {
    assert_eq!(set_imm_fused_read_back(3, 42, 8), 42);
    assert_eq!(set_imm_fused_read_back(0, u64::MAX, 1), u64::MAX);
}

#[test]
fn vec_set_fused_out_of_bounds_matches_stack() {
    let mut fused_code = make_vec(4);
    fused_code.extend_from_slice(&load(&[(1, 4), (2, 1)]));
    fused_code.extend_from_slice(&[fused::VEC_SET_REG, 0, 1, 2]);
    let fused_err = run_err(fused_code);

    let mut stack_code = make_vec(4);
    stack_code.extend_from_slice(&load(&[(1, 4), (2, 1)]));
    stack_code.extend_from_slice(&[
        stack::PUSH_REG,
        0,
        stack::PUSH_REG,
        1,
        stack::PUSH_REG,
        2,
        vector::VEC_SET,
    ]);
    let stack_err = run_err(stack_code);

    assert_eq!(fused_err, stack_err);
    assert_eq!(fused_err, VmError::HeapOutOfBounds);
}