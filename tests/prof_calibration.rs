//! 测量工具自证：`prof` 直方图总数必须等于解释器自己的 `instruction_count`。
//!
//! 为什么需要这个文件：`stack_share()` 的数曾被当作「已执行指令数」用来判性能收益，
//! 但当时 `run_with_natives` 这条执行循环**根本没有计数**——任何走这条路的负载都会
//! 静默统计成 0（或被当作「零栈操作」），而结论照样被采信。
//! 这里用解释器同一循环里自增的 `instruction_count` 作为真值，把计数器钉死。
//!
//! 运行：`cargo test --features prof --test prof_calibration`

#![cfg(feature = "prof")]

use aegis_vm::{
    build_config::opcodes::{arithmetic, exec, stack},
    bytecode::{BytecodeFlags, BytecodeHeader, BytecodePackage},
    crypto::CryptoContext,
    execute_with_state, prof,
};

/// 手写 4 条指令的程序：`40 + 2 = 42`，然后 HALT。
///
/// 用它做标尺而不随便找个负载：指令条数是可数的（4 条），
/// 一旦计数器多算/少算，等式立刻不成立。
#[test]
fn prof_total_equals_instruction_count() {
    let bytecode = [
        stack::PUSH_IMM8,
        40,
        stack::PUSH_IMM8,
        2,
        arithmetic::ADD,
        exec::HALT,
    ];

    let seed = [0x42u8; 32];
    let mut ctx = CryptoContext::new(seed);
    let (ciphertext, nonce, tag) = ctx.encrypt(&bytecode).expect("加密");

    let mut header = BytecodeHeader::new(ctx.build_id, 0, BytecodeFlags::Encrypted as u16);
    header.nonce = nonce;
    header.tag = tag;
    header.code_len = ciphertext.len() as u32;
    let package = BytecodePackage {
        header,
        code: ciphertext,
    };

    let code = ctx
        .decrypt(&package.code, &package.header.nonce, &package.header.tag)
        .expect("解密");

    prof::reset();
    let state = execute_with_state(&code, &[]).expect("执行");

    assert!(
        state.instruction_count > 0,
        "解释器自记 0 条，说明测试程序本身没跑起来"
    );
    assert_eq!(
        prof::total(),
        state.instruction_count,
        "prof 统计 {} 条，解释器自记 {} 条 —— 计数器与实际执行不一致",
        prof::total(),
        state.instruction_count
    );
}

/// 反复执行同一程序：计数必须与执行次数严格成比例（证明统计不漏、不重）。
#[test]
fn prof_total_scales_with_call_count() {
    let bytecode = [
        stack::PUSH_IMM8,
        7,
        stack::PUSH_IMM8,
        5,
        arithmetic::MUL,
        exec::HALT,
    ];

    let seed = [0x11u8; 32];
    let mut ctx = CryptoContext::new(seed);
    let (ciphertext, nonce, tag) = ctx.encrypt(&bytecode).expect("加密");

    let mut header = BytecodeHeader::new(ctx.build_id, 0, BytecodeFlags::Encrypted as u16);
    header.nonce = nonce;
    header.tag = tag;
    header.code_len = ciphertext.len() as u32;
    let package = BytecodePackage {
        header,
        code: ciphertext,
    };
    let code = ctx
        .decrypt(&package.code, &package.header.nonce, &package.header.tag)
        .expect("解密");

    prof::reset();
    let once = {
        let s = execute_with_state(&code, &[]).expect("执行");
        assert_eq!(prof::total(), s.instruction_count);
        s.instruction_count
    };

    for _ in 0..9 {
        execute_with_state(&code, &[]).expect("执行");
    }
    assert_eq!(
        prof::total(),
        once * 10,
        "执行 10 次后计数应为单次的 10 倍，实际 {} 条（单次 {once} 条）",
        prof::total()
    );
}

/// `total_without_junk()` 必须严格等于「总数 − NOP 计数」。
/// 它存在的理由是把注入噪声里**唯一能单独认出来的那部分**扣掉；
/// 这条用例钉住它的定义，防止以后有人把它改成「减掉所有可疑指令」。
#[test]
fn total_without_junk_subtracts_only_nops() {
    let bytecode = [
        stack::PUSH_IMM8,
        1,
        stack::PUSH_IMM8,
        2,
        arithmetic::ADD,
        exec::HALT,
    ];

    let seed = [0x7Au8; 32];
    let mut ctx = CryptoContext::new(seed);
    let (ciphertext, nonce, tag) = ctx.encrypt(&bytecode).expect("加密");
    let mut header = BytecodeHeader::new(ctx.build_id, 0, BytecodeFlags::Encrypted as u16);
    header.nonce = nonce;
    header.tag = tag;
    header.code_len = ciphertext.len() as u32;
    let package = BytecodePackage {
        header,
        code: ciphertext,
    };
    let code = ctx
        .decrypt(&package.code, &package.header.nonce, &package.header.tag)
        .expect("解密");

    prof::reset();
    execute_with_state(&code, &[]).expect("执行");

    let nops = prof::count(aegis_vm::build_config::opcodes::special::NOP);
    assert_eq!(prof::total_without_junk(), prof::total() - nops);
}
