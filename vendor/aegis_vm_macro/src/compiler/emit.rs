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

thread_local! {
    /// 本次编译发射的压栈、弹栈条数（线程局部：测试并行，全局变量会被兄弟用例污染）。
    static STACK_PUSHES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static STACK_POPS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };

    /// AEGIS_DUMP 用的指令流轨迹：(base opcode, 操作数字节)。
    /// 只记录**逻辑指令**——emit_op 里注入的 NOP / 死代码不经过 `emit`，
    /// 因此不会污染操作数，读出来的序列就是源码语义的那一份。
    static TRACE: std::cell::RefCell<Vec<(u8, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// 清空指令流轨迹；每个函数编译开始时由 `compile_function_full` 调用。
pub(crate) fn trace_reset() {
    TRACE.with(|t| t.borrow_mut().clear());
}

/// 把一条轨迹按「偏移 OP 操作数」打印出来（仅 AEGIS_DUMP=1 时输出）。
/// opcode → 助记符。由 `opcodes.rs` 的常量表生成（不手写，避免抄错）。
fn mnemonic(op: u8) -> &'static str {
    match op {
        0x00 => "JCC_Z",
        0x01 => "PUSH_IMM",
        0x02 => "PUSH_IMM8",
        0x03 => "PUSH_REG",
        0x04 => "POP_REG",
        0x05 => "DUP",
        0x06 => "SWAP",
        0x07 => "DROP",
        0x08 => "PUSH_IMM16",
        0x09 => "PUSH_IMM32",
        0x10 => "MOV_IMM",
        0x11 => "MOV_REG",
        0x12 => "LOAD_MEM",
        0x13 => "STORE_MEM",
        0x20 => "ADD",
        0x21 => "SUB",
        0x22 => "MUL",
        0x23 => "XOR",
        0x24 => "AND",
        0x25 => "OR",
        0x26 => "SHL",
        0x27 => "SHR",
        0x28 => "NOT",
        0x29 => "ROL",
        0x2A => "ROR",
        0x2B => "INC",
        0x2C => "DEC",
        0x30 => "CMP",
        0x31 => "JMP",
        0x32 => "JZ",
        0x33 => "JNZ",
        0x34 => "JGT",
        0x35 => "JLT",
        0x36 => "JGE",
        0x37 => "JLE",
        0x38 => "CALL",
        0x39 => "RET",
        0x40 => "NOP",
        0x41 => "NOP_N",
        0x42 => "OPAQUE_TRUE",
        0x43 => "OPAQUE_FALSE",
        0x44 => "HASH_CHECK",
        0x45 => "TIMING_CHECK",
        0x46 => "DIV",
        0x47 => "MOD",
        0x48 => "IDIV",
        0x49 => "IMOD",
        0x50 => "SEXT8",
        0x51 => "SEXT16",
        0x52 => "SEXT32",
        0x53 => "TRUNC8",
        0x54 => "TRUNC16",
        0x55 => "TRUNC32",
        0x60 => "LOAD8",
        0x61 => "LOAD16",
        0x62 => "LOAD32",
        0x63 => "LOAD64",
        0x64 => "STORE8",
        0x65 => "STORE16",
        0x66 => "STORE32",
        0x67 => "STORE64",
        0x70 => "HEAP_ALLOC",
        0x71 => "HEAP_FREE",
        0x72 => "HEAP_LOAD8",
        0x73 => "HEAP_LOAD16",
        0x74 => "HEAP_LOAD32",
        0x75 => "HEAP_LOAD64",
        0x76 => "HEAP_STORE8",
        0x77 => "HEAP_STORE16",
        0x78 => "HEAP_STORE32",
        0x79 => "HEAP_STORE64",
        0x7A => "HEAP_SIZE",
        0x80 => "VEC_NEW",
        0x81 => "VEC_LEN",
        0x82 => "VEC_CAP",
        0x83 => "VEC_PUSH",
        0x84 => "VEC_POP",
        0x85 => "VEC_GET",
        0x86 => "VEC_SET",
        0x87 => "VEC_REPEAT",
        0x88 => "VEC_CLEAR",
        0x89 => "VEC_RESERVE",
        0x90 => "STR_NEW",
        0x91 => "STR_LEN",
        0x92 => "STR_PUSH",
        0x93 => "STR_GET",
        0x94 => "STR_SET",
        0x95 => "STR_CMP",
        0x96 => "STR_EQ",
        0x97 => "STR_HASH",
        0x98 => "STR_CONCAT",
        0x9B => "CMP_REG_REG_JGE",
        0x9C => "INC_REG",
        0x9D => "ADD_REG_IMM",
        0x9E => "ADD_REG_REG",
        0x9F => "MUL_REG_IMM",
        0xA0 => "MUL_REG_REG",
        0xA1 => "CMP_REG_REG_JCC",
        0xA2 => "XOR_REG_REG",
        0xA3 => "SHR_REG_IMM",
        0xA4 => "XOR_REG_IMM",
        0xA5 => "AND_REG_REG",
        0xA6 => "AND_REG_IMM",
        0xA7 => "NATIVE_READ_REG",
        0xF0 => "NATIVE_CALL",
        0xF1 => "NATIVE_READ",
        0xF2 => "NATIVE_WRITE",
        0xF3 => "INPUT_LEN",
        0xFE => "HALT_ERR",
        0xFF => "HALT",
        0xFD => "CONST_ENC",
        0xFE => "LABEL",
        _ => "?",
    }
}
pub(crate) fn trace_dump(func: &str) {
    if std::env::var_os("AEGIS_DUMP").is_none() {
        return;
    }
    eprintln!("=== fn {func} ===");
    TRACE.with(|t| {
        let trace = t.borrow();
        let mut offset = 0usize;
        for (idx, (op, operands)) in trace.iter().enumerate() {
            if *op == 0xFD {
                // 伪指令：不占字节，故不计入偏移
                eprintln!("  {idx:>3} {offset:>4}  -- CONST_ENC {operands:02x?}");
                continue;
            }
            if *op == 0xFE {
                let name = String::from_utf8_lossy(operands);
                eprintln!("  {idx:>3} {offset:>4}  -- LABEL {name}");
                continue;
            }
            let hex: String = operands.iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("  {idx:>3} {offset:>4}  {op:02X} {:<16} {hex}", mnemonic(*op));
            offset += 1 + operands.len();
        }
    });
}

