//! 判定「内层 else if 的条件缺右操作数」是常量 `1` 的发射问题，还是嵌套路径的问题。
//!
//! 背景：`while` + `if/else if/else` 里，内层 `i & 3 == 1` 编译成
//! `PUSH_REG; XOR; ...` —— 右操作数没压栈，XOR 弹到的是上一轮的垃圾值，
//! 于是每轮净 +1 格栈，跑到 StackOverflow。
//! 外层 `i & 1 == 0` 正常，因为右值是字面量 0（走 emit_zero，压了三条指令）。
//!
//! 这两个形状用来区分：
//!   - 非嵌套的 `n == 1`：若也残缺 → 是「常量 1 的压栈」坏了；
//!   - 嵌套的 `else if n == 1`：两个都残缺说明与嵌套无关。

use aegis_vm::vm_protect;

#[vm_protect]
fn flat_eq_one(n: u64) -> u64 {
    if n == 1 {
        10
    } else {
        20
    }
}

#[vm_protect]
fn flat_eq_two(n: u64) -> u64 {
    if n == 2 {
        10
    } else {
        20
    }
}

#[vm_protect]
fn nested_eq_one(n: u64) -> u64 {
    let mut r = 0u64;
    if n == 0 {
        r = 10;
    } else if n == 1 {
        r = 30;
    } else {
        r = 20;
    }
    r
}

#[test]
fn shapes_are_semantically_right() {
    assert_eq!(flat_eq_one(1), 10, "flat_eq_one(1)");
    assert_eq!(flat_eq_one(0), 20, "flat_eq_one(0)");
    assert_eq!(flat_eq_two(2), 10, "flat_eq_two(2)");
    assert_eq!(flat_eq_two(1), 20, "flat_eq_two(1)");
    assert_eq!(nested_eq_one(0), 10, "nested_eq_one(0)");
    assert_eq!(nested_eq_one(1), 30, "nested_eq_one(1)");
    assert_eq!(nested_eq_one(5), 20, "nested_eq_one(5)");
}
