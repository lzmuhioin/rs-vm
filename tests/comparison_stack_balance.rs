//! `==` / `!=` 的栈平衡回归。
//!
//! 背景：`compile_eq` / `compile_ne` 用 `XOR` 得到「相等即 0」后直接分支压布尔值，
//! 但**没有丢弃 XOR 的结果**；而 `compile_lt/le/gt/ge` 的 `CMP; DROP; DROP` 是对的。
//! 结果：每求值一次 `==` 就多留一个值在栈上。循环里跑一千多轮就会 `E03`/`E04`，
//! 单次求值看不出问题——所以必须有迭代数大到能暴露的用例。

use aegis_vm_macro::vm_protect;

#[vm_protect]
fn eq_in_loop(n: u64) -> u64 {
    let mut hits = 0u64;
    let mut i = 0u64;
    while i < n {
        if i == 7 {
            hits = hits + 1;
        }
        i = i + 1;
    }
    hits
}

#[vm_protect]
fn ne_in_loop(n: u64) -> u64 {
    let mut hits = 0u64;
    let mut i = 0u64;
    while i < n {
        if i != 7 {
            hits = hits + 1;
        }
        i = i + 1;
    }
    hits
}

/// 条件走回退路径（字面量在左），确认修的是表达式而不是某条条件捷径
#[vm_protect]
fn eq_literal_left(n: u64) -> u64 {
    let mut hits = 0u64;
    let mut i = 0u64;
    while i < n {
        if 7 == i {
            hits = hits + 1;
        }
        i = i + 1;
    }
    hits
}

/// `==` 当值用（不在 if 里），这是最纯粹的暴露形态
#[vm_protect]
fn eq_as_value(n: u64) -> u64 {
    let mut hits = 0u64;
    let mut i = 0u64;
    while i < n {
        hits = hits + (i == 7);
        i = i + 1;
    }
    hits
}

#[vm_protect]
fn ne_as_value(n: u64) -> u64 {
    let mut hits = 0u64;
    let mut i = 0u64;
    while i < n {
        hits = hits + (i != 7);
        i = i + 1;
    }
    hits
}

/// 对照：`<` 一直是对的，修完不能把它改坏
#[vm_protect]
fn lt_as_value(n: u64) -> u64 {
    let mut hits = 0u64;
    let mut i = 0u64;
    while i < n {
        hits = hits + (i < 7);
        i = i + 1;
    }
    hits
}

// 迭代数取 2000：约 1000 轮就溢出，1000 以内测不出来
const N: u64 = 2000;

#[test]
fn eq_in_loop_is_stack_stable() {
    assert_eq!(eq_in_loop(N), 1);
}

#[test]
fn ne_in_loop_is_stack_stable() {
    assert_eq!(ne_in_loop(N), N - 1);
}

#[test]
fn eq_literal_left_is_stack_stable() {
    assert_eq!(eq_literal_left(N), 1);
}

#[test]
fn eq_as_value_is_stack_stable() {
    assert_eq!(eq_as_value(N), 1);
}

#[test]
fn ne_as_value_is_stack_stable() {
    assert_eq!(ne_as_value(N), N - 1);
}

#[test]
fn lt_as_value_still_correct() {
    assert_eq!(lt_as_value(N), 7);
}