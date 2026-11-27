//! 复现 `vm_workloads` 基准里 arith 负载的语义差异。
//! 该负载在 bench 里带 `assert_eq!(vm, native)`，但 bench 不在 `cargo test` 里，
//! 所以它红了也没人知道。

use aegis_vm::vm_protect;

#[vm_protect]
fn vm_arith(n: u64) -> u64 {
    let mut a = n;
    let mut b = n + 1;
    let mut c = n + 2;
    let mut d = n + 3;
    let mut i = 0u64;
    while i < n {
        a = a + 3;
        b = b ^ 5;
        c = c - 7;
        d = d | 9;
        a = a + b;
        b = b + c;
        c = c + d;
        d = d + a;
        i = i + 1;
    }
    a ^ b ^ c ^ d
}

fn native(n: u64) -> u64 {
    let mut a = n;
    let mut b = n.wrapping_add(1);
    let mut c = n.wrapping_add(2);
    let mut d = n.wrapping_add(3);
    let mut i = 0u64;
    while i < n {
        a = a.wrapping_add(3);
        b ^= 5;
        c = c.wrapping_sub(7);
        d |= 9;
        a = a.wrapping_add(b);
        b = b.wrapping_add(c);
        c = c.wrapping_add(d);
        d = d.wrapping_add(a);
        i += 1;
    }
    a ^ b ^ c ^ d
}

#[test]
fn arith_matches_native() {
    for n in [0u64, 1, 2, 3, 5, 8] {
        assert_eq!(vm_arith(n), native(n), "arith n={n}");
    }
}
