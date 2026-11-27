//! Modular AST to Bytecode Compiler
//!
//! Converts Rust expressions to VM bytecode at compile time.
//!
//! Module structure:
//! - mod.rs: Compiler struct and core infrastructure
//! - emit.rs: Bytecode emission helpers
//! - literal.rs: Literal compilation (int, bool, string)
//! - expr.rs: Expression compilation
//! - stmt.rs: Statement compilation
//! - array.rs: Array/vector operations
//! - control.rs: Control flow (if, while, loop, for)
//! - method.rs: Method call compilation (.len(), .push(), etc.)
//! - cast.rs: Type cast compilation (as i32, as u8, etc.)

pub(crate) mod emit;
mod regs;
mod literal;
mod expr;
mod stmt;
mod array;
mod control;
mod method;
mod cast;
pub mod native_call;

pub use native_call::NativeCallCollector;

use syn::{ItemFn, Pat, FnArg};
use std::collections::BTreeMap;
use crate::opcodes::exec;
use crate::crypto::OpcodeTable;
use crate::mba::MbaTransformer;
use crate::substitution::Substitution;
use crate::value_cryptor::ValueCryptor;

/// Compilation error
#[derive(Debug)]
pub struct CompileError(pub String);

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Variable type for proper method dispatch
#[allow(dead_code)] // IntegerSized reserved for future packed storage optimization
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum VarType {
    /// Integer (u8, u16, u32, u64, i8, i16, i32, i64) with optional size in bytes
    Integer,
    /// Sized integer with explicit byte size (1, 2, 4, or 8)
    IntegerSized(u8),
    /// String (heap allocated)
    String,
    /// Vector/Array (heap allocated)
    Vector,
    /// Boolean (1 byte logically, but stored as 8 bytes for alignment)
    Bool,
    /// Struct (heap allocated) - contains struct type name
    Struct(std::string::String),
    /// Tuple (heap allocated) - contains element types for proper offset calculation
    Tuple(Vec<VarType>),
}

#[allow(dead_code)] // Reserved for future packed storage optimization
impl VarType {
    /// Get the size in bytes for this type (for tuple offset calculation)
    /// All types are stored as 8 bytes for alignment, but this tracks logical size
    pub fn size_bytes(&self) -> usize {
        match self {
            VarType::Integer => 8,
            VarType::IntegerSized(size) => *size as usize,
            VarType::String => 8,  // pointer
            VarType::Vector => 8,  // pointer
            VarType::Bool => 8,    // stored as u64 for alignment
            VarType::Struct(_) => 8, // pointer
            VarType::Tuple(elems) => {
                // Sum of all element sizes (each aligned to 8 bytes)
                elems.iter().map(|e| e.aligned_size()).sum()
            }
        }
    }

    /// Get aligned size (always 8 bytes for heap storage)
    pub fn aligned_size(&self) -> usize {
        // For now, all values are stored as 8 bytes for simplicity
        // This could be optimized later for packed storage
        8
    }

    /// Check if this type needs heap cleanup
    pub fn needs_cleanup(&self) -> bool {
        match self {
            VarType::String | VarType::Vector => true,
            VarType::Struct(_) => true, // checked separately for unit structs
            VarType::Tuple(elems) => {
                // Tuple needs cleanup if it has elements (allocated on heap)
                // or if any element needs cleanup
                !elems.is_empty() || elems.iter().any(|e| e.needs_cleanup())
            }
            _ => false,
        }
    }
}

/// Struct field definition
#[derive(Debug, Clone)]
pub(crate) struct FieldDef {
    /// Field name
    pub name: std::string::String,
    /// Byte offset from struct start
    pub offset: usize,
}

/// Struct definition for compile-time field lookup
#[derive(Debug, Clone)]
pub(crate) struct StructDef {
    /// Struct name (kept for debugging/error messages)
    #[allow(dead_code)]
    pub name: std::string::String,
    /// Fields with their offsets
    pub fields: Vec<FieldDef>,
    /// Total size in bytes
    pub size: usize,
}

impl StructDef {
    /// Get field offset by name
    pub fn get_field_offset(&self, field_name: &str) -> Option<usize> {
        self.fields.iter()
            .find(|f| f.name == field_name)
            .map(|f| f.offset)
    }
}

