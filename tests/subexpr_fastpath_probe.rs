//! 定位「快路径在栈路径内部被调用」时是否写错值。
//!
//! 三个用例结构相同（快路径子表达式位于栈路径的操作数位置），
//! 只有运算符不同，用来区分「AND 特有」还是「所有快路径都这样」。

use aegis_vm::*;

/// 与：`&` 立即数形态，位于右移的右操作数（栈路径内部）
#[vm_protect]
fn shr_and(x: u64, shift: u64) -> u64 {
    x >> (shift & 63)
}

/// 加：`+` 立即数形态，位于右移的右操作数（同样结构，对照组）
#[vm_protect]
fn shr_add(x: u64, shift: u64) -> u64 {
    x >> (shift + 1)
}

/// 与：`&` 立即数形态，位于加法的右操作数（同样结构，对照组）
#[vm_protect]
fn add_and(x: u64, y: u64) -> u64 {
    x + (y & 63)
}

#[test]
fn probe_shr_and() {
    for (x, s) in [(u64::MAX, 0u64), (0xFFFF, 4), (1, 63)] {
        assert_eq!(shr_and(x, s), x >> (s & 63), "shr_and({x:#x}, {s})");
    }
}

#[test]
fn probe_shr_add() {
    for (x, s) in [(u64::MAX, 0u64), (0xFFFF, 4), (1, 62)] {
        assert_eq!(shr_add(x, s), x >> (s + 1), "shr_add({x:#x}, {s})");
    }
}

#[test]
fn probe_add_and() {
    for (x, y) in [(u64::MAX, 0u64), (255, 63), (7, 200)] {
        assert_eq!(add_and(x, y), x + (y & 63), "add_and({x}, {y})");
    }
}
