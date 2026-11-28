//! native call 的返回值 ABI：只有「≤8 字节且无需 Drop」的值能跨 64 位通道。
//!
//! 这条边界以前是静默的：>8 字节的返回值会被 `Box::into_raw` 装箱后当 u64 传回去，
//! 既泄漏内存，VM 又把这个 host 指针当成自己的堆地址（实测 E04: E19）。
//! 现在两侧都在编译期拒绝（`__to_u64` 里的 `const { assert!(…) }`）。
//!
//! 本文件只覆盖**允许**的那一侧；禁止的那一侧是编译错误，无法用运行期测试表达
//! （仓库没有 trybuild，`.stderr` 快照会随 rustc 版本腐烂）。它由宏 crate 的
//! `conversion_helper_never_leaks_host_pointers` 锁住生成代码形状。

use aegis_vm::vm_protect;

fn ret_u64(n: u64) -> u64 {
    n + 1
}

fn ret_bool(n: u64) -> bool {
    n > 0
}

fn ret_u8(n: u64) -> u8 {
    n as u8 + 1
}

#[vm_protect(level = "low")]
fn call_u64(n: u64) -> u64 {
    ret_u64(n)
}

#[vm_protect(level = "low")]
fn call_bool(n: u64) -> u64 {
    ret_bool(n) as u64
}

#[vm_protect(level = "low")]
fn call_u8(n: u64) -> u64 {
    ret_u8(n) as u64
}

#[test]
fn eight_byte_returns_round_trip() {
    assert_eq!(call_u64(41), 42);
    assert_eq!(call_bool(1), 1);
    assert_eq!(call_bool(0), 0);
    assert_eq!(call_u8(4), 5);
}