/// Variable information - register and type
#[derive(Debug, Clone)]
pub(crate) struct VarInfo {
    /// Register index
    pub reg: u8,
    /// Variable type
    pub var_type: VarType,
    /// Is signed (for integers)
    pub is_signed: bool,
    /// Needs heap cleanup on scope exit
    pub needs_cleanup: bool,
}

/// Variable location - either in input buffer or in a register
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum VarLocation {
    /// Input buffer offset (for function arguments)
    InputOffset(usize),
    /// Register index (for local variables)
    Register(u8),
    /// Array stored in register (register holds heap address)
    /// Contains: register index, element size (1, 2, 4, or 8 bytes)
    Array(u8, u8),
    /// String stored in register (register holds heap address)
    String(u8),
}

/// Loop context for break/continue support
#[derive(Debug, Clone)]
pub(crate) struct LoopContext {
    /// Label for continue (jump to condition/increment)
    pub continue_label: String,
    /// Label for break (jump past loop end)
    pub break_label: String,
    /// Scope depth when loop started (for cleanup on break/continue)
    pub scope_depth: usize,
}

/// Compiler state
pub struct Compiler {
    /// Generated bytecode
    pub(crate) bytecode: Vec<u8>,
    /// 正在给「栈式二元运算」编译操作数的层数。此层数 >0 时不许走寄存器快路径。
    pub(crate) stack_operand_depth: usize,
    /// 表达式临时寄存器池：只用 248..=255 这一段。
    /// 变量区上限是 248（`define_var_internal` 到 248 就报错），形参提升上限是 240，
    /// 所以 248 以上没有任何变量，快路径拿这里的号不会与变量撞车。
    pub(crate) scratch: std::rc::Rc<std::cell::RefCell<regs::RegPool>>,
    /// 栈式回退期间关掉寄存器快路径，避免 compile_expr → compile_expr_to_reg 互相递归。
    pub(crate) reg_fastpath_disabled: bool,
    /// Function argument name -> input buffer offset
    pub(crate) arg_offsets: BTreeMap<String, usize>,
    /// 标量参数在序言里读进寄存器后的映射（名字 → 寄存器）。
    pub(crate) arg_regs: BTreeMap<String, u8>,
    /// Scoped variable storage - Vec of scopes, each scope maps name -> VarInfo
    /// Innermost scope is at the end of the Vec
    pub(crate) scopes: Vec<BTreeMap<String, VarInfo>>,
    /// Legacy: Variable types for method dispatch (will be deprecated)
    pub(crate) var_types: BTreeMap<String, VarLocation>,
    /// Struct definitions for compile-time field lookup
    pub(crate) struct_defs: BTreeMap<String, StructDef>,
    /// Next available register for locals
    pub(crate) next_local_reg: u8,
    /// Current input buffer offset for next argument
    pub(crate) next_arg_offset: usize,
    /// Label name -> bytecode offset for jumps
    pub(crate) labels: BTreeMap<String, usize>,
    /// Pending jump fixups (bytecode offset, label name)
    pub(crate) fixups: Vec<(usize, String)>,
    /// Unique label counter
    pub(crate) label_counter: usize,
    /// Stack of active loops for break/continue
    pub(crate) loop_stack: Vec<LoopContext>,
    /// Opcode encoding table
    pub(crate) opcode_table: OpcodeTable,
    /// MBA transformer
    pub(crate) mba: MbaTransformer,
    /// Enable MBA transformations
    pub(crate) mba_enabled: bool,
    /// Substitution state
    pub(crate) subst: Substitution,
    /// ValueCryptor for constant obfuscation
    pub(crate) value_cryptor: ValueCryptor,
    /// Enable heavy value encryption
    pub(crate) value_cryptor_enabled: bool,
    /// Enable opaque predicates injection
    pub(crate) opaque_predicates_enabled: bool,
    /// Native call collector for external function calls
    pub(crate) native_collector: NativeCallCollector,
}

