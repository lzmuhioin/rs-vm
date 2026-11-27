//! 真实负载验证：不是玩具探针，而是「像真实被保护代码」的函数——
//! IPv4 首部校验和 + 分片/跳数判定（循环、分支、字节运算、常量、数组下标都齐）。
//!
//! 三个版本编在同一个二进制里：
//! - 原生（不保护），作为基准；
//! - `#[vm_protect]`（Standard 级别）；
//! - `#[vm_protect(level = "paranoid")]`（Paranoid 级别，`x = x + 1` 会回退到被混淆的栈式形态）。
//!
//! 先断言三者结果一致（否则量的是错的实现），再计 ns/次。
//!
//! 运行：`cargo bench --bench real_world`
//! ⚠ 与 `cargo test` 用同一个构建环境；换 `ANTICHEAT_BUILD_KEY` 后跑测试会报 E13。

use aegis_vm::vm_protect;
use std::hint::black_box;
use std::time::Instant;

/// IPv4 首部校验和 + 基本合法性判定。返回 (校验和, 是否合法)。
fn inspect_native(w0: u64, w1: u64, w2: u64, w3: u64) -> u64 {
    let mut sum: u64 = 0;
    let mut i = 0u64;
    while i < 16 {
        let w = if i < 4 {
            (w0 >> (i * 16)) & 0xFFFF
        } else if i < 8 {
            (w1 >> ((i - 4) * 16)) & 0xFFFF
        } else if i < 12 {
            (w2 >> ((i - 8) * 16)) & 0xFFFF
        } else {
            (w3 >> ((i - 12) * 16)) & 0xFFFF
        };
        sum += w;
        i += 1;
    }
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let checksum = (!sum) & 0xFFFF;

    let version = (w0 >> 60) & 0xF;
    let ttl = (w1 >> 32) & 0xFF;
    let proto = (w1 >> 40) & 0xFF;
    let frag = (w0 >> 32) & 0x1FFF;
    let mut score = 0u64;
    if version == 4 {
        score += 3;
    }
    if ttl > 60 {
        score += 1;
    }
    if ttl == 0 {
        score -= 1;
    }
    if proto == 6 {
        score += 2;
    } else if proto == 17 {
        score += 1;
    }
    if frag != 0 {
        score &= 0xFFFF;
    }
    (checksum << 16) | score
}

#[vm_protect]
fn inspect_standard(w0: u64, w1: u64, w2: u64, w3: u64) -> u64 {
    let mut sum: u64 = 0;
    let mut i = 0u64;
    while i < 16 {
        let w = if i < 4 {
            (w0 >> (i * 16)) & 0xFFFF
        } else if i < 8 {
            (w1 >> ((i - 4) * 16)) & 0xFFFF
        } else if i < 12 {
            (w2 >> ((i - 8) * 16)) & 0xFFFF
        } else {
            (w3 >> ((i - 12) * 16)) & 0xFFFF
        };
        sum += w;
        i += 1;
    }
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let checksum = (!sum) & 0xFFFF;

    let version = (w0 >> 60) & 0xF;
    let ttl = (w1 >> 32) & 0xFF;
    let proto = (w1 >> 40) & 0xFF;
    let frag = (w0 >> 32) & 0x1FFF;
    let mut score = 0u64;
    if version == 4 {
        score += 3;
    }
    if ttl > 60 {
        score += 1;
    }
    if ttl == 0 {
        score -= 1;
    }
    if proto == 6 {
        score += 2;
    } else if proto == 17 {
        score += 1;
    }
    if frag != 0 {
        score &= 0xFFFF;
    }
    (checksum << 16) | score
}

#[vm_protect(level = "paranoid")]
fn inspect_paranoid(w0: u64, w1: u64, w2: u64, w3: u64) -> u64 {
    let mut sum: u64 = 0;
    let mut i = 0u64;
    while i < 16 {
        let w = if i < 4 {
            (w0 >> (i * 16)) & 0xFFFF
        } else if i < 8 {
            (w1 >> ((i - 4) * 16)) & 0xFFFF
        } else if i < 12 {
            (w2 >> ((i - 8) * 16)) & 0xFFFF
        } else {
            (w3 >> ((i - 12) * 16)) & 0xFFFF
        };
        sum += w;
        i += 1;
    }
    while sum > 0xFFFF {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    let checksum = (!sum) & 0xFFFF;

    let version = (w0 >> 60) & 0xF;
    let ttl = (w1 >> 32) & 0xFF;
    let proto = (w1 >> 40) & 0xFF;
    let frag = (w0 >> 32) & 0x1FFF;
    let mut score = 0u64;
    if version == 4 {
        score += 3;
    }
    if ttl > 60 {
        score += 1;
    }
    if ttl == 0 {
        score -= 1;
    }
    if proto == 6 {
        score += 2;
    } else if proto == 17 {
        score += 1;
    }
    if frag != 0 {
        score &= 0xFFFF;
    }
    (checksum << 16) | score
}

fn sample_words() -> [u64; 4] {
    [0x4500_003C_4011_0000, 0x4006_0000_0A00_0001, 0x0A00_0002_1234_5678, 0x9ABC_DEF0_1122_3344]
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
    let w = sample_words();

    // 先对账：三者必须算出同一个结果，否则后面测的是错的实现
    let base = inspect_native(w[0], w[1], w[2], w[3]);
    assert_eq!(base, inspect_standard(w[0], w[1], w[2], w[3]), "Standard 与原生结果不一致");
    assert_eq!(base, inspect_paranoid(w[0], w[1], w[2], w[3]), "Paranoid 与原生结果不一致");
    println!("对账通过：结果=0x{base:X}");
    println!();

    let iters = 200u64;
    let n_repeat = 3;
    let mut nat = u64::MAX;
    let mut std_l = u64::MAX;
    let mut par = u64::MAX;
    for _ in 0..n_repeat {
        nat = nat.min(time_ns(|| inspect_native(black_box(w[0]), black_box(w[1]), black_box(w[2]), black_box(w[3])), iters));
        std_l = std_l.min(time_ns(|| inspect_standard(black_box(w[0]), black_box(w[1]), black_box(w[2]), black_box(w[3])), iters));
        par = par.min(time_ns(|| inspect_paranoid(black_box(w[0]), black_box(w[1]), black_box(w[2]), black_box(w[3])), iters));
    }

    let per = |tot: u64| tot as f64 / iters as f64;
    println!(
        "{:<12}{:>12}{:>12}{:>10}",
        "版本", "总时间ns", "ns/次", "相对原生"
    );
    println!("{}", "-".repeat(48));
    for (name, tot) in [
        ("原生", nat),
        ("Standard", std_l),
        ("Paranoid", par),
    ] {
        println!(
            "{:<12}{:>12}{:>12.1}{:>9.1}x",
            name,
            tot,
            per(tot),
            per(tot) / per(nat)
        );
    }
}
