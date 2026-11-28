//! 三路 if 栈泄漏的形状二分：四个变体，各自的循环体只差一点点。
//!
//! 已知：`while` 里 `if / else if / else` 每轮泄漏一格栈（n≈10 就 E03 StackOverflow）。
//! 但 `compile_if` 的静态记账是平衡的，所以泄漏一定来自某种**结构组合**。
//! 这四个变体每个只改一处，跑通=不泄漏，跑挂=E03，用于定位触发条件。

use aegis_vm::vm_protect;

/// A：三路齐全（已知泄漏，作为对照）
#[vm_protect]
fn a_three_way(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s + i;
        } else if i & 3 == 1 {
            s = s + 2;
        } else {
            s = s + 3;
        }
        i = i + 1;
    }
    s
}

/// B：两路 if/else（已知正常，作为对照）
#[vm_protect]
fn b_two_way(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s + i;
        } else {
            s = s + 1;
        }
        i = i + 1;
    }
    s
}

/// C：两条并列的独立 if（不是 else if 链）
#[vm_protect]
fn c_two_independent(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s + i;
        }
        if i & 3 == 1 {
            s = s + 2;
        }
        i = i + 1;
    }
    s
}

/// D：else 块里再写一个 if（等价语义，但不是 else-if 语法）
#[vm_protect]
fn d_nested_block(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s + i;
        } else {
            if i & 3 == 1 {
                s = s + 2;
            } else {
                s = s + 3;
            }
        }
        i = i + 1;
    }
    s
}

/// E：else if 但**没有**最后的 else
#[vm_protect]
fn e_else_if_no_else(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        if i & 1 == 0 {
            s = s + i;
        } else if i & 3 == 1 {
            s = s + 2;
        }
        i = i + 1;
    }
    s
}

/// F：对照组——循环体里**没有任何 if**。用来判定泄漏是来自循环本身还是来自 if。
#[vm_protect]
fn f_loop_only(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        s = s + i;
        i = i + 1;
    }
    s
}

const N: u64 = 300;

#[test]
fn t_a_three_way() {
    assert_eq!(a_three_way(N), {
        let (mut s, mut i) = (0u64, 0u64);
        while i < N {
            if i & 1 == 0 { s += i } else if i & 3 == 1 { s += 2 } else { s += 3 }
            i += 1;
        }
        s
    });
}

#[test]
fn t_b_two_way() {
    assert_eq!(b_two_way(N), {
        let (mut s, mut i) = (0u64, 0u64);
        while i < N {
            if i & 1 == 0 { s += i } else { s += 1 }
            i += 1;
        }
        s
    });
}

#[test]
fn t_c_two_independent() {
    assert_eq!(c_two_independent(N), {
        let (mut s, mut i) = (0u64, 0u64);
        while i < N {
            if i & 1 == 0 { s += i }
            if i & 3 == 1 { s += 2 }
            i += 1;
        }
        s
    });
}

#[test]
fn t_d_nested_block() {
    assert_eq!(d_nested_block(N), {
        let (mut s, mut i) = (0u64, 0u64);
        while i < N {
            if i & 1 == 0 { s += i } else { if i & 3 == 1 { s += 2 } else { s += 3 } }
            i += 1;
        }
        s
    });
}

#[test]
fn t_e_else_if_no_else() {
    assert_eq!(e_else_if_no_else(N), {
        let (mut s, mut i) = (0u64, 0u64);
        while i < N {
            if i & 1 == 0 { s += i } else if i & 3 == 1 { s += 2 }
            i += 1;
        }
        s
    });
}

#[test]
fn t_f_loop_only() {
    let expect = {
        let (mut s, mut i) = (0u64, 0u64);
        while i < N { s += i; i += 1; }
        s
    };
    assert_eq!(f_loop_only(N), expect);
}