impl Compiler {
    /// Create new compiler
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::with_options(false, false)
    }

    /// Create compiler with MBA transformations enabled
    #[allow(dead_code)]
    pub fn with_mba(mba_enabled: bool) -> Self {
        Self::with_options(mba_enabled, false)
    }

    /// Create compiler with full options
    pub fn with_options(mba_enabled: bool, substitution_enabled: bool) -> Self {
        let seed = crate::crypto::get_opcode_table().get_seed();

        // Start with one global scope
        let scopes = vec![BTreeMap::new()];

        Self {
            bytecode: Vec::new(),
            stack_operand_depth: 0,
            scratch: std::rc::Rc::new(std::cell::RefCell::new(regs::RegPool::new(248, 8))),
            reg_fastpath_disabled: false,
            arg_offsets: BTreeMap::new(),
            arg_regs: BTreeMap::new(),
            scopes,
            var_types: BTreeMap::new(),
            struct_defs: BTreeMap::new(),
            next_local_reg: 0,
            next_arg_offset: 0,
            labels: BTreeMap::new(),
            fixups: Vec::new(),
            label_counter: 0,
            loop_stack: Vec::new(),
            opcode_table: crate::crypto::get_opcode_table(),
            mba: MbaTransformer::new(seed),
            mba_enabled,
            subst: Substitution::new(seed, substitution_enabled),
            value_cryptor: ValueCryptor::new(seed),
            value_cryptor_enabled: mba_enabled,
            // Opaque predicates enabled for standard+ protection (when substitution is on)
            opaque_predicates_enabled: substitution_enabled,
            native_collector: NativeCallCollector::new(),
        }
    }

    /// Get reference to native call collector
    #[allow(dead_code)]
    pub fn native_collector(&self) -> &NativeCallCollector {
        &self.native_collector
    }

    /// Get mutable reference to native call collector
    #[allow(dead_code)]
    pub fn native_collector_mut(&mut self) -> &mut NativeCallCollector {
        &mut self.native_collector
    }

    /// Current bytecode position
    pub(crate) fn pos(&self) -> usize {
        self.bytecode.len()
    }

    /// Generate unique label
    pub(crate) fn unique_label(&mut self, prefix: &str) -> String {
        let label = format!("{}_{}", prefix, self.label_counter);
        self.label_counter += 1;
        label
    }

    /// Mark current position as label
    pub(crate) fn mark_label(&mut self, name: &str) {
        self.labels.insert(name.to_string(), self.pos());
        // 落点同时进反汇编轨迹：不然只有相对偏移，看不出 JMP/JZ 跳到哪一条
        crate::compiler::emit::trace_label(name);
    }

    /// Register a function argument
    pub(crate) fn register_arg(&mut self, name: &str) {
        self.arg_offsets.insert(name.to_string(), self.next_arg_offset);
        self.next_arg_offset += 8;
    }

    /// 把标量参数在序言里一次性读进寄存器。
    ///
    /// 参数原本是 `InputOffset`：每次使用都发一条 `NATIVE_READ`（一次 native 调用）。
    /// 函数参数不可变，所以在入口读一次即可；寄存器快满时放弃提升，退回 `InputOffset`。
    pub(crate) fn hoist_arg(&mut self, name: &str) {
        if self.next_local_reg >= 240 {
            return;
        }
        let Some(&offset) = self.arg_offsets.get(name) else {
            return;
        };
        let reg = self.next_local_reg;
        self.next_local_reg += 1;
        // 直接读进目标寄存器：原来是 NATIVE_READ + POP_REG，每次形参提升多两次栈操作。
        self.emit_native_read_reg(reg, offset as u16);
        self.arg_regs.insert(name.to_string(), reg);
    }

    // =========================================================================
    // Scope Management
    // =========================================================================

    /// Push a new scope (called at block entry)
    pub(crate) fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    /// Pop current scope and emit cleanup for heap variables
    pub(crate) fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            // Emit HEAP_FREE for variables that need cleanup
            for (_name, info) in scope.iter() {
                if info.needs_cleanup {
                    // Push the heap address from register, then free it
                    self.emit_push_reg(info.reg);
                    self.emit_heap_free();
                }
            }
        }
    }

    /// Get current scope depth (number of active scopes)
    pub(crate) fn current_scope_depth(&self) -> usize {
        self.scopes.len()
    }

    /// Emit cleanup code for all scopes from current down to target_depth (exclusive)
    /// Used for early exits (return, break, continue) to properly free heap memory
    ///
    /// Example: If we have scopes [global, func, loop, inner] (depth=4)
    /// and target_depth=2 (loop level), we clean up [inner, loop] -> scopes at index 3, 2
    pub(crate) fn emit_scope_cleanup(&mut self, target_depth: usize) {
        let current_depth = self.scopes.len();

        // Clean up scopes from innermost to target (exclusive)
        // We iterate backwards from current to target
        if current_depth <= target_depth {
            return; // Nothing to clean up
        }

        // First, collect all registers that need cleanup (to avoid borrow issues)
        let mut regs_to_free = Vec::new();
        for depth in (target_depth..current_depth).rev() {
            if let Some(scope) = self.scopes.get(depth) {
                for (_name, info) in scope.iter() {
                    if info.needs_cleanup {
                        regs_to_free.push(info.reg);
                    }
                }
            }
        }

        // Now emit cleanup code
        for reg in regs_to_free {
            self.emit_push_reg(reg);
            self.emit_heap_free();
        }
    }

    /// Define a variable in the current scope
    pub(crate) fn define_var(&mut self, name: &str, var_type: VarType, is_signed: bool) -> Result<u8, CompileError> {
        self.define_var_internal(name, var_type, is_signed, false)
    }

    /// Define a variable that borrows from another (no cleanup needed)
    /// Used when extracting tuple elements: let inner = t.0
    pub(crate) fn define_var_borrowed(&mut self, name: &str, var_type: VarType, is_signed: bool) -> Result<u8, CompileError> {
        self.define_var_internal(name, var_type, is_signed, true)
    }

    /// Internal variable definition
    fn define_var_internal(&mut self, name: &str, var_type: VarType, is_signed: bool, is_borrowed: bool) -> Result<u8, CompileError> {
        if self.next_local_reg >= 248 {
            return Err(CompileError("Too many local variables (max 248)".to_string()));
        }

        let reg = self.next_local_reg;
        self.next_local_reg += 1;

        // Determine if cleanup is needed - borrowed values never need cleanup
        // Unit structs/tuples (size 0) also don't need cleanup
        let needs_cleanup = if is_borrowed {
            false
        } else {
            match &var_type {
                VarType::String | VarType::Vector => true,
                VarType::Struct(struct_name) => {
                    // Check struct size - unit structs don't need cleanup
                    self.struct_defs.get(struct_name)
                        .map(|def| def.size > 0)
                        .unwrap_or(false)
                }
                VarType::Tuple(elems) => !elems.is_empty(),  // Empty tuple () doesn't need cleanup
                _ => false,
            }
        };

        // Update legacy var_types for compatibility (using reference before move)
        match &var_type {
            VarType::String => {
                self.var_types.insert(name.to_string(), VarLocation::String(reg));
            }
            VarType::Vector => {
                self.var_types.insert(name.to_string(), VarLocation::Array(reg, 8));
            }
            _ => {
                self.var_types.insert(name.to_string(), VarLocation::Register(reg));
            }
        }

        let info = VarInfo {
            reg,
            var_type,
            is_signed,
            needs_cleanup,
        };

        // Add to current scope
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), info);
        }

        Ok(reg)
    }

    /// Lookup a variable in all scopes (innermost first)
    pub(crate) fn lookup_var(&self, name: &str) -> Option<&VarInfo> {
        // Search from innermost to outermost scope
        for scope in self.scopes.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info);
            }
        }
        None
    }

    /// Check if a variable is signed
    pub(crate) fn is_var_signed(&self, name: &str) -> bool {
        if let Some(info) = self.lookup_var(name) {
            return info.is_signed;
        }
        false
    }

    /// Get variable type
    pub(crate) fn get_var_type(&self, name: &str) -> Option<VarType> {
        if let Some(info) = self.lookup_var(name) {
            return Some(info.var_type.clone());
        }
        None
    }

    /// Check if an expression is of boolean type
    pub(crate) fn is_bool_expr(&self, expr: &syn::Expr) -> bool {
        match expr {
            // Boolean literals
            syn::Expr::Lit(lit) => matches!(lit.lit, syn::Lit::Bool(_)),
            // Comparison operators always return bool
            syn::Expr::Binary(bin) => {
                matches!(bin.op,
                    syn::BinOp::Eq(_) | syn::BinOp::Ne(_) |
                    syn::BinOp::Lt(_) | syn::BinOp::Le(_) |
                    syn::BinOp::Gt(_) | syn::BinOp::Ge(_) |
                    syn::BinOp::And(_) | syn::BinOp::Or(_)
                )
            }
            // Variable reference - check type
            syn::Expr::Path(path) => {
                if path.path.segments.len() == 1 {
                    let name = path.path.segments[0].ident.to_string();
                    matches!(self.get_var_type(&name), Some(VarType::Bool))
                } else {
                    false
                }
            }
            // Method calls that return bool
            syn::Expr::MethodCall(mc) => {
                let method = mc.method.to_string();
                matches!(method.as_str(), "is_empty" | "contains" | "starts_with" | "ends_with" | "eq")
            }
            // Function call - we can't easily determine return type at compile time
            // The user should use explicit type annotation: let x: bool = func()
            syn::Expr::Call(_) => false,
            // Nested negation - if inner is bool, outer is bool
            syn::Expr::Unary(unary) => {
                if matches!(unary.op, syn::UnOp::Not(_)) {
                    self.is_bool_expr(&unary.expr)
                } else {
                    false
                }
            }
            // Parenthesized expression
            syn::Expr::Paren(paren) => self.is_bool_expr(&paren.expr),
            _ => false,
        }
    }

    /// Get variable location (argument or local) - uses new scope system
    pub(crate) fn get_var_location(&self, name: &str) -> Option<VarLocation> {
        // Check scopes first (innermost to outermost)
        if let Some(info) = self.lookup_var(name) {
            return match info.var_type {
                VarType::String => Some(VarLocation::String(info.reg)),
                VarType::Vector => Some(VarLocation::Array(info.reg, 8)),
                _ => Some(VarLocation::Register(info.reg)),
            };
        }
        // Check hoisted arguments (read once in the prologue)
        if let Some(&reg) = self.arg_regs.get(name) {
            return Some(VarLocation::Register(reg));
        }
        // Check arguments
        if let Some(&offset) = self.arg_offsets.get(name) {
            return Some(VarLocation::InputOffset(offset));
        }
        // Legacy fallback
        if let Some(loc) = self.var_types.get(name) {
            return Some(loc.clone());
        }
        None
    }

    /// Apply all jump fixups
    pub(crate) fn apply_fixups(&mut self) -> Result<(), CompileError> {
        for (fixup_pos, label) in &self.fixups {
            let target = self.labels.get(label)
                .ok_or_else(|| CompileError(format!("Unknown label: {}", label)))?;

            let from = fixup_pos + 2;
            let offset = (*target as isize) - (from as isize);

            if offset < i16::MIN as isize || offset > i16::MAX as isize {
                return Err(CompileError(format!("Jump offset out of range: {}", offset)));
            }

            let offset_bytes = (offset as i16).to_le_bytes();
            self.bytecode[*fixup_pos] = offset_bytes[0];
            self.bytecode[*fixup_pos + 1] = offset_bytes[1];
        }
        Ok(())
    }

    /// Extract variable name from pattern
    pub(crate) fn extract_pat_name(pat: &Pat) -> Result<String, CompileError> {
        match pat {
            Pat::Ident(pat_ident) => Ok(pat_ident.ident.to_string()),
            Pat::Type(pat_type) => Self::extract_pat_name(&pat_type.pat),
            _ => Err(CompileError("Unsupported pattern in let binding".to_string())),
        }
    }

    /// Finalize compilation
    pub(crate) fn finalize(&mut self) -> Result<Vec<u8>, CompileError> {
        self.apply_fixups()?;

        if self.bytecode.is_empty() || self.bytecode.last().copied() != Some(exec::HALT) {
            self.emit_op(exec::HALT);
        }

        Ok(self.bytecode.clone())
    }
}

