//! VM Execution Engine
//!
//! Main dispatch loop using indirect threading (function pointer table)
//! This eliminates the switch-case pattern visible in binary analysis.

use crate::error::{VmError, VmResult};
use crate::native::NativeRegistry;
use crate::state::{VmState, MAX_INSTRUCTIONS};

// Indirect dispatch via function pointer table
use crate::handlers::dispatch::dispatch_indirect;

/// Execute bytecode with given input, return result
pub fn execute(code: &[u8], input: &[u8]) -> VmResult<u64> {
    let mut state = VmState::new(code, input);
    run(&mut state)?;
    Ok(state.result)
}

/// Execute bytecode with native function registry
pub fn execute_with_natives(code: &[u8], input: &[u8], registry: &NativeRegistry) -> VmResult<u64> {
    let mut state = VmState::new(code, input);
    run_with_natives(&mut state, registry)?;
    Ok(state.result)
}

/// Execute bytecode with native function table (array of function pointers)
/// This is used by the vm_protect macro for compiled native calls
pub fn execute_with_native_table(code: &[u8], input: &[u8], native_table: &[fn(&[u64]) -> u64]) -> VmResult<u64> {
    let mut state = VmState::new(code, input);
    state.set_native_table(native_table);
    run_with_native_table(&mut state)?;
    Ok(state.result)
}

/// Main execution loop with native function table support
pub fn run_with_native_table(state: &mut VmState) -> VmResult<()> {
    let empty_registry = crate::native::empty_registry();
    while !state.halted && state.ip < state.code.len() {
        // Instruction count limit
        state.instruction_count += 1;
        if state.instruction_count > MAX_INSTRUCTIONS {
            return Err(VmError::MaxInstructionsExceeded);
        }

        // Fetch opcode
        // release 下不定义，避免 clippy 报「未使用变量」
        #[cfg(debug_assertions)]
        let trace_ip = state.ip;
        let opcode = state.read_u8()?;
        // 全指令跟踪（仅 debug）：`AEGIS_TRACEALL=1` 时把每条**已执行**指令和当时的栈深打出来。
        // 为什么不只用环形缓冲：环缓冲只记压栈点，看不出「哪一条之后深度不降」。
        // 小 n（4~6）时整段执行流只有几十条，直接读即可，不需要任何关于语义的假设。
        #[cfg(debug_assertions)]
        if std::env::var_os("AEGIS_TRACEALL").is_some() {
            eprintln!("TRACE ip={trace_ip:>4} op={opcode:02X} sp={}", state.stack.len());
        }

        // 动态剖析：只统计，不改执行路径。
        #[cfg(feature = "prof")]
        crate::prof::tally(crate::build_config::OPCODE_DECODE[opcode as usize]);

        // Indirect dispatch via function pointer table
        dispatch_indirect(state, opcode, empty_registry)?;
    }

    Ok(())
}

/// Execute bytecode, return full state (for debugging)
pub fn execute_with_state<'a>(code: &'a [u8], input: &'a [u8]) -> VmResult<VmState<'a>> {
    let mut state = VmState::new(code, input);
    run(&mut state)?;
    Ok(state)
}

/// Main execution loop (without native functions)
pub fn run(state: &mut VmState) -> VmResult<()> {
    let empty_registry = crate::native::empty_registry();
    run_with_natives(state, empty_registry)
}

/// Main execution loop with native function support
/// Uses indirect threading (function pointer table) for opcode dispatch
pub fn run_with_natives(state: &mut VmState, registry: &NativeRegistry) -> VmResult<()> {
    while !state.halted && state.ip < state.code.len() {
        // Instruction count limit
        state.instruction_count += 1;
        if state.instruction_count > MAX_INSTRUCTIONS {
            return Err(VmError::MaxInstructionsExceeded);
        }

        // Fetch opcode
        let opcode = state.read_u8()?;
        // 动态剖析：两条执行循环都必须计数，否则走 run_with_natives 的负载
        // 会静默统计成 0，测出来的「指令数」根本不是真实执行条数。
        #[cfg(feature = "prof")]
        crate::prof::tally(crate::build_config::OPCODE_DECODE[opcode as usize]);


        // Indirect dispatch via function pointer table
        // This replaces the switch-case pattern for better obfuscation
        dispatch_indirect(state, opcode, registry)?;
    }

    Ok(())
}

