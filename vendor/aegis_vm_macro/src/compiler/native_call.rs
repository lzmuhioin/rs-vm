//! Native Call Support
//!
//! Enables calling external Rust functions from VM-protected code.
//!
//! # How It Works
//!
//! 1. Macro collects all function calls in the protected function
//! 2. Each unique call gets an index in the native table
//! 3. NATIVE_CALL <index> <arg_count> bytecode is emitted
//! 4. At runtime, the native table maps index -> actual function
//!
//! # Example
//!
//! ```ignore
//! // Input:
//! #[vm_protect]
//! fn protected() -> u64 {
//!     is_license_valid() as u64
//! }
//!
//! // Generated:
//! fn protected() -> u64 {
//!     fn __native_0(args: &[u64]) -> u64 {
//!         if is_license_valid() { 1 } else { 0 }
//!     }
//!     static __NATIVE_TABLE: [fn(&[u64]) -> u64; 1] = [__native_0];
//!     __aegis_execute(&BYTECODE, &__NATIVE_TABLE)
//! }
//! ```

use proc_macro2::TokenStream;
use quote::{quote, format_ident};
use syn::{Expr, ExprCall, ExprMethodCall};
use std::collections::HashMap;

use super::CompileError;

/// Type of function call
#[derive(Debug, Clone, PartialEq)]
pub enum CallKind {
    /// Regular function call: foo(), module::func()
    Function,
    /// Method call: obj.method()
    Method,
    /// Associated function: Type::func()
    AssociatedFn,
    /// Closure call: closure_var()
    Closure,
}

/// Information about a native call
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct NativeCallInfo {
    /// Index in the native table
    pub index: usize,
    /// The original call expression (for wrapper generation)
    pub call_expr: Expr,
    /// Number of arguments
    pub arg_count: usize,
    /// Type of call
    pub call_kind: CallKind,
    /// String key for deduplication
    pub call_key: String,
}

/// Collects and manages native calls
pub struct NativeCallCollector {
    /// All registered native calls
    calls: Vec<NativeCallInfo>,
    /// Map from call key to index (for deduplication)
    call_map: HashMap<String, usize>,
}

impl NativeCallCollector {
    /// Create a new collector
    pub fn new() -> Self {
        Self {
            calls: Vec::new(),
            call_map: HashMap::new(),
        }
    }

    /// Check if there are any native calls
    pub fn has_calls(&self) -> bool {
        !self.calls.is_empty()
    }

