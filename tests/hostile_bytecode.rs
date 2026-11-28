//! 畸形/恶意字节码的健壮性：地址与尺寸算术溢出不得逃逸为宿主 panic。
//!
//! 每条用例都对应一条实测过的崩溃路径（先 alloc 让 heap 非空，否则 `addr + N`
//! 回绕后仍大于 len，检查会被偶然拦住，测不出真问题）。修复前是 panic，
//! 修复后必须是干净的 `Err`。
//!
//! 为什么要求 `Err` 而不是「不 panic 就行」：这些输入在语义上就是越界/超限，
//! 静默返回 Ok 会让调用方拿着一个不可写的地址继续跑，比直接报错更难查。

use aegis_vm::build_config::opcodes::{control, exec, heap, stack, vector};
use aegis_vm::engine::execute;
use aegis_vm::error::VmError;

/// 堆块头里的「已分配」标志位（state.rs 内部常量）
const ALLOCATED_FLAG: u64 = 0x8000_0000_0000_0000;

fn push_imm(v: u64) -> Vec<u8> {
    let mut c = Vec::with_capacity(9);
    c.push(stack::PUSH_IMM);
    c.extend_from_slice(&v.to_le_bytes());
    c
}

/// 先分配 64 字节，让 `heap.len()` 足够大，后续的溢出地址才有机会绕过检查。
fn alloc_then(rest: Vec<u8>) -> Vec<u8> {
    let mut c = push_imm(64);
    c.push(heap::HEAP_ALLOC);
    c.push(stack::DROP);
    c.extend(rest);
    c.push(exec::HALT);
    c
}

// =============================================================================
// 越界读：addr + N 回绕
// =============================================================================

#[test]
fn heap_load64_rejects_overflowing_address() {
    let code = alloc_then({
        let mut c = push_imm(u64::MAX - 3);
        c.push(heap::HEAP_LOAD64);
        c
    });
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfBounds));
}

#[test]
fn heap_load32_rejects_overflowing_address() {
    let code = alloc_then({
        let mut c = push_imm(u64::MAX - 1);
        c.push(heap::HEAP_LOAD32);
        c
    });
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfBounds));
}

#[test]
fn heap_load16_rejects_overflowing_address() {
    let code = alloc_then({
        let mut c = push_imm(u64::MAX - 1);
        c.push(heap::HEAP_LOAD16);
        c
    });
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfBounds));
}

// =============================================================================
// 越界写：addr + N 回绕
// =============================================================================

#[test]
fn heap_store64_rejects_overflowing_address() {
    let code = alloc_then({
        let mut c = push_imm(u64::MAX - 5);
        c.extend(push_imm(0xDEAD));
        c.push(heap::HEAP_STORE64);
        c
    });
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfBounds));
}

#[test]
fn heap_store32_rejects_overflowing_address() {
    let code = alloc_then({
        let mut c = push_imm(u64::MAX - 2);
        c.extend(push_imm(0xDEAD));
        c.push(heap::HEAP_STORE32);
        c
    });
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfBounds));
}

#[test]
fn heap_store16_rejects_overflowing_address() {
    let code = alloc_then({
        let mut c = push_imm(u64::MAX - 1);
        c.extend(push_imm(0xDEAD));
        c.push(heap::HEAP_STORE16);
        c
    });
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfBounds));
}

// =============================================================================
// 分配尺寸溢出
// =============================================================================

#[test]
fn heap_alloc_rejects_size_that_wraps_after_alignment() {
    // (u64::MAX - 7) + 7 回绕成 0，会被当成「0 字节分配」放行
    let code = {
        let mut c = push_imm(u64::MAX - 7);
        c.push(heap::HEAP_ALLOC);
        c.push(exec::HALT);
        c
    };
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfMemory));
}

#[test]
fn heap_alloc_rejects_size_u64_max() {
    let code = {
        let mut c = push_imm(u64::MAX);
        c.push(heap::HEAP_ALLOC);
        c.push(exec::HALT);
        c
    };
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfMemory));
}