// ============================================================================
// Public API - Compile functions
// ============================================================================

/// Compile a function to bytecode (without MBA or substitution)
#[allow(dead_code)]
pub fn compile_function(func: &ItemFn) -> Result<Vec<u8>, CompileError> {
    compile_function_full(func, false, false)
}

/// Compile a function to bytecode with MBA transformations
#[allow(dead_code)]
pub fn compile_function_with_mba(func: &ItemFn) -> Result<Vec<u8>, CompileError> {
    compile_function_full(func, true, false)
}

/// Compile a function with substitution obfuscation
#[allow(dead_code)]
pub fn compile_function_with_substitution(func: &ItemFn) -> Result<Vec<u8>, CompileError> {
    compile_function_full(func, false, true)
}

/// Compile a function with full obfuscation (MBA + Substitution)
#[allow(dead_code)]
pub fn compile_function_paranoid(func: &ItemFn) -> Result<Vec<u8>, CompileError> {
    compile_function_full(func, true, true)
}

/// Compile a function with configurable options
pub fn compile_function_full(func: &ItemFn, mba_enabled: bool, substitution_enabled: bool) -> Result<Vec<u8>, CompileError> {
    let (bytecode, _) = compile_function_with_natives(func, mba_enabled, substitution_enabled)?;
    Ok(bytecode)
}

