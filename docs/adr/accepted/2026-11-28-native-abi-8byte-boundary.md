# 决策记录: native call 返回值限定 ≤8 字节且无需 Drop，超限在编译期拒绝

Status: accepted — 删掉 `__to_u64` 的 `Box::into_raw` 兜底，改用 `const { assert!(…) }` 在编译期拒绝两类返回值。

## 问题

`#[vm_protect]` 生成的 native 包装函数只能返回 `u64`（native 表类型是 `fn(&[u64]) -> u64`），所以 `__to_u64` 必须把任意返回类型塞进 64 位。原实现为此留了两条口子：

1. **大小 > 8 字节**：`Box::into_raw(Box::new(value)) as u64` —— 把 host 指针当数值传回 VM。源码注释自己写着 `// leaks memory, but safe for now`。
2. **≤8 字节但 `needs_drop`**（如 `Box<u8>`、自定义含所有权的 8 字节类型）：`mem::forget(value)` 会把它的堆内存漏掉。

实测这两条都不是「潜在」问题：

- `let s = make_string(n); s.len()` —— 编译通过，运行时 `E04: E19`（`HeapOutOfBounds`）。原因是 VM 把这个 host 指针当成**自己的堆地址**去 `heap_read_u64`，必然越界。即：>8 字节返回值**根本不可用**。
- `make_string(n); 7`（丢弃返回值）—— 能跑出正确结果，但每次调用泄漏一个 `Box<String>`，且 VM 侧没有任何回收协议。

也就是说：这条路径要么泄漏，要么报错，没有一种情况是正确的。

## 决策

在 `generate_conversion_helper` 生成的 `__to_u64` 里加两条**编译期**断言，并删掉 `Box::into_raw` 分支：

```rust
const {
    assert!(core::mem::size_of::<T>() <= 8, "…超过 8 字节…请改返回 u64");
    assert!(!core::mem::needs_drop::<T>() || core::mem::size_of::<T>() == 0,
            "…需要 Drop 的所有权类型会泄漏…");
}
```

`const {}` 块在泛型 `T` 上按单态化实例求值，`rustc 1.100.1` 稳定支持（实测：`u64`/`bool`/无 Drop 结构体通过，`String` 与带 `Drop` 的结构体各自报 `E0080`，错误信息为这里写的中文文案）。

同时把该函数体内的 `std::mem` / `std::ptr` 换成 `core::`，让生成的代码在 `no_std` 使用方也能编译。

## 备选方案

- **实现 host 侧 handle 注册表 + 新 opcode**（真正让 >8 字节返回值可用）：本轮不做，但要写清路线，因为这是唯一可行的「真支持」方案。需要：包装函数把值交给运行时注册表换一个 handle（不能再用裸指针，否则 VM 侧把任意 u64 当指针解引用就是 UB）；新增一条 opcode 让 VM 用 handle 换回「把 `size` 字节拷进 VM 堆」并触发回收；`BASE_OPCODES` 需在 `build.rs` 与 `vendor/aegis_vm_macro/src/crypto.rs` 两处同步追加，属宏/运行时同轮升级。**不做的理由**：该能力今天没有任何可用消费者（一用就 `E19`），而每执行分支未命中都会在注册表里留下一个待回收 handle，需要额外的生命周期管理；按「没有真实消费者的新 opcode 不收」处理。等真有负载需要跨 VM 传大对象时再立项。
- **运行期 `panic!` 而不是编译期断言**：被否（次优）。用户只有在跑到那行代码时才发现问题，而这本质是类型层面的错误，编译期就能判定；且 `release` 是 `panic = "abort"`，运行期 panic 会直接终止进程。
- **保持原样**：不可接受。静默泄漏 + 返回不可用值是最坏的组合。
- **只加 `size > 8` 断言，不管 `needs_drop`**：被否。`Box<u8>`（8 字节、有所有权）同样会被 `mem::forget` 漏掉，属同一类错误，一条断言能一起堵住。

## 后果

- **破坏性变更**：任何在 `#[vm_protect]` 函数里调用「返回 >8 字节或含 Drop 类型」的函数，现在**编译期**报错。考虑到该路径本来就只能得到 `E19`，这不是能力回退，而是把静默错误变成明确错误。迁移方式：改为返回 `u64`（例如把 `s` 换成 `s.len() as u64`）。
- 泄漏面消失：生成代码里不再有 `Box::into_raw`，宏 crate 的单测 `conversion_helper_never_leaks_host_pointers` 锁住这一点（一旦回归即红）。
- 正向覆盖：`tests/native_return_abi.rs` 覆盖允许的那一侧（`u64` / `bool` / `u8` 返回值经 VM 往返）。
- 已知限制：负向（编译失败）用例没有自动化。仓库没有 `trybuild`，而 `.stderr` 快照会随 rustc 版本腐烂（本仓不钉工具链）。替代做法是锁生成代码形状 + 在决议里记录手工复现证据（见下）。
- 手工复现（2026-11-28，`rustc 1.100.1`）：对 `#[vm_protect] fn f(n:u64)->u64 { let s = make_string(n); s.len() as u64 }` 编译，得到两条 `error[E0080]: evaluation panicked: aegis_vm: 原生调用返回的类型超过 8 字节…`。
- 仍然存在、本次未处理的相邻限制：宏的表达式编译器无法从 native 调用结果推断结构体类型（`let p = ret_pair(n); p.lo` 报 `Cannot infer struct type from variable 'p'`）。这与 ABI 边界无关，属于编译器能力问题，另立项。
