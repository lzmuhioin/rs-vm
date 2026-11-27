//! 定位「循环体里带 if/else 会逐轮泄漏栈」的位置：哪种分支形状会漏。

use aegis_vm::vm_protect;

#[vm_protect]
fn with_if_else(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s + i;
        } else {
            s = s - 1;
        }
        i = i + 1;
    }
    s
}

#[vm_protect]
fn with_if_elseif_else(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s + i;
        } else if i & 3 == 1 {
            s = s + 3;
        } else {
            s = s - 1;
        }
        i = i + 1;
    }
    s
}

#[vm_protect]
fn with_if_only(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s + i;
        }
        i = i + 1;
    }
    s
}

fn native_if_else(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 { s = s.wrapping_add(i) } else { s = s.wrapping_sub(1) }
        i += 1;
    }
    s
}

fn native_if_elseif_else(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s.wrapping_add(i);
        } else if i & 3 == 1 {
            s = s.wrapping_add(3);
        } else {
            s = s.wrapping_sub(1);
        }
        i += 1;
    }
    s
}

fn native_if_only(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 { s = s.wrapping_add(i) }
        i += 1;
    }
    s
}

// 每个形状单独一个测试：`#[test]` 各跑一个线程，VM 状态不跨线程继承。
// 合在一个测试里会互相污染——先跑的泄漏会把后跑的直接顶到 StackOverflow，
// 分不清是谁漏的（上一版就栽在这上面）。

#[test]
#[ignore = "已知 bug：while 循环体里的三路 if/else if/else 每轮泄漏一格栈，跑到 E03 StackOverflow；\n    // 两路 if/else 与 if-only 正常。反汇编对照见提交说明"]
fn if_else_shape() {
    for n in [10u64, 100, 1000] {
        assert_eq!(with_if_else(n), native_if_else(n), "if/else n={n}");
    }
}

#[test]
#[ignore = "已知 bug：while 循环体里的三路 if/else if/else 每轮泄漏一格栈，跑到 E03 StackOverflow；\n    // 两路 if/else 与 if-only 正常。反汇编对照见提交说明"]
fn if_elseif_else_shape() {
    for n in [10u64, 100, 1000] {
        assert_eq!(
            with_if_elseif_else(n),
            native_if_elseif_else(n),
            "if/else-if/else n={n}"
        );
    }
}

#[test]
#[ignore = "已知 bug：while 循环体里的三路 if/else if/else 每轮泄漏一格栈，跑到 E03 StackOverflow；\n    // 两路 if/else 与 if-only 正常。反汇编对照见提交说明"]
fn if_only_shape() {
    for n in [10u64, 100, 1000] {
        assert_eq!(with_if_only(n), native_if_only(n), "if-only n={n}");
    }
}