/// 记录一条「绕过 emit_op 直接写进 bytecode」的指令。
///
/// 为什么要这个钩子：`ZeroSubstitution::emit_prefix` / `emit_original` 都是
/// `bytecode.push(...)` 直写，不经过 emit_op，于是它们既不在轨迹里、也让轨迹里的偏移量
/// 失去意义（偏移是记录的累加和，隐形字节不会计入）。缺了它们就无法做栈平衡核算——
/// 实测正是靠这一点把「三路 if 的栈账」算错成 −1。
/// 记录一次标签落点（伪指令，op = 0xFE，不占字节）。
///
/// 没有它就看不出 `JMP/JZ` 的落点：反汇编只有指令和相对偏移，
/// 而「跳到哪一条」恰恰是判断控制流与栈平衡的关键。
pub(crate) fn trace_label(name: &str) {
    if std::env::var_os("AEGIS_DUMP").is_none() {
        return;
    }
    TRACE.with(|t| t.borrow_mut().push((0xFE, name.as_bytes().to_vec())));
}

pub(crate) fn trace_raw(op: u8, operands: &[u8]) {
    if std::env::var_os("AEGIS_DUMP").is_none() {
        return;
    }
    TRACE.with(|t| t.borrow_mut().push((op, operands.to_vec())));
}

/// 把操作数记到当前指令上。
fn trace_operand(bytes: &[u8]) {
    if std::env::var_os("AEGIS_DUMP").is_none() {
        return;
    }
    TRACE.with(|t| {
        if let Some(last) = t.borrow_mut().last_mut() {
            last.1.extend_from_slice(bytes);
        }
    });
}

/// 清零，供测试在编译前调用。
pub(crate) fn reset_stack_moves() {
    STACK_PUSHES.with(|c| c.set(0));
    STACK_POPS.with(|c| c.set(0));
}

/// 取本次编译发射的压栈/弹栈条数。
pub(crate) fn stack_moves_emitted() -> u64 {
    STACK_PUSHES.with(|c| c.get()) + STACK_POPS.with(|c| c.get())
}

/// 分别给出 (压栈次数, 弹栈次数)，用于判断搬运来自哪一侧。
pub(crate) fn stack_moves_split() -> (u64, u64) {
    (STACK_PUSHES.with(|c| c.get()), STACK_POPS.with(|c| c.get()))
}
/// 把栈搬运计数恢复到快照值：`try_expr_to_reg` 回滚时用（字节码截断了，计数也必须退）。
pub(crate) fn restore_stack_moves(snapshot: (u64, u64)) {
    STACK_PUSHES.with(|c| c.set(snapshot.0));
    STACK_POPS.with(|c| c.set(snapshot.1));
}


