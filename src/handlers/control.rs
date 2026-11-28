//! Control Flow Handlers
//!
//! CMP, JMP, JZ, JNZ, JGT, JLT, JGE, JLE, CALL, RET

use crate::error::{VmError, VmResult};
use crate::state::{VmState, MAX_CALL_DEPTH};

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
    if state.call_stack.len() >= MAX_CALL_DEPTH {
        return Err(VmError::StackOverflow);
    }
    let return_addr = state.ip;
    jump_relative(state, offset)?;
    // 跳转有效才记账：否则一次失败的 CALL 会在 call_stack 上留一条死记录，
    // 既占用深度配额，也让 RET 跳到错误的地址
    state.call_stack.push(return_addr);
    Ok(())
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

/// CMP_REG_REG_JCC: fused compare-and-branch with a condition selector.
///
/// Encoding: `CMP_REG_REG_JCC <a:u8> <b:u8> <cond:u8> <offset:i16>`.
/// Sets the same flags as `CMP a, b` and branches when `cond` holds:
/// 0=eq, 1=ne, 2=gt, 3=lt, 4=ge, 5=le (signed).
pub fn handle_cmp_reg_reg_jcc(state: &mut VmState) -> VmResult<()> {
    let a_idx = state.read_u8()?;
    let b_idx = state.read_u8()?;
    let cond = state.read_u8()?;
    let offset = state.read_i16()?;

    let a = state.get_reg(a_idx)?;
    let b = state.get_reg(b_idx)?;
    state.update_cmp_flags(a, b);

    let zero = state.is_zero();
    let sign_ne_overflow = state.is_negative() != state.is_overflow();

    let taken = match cond {
        0 => zero,                          // eq
        1 => !zero,                         // ne
        2 => !zero && !sign_ne_overflow,    // gt
        3 => sign_ne_overflow,              // lt
        4 => !sign_ne_overflow,             // ge
        5 => zero || sign_ne_overflow,      // le
        _ => return Err(VmError::InvalidOpcode),
    };

    if taken {
        jump_relative(state, offset)
    } else {
        Ok(())
    }
}