/// Compilation result including bytecode and native call info
#[allow(dead_code)]
pub struct CompileResult {
    /// Generated bytecode
    pub bytecode: Vec<u8>,
    /// Native call collector with all external function calls
    pub native_collector: NativeCallCollector,
}

/// Compile a function with configurable options, returning native call info
pub fn compile_function_with_natives(func: &ItemFn, mba_enabled: bool, substitution_enabled: bool) -> Result<(Vec<u8>, NativeCallCollector), CompileError> {
    // AEGIS_DUMP=1 时记录逻辑指令流（含操作数），函数返回前打印。
    emit::trace_reset();
    let mut compiler = Compiler::with_options(mba_enabled, substitution_enabled);

    // Register function arguments
    for arg in &func.sig.inputs {
        if let FnArg::Typed(pat_type) = arg {
            if let Pat::Ident(pat_ident) = &*pat_type.pat {
                let name = pat_ident.ident.to_string();
                compiler.register_arg(&name);
                if is_hoistable_arg(&pat_type.ty) {
                    compiler.hoist_arg(&name);
                }
            }
        }
    }

    // Compile function body
    compiler.compile_block(&func.block)?;

    let bytecode = compiler.finalize()?;

    // Extract native collector
    let native_collector = std::mem::take(&mut compiler.native_collector);

    emit::trace_dump(&func.sig.ident.to_string());
    Ok((bytecode, native_collector))
}