// =============================================================================
// 容器尺寸回绕：header + data 溢出后「分配成功但容量谎报」
// =============================================================================

#[test]
fn vec_new_rejects_capacity_whose_total_size_wraps() {
    // VEC_HEADER_SIZE(24) + u64::MAX 回绕成 23，曾能放行并写入 capacity = u64::MAX
    let code = {
        let mut c = push_imm(u64::MAX);
        c.extend(push_imm(1)); // elem_size
        c.push(vector::VEC_NEW);
        c.push(exec::HALT);
        c
    };
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfMemory));
}

#[test]
fn vec_repeat_rejects_count_whose_total_size_wraps() {
    let code = {
        let mut c = push_imm(0); // value
        c.extend(push_imm(u64::MAX)); // count
        c.extend(push_imm(1)); // elem_size
        c.push(vector::VEC_REPEAT);
        c.push(exec::HALT);
        c
    };
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfMemory));
}

#[test]
fn str_new_rejects_capacity_whose_total_size_wraps() {
    let code = {
        let mut c = push_imm(u64::MAX);
        c.push(aegis_vm::build_config::opcodes::string::STR_NEW);
        c.push(exec::HALT);
        c
    };
    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfMemory));
}

// =============================================================================
// 伪造块头：size 合法但块尾越过 heap_ptr
// =============================================================================

#[test]
fn heap_free_rejects_forged_header_extending_past_heap() {
    // 布局：块1 用户 8（头@0），块2 用户 80（头@72），heap_ptr = 144
    // 把块2 的头改成 ALLOCATED | 80 —— 80 <= heap_ptr 能过旧检查，
    // 但 72 + 80 = 152 > heap_ptr，接受它就会造出越界 free block。
    let mut code = push_imm(64);
    code.push(heap::HEAP_ALLOC);
    code.push(stack::DROP);
    code.extend(push_imm(64));
    code.push(heap::HEAP_ALLOC);
    code.push(stack::DROP);
    // 写伪造 header 到 72
    code.extend(push_imm(72));
    code.extend(push_imm(ALLOCATED_FLAG | 80));
    code.push(heap::HEAP_STORE64);
    // 释放块2 的用户地址 80
    code.extend(push_imm(80));
    code.push(heap::HEAP_FREE);
    code.push(exec::HALT);

    assert_eq!(execute(&code, &[]), Err(VmError::HeapOutOfBounds));
}

// =============================================================================
// 回归护栏：合法访问不能被这次加固误伤
// =============================================================================

#[test]
fn legitimate_heap_access_still_works() {
    let code = {
        let mut c = push_imm(64);
        c.push(heap::HEAP_ALLOC); // addr = 8
        c.push(stack::DUP);
        c.extend(push_imm(0x1122_3344_5566_7788));
        c.push(heap::HEAP_STORE64);
        c.push(heap::HEAP_LOAD64);
        c.push(exec::HALT);
        c
    };
    assert_eq!(execute(&code, &[]), Ok(0x1122_3344_5566_7788));
}

#[test]
fn legitimate_vec_push_pop_still_works() {
    let code = {
        let mut c = push_imm(4); // capacity
        c.extend(push_imm(8)); // elem_size
        c.push(vector::VEC_NEW);
        c.push(stack::DUP);
        c.extend(push_imm(42)); // value
        c.push(vector::VEC_PUSH);
        c.push(vector::VEC_POP);
        c.push(exec::HALT);
        c
    };
    assert_eq!(execute(&code, &[]), Ok(42));
}

// =============================================================================
// 调用栈深度：必须有显式上限
// =============================================================================

#[test]
fn call_stack_depth_is_bounded() {
    // `CALL -3` 跳回自身 ⇒ 无限递归。没有显式上限时，call_stack 会一直涨到
    // MAX_INSTRUCTIONS（1,000,000 条 × 8 字节 ≈ 7.9 MiB）才被指令数限制拦住。
    // 期望：撞到调用深度上限后立刻返回 StackOverflow。
    let code = [control::CALL, 0xFD, 0xFF];
    assert_eq!(execute(&code, &[]), Err(VmError::StackOverflow));
}
