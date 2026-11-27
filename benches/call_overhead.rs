//! 单次调用固定开销：回答「短函数该不该上 VM」。
//!
//! 三个函数体都只有 `x`（一条取值+返回），差别只在保护级别。测出的 ns/次
//! 就是**每次调用必须付的固定成本**（状态建立 + 解密入口 + 调用约定 + 派发）。
//!
//! 运行：`cargo bench --bench call_overhead`

use aegis_vm::vm_protect;
use std::hint::black_box;
use std::time::Instant;

fn noop_native(x: u64) -> u64 {
    x
}

#[vm_protect]
fn noop_standard(x: u64) -> u64 {
    x
}

#[vm_protect(level = "paranoid")]
fn noop_paranoid(x: u64) -> u64 {
    x
}

fn time_ns<F: FnMut() -> u64>(mut f: F, iters: u64) -> u64 {
    let mut best = u64::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        let mut acc = 0u64;
        for _ in 0..iters {
            acc = acc.wrapping_add(f());
        }
        black_box(acc);
        let el = t.elapsed().as_nanos() as u64;
        if el < best {
            best = el;
        }
    }
    best
}

fn main() {
    let x = black_box(0x1234_5678u64);
    assert_eq!(noop_native(x), noop_standard(x));
    assert_eq!(noop_native(x), noop_paranoid(x));

    let iters = 1000u64;
    let mut nat = u64::MAX;
    let mut std_l = u64::MAX;
    let mut par = u64::MAX;
    for _ in 0..4 {
        nat = nat.min(time_ns(|| noop_native(black_box(x)), iters));
        std_l = std_l.min(time_ns(|| noop_standard(black_box(x)), iters));
        par = par.min(time_ns(|| noop_paranoid(black_box(x)), iters));
    }

    let per = |t: u64| t as f64 / iters as f64;
    println!(
        "{:<12}{:>12}{:>10}",
        "版本", "ns/次", "相对原生"
    );
    println!("{}", "-".repeat(36));
    for (n, t) in [("原生", nat), ("Standard", std_l), ("Paranoid", par)] {
        println!("{:<12}{:>12.1}{:>9.0}x", n, per(t), per(t) / per(nat));
    }
    println!();
    println!(
        "=> 每次调用的固定开销：Standard ≈ {:.2}µs，Paranoid ≈ {:.2}µs",
        (std_l - nat) as f64 / 1000.0 / iters as f64,
        (par - nat) as f64 / 1000.0 / iters as f64
    );
}