#[cfg(test)]
mod arg_prologue_tests {
    use super::*;
    use quote::quote;

    /// 参数作边界的 while 循环：参数应在序言里一次性读进寄存器，
    /// 循环体内不得再出现 NATIVE_READ。
    fn compile_sample() -> Vec<u8> {
        let func: syn::ItemFn = syn::parse2(quote! {
            fn sample(n: u64) -> u64 {
                let mut i = 0u64;
                while i < n {
                    i = i + 1;
                }
                i
            }
        })
        .expect("parse sample fn");
        compile_function_full(&func, false, false).expect("compile sample fn")
    }

    #[test]
    fn arg_bound_loop_bytecode_budget() {
        let code = compile_sample();
        assert!(
            code.len() <= 45,
            "参数边界循环的字节码应被压到 45 字节以内，实际 {} 字节",
            code.len()
        );
    }

    /// 粗略统计某 opcode 值在字节码里出现的次数（操作数可能误计，故断言用上界）。
    fn count_opcode(code: &[u8], op: u8) -> usize {
        code.iter().filter(|&&b| b == op).count()
    }
}

/// 只提升 8 字节标量参数；结构体/切片/引用参数仍按 `InputOffset` 走原路径。
fn is_hoistable_arg(ty: &syn::Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    let Some(segment) = type_path.path.segments.last() else {
        return false;
    };
    matches!(
        segment.ident.to_string().as_str(),
        "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize" | "bool"
    )
}

// TDD：M1 的第一步测试。**先红后绿**——此刻编译器还是栈式求值，所以它必须失败。
#[cfg(test)]
mod regalloc_tdd {
    use super::*;
    use quote::quote;
    use syn::{parse2, ItemFn};

    fn compile_src(src: proc_macro2::TokenStream) -> Vec<u8> {
        let func: ItemFn = parse2(src).expect("测试源码应能解析成函数");
        compile_function_full(&func, false, false).expect("测试源码应能编译")
    }