impl Compiler {
    /// Emit an opcode (automatically encoded via shuffle table)
    pub(crate) fn emit_op(&mut self, base_opcode: u8) {
        // 指令流诊断：AEGIS_DUMP=1 时把每条 base opcode 打到 stderr，供
        // ./scripts/vm-opcode-census.sh 统计构成。注意是**编译期**读取，
        // 改环境变量后必须 touch 本文件强制重展开。
        if std::env::var_os("AEGIS_DUMP").is_some() {
            eprintln!("OP {base_opcode:02X} at {}", self.bytecode.len());
        }
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

        if std::env::var_os("AEGIS_DUMP").is_some() {
            TRACE.with(|t| t.borrow_mut().push((base_opcode, Vec::new())));
        }

        let shuffled = self.opcode_table.encode(base_opcode);
        self.bytecode.push(shuffled);
    }

    /// Emit a single byte
    pub(crate) fn emit(&mut self, byte: u8) {
        trace_operand(&[byte]);
        self.bytecode.push(byte);
    }

    /// Emit a u16 (little-endian)
    pub(crate) fn emit_u16(&mut self, value: u16) {
        trace_operand(&value.to_le_bytes());
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
    #[track_caller]
    pub(crate) fn emit_push_reg(&mut self, reg: u8) {
        STACK_PUSHES.with(|c| c.set(c.get() + 1));
        // 栈搬运归因：AEGIS_STK=1 时打印调用点，供统计「谁在搬栈」。
        if std::env::var_os("AEGIS_STK").is_some() {
            eprintln!("STK PUSH {} {:?}", reg, std::panic::Location::caller());
        }
        self.emit_op(stack::PUSH_REG);
        self.emit(reg);
    }

    /// Emit POP_REG instruction
    #[track_caller]
    pub(crate) fn emit_pop_reg(&mut self, reg: u8) {
        STACK_POPS.with(|c| c.set(c.get() + 1));
        // 同上：AEGIS_STK=1 时打印调用点。
        if std::env::var_os("AEGIS_STK").is_some() {
            eprintln!("STK POP {} {:?}", reg, std::panic::Location::caller());
        }
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
    /// NATIVE_READ_REG <reg:u8> <offset:u16>
    pub(crate) fn emit_native_read_reg(&mut self, reg: u8, offset: u16) {
        self.emit_op(control::NATIVE_READ_REG);
        self.emit(reg);
        self.emit_u16(offset);
    }

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
        trace_operand(&value.to_le_bytes());
        self.bytecode.extend_from_slice(&value.to_le_bytes());
    }

    /// ADD_REG_IMM dst, imm:u32
    pub(crate) fn emit_add_reg_imm(&mut self, dst: u8, imm: u32) {
        self.emit_op(control::ADD_REG_IMM);
        self.emit(dst);
        trace_operand(&imm.to_le_bytes());
        self.bytecode.extend_from_slice(&imm.to_le_bytes());
    }

    /// ADD_REG_REG dst, a, b
    pub(crate) fn emit_add_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_op(control::ADD_REG_REG);
        self.emit(dst);
        self.emit(a);
        self.emit(b);
    }

    /// MUL_REG_IMM dst, imm:u32
    pub(crate) fn emit_mul_reg_imm(&mut self, dst: u8, imm: u32) {
        self.emit_op(control::MUL_REG_IMM);
        self.emit(dst);
        trace_operand(&imm.to_le_bytes());
        self.bytecode.extend_from_slice(&imm.to_le_bytes());
    }

    /// MUL_REG_REG dst, a, b
    pub(crate) fn emit_mul_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_op(control::MUL_REG_REG);
        self.emit(dst);
        self.emit(a);
        self.emit(b);
    }

    pub(crate) fn emit_xor_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_op(control::XOR_REG_REG);
        self.emit(dst);
        self.emit(a);
        self.emit(b);
    }

    pub(crate) fn emit_and_reg_reg(&mut self, dst: u8, a: u8, b: u8) {
        self.emit_op(control::AND_REG_REG);
        self.emit(dst);
        self.emit(a);
        self.emit(b);
    }

    pub(crate) fn emit_and_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_op(control::AND_REG_IMM);
        self.emit(reg);
        for b in imm.to_le_bytes() {
            self.emit(b);
        }
    }

    pub(crate) fn emit_xor_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_op(control::XOR_REG_IMM);
        self.emit(reg);
        for b in imm.to_le_bytes() {
            self.emit(b);
        }
    }

    pub(crate) fn emit_shr_reg_imm(&mut self, reg: u8, imm: u32) {
        self.emit_op(control::SHR_REG_IMM);
        self.emit(reg);
        for b in imm.to_le_bytes() {
            self.emit(b);
        }
    }
}
