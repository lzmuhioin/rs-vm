# DEVELOPMENT — 代码地图与维护手册

面向上手者与 agent：只写当前状态（演进历史归 git 与 `docs/adr/`）。常令在 `AGENTS.md`。

## 构建流水线

```text
build.rs (2378 行, 生成器)
  └─> $OUT_DIR/build_config.rs
        ├─ BUILD_SEED / BUILD_ID / BUILD_TIMESTAMP / BUILD_SEQ / GIT_COMMIT / CUSTOMER_ID
        ├─ MAGIC / PROTECTION_LEVEL / MBL_SIZE
        ├─ OPCODE_ENCODE / OPCODE_DECODE      ← 每次构建随机的 opcode 置换与逆映射
        ├─ REGISTER_MAP / REGISTER_REVERSE    ← 寄存器重编号
        ├─ native_ids / flags / FNV 常量
        ├─ ENTROPY_POOL / POOL_SIZE + get_build_seed() / get_smc_domain_hash() / get_nonce_domain_hash()
        └─> src/lib.rs  `include!(concat!(env!("OUT_DIR"), "/build_config.rs"))`
aegis_vm_macro 0.2.52 (`vendor/aegis_vm_macro`，已收进仓库) ── 编译期读 .anticheat_build_seed，用同一套密钥/置换加密字节码
```

- 本仓是**运行时**；编译期（`#[vm_protect]` / `#[obfuscate_strings]` / `aegis_str`）在 `aegis_vm_macro` 里。
- 两端靠 `.anticheat_build_seed`（构建期生成、gitignore）握手；不一致的症状是 `Build ID mismatch`，处理是 `cargo clean` 重建。

## 模块表

| 路径 | 职责 |
|---|---|
| `src/lib.rs` | crate 入口、`compile_error!` 特性护栏、`build_config` 模块、公开 API re-export |
| `src/engine.rs` | 取指-解码-派发循环（`run` / `execute*`），`MAX_INSTRUCTIONS` 上限 |
| `src/handlers/dispatch.rs` | `HANDLER_TABLE[256]`（函数指针表）+ `dispatch_indirect`：先 `OPCODE_DECODE[opcode]` 再间接调用 |
| `src/handlers/{stack,arithmetic,register,memory,heap,vector,string,native,control,convert,special,exec,mutation}.rs` | 各类 opcode 处理函数 |
| `src/state.rs` | `VmState`：R0–R255、值栈、调用栈、自由链表堆、CPU flags、`yield_mask` |
| `src/opcodes.rs` | 规范 opcode 常量（`stack::PUSH_IMM = 0x01` 这类），按类别分子模块；`flags` 转发自 `build_config` |
| `src/bytecode.rs` | 加密字节码封包：magic/version/flags/build id/timestamp/nonce/tag，`ProtectionLevel`、`BuildInfo` |
| `src/crypto.rs` | AES-256-GCM（`aes-gcm`）+ HMAC-SHA256 派生，`CryptoContext` |
| `src/integrity.rs` | 分区域哈希校验 + FNV 常量（**未接入引擎**，需在受保护代码里显式调用 `verify_hash`） |
| `src/smc.rs` | 自修改代码：执行前解密当前指令、执行后重加密，`OPCODE_DECODE` 在这里被消费（**主引擎 `execute()` 不走此模块**，需显式调用 `execute_smc`） |
| `src/whitebox/{cipher,generator,sbox,tables}.rs` | Chow 等白盒 AES；AES 密钥仅存在于编译期，运行期由白盒表 + 64 KiB 熵池重建（`whitebox_decrypt` 未实现，只有加密方向） |
| `src/native.rs` | `NativeRegistry` / `NativeFunction`，native id 来自 `build_config::native_ids` |
| `src/string_obfuscation.rs` | `#[obfuscate_strings]` 的运行时解密与缓存 |
| `src/async_vm/{engine,executor,yielder}.rs` | `async_vm` 特性下的状态机式执行器（实验性） |

规模参考：`src/` 约 7.9k 行；`build.rs` 2.4k 行（生成器，改它等于改构建契约）。

## 特性矩阵

| 特性 | 作用 | 约束 |
|---|---|---|
| `default = std, handler_mutation, whitebox` | 常规保护 | — |
| `whitebox` | 完整白盒 AES | 加密字节码必需 |
| `whitebox_lite` | 轻量（T-boxes，约 50KB） | 与 `whitebox` 二选一，WASM 用这个 |
| `std` | 关闭即 `no_std`（用 `alloc`） | 无 `std` 时不得引入 std 依赖 |
| `async_vm` | 实验性异步执行器 | 有独立 CI job，不进默认门禁 |
| `vm_debug` | VM 调试输出 | 发布产物不要带 |

