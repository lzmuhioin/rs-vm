//! Bytecode Emission Helpers
//!
//! All emit_* functions for generating bytecode with optional obfuscation.

use super::Compiler;
use crate::opcodes::{stack, arithmetic, control, native, vector, string, heap, convert, special, register};
use crate::substitution::{
    IncSubstitution, NotSubstitution,
    AndSubstitution, OrSubstitution, ConstantSubstitution, ZeroSubstitution,
    AddSubstitution, SubSubstitution, DeadCodeInsertion,
    ControlFlowSubstitution,
};

impl Compiler {
    /// Emit an opcode (automatically encoded via shuffle table)
    pub(crate) fn emit_op(&mut self, base_opcode: u8) {
        // Dead Code Insertion
        if self.subst.is_enabled() {
            let table = self.opcode_table.clone();
            let encode = |op: u8| table.encode(op);
            DeadCodeInsertion::emit_deterministic(
                self.bytecode.len(),
                &mut self.bytecode,
                &encode,
            );
        }

        // Junk Code Insertion (10% chance for NOP)
        let entropy = (self.bytecode.len() as u64)
            .wrapping_mul(0x5deece66d)
            .wrapping_add(0xb);

        if (entropy % 100) < 10 {
            let nop_shuffled = self.opcode_table.encode(special::NOP);
            self.bytecode.push(nop_shuffled);
        }

        let shuffled = self.opcode_table.encode(base_opcode);
        self.bytecode.push(shuffled);
    }

    /// Emit a single byte
    pub(crate) fn emit(&mut self, byte: u8) {
        self.bytecode.push(byte);
    }

