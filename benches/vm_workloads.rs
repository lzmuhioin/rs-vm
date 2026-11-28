//! VM 负载基准（无外部依赖，`harness = false`）。
//!
//! 目的：把「保护后 VM 执行 vs 同语义原生执行」的倍率固化进仓库，
//! 让每次改动的数字可复现、可对比，而不是依赖仓库外的临时探针。
//!
//! 运行：`cargo bench --bench vm_workloads`
//!
//! ⚠ 与 `cargo test` 必须用同一个 `ANTICHEAT_BUILD_KEY`（或都不用）。用不同 key 构建会让
//! build.rs 写入的 seed 与宏侧产物错位，随后跑测试会报 `E13`（字节码解密失败）；
//! 出现时 `cargo clean -p aegis_vm -p aegis_vm_macro` 再跑即可。
//!
//! 口径：
//! - 每个负载先跑原生孪生体，再跑 `#[vm_protect]` 版本，取多轮最小值；
//! - 倍率 = VM 每轮耗时 / 原生每轮耗时（同机对比，跨机只比相对变化）；
//! - 结果必须与原生一致，`assert_eq!` 不一致直接 panic —— 防止在测一个算错的实现。

use std::hint::black_box;
use std::time::Instant;

use aegis_vm::vm_protect;

// ============================================================================
// 负载：VM 版本
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
// 负载：原生孪生体（同语义，用作倍率分母）
// ============================================================================

fn native_arith(n: u64) -> u64 {
    let mut a = n;
    let mut b = n + 1;
    let mut c = n + 2;
    let mut d = n + 3;
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
        black_box((a, b, c, d));
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
        black_box(x);
        i += 1;
    }
    x
}

fn native_sum(n: u64) -> u64 {
    let mut s = 0u64;
    let mut i = 0u64;
    while i < n {
        s = s.wrapping_add(black_box(i));
        black_box(s);
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
        black_box(s);
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
        black_box(s);
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
        black_box(&buf);
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
// 计时
// ============================================================================

const ROUNDS: usize = 5;

/// 取多轮最小值，返回纳秒
fn time_ns<F: FnMut() -> u64>(mut f: F) -> f64 {
    f(); // 预热（首次调用含惰性初始化）
    let mut best = f64::MAX;
    for _ in 0..ROUNDS {
        let t0 = Instant::now();
        black_box(f());
        let ns = t0.elapsed().as_nanos() as f64;
        if ns < best {
            best = ns;
        }
    }
    best
}

fn bench_one(name: &str, n: u64, vm: fn(u64) -> u64, native: fn(u64) -> u64) {
    let expected = native(black_box(n));
    let actual = vm(black_box(n));
    assert_eq!(actual, expected, "{name}: VM 结果与原生不一致");

    let native_ns = time_ns(|| native(black_box(n)));
    let vm_ns = time_ns(|| vm(black_box(n)));
    // n 轮迭代，换算成「每次循环迭代」的耗时，便于跨负载比较
    let per_iter = if n == 0 { 1.0 } else { n as f64 };
    println!(
        "{name:<10} n={n:<6} vm={:>10.1}ns native={:>8.1}ns ratio={:>7.1}x  vm/iter={:>7.2}ns",
        vm_ns,
        native_ns,
        vm_ns / native_ns,
        vm_ns / per_iter
    );
}

fn main() {
    println!("VM 负载基准（多轮取最小，ratio = VM / 原生，同机可比）");
    bench_one("arith", 1000, vm_arith, native_arith);
    bench_one("bitmix", 1000, vm_bitmix, native_bitmix);
    bench_one("sum", 1000, vm_sum, native_sum);
    bench_one("branchy", 1000, vm_branchy, native_branchy);
    bench_one("arrloop", 1000, vm_arrloop, native_arrloop);
    bench_one("writeback", 500, vm_writeback, native_writeback);
    bench_one("fib", 20, vm_fib, native_fib);
}
