//! `benches/vm_workloads.rs` 七个负载的**语义对拍**，全部搬进门禁。
//!
//! 为什么需要这个文件：bench 里的每个负载都带 `assert_eq!(vm, native)`，但 bench **不在
//! `cargo test` 里**，那些断言从来没被执行过。M4（`let` 初始化走寄存器）就是因此让
//! `arith` 算出 0（正确 11367583203247441732）却一路绿灯通过了 883 条门禁。
//!
//! 这里保持与 bench **逐字相同**的写法：负载形状一变，对拍就失去意义——测的必须是
//! 基准真正在跑的那段代码，不是它的近似改写。
//!
//! 原生孪生体用 `wrapping_*`：VM 的加/减/乘就是回绕语义，用普通 `+` 会在 debug 下 panic。

use aegis_vm::vm_protect;

// ============================================================================
// 负载（与 bench 同形）
// ============================================================================

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

#[vm_protect]
fn vm_bitmix(n: u64) -> u64 {
    let mut x = n;
    let mut i = 0u64;
    while i < n {
        x = x ^ (x << 13);
        x = x ^ (x >> 7);
        x = x ^ (x << 17);
        i = i + 1;
    }
    x
}

#[vm_protect]
fn vm_sum(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        s = s + i;
        i = i + 1;
    }
    s
}

#[vm_protect]
fn vm_branchy(n: u64) -> u64 {
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
fn vm_arrloop(n: u64) -> u64 {
    let buf = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        let idx = i & 15;
        s = s + buf[idx];
        i = i + 1;
    }
    s
}

#[vm_protect]
fn vm_writeback(n: u64) -> u64 {
    let mut buf = [0u64; 16];
    let mut i = 0u64;
    while i < n {
        let idx = i & 15;
        buf[idx] = buf[idx] + i;
        i = i + 1;
    }
    buf[0] + buf[7] + buf[15]
}

#[vm_protect]
fn vm_fib(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    vm_fib(n - 1) + vm_fib(n - 2)
}

// ============================================================================
// 原生孪生体
// ============================================================================

fn native_arith(n: u64) -> u64 {
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

fn native_bitmix(n: u64) -> u64 {
    let mut x = n;
    let mut i = 0u64;
    while i < n {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        i += 1;
    }
    x
}

fn native_sum(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        s = s.wrapping_add(i);
        i += 1;
    }
    s
}

fn native_branchy(n: u64) -> u64 {
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

fn native_arrloop(n: u64) -> u64 {
    let buf = [1u64, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        s = s.wrapping_add(buf[(i & 15) as usize]);
        i += 1;
    }
    s
}

fn native_writeback(n: u64) -> u64 {
    let mut buf = [0u64; 16];
    let mut i = 0u64;
    while i < n {
        let idx = (i & 15) as usize;
        buf[idx] = buf[idx].wrapping_add(i);
        i += 1;
    }
    buf[0] + buf[7] + buf[15]
}

fn native_fib(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    native_fib(n - 1) + native_fib(n - 2)
}

// ============================================================================
// 对拍
// ============================================================================

/// 边界点包含 0、1（循环一次不进的路径）与跨过数组长度、位宽回绕的位置。
const NS: [u64; 6] = [0, 1, 2, 7, 16, 33];

#[test]
fn arith_matches_native() {
    for n in NS {
        assert_eq!(vm_arith(n), native_arith(n), "arith n={n}");
    }
}

#[test]
fn bitmix_matches_native() {
    for n in NS {
        assert_eq!(vm_bitmix(n), native_bitmix(n), "bitmix n={n}");
    }
}

#[test]
fn sum_matches_native() {
    for n in NS {
        assert_eq!(vm_sum(n), native_sum(n), "sum n={n}");
    }
}

#[test]
fn branchy_matches_native() {
    for n in NS {
        assert_eq!(vm_branchy(n), native_branchy(n), "branchy n={n}");
    }
}

#[test]
fn arrloop_matches_native() {
    for n in NS {
        assert_eq!(vm_arrloop(n), native_arrloop(n), "arrloop n={n}");
    }
}

#[test]
fn writeback_matches_native() {
    for n in NS {
        assert_eq!(vm_writeback(n), native_writeback(n), "writeback n={n}");
    }
}

#[test]
fn fib_matches_native() {
    // 递归深度限制在 16：这是 VM 递归能否走通的探针，不是性能测试。
    for n in [0u64, 1, 2, 5, 10, 16] {
        assert_eq!(vm_fib(n), native_fib(n), "fib n={n}");
    }
}