    /// Emit a u16 (little-endian)
    pub(crate) fn emit_u16(&mut self, value: u16) {
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit jump with fixup
    pub(crate) fn emit_jump(&mut self, opcode: u8, label: &str) {
        // Opaque predicate injection (~15% chance)
        if self.opaque_predicates_enabled && self.should_inject_opaque() {
            let table = self.opcode_table.clone();
            let encode = |op: u8| table.encode(op);
            ControlFlowSubstitution::emit_fake_conditional(
                &mut self.subst,
                &mut self.bytecode,
                &encode,
            );
        }

        self.emit_op(opcode);
        let fixup_pos = self.pos();
        self.emit_u16(0); // Placeholder
        self.fixups.push((fixup_pos, label.to_string()));
    }

    /// Emit fused loop condition: `CMP_REG_REG_JGE <a> <b> <i16 offset>`
    ///
    /// Equivalent to `PUSH_REG a; PUSH_REG b; CMP; DROP; DROP; JGE label`
    /// (stack-neutral, same flags) in a single dispatch.
    pub(crate) fn emit_cmp_reg_reg_jge(&mut self, a: u8, b: u8, label: &str) {
        self.emit_op(control::CMP_REG_REG_JGE);
        self.emit(a);
        self.emit(b);
        let fixup_pos = self.pos();
        self.emit_u16(0); // Placeholder
        self.fixups.push((fixup_pos, label.to_string()));
    }

    /// Emit fused compare-and-branch: `CMP_REG_REG_JCC <a> <b> <cond> <i16 offset>`
    /// `VEC_GET_REG <dst> <vec> <idx>`（栈中性）
    pub(crate) fn emit_vec_get_reg(&mut self, dst: u8, vec_reg: u8, idx_reg: u8) {
        self.emit_op(control::VEC_GET_REG);
        self.emit(dst);
        self.emit(vec_reg);
        self.emit(idx_reg);
    }

    /// `VEC_GET_REG_IMM <dst> <vec> <idx:u32>`（栈中性）
    pub(crate) fn emit_vec_get_reg_imm(&mut self, dst: u8, vec_reg: u8, index: u32) {
        self.emit_op(control::VEC_GET_REG_IMM);
        self.emit(dst);
        self.emit(vec_reg);
        for byte in index.to_le_bytes() {
            self.emit(byte);
        }
    }

    /// `VEC_SET_REG <vec> <idx> <src>`（栈中性）
    pub(crate) fn emit_vec_set_reg(&mut self, vec_reg: u8, idx_reg: u8, src_reg: u8) {
        self.emit_op(control::VEC_SET_REG);
        self.emit(vec_reg);
        self.emit(idx_reg);
        self.emit(src_reg);
    }

    /// `VEC_SET_REG_IMM <vec> <idx:u32> <src>`（栈中性）
    pub(crate) fn emit_vec_set_reg_imm(&mut self, vec_reg: u8, index: u32, src_reg: u8) {
        self.emit_op(control::VEC_SET_REG_IMM);
        self.emit(vec_reg);
        for byte in index.to_le_bytes() {
            self.emit(byte);
        }
        self.emit(src_reg);
    }

    /// Emit `CMP_REG_IMM_JCC <a> <imm:u32> <cond> <i16 offset>`
    pub(crate) fn emit_cmp_reg_imm_jcc(&mut self, a: u8, imm: u32, cond: u8, label: &str) {
        self.emit_op(control::CMP_REG_IMM_JCC);
        self.emit(a);
        for byte in imm.to_le_bytes() {
            self.emit(byte);
        }
        self.emit(cond);
        let fixup_pos = self.pos();
        self.emit_u16(0); // Placeholder
        self.fixups.push((fixup_pos, label.to_string()));
    }

    pub(crate) fn emit_cmp_reg_reg_jcc(&mut self, a: u8, b: u8, cond: u8, label: &str) {
        self.emit_op(control::CMP_REG_REG_JCC);
        self.emit(a);
        self.emit(b);
        self.emit(cond);
        let fixup_pos = self.pos();
        self.emit_u16(0); // Placeholder
        self.fixups.push((fixup_pos, label.to_string()));
    }

    /// Emit fused increment: `INC_REG <reg>`
    ///
    /// Equivalent to `PUSH_REG r; INC; POP_REG r`.
    pub(crate) fn emit_inc_reg(&mut self, reg: u8) {
        self.emit_op(control::INC_REG);
        self.emit(reg);
    }

    /// Deterministic check for opaque predicate injection
    fn should_inject_opaque(&self) -> bool {
        let entropy = (self.bytecode.len() as u64)
            .wrapping_mul(0x9E3779B97F4A7C15)
            .wrapping_add(0xC6A4A7935BD1E995);
        (entropy % 100) < 15
    }

    // =========================================================================
    // Arithmetic Operations (with MBA/Substitution)
    // =========================================================================

    /// Emit ADD instruction
    pub(crate) fn emit_add(&mut self) {
        if self.mba_enabled {
            let table = self.opcode_table.clone();
            self.mba.emit_add(&mut self.bytecode, |op| table.encode(op));
        } else {
            let table = self.opcode_table.clone();
            let encode = |op: u8| table.encode(op);
            let variant = AddSubstitution::choose(&mut self.subst);
            variant.emit(&mut self.bytecode, &encode);
        }
    }

    /// Emit SUB instruction
    pub(crate) fn emit_sub(&mut self) {
        if self.mba_enabled {
            let table = self.opcode_table.clone();
            self.mba.emit_sub(&mut self.bytecode, |op| table.encode(op));
        } else {
            let table = self.opcode_table.clone();
            let encode = |op: u8| table.encode(op);
            let variant = SubSubstitution::choose(&mut self.subst);
            variant.emit(&mut self.bytecode, &encode);
        }
    }

    /// Emit MUL instruction
    pub(crate) fn emit_mul(&mut self) {
        self.emit_op(arithmetic::MUL);
    }

    /// Emit DIV instruction
    pub(crate) fn emit_div(&mut self) {
        self.emit_op(arithmetic::DIV);
    }

    /// Emit MOD instruction
    pub(crate) fn emit_mod(&mut self) {
        self.emit_op(arithmetic::MOD);
    }

    /// Emit IDIV instruction (signed)
    pub(crate) fn emit_idiv(&mut self) {
        self.emit_op(arithmetic::IDIV);
    }

    /// Emit IMOD instruction (signed)
    pub(crate) fn emit_imod(&mut self) {
        self.emit_op(arithmetic::IMOD);
    }

    /// Emit XOR instruction
    pub(crate) fn emit_xor(&mut self) {
        if self.mba_enabled {
            let table = self.opcode_table.clone();
            self.mba.emit_xor(&mut self.bytecode, |op| table.encode(op));
        } else {
            self.emit_op(arithmetic::XOR);
        }
    }

    /// Emit AND instruction
    pub(crate) fn emit_and(&mut self) {
        let table = self.opcode_table.clone();
        let encode = |op: u8| table.encode(op);
        if AndSubstitution::should_use(&mut self.subst) {
            // De Morgan: a & b = ~(~a | ~b)
            AndSubstitution::emit_demorgan_prefix(&mut self.bytecode, &encode);
            self.emit_not();
            AndSubstitution::emit_demorgan_swap(&mut self.bytecode, &encode);
            self.emit_not();
            AndSubstitution::emit_demorgan_or(&mut self.bytecode, &encode);
            self.emit_not();
        } else {
            AndSubstitution::emit_original(&mut self.bytecode, &encode);
        }
    }

    /// Emit OR instruction
    pub(crate) fn emit_or(&mut self) {
        let table = self.opcode_table.clone();
        let encode = |op: u8| table.encode(op);
        if OrSubstitution::should_use(&mut self.subst) {
            // De Morgan: a | b = ~(~a & ~b)
            OrSubstitution::emit_demorgan_prefix(&mut self.bytecode, &encode);
            self.emit_not();
            OrSubstitution::emit_demorgan_swap(&mut self.bytecode, &encode);
            self.emit_not();
            OrSubstitution::emit_demorgan_and(&mut self.bytecode, &encode);
            self.emit_not();
        } else {
            OrSubstitution::emit_original(&mut self.bytecode, &encode);
        }
    }

    /// Emit NOT instruction
    pub(crate) fn emit_not(&mut self) {
        let table = self.opcode_table.clone();
        let encode = |op: u8| table.encode(op);
        let variant = NotSubstitution::choose(&mut self.subst);
        let needs_xor = variant.emit(&mut self.bytecode, &encode);
        if needs_xor {
            self.emit_xor();
        }
    }

    /// Emit SHL instruction
    pub(crate) fn emit_shl(&mut self) {
        self.emit_op(arithmetic::SHL);
    }

    /// Emit SHR instruction
    pub(crate) fn emit_shr(&mut self) {
        self.emit_op(arithmetic::SHR);
    }

    /// Emit ROL instruction
    pub(crate) fn emit_rol(&mut self) {
        self.emit_op(arithmetic::ROL);
    }

    /// Emit ROR instruction
    pub(crate) fn emit_ror(&mut self) {
        self.emit_op(arithmetic::ROR);
    }

    /// Emit INC instruction
    pub(crate) fn emit_inc(&mut self) {
        let table = self.opcode_table.clone();
        let encode = |op: u8| table.encode(op);
        let variant = IncSubstitution::choose(&mut self.subst);
        let (needs_add, needs_sub) = variant.emit(&mut self.bytecode, &encode);
        if needs_add {
            self.emit_add();
        } else if needs_sub {
            self.emit_sub();
        }
    }

    // =========================================================================
    // Constants and Zero
    // =========================================================================

    /// Emit a constant value with potential obfuscation
    pub(crate) fn emit_constant(&mut self, value: u64) {
        let table = self.opcode_table.clone();
        let encode = |op: u8| table.encode(op);

        if self.value_cryptor_enabled {
            self.value_cryptor.emit_encrypted_value(value, &mut self.bytecode, &encode);
        } else if ConstantSubstitution::should_split(&mut self.subst, value) {
            let (a, b) = ConstantSubstitution::split(&mut self.subst, value);
            ConstantSubstitution::emit_value(&mut self.bytecode, a, &encode);
            ConstantSubstitution::emit_value(&mut self.bytecode, b, &encode);
            self.emit_add();
        } else {
            ConstantSubstitution::emit_value(&mut self.bytecode, value, &encode);
        }
    }

    /// Emit zero with potential obfuscation
    pub(crate) fn emit_zero(&mut self) {
        let table = self.opcode_table.clone();
        let encode = |op: u8| table.encode(op);
        if ZeroSubstitution::should_obfuscate(&mut self.subst) {
            let x = ZeroSubstitution::get_xor_value(&mut self.subst);
            ZeroSubstitution::emit_prefix(&mut self.bytecode, x, &encode);
            self.emit_xor();
        } else {
            ZeroSubstitution::emit_original(&mut self.bytecode, &encode);
        }
    }

    // =========================================================================
    // Type Conversions
    // =========================================================================

    /// Sign-extend 8-bit to 64-bit
    pub(crate) fn emit_sext8(&mut self) {
        self.emit_op(convert::SEXT8);
    }

    /// Sign-extend 16-bit to 64-bit
    pub(crate) fn emit_sext16(&mut self) {
        self.emit_op(convert::SEXT16);
    }

    /// Sign-extend 32-bit to 64-bit
    pub(crate) fn emit_sext32(&mut self) {
        self.emit_op(convert::SEXT32);
    }

    /// Truncate to 8-bit
    pub(crate) fn emit_trunc8(&mut self) {
        self.emit_op(convert::TRUNC8);
    }

    /// Truncate to 16-bit
    pub(crate) fn emit_trunc16(&mut self) {
        self.emit_op(convert::TRUNC16);
    }

    /// Truncate to 32-bit
    pub(crate) fn emit_trunc32(&mut self) {
        self.emit_op(convert::TRUNC32);
    }

    // =========================================================================
    // Stack Operations
    // =========================================================================

    /// Emit DUP instruction
    pub(crate) fn emit_dup(&mut self) {
        self.emit_op(stack::DUP);
    }

    /// Emit DROP instruction
    pub(crate) fn emit_drop(&mut self) {
        self.emit_op(stack::DROP);
    }

    /// Emit SWAP instruction
    pub(crate) fn emit_swap(&mut self) {
        self.emit_op(stack::SWAP);
    }

    /// Emit PUSH_REG instruction
    pub(crate) fn emit_push_reg(&mut self, reg: u8) {
        self.emit_op(stack::PUSH_REG);
        self.emit(reg);
    }

    /// Emit POP_REG instruction
    pub(crate) fn emit_pop_reg(&mut self, reg: u8) {
        self.emit_op(stack::POP_REG);
        self.emit(reg);
    }

    // =========================================================================
    // Vector Operations
    // =========================================================================

    /// Emit VEC_NEW: Stack: [capacity, elem_size] -> [vec_addr]
    pub(crate) fn emit_vec_new(&mut self) {
        self.emit_op(vector::VEC_NEW);
    }

    /// Emit VEC_LEN: Stack: [vec_addr] -> [length]
    pub(crate) fn emit_vec_len(&mut self) {
        self.emit_op(vector::VEC_LEN);
    }

    /// Emit VEC_CAP: Stack: [vec_addr] -> [capacity]
    pub(crate) fn emit_vec_cap(&mut self) {
        self.emit_op(vector::VEC_CAP);
    }

    /// Emit VEC_PUSH: Stack: [vec_addr, value] -> []
    pub(crate) fn emit_vec_push(&mut self) {
        self.emit_op(vector::VEC_PUSH);
    }

    /// Emit VEC_POP: Stack: [vec_addr] -> [value]
    pub(crate) fn emit_vec_pop(&mut self) {
        self.emit_op(vector::VEC_POP);
    }

    /// Emit VEC_GET: Stack: [vec_addr, index] -> [value]
    pub(crate) fn emit_vec_get(&mut self) {
        self.emit_op(vector::VEC_GET);
    }

    /// Emit VEC_SET: Stack: [vec_addr, index, value] -> []
    pub(crate) fn emit_vec_set(&mut self) {
        self.emit_op(vector::VEC_SET);
    }

    /// Emit VEC_REPEAT: Stack: [value, count, elem_size] -> [vec_addr]
    pub(crate) fn emit_vec_repeat(&mut self) {
        self.emit_op(vector::VEC_REPEAT);
    }

    /// Emit VEC_CLEAR: Stack: [vec_addr] -> []
    pub(crate) fn emit_vec_clear(&mut self) {
        self.emit_op(vector::VEC_CLEAR);
    }

    /// Emit VEC_RESERVE: Stack: [vec_addr, additional] -> []
    pub(crate) fn emit_vec_reserve(&mut self) {
        self.emit_op(vector::VEC_RESERVE);
    }

    // =========================================================================
    // String Operations
    // =========================================================================

    /// Emit STR_NEW: Stack: [capacity] -> [str_addr]
    pub(crate) fn emit_str_new(&mut self) {
        self.emit_op(string::STR_NEW);
    }

    /// Emit STR_LEN: Stack: [str_addr] -> [length]
    pub(crate) fn emit_str_len(&mut self) {
        self.emit_op(string::STR_LEN);
    }

    /// Emit STR_PUSH: Stack: [str_addr, byte] -> []
    pub(crate) fn emit_str_push(&mut self) {
        self.emit_op(string::STR_PUSH);
    }

    /// Emit STR_GET: Stack: [str_addr, index] -> [byte]
    pub(crate) fn emit_str_get(&mut self) {
        self.emit_op(string::STR_GET);
    }

    /// Emit STR_SET: Stack: [str_addr, index, byte] -> []
    pub(crate) fn emit_str_set(&mut self) {
        self.emit_op(string::STR_SET);
    }

    /// Emit STR_CMP: Stack: [str1_addr, str2_addr] -> [result]
    pub(crate) fn emit_str_cmp(&mut self) {
        self.emit_op(string::STR_CMP);
    }

    /// Emit STR_EQ: Stack: [str1_addr, str2_addr] -> [0/1]
    pub(crate) fn emit_str_eq(&mut self) {
        self.emit_op(string::STR_EQ);
    }

    /// Emit STR_HASH: Stack: [str_addr] -> [hash]
    pub(crate) fn emit_str_hash(&mut self) {
        self.emit_op(string::STR_HASH);
    }

    /// Emit STR_CONCAT: Stack: [str1_addr, str2_addr] -> [new_str_addr]
    pub(crate) fn emit_str_concat(&mut self) {
        self.emit_op(string::STR_CONCAT);
    }

    // =========================================================================
    // Heap Operations
    // =========================================================================

    /// Emit HEAP_ALLOC: Stack: [size] -> [address]
    pub(crate) fn emit_heap_alloc(&mut self) {
        self.emit_op(heap::HEAP_ALLOC);
    }

    /// Emit HEAP_FREE: Stack: [address] -> []
    pub(crate) fn emit_heap_free(&mut self) {
        self.emit_op(heap::HEAP_FREE);
    }

    /// Emit HEAP_LOAD64: Stack: [address] -> [value]
    pub(crate) fn emit_heap_load64(&mut self) {
        self.emit_op(heap::HEAP_LOAD64);
    }

    /// Emit HEAP_STORE64: Stack: [address, value] -> []
    pub(crate) fn emit_heap_store64(&mut self) {
        self.emit_op(heap::HEAP_STORE64);
    }

    // =========================================================================
    // Native Operations
    // =========================================================================

    /// Emit NATIVE_READ: Read from input buffer
    pub(crate) fn emit_native_read(&mut self, offset: u16) {
        self.emit_op(native::NATIVE_READ);
        self.emit_u16(offset);
    }

    /// Emit NATIVE_CALL: Call external function by index
    /// Format: NATIVE_CALL <func_index: u8> <arg_count: u8>
    pub(crate) fn emit_native_call(&mut self, func_index: u8, arg_count: u8) {
        self.emit_op(native::NATIVE_CALL);
        self.emit(func_index);
        self.emit(arg_count);
    }

    // =========================================================================
    // Control Flow
    // =========================================================================

    /// Emit CMP instruction
    pub(crate) fn emit_cmp(&mut self) {
        self.emit_op(control::CMP);
    }
}

impl Compiler {
    /// MOV_REG dst, src
    pub(crate) fn emit_mov_reg(&mut self, dst: u8, src: u8) {
        self.emit_op(register::MOV_REG);
        self.emit(dst);
        self.emit(src);
    }

