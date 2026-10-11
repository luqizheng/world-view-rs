//! # wv-edit —— World View 编辑层
//!
//! 编辑命令、Undo/Redo、事务与脏数据集合。
//!
//! 核心契约（原则六：非破坏编辑）：
//! - 所有修改必须表达为 `EditCommand`，不得直接改 `World`。
//! - Undo 不是「反向操作」，而是命令栈的回退——因为历史只被追加，不被覆盖。
//! - 写盘由 `dirty set` 驱动，不做每帧写盘。

pub mod command;
pub mod history;

pub use command::{EditCommand, EditError, EditResult};
pub use history::{History, Transaction};

/// 脏数据集合：记录哪些文件在下次保存时需要写回。
///
/// 以「相对工程根的路径」为键，而非实体 ID：
/// 落盘时只关心文件，不关心领域语义。
#[derive(Debug, Default, Clone)]
pub struct DirtySet {
    paths: std::collections::BTreeSet<String>,
}

impl DirtySet {
    pub fn mark(&mut self, rel_path: impl Into<String>) {
        self.paths.insert(rel_path.into());
    }

    pub fn clear(&mut self) {
        self.paths.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &String> {
        self.paths.iter()
    }
}
