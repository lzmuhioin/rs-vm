//! Control Flow Handlers
//!
//! CMP, JMP, JZ, JNZ, JGT, JLT, JGE, JLE, CALL, RET

use crate::error::{VmError, VmResult};
use crate::state::VmState;

/// CMP: Compare top two stack values, set flags
pub fn handle_cmp(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    state.update_cmp_flags(a, b);
    // Push values back (CMP doesn't consume)
    state.push(a)?;
    state.push(b)
}

/// JMP: Unconditional jump
pub fn handle_jmp(state: &mut VmState) -> VmResult<()> {
    let offset = state.read_i16()?;
    jump_relative(state, offset)
}

/// JZ: Jump if zero flag set
pub fn handle_jz(state: &mut VmState) -> VmResult<()> {
    let offset = state.read_i16()?;
    if state.is_zero() {
        jump_relative(state, offset)
    } else {
        Ok(())
    }
}

/// JNZ: Jump if zero flag not set
pub fn handle_jnz(state: &mut VmState) -> VmResult<()> {
    let offset = state.read_i16()?;
    if !state.is_zero() {
        jump_relative(state, offset)
    } else {
        Ok(())
    }
}

/// JGT: Jump if greater (signed)
pub fn handle_jgt(state: &mut VmState) -> VmResult<()> {
    let offset = state.read_i16()?;
    // Greater: not zero AND sign == overflow
    if !state.is_zero() && (state.is_negative() == state.is_overflow()) {
        jump_relative(state, offset)
    } else {
        Ok(())
    }
}

/// JLT: Jump if less (signed)
pub fn handle_jlt(state: &mut VmState) -> VmResult<()> {
    let offset = state.read_i16()?;
    // Less: sign != overflow
    if state.is_negative() != state.is_overflow() {
        jump_relative(state, offset)
    } else {
        Ok(())
    }
}

/// JGE: Jump if greater or equal
pub fn handle_jge(state: &mut VmState) -> VmResult<()> {
    let offset = state.read_i16()?;
    // Greater or equal: sign == overflow
    if state.is_negative() == state.is_overflow() {
        jump_relative(state, offset)
    } else {
        Ok(())
    }
}

/// JLE: Jump if less or equal
pub fn handle_jle(state: &mut VmState) -> VmResult<()> {
    let offset = state.read_i16()?;
    // Less or equal: zero OR (sign != overflow)
    if state.is_zero() || (state.is_negative() != state.is_overflow()) {
        jump_relative(state, offset)
    } else {
        Ok(())
    }
}

/// Helper: Jump by relative offset
pub fn jump_relative(state: &mut VmState, offset: i16) -> VmResult<()> {
    let new_ip = if offset >= 0 {
        state.ip.checked_add(offset as usize)
    } else {
        state.ip.checked_sub((-offset) as usize)
    };

    match new_ip {
        Some(ip) if ip <= state.code.len() => {
            state.ip = ip;
            Ok(())
        }
        _ => Err(VmError::InvalidJumpTarget),
    }
}

/// CALL: Call subroutine
pub fn handle_call(state: &mut VmState) -> VmResult<()> {
    let offset = state.read_i16()?;
    // Push return address
    state.call_stack.push(state.ip);
    jump_relative(state, offset)
}

/// RET: Return from subroutine
pub fn handle_ret(state: &mut VmState) -> VmResult<()> {
    match state.call_stack.pop() {
        Some(return_addr) => {
            state.ip = return_addr;
            Ok(())
        }
        None => {
            // Return from main = halt
            state.halted = true;
            state.result = state.peek().unwrap_or(0);
            Ok(())
        }
    }
}

/// CMP_REG_REG_JGE: fused loop condition (superinstruction).
///
/// Encoding: `CMP_REG_REG_JGE <a:u8> <b:u8> <offset:i16>`.
/// Equivalent to `PUSH_REG a; PUSH_REG b; CMP; DROP; DROP; JGE offset`:
/// sets the same flags, leaves the stack untouched, then branches if `a >= b` (signed).
pub fn handle_cmp_reg_reg_jge(state: &mut VmState) -> VmResult<()> {
    let a_idx = state.read_u8()?;
    let b_idx = state.read_u8()?;
    let offset = state.read_i16()?;

    let a = state.get_reg(a_idx)?;
    let b = state.get_reg(b_idx)?;
    state.update_cmp_flags(a, b);

    // Greater or equal: sign == overflow
    if state.is_negative() == state.is_overflow() {
        jump_relative(state, offset)
    } else {
        Ok(())
    }
}
