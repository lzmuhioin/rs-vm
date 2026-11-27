//! 动态性能剖析：找出真实负载里**实际执行**最多的指令（不是编译期条数）。
//!
//! 运行：`cargo bench --features prof --bench bottleneck`
//!
//! 与 `vm_workloads` / `real_world` 的区别：那两个测「花多长时间」，
//! 这个测「时间花在哪些指令上」——静态条数（`vm-opcode-census.sh`）
//! 回答不了热循环里到底谁在跑。

use aegis_vm::vm_protect;
use std::hint::black_box;
use std::time::Instant;

/// 与被保护的 IPv4 校验和同形态：16 轮循环 + 折叠 + 判定。
#[vm_protect]
fn workload(n: u64) -> u64 {
    let mut sum = 0u64;
    let mut i = 0u64;
    while i < n {
        let w = (i.wrapping_mul(2654435761) & 0xFFFF) ^ 0x1234;
        sum += w;
        i += 1;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum & 0xFFFF
}

fn native(n: u64) -> u64 {
    let mut sum = 0u64;
    let mut i = 0u64;
    while i < n {
        let w = (i.wrapping_mul(2654435761) & 0xFFFF) ^ 0x1234;
        sum += w;
        i += 1;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !sum & 0xFFFF
}

fn main() {
    let n = 16u64;
    let expect = black_box(native(black_box(n)));
    assert_eq!(workload(black_box(n)), expect);

    let iters = 200u64;
    let t = Instant::now();
    for _ in 0..iters {
        black_box(workload(black_box(n)));
    }
    let el = t.elapsed();
    println!(
        "跑 {iters} 次耗时 {:?}（{:.1}ns/次）",
        el,
        el.as_nanos() as f64 / iters as f64
    );

    #[cfg(feature = "prof")]
    {
        // 单个负载的分布：扣掉前面热身那批，重新统计一轮
        aegis_vm::prof::reset();
        for _ in 0..100 {
            black_box(workload(black_box(n)));
        }
        aegis_vm::prof::dump();
    }
    #[cfg(not(feature = "prof"))]
    println!("需要 --features prof 才能统计动态分布");
}
