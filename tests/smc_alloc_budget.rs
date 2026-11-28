//! SMC 执行的分配预算：状态必须搬运，不能按指令克隆。
//!
//! 判据（行为，而非实现细节）：同一个程序，`execute_smc` 期间的系统分配总字节数
//! 不能随「堆大小 × 指令数」增长。
//!
//! 背景：`execute_smc_with_natives` 需要执行期间可变地原地改字节码，而 `VmState`
//! 以 `&'a [u8]` 借用代码，所以每条指令都被迫新建一个临时 `VmState`。
//! 修复前它用 `apply_to`/`copy_from`（`clone_from`）把 heap/stack/regs/output
//! 整份拷进拷出 —— 512 KiB 堆 × 200 条指令 ≈ 100 MB 分配，实测慢 174 倍。
//!
//! 全局分配计数器是进程级的，因此本文件只保留一个测试函数（与 `alloc_budget.rs` 同因）。

use aegis_vm::build_config::opcodes::{arithmetic, control, exec, heap, stack};
use aegis_vm::smc::{encrypt_bytecode, execute_smc, SmcConfig};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static BYTES: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        BYTES.fetch_add(new_size, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTER: Counting = Counting;

/// 分配 `heap_bytes` 字节后循环 `loops` 次自增，返回循环计数。
fn program(heap_bytes: u64, loops: u8) -> Vec<u8> {
    let mut c = Vec::new();
    // 占住一块堆内存（只分配不触碰，用来放大每条指令的状态搬运成本）
    c.push(stack::PUSH_IMM);
    c.extend_from_slice(&heap_bytes.to_le_bytes());
    c.push(heap::HEAP_ALLOC);
    c.push(stack::DROP);
    // R0 = 0
    c.extend([stack::PUSH_IMM8, 0, stack::POP_REG, 0]);

    let loop_start = c.len();
    c.extend([stack::PUSH_REG, 0, arithmetic::INC, stack::POP_REG, 0]);
    c.extend([stack::PUSH_REG, 0, stack::PUSH_IMM8, loops, control::CMP]);
    // JLT 回 loop_start
    let off = loop_start as i32 - (c.len() as i32 + 3);
    c.push(control::JLT);
    c.extend_from_slice(&(off as i16).to_le_bytes());
    // 返回 R0
    c.extend([stack::PUSH_REG, 0, exec::HALT]);
    c
}

#[test]
fn smc_execution_does_not_copy_state_per_instruction() {
    let code = program(512 * 1024, 200);
    let cfg = SmcConfig::from_build_seed(42);

    let mut warm = code.clone();
    encrypt_bytecode(&mut warm, &cfg);
    let _ = execute_smc(warm, &[], &cfg);

    let mut enc = code.clone();
    encrypt_bytecode(&mut enc, &cfg);

    BYTES.store(0, Ordering::Relaxed);
    let result = execute_smc(enc, &[], &cfg);
    let used = BYTES.load(Ordering::Relaxed);

    assert_eq!(result, Ok(200), "SMC 结果必须与普通执行一致");
    println!("SMC 单次执行分配 {used} 字节（512 KiB 堆 / 200 条指令）");

    // 512 KiB 堆 × 200 条指令的克隆路径约 100 MB；搬运路径只需常数级。
    // 4 MiB 留出两个数量级的余量，同时远低于克隆路径的下界。
    assert!(
        used < 4 * 1024 * 1024,
        "SMC 一次执行分配了 {used} 字节（预算 4 MiB）。\
         超出说明状态仍在按指令克隆：512 KiB 堆 × 200 指令 ≈ 100 MB"
    );
}
