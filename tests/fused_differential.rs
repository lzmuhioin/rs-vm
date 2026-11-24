//! S3-real：真实后端上的差分测试。
//!
//! 玩具 IR 的差分骨架只证明了「方法可行」。这里换成真实的宏 + 真实运行时：
//! 每个 `#[vm_protect]` 函数都配一个**语义契约相同的原生孪生体**，用确定性随机语料
//! 逐值比对。语料刻意覆盖边界：0、1、2 的幂、`u64::MAX`、移位量 63/64/65、
//! 除零与模零。
//!
//! 注意 oracle 不是「Rust 语义」而是「VM 契约语义」：整数回绕、除零/模零得 0、
//! 移位量按 64 取模。孪生体照这个契约写，否则测的是两套不同的规范。

use aegis_vm_macro::vm_protect;

// ---- 原生孪生体（VM 契约口径）----

fn ndiv(a: u64, b: u64) -> u64 {
    a.checked_div(b).unwrap_or(0)
}
fn nmod(a: u64, b: u64) -> u64 {
    if b == 0 {
        0
    } else {
        a % b
    }
}

// ---- 被测函数（真实宏编译）----

#[vm_protect]
fn d_sub(a: u64, b: u64) -> u64 {
    a - b
}

#[vm_protect]
fn d_divmod(a: u64, b: u64) -> u64 {
    let q = a / b;
    let r = a % b;
    q ^ r
}

#[vm_protect]
fn d_bits(a: u64, b: u64) -> u64 {
    let x = a & b;
    let y = a | b;
    let z = a ^ b;
    x + y + z
}

#[vm_protect]
fn d_shift(a: u64, b: u64) -> u64 {
    let l = a << b;
    let r = a >> b;
    l ^ r
}

/// 全字面量 RHS：专门压 `*_REG_IMM`
#[vm_protect]
fn d_imm(a: u64, b: u64) -> u64 {
    let mut x = a;
    x = x & 255;
    x = x | 7;
    x = x ^ b;
    x = x + 5;
    x = x - 2;
    x = x / 7;
    x = x % 13;
    x = x << 3;
    x = x >> 1;
    x
}

/// 逐条打满 8 条立即数指令：每个算子都配一个**字面量** RHS，
/// 少一条就说明语料没覆盖到那条 handler（变异自检会暴露）
#[vm_protect]
fn d_imm_each(a: u64) -> u64 {
    let mut x = a;
    x = x - 3;
    x = x / 2;
    x = x % 5;
    x = x & 0xF0F0;
    x = x | 0x0F0F;
    x = x ^ 0x5A5A;
    x = x << 2;
    x = x >> 1;
    x
}

fn native_d_imm_each(a: u64) -> u64 {
    let mut x = a;
    x = x.wrapping_sub(3);
    x = ndiv(x, 2);
    x = nmod(x, 5);
    x &= 0xF0F0;
    x |= 0x0F0F;
    x ^= 0x5A5A;
    x = x.wrapping_shl(2);
    x = x.wrapping_shr(1);
    x
}

/// 嵌套表达式：右侧是子表达式，走临时寄存器路径
#[vm_protect]
fn d_nested(a: u64, b: u64) -> u64 {
    (a - (b ^ 7)) | (a & b) / (b + 1)
}

/// 循环 + 混合算子（寄存器与立即数两条路都要走）
#[vm_protect]
fn d_loop(a: u64) -> u64 {
    let mut x = a | 1;
    let mut i = 0u64;
    while i < 40 {
        x = x ^ (x >> 3);
        x &= 0xFFFF_FFFFu64;
        x |= i;
        x = x / 7;
        x = x + 11;
        i = i + 1;
    }
    x
}

fn native_d_loop(a: u64) -> u64 {
    let mut x = a | 1;
    let mut i = 0u64;
    while i < 40 {
        x = x ^ (x >> 3);
        x &= 0xFFFF_FFFF;
        x |= i;
        x = ndiv(x, 7);
        x = x.wrapping_add(11);
        i += 1;
    }
    x
}

// ---- 语料 ----

const EDGES: [u64; 14] = [
    0,
    1,
    2,
    3,
    7,
    8,
    13,
    63,
    64,
    65,
    255,
    256,
    u64::MAX,
    u64::MAX - 1,
];

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 17
    }

    /// 一半取边界值，一半取全空间随机值
    fn value(&mut self) -> u64 {
        if self.next() & 1 == 0 {
            EDGES[(self.next() % EDGES.len() as u64) as usize]
        } else {
            self.next()
        }
    }
}

const CASES: usize = 20_000;

#[test]
fn sub_matches_contract() {
    let mut rng = Rng(0x5EED_0001);
    for _ in 0..CASES {
        let (a, b) = (rng.value(), rng.value());
        assert_eq!(d_sub(a, b), a.wrapping_sub(b), "({a}, {b})");
    }
}

#[test]
fn divmod_matches_contract() {
    let mut rng = Rng(0x5EED_0002);
    for _ in 0..CASES {
        let (a, b) = (rng.value(), rng.value());
        assert_eq!(d_divmod(a, b), ndiv(a, b) ^ nmod(a, b), "({a}, {b})");
    }
}

#[test]
fn bitwise_matches_contract() {
    let mut rng = Rng(0x5EED_0003);
    for _ in 0..CASES {
        let (a, b) = (rng.value(), rng.value());
        let native = (a & b).wrapping_add(a | b).wrapping_add(a ^ b);
        assert_eq!(d_bits(a, b), native, "({a}, {b})");
    }
}

#[test]
fn shifts_match_contract() {
    let mut rng = Rng(0x5EED_0004);
    for _ in 0..CASES {
        let (a, b) = (rng.value(), rng.value());
        let native = a.wrapping_shl(b as u32) ^ a.wrapping_shr(b as u32);
        assert_eq!(d_shift(a, b), native, "({a}, {b})");
    }
}

#[test]
fn immediate_forms_match_contract() {
    let mut rng = Rng(0x5EED_0005);
    for _ in 0..CASES {
        let (a, b) = (rng.value(), rng.value());
        let mut x = a;
        x &= 255;
        x |= 7;
        x ^= b;
        x = x.wrapping_add(5);
        x = x.wrapping_sub(2);
        x = ndiv(x, 7);
        x = nmod(x, 13);
        x = x.wrapping_shl(3);
        x = x.wrapping_shr(1);
        assert_eq!(d_imm(a, b), x, "({a}, {b})");
    }
}

#[test]
fn nested_matches_contract() {
    let mut rng = Rng(0x5EED_0006);
    for _ in 0..CASES {
        let (a, b) = (rng.value(), rng.value());
        let native = a.wrapping_sub(b ^ 7) | ndiv(a & b, b.wrapping_add(1));
        assert_eq!(d_nested(a, b), native, "({a}, {b})");
    }
}

#[test]
fn loop_matches_contract() {
    let mut rng = Rng(0x5EED_0007);
    for _ in 0..2_000 {
        let a = rng.value();
        assert_eq!(d_loop(a), native_d_loop(a), "a={a}");
    }
}

#[test]
fn each_imm_opcode_matches_contract() {
    let mut rng = Rng(0x5EED_0008);
    for _ in 0..CASES {
        let a = rng.value();
        assert_eq!(d_imm_each(a), native_d_imm_each(a), "a={a}");
    }
}