    /// Get number of native calls
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.calls.len()
    }

    /// Register a function call, returns the index
    pub fn register_call(&mut self, call: &ExprCall) -> Result<usize, CompileError> {
        // Generate a unique key for this call
        let call_key = self.call_to_key_expr(&Expr::Call(call.clone()));

        // Check if already registered (dedup)
        if let Some(&index) = self.call_map.get(&call_key) {
            return Ok(index);
        }

        // Determine call kind
        let call_kind = self.determine_call_kind(&call.func);

        let index = self.calls.len();
        let info = NativeCallInfo {
            index,
            call_expr: Expr::Call(call.clone()),
            arg_count: call.args.len(),
            call_kind,
            call_key: call_key.clone(),
        };

        self.calls.push(info);
        self.call_map.insert(call_key, index);

        Ok(index)
    }

    /// Register a method call, returns the index
    pub fn register_method(&mut self, method: &ExprMethodCall) -> Result<usize, CompileError> {
        // Generate a unique key for this call
        let call_key = self.method_to_key(method);

        // Check if already registered (dedup)
        if let Some(&index) = self.call_map.get(&call_key) {
            return Ok(index);
        }

        let index = self.calls.len();
        let info = NativeCallInfo {
            index,
            call_expr: Expr::MethodCall(method.clone()),
            arg_count: method.args.len() + 1, // +1 for receiver
            call_kind: CallKind::Method,
            call_key: call_key.clone(),
        };

        self.calls.push(info);
        self.call_map.insert(call_key, index);

        Ok(index)
    }

    /// Generate wrapper functions for all native calls
    pub fn generate_wrappers(&self) -> TokenStream {
        let wrappers: Vec<TokenStream> = self.calls.iter().map(|info| {
            let wrapper_name = format_ident!("__native_{}", info.index);

            match &info.call_expr {
                Expr::Call(call) => self.generate_call_wrapper(&wrapper_name, call, info),
                Expr::MethodCall(method) => self.generate_method_wrapper(&wrapper_name, method, info),
                _ => quote! {},
            }
        }).collect();

        quote! {
            #(#wrappers)*
        }
    }

    /// Generate the native table
    pub fn generate_table(&self) -> TokenStream {
        if self.calls.is_empty() {
            return quote! {
                let __native_table: &[fn(&[u64]) -> u64] = &[];
            };
        }

        let wrapper_refs: Vec<TokenStream> = self.calls.iter().map(|info| {
            let wrapper_name = format_ident!("__native_{}", info.index);
            quote! { #wrapper_name }
        }).collect();

        let count = self.calls.len();

        quote! {
            let __native_table: [fn(&[u64]) -> u64; #count] = [
                #(#wrapper_refs),*
            ];
        }
    }

    /// Generate wrapper for a function call
    fn generate_call_wrapper(&self, wrapper_name: &syn::Ident, call: &ExprCall, _info: &NativeCallInfo) -> TokenStream {
        let func = &call.func;
        let arg_count = call.args.len();

        // Generate argument extraction
        let arg_extracts: Vec<TokenStream> = (0..arg_count).map(|i| {
            quote! { args[#i] }
        }).collect();

        // Generate the call
        let call_expr = if arg_extracts.is_empty() {
            quote! { #func() }
        } else {
            quote! { #func(#(#arg_extracts as _),*) }
        };

        // Wrapper function - convert result to u64
        quote! {
            #[inline(never)]
            fn #wrapper_name(args: &[u64]) -> u64 {
                let __result = #call_expr;
                __to_u64(__result)
            }
        }
    }

    /// Generate wrapper for a method call
    fn generate_method_wrapper(&self, wrapper_name: &syn::Ident, method: &ExprMethodCall, _info: &NativeCallInfo) -> TokenStream {
        let method_name = &method.method;
        let arg_count = method.args.len();

        // args[0] is the receiver, args[1..] are the method arguments
        let receiver_extract = quote! { args[0] };

        let arg_extracts: Vec<TokenStream> = (0..arg_count).map(|i| {
            let idx = i + 1; // Skip receiver
            quote! { args[#idx] }
        }).collect();

        // Generate the method call
        // We need to reconstruct the receiver type - for now use unsafe pointer cast
        let call_expr = if arg_extracts.is_empty() {
            quote! {
                let __receiver_ptr = #receiver_extract as *mut ();
                // Method call on raw pointer - caller must ensure safety
                unsafe { (*(__receiver_ptr as *mut _)).#method_name() }
            }
        } else {
            quote! {
                let __receiver_ptr = #receiver_extract as *mut ();
                unsafe { (*(__receiver_ptr as *mut _)).#method_name(#(#arg_extracts as _),*) }
            }
        };

        quote! {
            #[inline(never)]
            fn #wrapper_name(args: &[u64]) -> u64 {
                let __result = #call_expr;
                __to_u64(__result)
            }
        }
    }

    /// Generate the u64 conversion helper
    pub fn generate_conversion_helper() -> TokenStream {
        quote! {
            // Convert any type to u64
            //
            // native ABI 只有 64 位通道，而且值是按位拷贝过去的。因此两类返回值
            // 都无法安全跨 VM 传递，必须在编译期拒绝：
            //  - 大小 > 8 字节：原实现用 `Box::into_raw(boxed) as u64` 把 host 指针
            //    塞进 u64。它既泄漏内存（VM 侧没有回收协议），又无法使用 ——
            //    VM 会把这个 host 指针当成自己的堆地址，读它时报 HeapOutOfBounds。
            //    实测：`let s = make_string(n); s.len()` 直接 E04: E19。
            //  - 需要 Drop 但 <= 8 字节（如 `Box<u8>`）：`mem::forget` 会把它的
            //    堆内存漏掉。
            #[inline(always)]
            fn __to_u64<T>(value: T) -> u64 {
                const {
                    assert!(
                        core::mem::size_of::<T>() <= 8,
                        "aegis_vm: 原生调用返回的类型超过 8 字节，64 位 ABI 承载不了。\
                         请改返回 u64（例如 `s.len() as u64`）。\
                         旧的 Box::into_raw 兜底会泄漏内存，且该指针不能被当作 VM 堆地址使用。"
                    );
                    assert!(
                        !core::mem::needs_drop::<T>() || core::mem::size_of::<T>() == 0,
                        "aegis_vm: 原生调用返回了需要 Drop 的所有权类型，跨 64 位通道传递时\
                         它会被 mem::forget 掉并泄漏。请改返回 u64 或 Copy 类型。"
                    );
                }
                // 编译期断言已经保证 size <= 8；ZST（unit 等）直接给 0。
                if core::mem::size_of::<T>() == 0 {
                    return 0;
                }
                let mut result = 0u64;
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &value as *const T as *const u8,
                        &mut result as *mut u64 as *mut u8,
                        core::mem::size_of::<T>(),
                    );
                }
                // 值已被按位拷进 result，防止 T 的析构再释放一次
                core::mem::forget(value);
                result
            }

            // Convert u64 back to bool
            #[inline(always)]
            fn __from_u64_bool(value: u64) -> bool {
                value != 0
            }
        }
    }

    /// Determine the kind of call from the function expression
    fn determine_call_kind(&self, func: &Expr) -> CallKind {
        match func {
            Expr::Path(path) => {
                // Check if it's an associated function (has ::)
                if path.path.segments.len() > 1 {
                    CallKind::AssociatedFn
                } else {
                    // Could be a closure or regular function
                    // For now, treat single-segment paths as functions
                    CallKind::Function
                }
            }
            _ => CallKind::Closure,
        }
    }

    /// Generate a unique key for a call expression
    fn call_to_key_expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::Call(call) => {
                let func_key = self.expr_to_key(&call.func);
                let args_key: Vec<String> = call.args.iter()
                    .map(|arg| self.expr_to_key(arg))
                    .collect();
                format!("call:{}({})", func_key, args_key.join(","))
            }
            _ => format!("{:?}", expr),
        }
    }

    /// Generate a unique key for a method call
    fn method_to_key(&self, method: &ExprMethodCall) -> String {
        let receiver_key = self.expr_to_key(&method.receiver);
        let method_name = method.method.to_string();
        let args_key: Vec<String> = method.args.iter()
            .map(|arg| self.expr_to_key(arg))
            .collect();
        format!("method:{}.{}({})", receiver_key, method_name, args_key.join(","))
    }

    /// Generate a simple string key for an expression (for dedup)
    fn expr_to_key(&self, expr: &Expr) -> String {
        match expr {
            Expr::Path(path) => {
                path.path.segments.iter()
                    .map(|s| s.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::")
            }
            Expr::Lit(lit) => format!("{:?}", lit.lit),
            _ => format!("{:p}", expr), // Fallback: use pointer address for uniqueness
        }
    }
}

impl Default for NativeCallCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collector_new() {
        let collector = NativeCallCollector::new();
        assert_eq!(collector.len(), 0);
        assert!(!collector.has_calls());
    }

    /// 生成代码不得再出现「把 host 指针塞进 u64」的兜底。
    ///
    /// 这条断言替代不了编译失败测试（仓库没有 trybuild，且 `.stderr` 快照会随
    /// rustc 版本腐烂），但它锁住了泄漏的根因：只要装箱调用回到生成代码里，
    /// 测试就红。
    ///
    /// 注意只查**调用形状**（`into_raw (` / `Box :: new (`）：断言文案里本身会
    /// 提到这两个名字来解释「为什么不允许」，查裸词会误判自己。
    #[test]
    fn conversion_helper_never_leaks_host_pointers() {
        let emitted = NativeCallCollector::generate_conversion_helper().to_string();
        assert!(
            !emitted.contains("into_raw ("),
            "生成的 __to_u64 里重新出现了 into_raw 调用：>8 字节返回值会再次泄漏"
        );
        assert!(
            !emitted.contains("Box :: new ("),
            "生成的 __to_u64 里重新出现了 Box::new 调用：说明又在堆上装箱"
        );
        // 编译期拦截必须存在，且同时覆盖「太大」与「需要 Drop」两种
        assert!(emitted.contains("size_of"), "缺少 size_of 编译期断言");
        assert!(emitted.contains("needs_drop"), "缺少 needs_drop 编译期断言");
        assert!(emitted.contains("assert !"), "编译期断言未生成");
        // 用户看到的报错文案必须讲清「改返回 u64」，否则等于让人猜
        assert!(
            emitted.contains("请改返回 u64"),
            "编译期断言的文案丢失了迁移指引"
        );
    }
}