    /// MOV_IMM dst, value:u64
    pub(crate) fn emit_mov_imm(&mut self, dst: u8, value: u64) {
        self.emit_op(register::MOV_IMM);
        self.emit(dst);
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    /// ADD_REG_IMM dst, imm:u32
    pub(crate) fn emit_add_reg_imm(&mut self, dst: u8, imm: u32) {
        self.emit_op(control::ADD_REG_IMM);
        self.emit(dst);
        self.bytecode.extend_from_slice(&imm.to_le_bytes());
    }

    /// ADD_REG_REG dst, a, b
    pub(crate) fn emit_add_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_op(control::ADD_REG_REG);
        self.emit(dst);
        self.emit(a);
        self.emit(b);
    }

    /// 8 条寄存器式二元算术统一形状：<op> dst, a, b
    fn emit_binop_reg_reg(&mut self, opcode: u8, dst: u8, a: u8, b: u8) {
        self.emit_op(opcode);
        self.emit(dst);
        self.emit(a);
        self.emit(b);
    }

    /// 寄存器式二元算子的编译期描述：决定可用哪条融合指令、有没有立即数形式
    /// 立即数形态统一形状：<op> reg, imm:u32
    fn emit_binop_reg_imm(&mut self, opcode: u8, reg: u8, imm: u32) {
        self.emit_op(opcode);
        self.emit(reg);
        for byte in imm.to_le_bytes() {
            self.emit(byte);
        }
    }