    /// `x + y`：求值全程走寄存器，栈上只留下两处与表达式无关的既定动作。
    ///
    /// 实测拆解（`stack_moves_split`）：
    /// - 压栈 1 次 = 把返回值交给 `HALT`（`expr.rs:290-300`，HALT 从栈顶取值，属既有约定，M1 不改）；
    /// - 弹栈 0 次 = M3 的成果：`hoist_arg` 改用 `NATIVE_READ_REG` 直接读进寄存器，
    ///   原来这里是「NATIVE_READ + POP_REG」，每个形参两次栈操作。
    /// 所以现在的规格是 (1, 0)。
    #[test]
    fn binop_add_uses_registers_not_stack() {
        emit::reset_stack_moves();
        let _ = compile_src(quote! {
            fn add(x: u64, y: u64) -> u64 { x + y }
        });
        assert_eq!(
            emit::stack_moves_split(),
            (1, 0),
            "x + y：压栈应为 1（返回值交给 HALT），弹栈应为 0（形参直接用 NATIVE_READ_REG 读进寄存器）"
        );
    }

    /// `let` 初始化不许在栈上留残留。
    ///
    /// 曾经 `compile_local` 先无条件 `compile_expr`（值入栈），M4 快路径成功时直接 return，
    /// 那次压栈就没人弹——`let mut x = b;` 实测留下一条 `PUSH_REG`，
    /// 循环体里的 let 每轮涨一格栈（栈无限增长）。
    /// 现在快路径成功前会把那串栈式求值整体回滚，所以压弹必须成对。
    #[test]
    fn let_init_leaves_no_stack_ops() {
        emit::reset_stack_moves();
        let _ = compile_src(quote! {
            fn let_only(a: u64) -> u64 {
                let x = a;
                x
            }
        });
        // 判据是「压 − 弹 == 1」：那 1 次是返回值交给 HALT 的既有约定（与 let 无关）。
        // 曾经 let 的寄存器快路径成功时直接 return，压进去的值没人弹，差值会变成 2——
        // 循环体里的 let 每轮涨一格栈。这条用例守的就是这个差值。
        let (pushes, pops) = emit::stack_moves_split();
        assert_eq!(
            pushes.wrapping_sub(pops),
            1,
            "压/弹应只差返回值那一条（实际 {pushes}/{pops}），差更多就是栈泄漏"
        );
    }

    /// 赋值语句的「值被丢弃」路径：`a = a + b;` 不该出现任何栈搬运。
    ///
    /// 原来所有带分号的语句都编译成 `compile_expr(); DROP;`，
    /// 赋值因此先压一次结果再丢掉——实测 locals_heavy 每次调用 20 次 DUP、63 次 DROP。
    /// 改成 `compile_expr_discarded` 后赋值直接落寄存器，弹栈应为 0。
    #[test]
    fn assign_stmt_drops_no_stack_ops() {
        // 比「有赋值」与「无赋值」两版的差值，而不是绝对值：
        // `let` 本身走栈（压+弹各一条）与本次改动无关，绝对数会随 let 的实现变化，
        // 而「赋值多不多出栈操作」才是这条用例要守的性质。
        emit::reset_stack_moves();
        let _ = compile_src(quote! {
            fn only_let(x: u64, y: u64) -> u64 {
                let mut a = x;
                a
            }
        });
        let base = emit::stack_moves_split();

        emit::reset_stack_moves();
        let _ = compile_src(quote! {
            fn with_assign(x: u64, y: u64) -> u64 {
                let mut a = x;
                a = a + y;
                a
            }
        });
        let with = emit::stack_moves_split();

        assert_eq!(
            with, base,
            "赋值语句不该引入任何栈操作（有赋值 {with:?} vs 无赋值 {base:?}）"
        );
    }

    /// `(a + b) * (c + d)`：中间值全部走寄存器，栈上同样只剩返回值与四个实参的 prologue。
    #[test]
    fn nested_binop_uses_registers_not_stack() {
        emit::reset_stack_moves();
        let _ = compile_src(quote! {
            fn calc(a: u64, b: u64, c: u64, d: u64) -> u64 { (a + b) * (c + d) }
        });
        assert_eq!(
            emit::stack_moves_split(),
            (1, 0),
            "嵌套运算：压栈应为 1（返回值交给 HALT），弹栈应为 0（四个形参同样不再经过栈）"
        );
    }
}
