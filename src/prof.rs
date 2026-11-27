//! 动态指令剖析：统计运行期每条指令真正执行了多少次。
//!
//! 只由 `prof` 特性启用（非默认），普通构建完全不参与编译，零开销。
//! 用法：`AEGIS_PROF=1 cargo bench --features prof --bench real_world`，
//! 或测试里直接调 `prof::dump()`。

use core::cell::RefCell;

thread_local! {
    static HIST: RefCell<[u64; 256]> = const { RefCell::new([0; 256]) };
}

/// 记录一条指令的执行（`opcode` 为解密后的 base opcode）。
#[inline(always)]
pub fn tally(opcode: u8) {
    HIST.with(|h| h.borrow_mut()[opcode as usize] += 1);
}

/// 打印直方图，按次数降序。
pub fn dump() {
    let mut rows: Vec<(u8, u64)> = HIST.with(|h| {
        h.borrow()
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .map(|(op, n)| (op as u8, *n))
            .collect()
    });
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let total: u64 = rows.iter().map(|r| r.1).sum();
    println!("=== 动态执行分布（共 {total} 条）===");
    println!("{:<8}{:>14}{:>9}", "编号", "执行次数", "占比");
    for (op, n) in rows.iter().take(15) {
        println!(
            "0x{op:02X}{:>8}{n:>14}{:>8.1}%",
            "",
            *n as f64 / total as f64 * 100.0
        );
    }
}

/// 清空直方图（每个负载测前调用，避免互相污染）。
pub fn reset() {
    HIST.with(|h| h.borrow_mut().fill(0));
}