    pub(crate) fn emit_sub_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_binop_reg_imm(control::SUB_REG_IMM, reg, imm);
    }

    pub(crate) fn emit_div_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_binop_reg_imm(control::DIV_REG_IMM, reg, imm);
    }

    pub(crate) fn emit_mod_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_binop_reg_imm(control::MOD_REG_IMM, reg, imm);
    }

    pub(crate) fn emit_and_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_binop_reg_imm(control::AND_REG_IMM, reg, imm);
    }

    pub(crate) fn emit_or_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_binop_reg_imm(control::OR_REG_IMM, reg, imm);
    }

    pub(crate) fn emit_xor_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_binop_reg_imm(control::XOR_REG_IMM, reg, imm);
    }

    pub(crate) fn emit_shl_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_binop_reg_imm(control::SHL_REG_IMM, reg, imm);
    }

    pub(crate) fn emit_shr_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_binop_reg_imm(control::SHR_REG_IMM, reg, imm);
    }

    /// 立即数形态的统一分派
    pub(crate) fn emit_reg_binop_imm(&mut self, op: RegOp, dst: u8, imm: u32) {
        match op {
            RegOp::Add => self.emit_add_reg_imm(dst, imm),
            RegOp::Mul => self.emit_mul_reg_imm(dst, imm),
            RegOp::Sub => self.emit_sub_reg_imm(dst, imm),
            RegOp::Div => self.emit_div_reg_imm(dst, imm),
            RegOp::Mod => self.emit_mod_reg_imm(dst, imm),
            RegOp::And => self.emit_and_reg_imm(dst, imm),
            RegOp::Or => self.emit_or_reg_imm(dst, imm),
            RegOp::Xor => self.emit_xor_reg_imm(dst, imm),
            RegOp::Shl => self.emit_shl_reg_imm(dst, imm),
            RegOp::Shr => self.emit_shr_reg_imm(dst, imm),
        }
    }

    pub(crate) fn emit_reg_binop(&mut self, op: RegOp, dst: u8, a: u8, b: u8) {
        match op {
            RegOp::Add => self.emit_add_reg_reg(dst, a, b),
            RegOp::Mul => self.emit_mul_reg_reg(dst, a, b),
            RegOp::Sub => self.emit_sub_reg_reg(dst, a, b),
            RegOp::Div => self.emit_div_reg_reg(dst, a, b),
            RegOp::Mod => self.emit_mod_reg_reg(dst, a, b),
            RegOp::And => self.emit_and_reg_reg(dst, a, b),
            RegOp::Or => self.emit_or_reg_reg(dst, a, b),
            RegOp::Xor => self.emit_xor_reg_reg(dst, a, b),
            RegOp::Shl => self.emit_shl_reg_reg(dst, a, b),
            RegOp::Shr => self.emit_shr_reg_reg(dst, a, b),
        }
    }

    pub(crate) fn emit_sub_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_binop_reg_reg(control::SUB_REG_REG, dst, a, b);
    }

    pub(crate) fn emit_div_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_binop_reg_reg(control::DIV_REG_REG, dst, a, b);
    }

    pub(crate) fn emit_mod_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_binop_reg_reg(control::MOD_REG_REG, dst, a, b);
    }

    pub(crate) fn emit_and_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_binop_reg_reg(control::AND_REG_REG, dst, a, b);
    }

    pub(crate) fn emit_or_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_binop_reg_reg(control::OR_REG_REG, dst, a, b);
    }

    pub(crate) fn emit_xor_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_binop_reg_reg(control::XOR_REG_REG, dst, a, b);
    }

    pub(crate) fn emit_shl_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_binop_reg_reg(control::SHL_REG_REG, dst, a, b);
    }

    pub(crate) fn emit_shr_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_binop_reg_reg(control::SHR_REG_REG, dst, a, b);
    }

    /// MUL_REG_IMM dst, imm:u32
    pub(crate) fn emit_mul_reg_imm(&mut self, dst: u8, imm: u32) {
        self.emit_op(control::MUL_REG_IMM);
        self.emit(dst);
        self.bytecode.extend_from_slice(&imm.to_le_bytes());
    }

    /// MUL_REG_REG dst, a, b
    pub(crate) fn emit_mul_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_op(control::MUL_REG_REG);
        self.emit(dst);
        self.emit(a);
        self.emit(b);
    }
}

