//! 表达式语义对拍：VM 编译出的结果必须与原生 Rust 求值完全一致。
//!
//! 为什么需要这个文件：快路径（`compile_expr_to_reg`）与栈路径（`compile_expr`）
//! 是两套独立的发射逻辑，任何一方漏掉一种常量/运算符形态都会算错值，而
//! 既有的 871 个用例里没有一个覆盖「大常量」「移位量是表达式」这类组合——
//! 结果是 MUL 兜底 bug 与常量截断 bug 两次都躲过单元测试，靠门禁才抓到。
//!
//! 写法约定：每个用例一个 `#[vm_protect]` 函数 + 一个对照断言，
//! 期望值全部由原生 Rust 现算，不写死魔数。

use aegis_vm::*;

// ---------- 大常量（超出 u32，最容易在「立即数只编 32 位」处截断）----------

#[vm_protect]
fn big_add(x: u64) -> u64 {
    x + 0x123456789ABCDEF0
}

#[vm_protect]
fn big_mul(x: u64) -> u64 {
    x * 0x1_0000_0001
}

#[vm_protect]
fn big_xor(x: u64) -> u64 {
    x ^ 0xDEAD_BEEF_CAFE_F00D
}

#[vm_protect]
fn big_and(x: u64) -> u64 {
    x & 0xFFFF_FFFF_0000_0000
}

// ---------- 移位：移位量是变量 / 是表达式 ----------

#[vm_protect]
fn shl_var(x: u64, s: u64) -> u64 {
    x << (s & 63)
}

#[vm_protect]
fn shr_var(x: u64, s: u64) -> u64 {
    x >> (s & 63)
}

#[vm_protect]
fn shr_expr(x: u64, s: u64) -> u64 {
    x >> (s + 1)
}

// ---------- 减法：没有寄存器形态，必须回退栈路径 ----------

#[vm_protect]
fn sub_two(x: u64, y: u64) -> u64 {
    x - y
}

#[vm_protect]
fn sub_in_operand(x: u64, y: u64) -> u64 {
    (x - y) + (x & 0xFF)
}

// ---------- 位运算组合 ----------

#[vm_protect]
fn xor_var(x: u64, y: u64) -> u64 {
    x ^ y
}

#[vm_protect]
fn and_var(x: u64, y: u64) -> u64 {
    x & y
}

#[vm_protect]
fn nested_mix(x: u64, y: u64) -> u64 {
    (x + y) * (x & 0xFF) ^ (y >> 3)
}

#[vm_protect]
fn deep_paren(x: u64, y: u64, z: u64) -> u64 {
    x + (y + (z + 1)) * 2
}

// ---------- `let` 绑定：语句级寄存器路径的核心场景 ----------

#[vm_protect]
fn let_then_use(x: u64, y: u64) -> u64 {
    let a = x & 0xFF;
    let b = y + 1;
    a * b
}

/// 大常量先绑到局部变量再参与运算：这条正是「立即数只编 32 位」会出错的形态。
#[vm_protect]
fn let_big_const(x: u64) -> u64 {
    let k = 0x123456789ABCDEF0;
    x ^ k
}

#[vm_protect]
fn let_chain(x: u64) -> u64 {
    let a = x + 1;
    let b = a * 3;
    let c = b ^ 0xFF;
    c
}

// ---------- 断言 ----------

const EDGES: [u64; 5] = [0, 1, 255, u64::MAX, 0x8000_0000_0000_0001];

#[test]
fn parity_big_constants() {
    for x in EDGES {
        assert_eq!(
            big_add(x),
            x.wrapping_add(0x123456789ABCDEF0),
            "big_add({x:#x})"
        );
        assert_eq!(big_mul(x), x.wrapping_mul(0x1_0000_0001), "big_mul({x:#x})");
        assert_eq!(big_xor(x), x ^ 0xDEAD_BEEF_CAFE_F00D, "big_xor({x:#x})");
        assert_eq!(big_and(x), x & 0xFFFF_FFFF_0000_0000, "big_and({x:#x})");
    }
}

#[test]
fn parity_shifts() {
    for x in EDGES {
        for s in [0u64, 1, 31, 62, 63, 64, 100] {
            assert_eq!(shl_var(x, s), x << (s & 63), "shl_var({x:#x}, {s})");
            assert_eq!(shr_var(x, s), x >> (s & 63), "shr_var({x:#x}, {s})");
        }
        // `x >> (s + 1)` 的期望值必须用原始右移算：s 取到 62 时 s+1=63 刚好合法，
        // 再大就会在原生侧触发移位溢出 panic（VM 侧则是掩码语义），比较就没有意义了。
        for s in [0u64, 1, 31, 61, 62] {
            assert_eq!(shr_expr(x, s), x >> (s + 1), "shr_expr({x:#x}, {s})");
        }
    }
}

#[test]
fn parity_sub() {
    for x in EDGES {
        for y in [0u64, 1, 7, u64::MAX] {
            assert_eq!(sub_two(x, y), x.wrapping_sub(y), "sub_two({x:#x}, {y:#x})");
            assert_eq!(
                sub_in_operand(x, y),
                x.wrapping_sub(y).wrapping_add(x & 0xFF),
                "sub_in_operand({x:#x}, {y:#x})"
            );
        }
    }
}

#[test]
fn parity_bitops() {
    for x in EDGES {
        for y in [0u64, 1, 0x5555_5555_5555_5555, u64::MAX] {
            assert_eq!(xor_var(x, y), x ^ y, "xor_var({x:#x}, {y:#x})");
            assert_eq!(and_var(x, y), x & y, "and_var({x:#x}, {y:#x})");
            assert_eq!(
                nested_mix(x, y),
                (x.wrapping_add(y)).wrapping_mul(x & 0xFF) ^ (y >> 3),
                "nested_mix({x:#x}, {y:#x})"
            );
            // deep_paren 由 parity_deep_paren 单独覆盖（三参数，期望值照抄原生写法）。
        }
    }
}

/// `deep_paren` 单独一个用例：`x + (y + (z + 1)) * 2`，
/// 期望值直接照抄表达式的原生写法，避免上面那条长式子里自己算错。
#[test]
fn parity_deep_paren() {
    for x in EDGES {
        for y in [0u64, 1, 255, u64::MAX] {
            let z = 3u64;
            assert_eq!(
                deep_paren(x, y, z),
                x.wrapping_add(y.wrapping_add(z.wrapping_add(1)).wrapping_mul(2)),
                "deep_paren({x:#x}, {y:#x}, {z})"
            );
        }
    }
}

#[test]
fn parity_let_bindings() {
    for x in EDGES {
        for y in [0u64, 1, 7, u64::MAX] {
            assert_eq!(
                let_then_use(x, y),
                (x & 0xFF).wrapping_mul(y.wrapping_add(1)),
                "let_then_use({x:#x}, {y:#x})"
            );
        }
        assert_eq!(let_big_const(x), x ^ 0x123456789ABCDEF0, "let_big_const({x:#x})");
        assert_eq!(
            let_chain(x),
            x.wrapping_add(1).wrapping_mul(3) ^ 0xFF,
            "let_chain({x:#x})"
        );
    }
}