CI（`.github/workflows/rust.yml`）：`build+test`（默认特性）、`async_vm`、`wasm`（`--no-default-features --features whitebox_lite` + `wasm32-unknown-unknown`）、`clippy`（`--all-targets`）、`docs`（`./scripts/check.sh --docs-only`）。

## 测试布局

- `tests/` 33 个集成测试文件、740 个 `#[test]`：`vm_opcodes`、`handler_mutation_integration`、`smc_tests`、`encrypted_execution`、`whitebox_tests`、`polymorphic`、`integrity`、`stress_test` / `loop_stress` 等。
- `src/` 内嵌 13 个 `#[cfg(test)] mod tests`、54 个单元测试（白盒雪崩、整数运算等）。
- 每个集成测试是独立二进制，`cargo test --quiet` 会分块打印 `test result:`；只看最后一块会漏失败。

## 常用任务

- **加一个 opcode**：`src/opcodes.rs` 常量 → `dispatch.rs` 的 `HANDLER_TABLE` 槽位 + 处理函数 → `tests/vm_opcodes.rs` 断言。**宏侧同步**在 `vendor/aegis_vm_macro`：`src/opcodes.rs` 加同一常量、`src/crypto.rs` 的 `BASE_OPCODES` **只能追加到最后一行**（置换槽位按列表顺序分配，插中间会让既有字节码整体解错，报 `InvalidOpcode`），再写决议。
- **加 native 函数**：`src/native.rs` 的 `standard_ids` + `build.rs` 的 native id 生成，再补测试。
- **跑示例**：`cargo run --example 01_arithmetic`；异步示例加 `--features async_vm`。
- **WASM**：`cd examples/wasm_test && wasm-pack test --node`（需 `rustup target add wasm32-unknown-unknown`）。
- **可复现构建**：`ANTICHEAT_BUILD_KEY=<key>`（必要时加 `SOURCE_DATE_EPOCH`），产出同 opcode 表与同密钥。

## 已知基线（写文档/改门禁时别当成新问题）

- `cargo clippy --all-targets -- -D warnings`：**绿**（2026-11-23 修掉 17 处 + 30 处；清单见 `docs/adr/accepted/2026-11-23-gate-baseline.md`）。本机与 CI 都追最新 stable（不钉 `rust-toolchain.toml`，`scripts/check.sh` 首行打印 rustc/cargo/clippy 版本），未使用 `#[allow]` 豁免。
- 依赖：`hex` 已删除（`build-dependencies` / `dev-dependencies` 均无 `hex::` 引用，clippy 1.100 报 unused dependency）。
- `cargo fmt --check`：**红，723 处差异**；因此不进主门禁（见 `docs/adr/accepted/2026-11-23-gate-baseline.md`）。
- 未钉 `rust-toolchain.toml`；`Cargo.lock` 被 gitignore（库 crate 惯例），依赖漂移只在 CI 首次构建时暴露。
- **可复现构建只覆盖 seed**：`ANTICHEAT_BUILD_KEY` 固定 build seed 与 opcode 置换，但熵池/增量取自 `/dev/urandom`，`build_config.rs` 不逐字节可复现。
- **`CARGO_TARGET_DIR` 目录必须字面叫 `target`**：`build.rs::write_shared_seed` 只在 OUT_DIR 的祖先里找名叫 `target` 的目录，找不到就不写 `.anticheat_build_seed`；随后 `#[vm_protect]` 在编译期 panic（`vm-macro: Could not find shared build seed file`）。在外部 crate 里用 `#[vm_protect]`、或换 target 目录都会撞到，不是编译警告而是宏 panic。
- **性能基线（绑核实测）**：纯算术调用 ~0.46k×、`fib(20)` ~0.72k×、`sum(2000)` ~0.1k×（standard）；per-call 固定 0.4–0.6 µs、per-VM-instruction 18–23 ns、冷启动 1–2 ms/函数。测 VM 性能必须 `taskset` 绑核，否则同一代码能差 2–4 倍。数值随机器不同，引用前重测。

## 提交约定

- 一次改动一个主题；commit message 带决议标识（如 `adr:2026-11-23-gate-baseline`）。
- 提交前 `./scripts/check.sh`；不提交 `.anticheat_build_seed` 与 `target/`。