/// 寄存器式二元算子（阶段一支持的全部算子）。
///
/// 与栈式 `arithmetic::*` 一一对应，语义完全一致；只有 `Add`/`Mul` 有立即数融合形式
/// （`ADD_REG_IMM` / `MUL_REG_IMM`），其余算子只支持寄存器-寄存器形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RegOp {
    Add,
    Mul,
    Sub,
    Div,
    Mod,
    And,
    Or,
    Xor,
    Shl,
    Shr,
}

impl RegOp {
    /// `syn` 的二元运算 → 寄存器式算子；不支持的返回 `None`（调用方回退栈序列）
    pub(crate) fn from_binop(op: &syn::BinOp) -> Option<Self> {
        Some(match op {
            syn::BinOp::Add(_) => Self::Add,
            syn::BinOp::Mul(_) => Self::Mul,
            syn::BinOp::Sub(_) => Self::Sub,
            syn::BinOp::Div(_) => Self::Div,
            syn::BinOp::Rem(_) => Self::Mod,
            syn::BinOp::BitAnd(_) => Self::And,
            syn::BinOp::BitOr(_) => Self::Or,
            syn::BinOp::BitXor(_) => Self::Xor,
            syn::BinOp::Shl(_) => Self::Shl,
            syn::BinOp::Shr(_) => Self::Shr,
            _ => return None,
        })
    }

    /// 是否已有 `*_REG_IMM` 形式的融合指令（阶段一补齐后 10 个算子全部具备）
    pub(crate) fn has_imm_form(self) -> bool {
        true
    }
}
