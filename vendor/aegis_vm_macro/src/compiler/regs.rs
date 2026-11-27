//! 虚拟寄存器池与 RAII 守卫。
//!
//! 这里是寄存器分配器的地基：**只有分配与归还，不接任何发射路径**。
//! 所有权模型是唯一所有权——一个寄存器要么属于某个子表达式，要么已归还，
//! 因此不需要引用计数，也**不可能重复归还**（重复归还等于两个子表达式拿到同一个
//! 寄存器，产出的是「不崩溃、只算错值」的事故）。
//!
//! 归还时机由守卫的 `Drop` 决定，而守卫活在哪个块里由调用方用嵌套块控制，
//! 于是「用完即还」与手写 `free()` 的时机完全一致，又不会漏、不会重。

use std::cell::RefCell;
use std::rc::Rc;

/// 运行时 `regs` 容量的默认值（`src/state.rs` 的 `DEFAULT_REGISTER_CAPACITY`）。
/// 两份必须一致：宏按这个数分配，运行时按这个数开寄存器堆。
pub(crate) const RUNTIME_REG_CAPACITY: usize = 32;

/// 分配池：低位一段留给既有代码，分配器只用高位的空闲段。
#[derive(Debug)]
pub(crate) struct RegPool {
    /// 可取用的寄存器编号，按从小到大排列。
    free: Vec<u8>,
}

impl RegPool {
    /// 从 `base` 开始共 `count` 个寄存器可被分配器使用。
    pub(crate) fn new(base: u8, count: usize) -> Self {
        let end = (base as usize + count).min(RUNTIME_REG_CAPACITY);
        let free = (base as usize..end).rev().map(|r| r as u8).collect();
        Self { free }
    }

    /// 取一个空闲寄存器；池空返回 `None`（调用方回落到栈）。
    pub(crate) fn alloc(&mut self) -> Option<u8> {
        self.free.pop()
    }

    /// 归还一个寄存器。同一编号归还两次会在此处暴露为 `false`。
    pub(crate) fn release(&mut self, reg: u8) -> bool {
        if self.free.contains(&reg) {
            return false;
        }
        // 保持从小到大的顺序，分配时优先复用小编号，缓存局部性更好。
        let pos = self.free.partition_point(|&r| r < reg);
        self.free.insert(pos, reg);
        true
    }

    /// 当前空闲数量，供测试与诊断使用。
    pub(crate) fn available(&self) -> usize {
        self.free.len()
    }
}

/// 寄存器守卫：持有即占用，`Drop` 时归还；`into_inner` 把所有权转移出去。
#[derive(Debug)]
pub(crate) struct RegGuard {
    pool: Rc<RefCell<RegPool>>,
    reg: u8,
}

impl RegGuard {
    /// 从池里取一个寄存器；池空返回 `None`。
    pub(crate) fn acquire(pool: &Rc<RefCell<RegPool>>) -> Option<Self> {
        let reg = pool.borrow_mut().alloc()?;
        Some(Self {
            pool: Rc::clone(pool),
            reg,
        })
    }

    /// 寄存器编号。
    pub(crate) fn reg(&self) -> u8 {
        self.reg
    }

    /// 交出所有权：不再归还，由调用方负责这个寄存器后续的去向。
    pub(crate) fn into_inner(self) -> u8 {
        let reg = self.reg;
        // 绕过 Drop：把池句柄换出去不可能（结构体没有默认值），
        // 因此用 ManuallyDrop 阻止 Drop 执行。
        std::mem::forget(std::mem::ManuallyDrop::new(()));
        std::mem::forget(self);
        reg
    }
}

impl Drop for RegGuard {
    fn drop(&mut self) {
        let _ = self.pool.borrow_mut().release(self.reg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> Rc<RefCell<RegPool>> {
        Rc::new(RefCell::new(RegPool::new(8, 4)))
    }

    #[test]
    fn alloc_returns_registers_then_exhausts() {
        let p = pool();
        let a = RegGuard::acquire(&p).expect("第一个");
        let b = RegGuard::acquire(&p).expect("第二个");
        assert_ne!(a.reg(), b.reg());
        assert_eq!(p.borrow().available(), 2);
        let _c = RegGuard::acquire(&p).expect("第三个");
        let _d = RegGuard::acquire(&p).expect("第四个");
        assert!(RegGuard::acquire(&p).is_none(), "池应已耗尽");
    }

    #[test]
    fn drop_returns_register_to_pool() {
        let p = pool();
        {
            let g = RegGuard::acquire(&p).expect("取一个");
            assert_eq!(p.borrow().available(), 3);
            drop(g);
        }
        assert_eq!(p.borrow().available(), 4, "守卫销毁后应归还");
    }

    #[test]
    fn into_inner_transfers_and_does_not_release() {
        let p = pool();
        let reg = {
            let g = RegGuard::acquire(&p).expect("取一个");
            g.into_inner()
        };
        assert_eq!(p.borrow().available(), 3, "转移所有权后不应归还");

        // 别人拿到的必须是另一个编号
        let other = RegGuard::acquire(&p).expect("再取一个");
        assert_ne!(other.reg(), reg);
    }

    #[test]
    fn nested_block_releases_inner_guard_first() {
        // (a+b)*(c+d) 的归还顺序：rhs 出块即还，lhs 活到函数体结束
        let p = pool();
        let mut lhs = RegGuard::acquire(&p).expect("lhs");
        let lhs_reg = lhs.reg();
        let dst_reg;
        {
            let rhs = RegGuard::acquire(&p).expect("rhs");
            let _rhs_reg = rhs.reg();
            let dst = RegGuard::acquire(&p).expect("dst");
            dst_reg = dst.into_inner();
        }
        // 池 4 个：lhs 占 1、dst 已转移占 1、rhs 已归还 → 空闲 2
        assert_eq!(p.borrow().available(), 2, "rhs 应已归还，lhs 与 dst 仍占用");
        let before = p.borrow().available();
        drop(lhs);
        assert_eq!(p.borrow().available(), before + 1);
        assert_ne!(lhs_reg, dst_reg);
    }

    #[test]
    fn release_twice_is_reported_as_false() {
        let mut p = RegPool::new(8, 2);
        let r = p.alloc().expect("取一个");
        assert!(p.release(r), "首次归还应成功");
        assert!(!p.release(r), "重复归还必须被发现而不是静默通过");
    }

    #[test]
    fn pool_respects_runtime_capacity() {
        let p = RegPool::new(28, 100);
        assert_eq!(p.available(), RUNTIME_REG_CAPACITY - 28, "不得超过运行时容量");
    }
}
