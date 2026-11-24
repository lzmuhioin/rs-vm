//! 阶段一：宏侧能否把 `-` `/` `%` `&` `|` `^` `<<` `>>` 编到寄存器式融合指令上。
//!
//! 只断言**行为**（结果与原生同语义），不断言字节码——字节码在 `finalize()` 里已置换编码，
//! 数 opcode 出现次数只会数出 0（本仓曾经写过这种空转测试，已删）。
//!
//! 语义口径与栈式一致：整数运算回绕；除零/模零得 0；移位量按 64 取模。

use aegis_vm_macro::vm_protect;

#[vm_protect]
fn guarded_sub(a: u64, b: u64) -> u64 {
    a - b
}

#[vm_protect]
fn guarded_div(a: u64, b: u64) -> u64 {
    a / b
}

#[vm_protect]
fn guarded_mod(a: u64, b: u64) -> u64 {
    a % b
}

#[vm_protect]
fn guarded_bitmix(a: u64, b: u64) -> u64 {
    let x = a & b;
    let y = a | b;
    let z = a ^ b;
    x + y + z
}

#[vm_protect]
fn guarded_shift(a: u64, b: u64) -> u64 {
    let left = a << b;
    let right = a >> b;
    left ^ right
}

/// 混合表达式：右侧是子表达式，走临时寄存器路径
#[vm_protect]
fn guarded_nested(a: u64, b: u64) -> u64 {
    a - (b ^ 7) | (a & b) / (b + 1)
}

#[test]
fn sub_matches_native() {
    assert_eq!(guarded_sub(10, 3), 7);
    assert_eq!(guarded_sub(0, 1), u64::MAX, "减法下溢必须回绕");
}

#[test]
fn div_and_mod_match_native() {
    assert_eq!(guarded_div(20, 6), 3);
    assert_eq!(guarded_mod(20, 6), 2);
    assert_eq!(guarded_div(20, 0), 0, "除零得 0");
    assert_eq!(guarded_mod(20, 0), 0, "模零得 0");
}

#[test]
fn bitmix_matches_native() {
    for (a, b) in [(0b1100u64, 0b1010u64), (7, 3), (u64::MAX, 2), (123_456, 789)] {
        let native = (a & b).wrapping_add(a | b).wrapping_add(a ^ b);
        assert_eq!(guarded_bitmix(a, b), native, "({a}, {b}) 上不一致");
    }
}

#[test]
fn shifts_match_native() {
    for (a, b) in [(1u64, 4u64), (16, 4), (u64::MAX, 3), (255, 64)] {
        let native = a.wrapping_shl(b as u32) ^ a.wrapping_shr(b as u32);
        assert_eq!(guarded_shift(a, b), native, "({a}, {b}) 上不一致");
    }
}

#[test]
fn nested_expression_matches_native() {
    for (a, b) in [(100u64, 3u64), (0, 0), (u64::MAX, 5), (42, 41)] {
        let native = (a.wrapping_sub(b ^ 7)) | ((a & b) / (b + 1));
        assert_eq!(guarded_nested(a, b), native, "({a}, {b}) 上不一致");
    }
}