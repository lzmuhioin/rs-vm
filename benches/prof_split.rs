//! 把栈搬运按「函数形态」拆分，决定寄存器分配器先做哪一块。
//!
//! 运行：`cargo bench --features prof --bench prof_split`

use aegis_vm::vm_protect;
use std::hint::black_box;

/// 形态 A：纯表达式，无循环、无变量读写——M1（表达式内分配）能吃到的部分。
#[vm_protect]
fn expr_only(n: u64) -> u64 {
    ((n.wrapping_mul(3) + 7) ^ (n >> 2)).wrapping_mul(5) & 0xFFFF
}

/// 形态 B：循环 + 局部变量读写为主，表达式很浅——M2（变量常驻寄存器）的地盘。
#[vm_protect]
fn locals_heavy(n: u64) -> u64 {
    let mut a = n;
    let mut b = n ^ 0x55;
    let mut i = 0u64;
    while i < 8 {
        a = a + b;
        b = b ^ a;
        i = i + 1;
    }
    a ^ b
}

/// 形态 C：真实负载形态（循环 + 中间表达式），两者混合。
#[vm_protect]
fn mixed(n: u64) -> u64 {
    let mut sum = 0u64;
    let mut i = 0u64;
    while i < n {
        let w = (i.wrapping_mul(2654435761) & 0xFFFF) ^ 0x1234;
        sum += w;
        i += 1;
    }
    !sum & 0xFFFF
}

fn measure(name: &str, f: fn(u64) -> u64) {
    let n = 8u64;
    black_box(f(black_box(n)));
    #[cfg(feature = "prof")]
    {
        aegis_vm::prof::reset();
        for _ in 0..100 {
            black_box(f(black_box(n)));
        }
        let (moves, total) = aegis_vm::prof::stack_share();
        println!(
            "{name:<12} 搬运 {moves:>6} / 总计 {total:>6}  = {:>5.1}%   （每次调用 {:.0} 条）",
            moves as f64 / total as f64 * 100.0,
            total as f64 / 100.0
        );
    }
    #[cfg(not(feature = "prof"))]
    println!("{name}: 需要 --features prof");
}

fn main() {
    measure("expr_only", expr_only);
    measure("locals_heavy", locals_heavy);
    measure("mixed", mixed);
}
