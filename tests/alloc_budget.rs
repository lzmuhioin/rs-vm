//! 热路径分配预算：`execute()` 每次调用的堆分配次数必须保持在预算内。
//!
//! 判据（行为，而非实现细节）：一次 `execute()` 只应承担一次 VM 执行本身的状态开销，
//! 不该额外重建进程级常量结构，也不该为热路径用不到的容量支付分配。
//! 用计数分配器测量，与机器快慢无关，可在 CI 稳定运行。
//!
//! 注意：全局分配器计数器是进程级的，因此本文件只保留**一个**测试函数——
//! cargo test 并行执行时，两个测试会互相污染计数（实测读到 9.5–10 次而非 5 次）。

use aegis_vm::build_config::opcodes::{exec, stack};
use aegis_vm::execute;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static ALLOCS: AtomicUsize = AtomicUsize::new(0);

struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static COUNTER: Counting = Counting;

fn allocs_per_call(code: &[u8], warmup: usize, calls: usize) -> f64 {
    for _ in 0..warmup {
        let _ = execute(code, &[]);
    }
    let before = ALLOCS.load(Ordering::Relaxed);
    for _ in 0..calls {
        assert_eq!(execute(code, &[]).unwrap(), 42);
    }
    (ALLOCS.load(Ordering::Relaxed) - before) as f64 / calls as f64
}

#[test]
fn execute_hot_path_allocation_budget() {
    // PUSH_IMM8 42; HALT —— 只有 2 条指令，分配次数几乎全部来自每次调用的固定结构。
    let code = [stack::PUSH_IMM8, 42, exec::HALT];
    let per_call = allocs_per_call(&code, 50, 200);

    // 预算 3 = 寄存器文件 + 值栈（热路径必用）两个 Vec，外加最多 1 次增长。
    // 反例基线：每次重建 256 项 NativeRegistry = 6 次；VmState 再预分配 heap/free_list/call_stack = 5 次。
    assert!(
        per_call <= 3.0,
        "每次 execute() 产生 {per_call:.1} 次堆分配；预算 3（regs + stack）"
    );
}