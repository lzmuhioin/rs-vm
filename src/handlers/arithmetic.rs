//! Arithmetic Operation Handlers
//!
//! ADD, SUB, MUL, XOR, AND, OR, SHL, SHR, NOT, ROL, ROR, INC, DEC, DIV, MOD, IDIV, IMOD

use crate::error::VmResult;
use crate::state::VmState;

/// ADD: Pop 2, push sum
pub fn handle_add(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a.wrapping_add(b);
    state.set_zero_flag(result);
    state.push(result)
}

/// SUB: Pop 2, push difference
pub fn handle_sub(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a.wrapping_sub(b);
    state.set_zero_flag(result);
    state.push(result)
}

/// MUL: Pop 2, push product
pub fn handle_mul(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a.wrapping_mul(b);
    state.set_zero_flag(result);
    state.push(result)
}

/// XOR: Pop 2, push XOR
pub fn handle_xor(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a ^ b;
    state.set_zero_flag(result);
    state.push(result)
}

/// AND: Pop 2, push AND
pub fn handle_and(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a & b;
    state.set_zero_flag(result);
    state.push(result)
}

/// OR: Pop 2, push OR
pub fn handle_or(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a | b;
    state.set_zero_flag(result);
    state.push(result)
}

/// SHL: Pop 2, push left shift
pub fn handle_shl(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a.wrapping_shl(b as u32);
    state.set_zero_flag(result);
    state.push(result)
}

/// SHR: Pop 2, push right shift
pub fn handle_shr(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a.wrapping_shr(b as u32);
    state.set_zero_flag(result);
    state.push(result)
}

/// NOT: Pop 1, push bitwise NOT
pub fn handle_not(state: &mut VmState) -> VmResult<()> {
    let a = state.pop()?;
    let result = !a;
    state.set_zero_flag(result);
    state.push(result)
}

/// ROL: Rotate left
pub fn handle_rol(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a.rotate_left(b as u32);
    state.push(result)
}

/// ROR: Rotate right
pub fn handle_ror(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a.rotate_right(b as u32);
    state.push(result)
}

/// INC: Increment top of stack
pub fn handle_inc(state: &mut VmState) -> VmResult<()> {
    let a = state.pop()?;
    let result = a.wrapping_add(1);
    state.set_zero_flag(result);
    state.push(result)
}

/// DEC: Decrement top of stack
pub fn handle_dec(state: &mut VmState) -> VmResult<()> {
    let a = state.pop()?;
    let result = a.wrapping_sub(1);
    state.set_zero_flag(result);
    state.push(result)
}

/// DIV: Unsigned division (a / b), division by zero returns 0
pub fn handle_div(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = a.checked_div(b).unwrap_or(0);
    state.set_zero_flag(result);
    state.push(result)
}

/// MOD: Unsigned modulo (a % b), division by zero returns 0
pub fn handle_mod(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()?;
    let a = state.pop()?;
    let result = if b == 0 { 0 } else { a % b };
    state.set_zero_flag(result);
    state.push(result)
}

/// IDIV: Signed division ((a as i64) / (b as i64))
pub fn handle_idiv(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()? as i64;
    let a = state.pop()? as i64;
    let result = if b == 0 { 0 } else { (a / b) as u64 };
    state.set_zero_flag(result);
    state.push(result)
}

/// IMOD: Signed modulo ((a as i64) % (b as i64))
pub fn handle_imod(state: &mut VmState) -> VmResult<()> {
    let b = state.pop()? as i64;
    let a = state.pop()? as i64;
    let result = if b == 0 { 0 } else { (a % b) as u64 };
    state.set_zero_flag(result);
    state.push(result)
}

/// INC_REG: fused `PUSH_REG r; INC; POP_REG r` (superinstruction).
///
/// Encoding: `INC_REG <reg:u8>`. Increments the register in place and sets the zero
/// flag from the result, matching `INC`'s flag behaviour (ZF only).
pub fn handle_inc_reg(state: &mut VmState) -> VmResult<()> {
    let idx = state.read_u8()?;
    let value = state.get_reg(idx)?.wrapping_add(1);
    state.set_reg(idx, value)?;
    state.set_zero_flag(value);
    Ok(())
}

/// ADD_REG_IMM: `r = r + imm` (fused, ZF only).
pub fn handle_add_reg_imm(state: &mut VmState) -> VmResult<()> {
    let idx = state.read_u8()?;
    let imm = state.read_u32()? as u64;
    let result = state.get_reg(idx)?.wrapping_add(imm);
    state.set_reg(idx, result)?;
    state.set_zero_flag(result);
    Ok(())
}

/// ADD_REG_REG: `dst = a + b` (fused, ZF only).
pub fn handle_add_reg_reg(state: &mut VmState) -> VmResult<()> {
    let dst = state.read_u8()?;
    let a = state.read_u8()?;
    let b = state.read_u8()?;
    let result = state.get_reg(a)?.wrapping_add(state.get_reg(b)?);
    state.set_reg(dst, result)?;
    state.set_zero_flag(result);
    Ok(())
}

/// MUL_REG_IMM: `r = r * imm` (fused, ZF only).
pub fn handle_mul_reg_imm(state: &mut VmState) -> VmResult<()> {
    let idx = state.read_u8()?;
    let imm = state.read_u32()? as u64;
    let result = state.get_reg(idx)?.wrapping_mul(imm);
    state.set_reg(idx, result)?;
    state.set_zero_flag(result);
    Ok(())
}

/// MUL_REG_REG: `dst = a * b` (fused, ZF only).
pub fn handle_mul_reg_reg(state: &mut VmState) -> VmResult<()> {
    let dst = state.read_u8()?;
    let a = state.read_u8()?;
    let b = state.read_u8()?;
    let result = state.get_reg(a)?.wrapping_mul(state.get_reg(b)?);
    state.set_reg(dst, result)?;
    state.set_zero_flag(result);
    Ok(())
}
