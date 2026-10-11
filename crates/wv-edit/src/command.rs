//! 编辑命令。
//!
//! 每个修改动作都实现 `EditCommand`。命令必须满足：
//! - 可序列化（用于事务日志与快照）；
//! - 可逆（或声明为不可逆，走「追加区间」路径）；
//! - 不直接触碰文件系统（那是 `wv-io` 的职责）。

use wv_core::World;

pub type EditResult<T> = Result<T, EditError>;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("命令作用对象不存在: {0}")]
    TargetMissing(String),

    #[error("命令不满足前置条件: {0}")]
    Precondition(String),

    #[error("时间区间冲突: {0}")]
    SpanConflict(String),
}

/// 编辑命令 trait。
///
/// `apply` 就地修改世界；`revert` 必须能把世界恢复到 `apply` 之前的状态。
/// 实现者若无法提供 `revert`，应改为「追加一个区间」而非覆盖。
pub trait EditCommand: std::fmt::Debug {
    /// 命令的人类可读描述，用于 Undo/Redo UI。
    fn label(&self) -> String;

    /// 执行。返回受影响的文件相对路径，供 `DirtySet` 使用。
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>>;

    /// 回退。
    fn revert(&self, world: &mut World) -> EditResult<()>;
}
