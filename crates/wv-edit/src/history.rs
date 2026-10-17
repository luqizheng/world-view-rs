//! Undo/Redo 命令栈与事务。

use wv_core::World;

use crate::command::{EditCommand, EditResult};

/// 一次事务：一组要么全成功、要么全回滚的命令。
///
/// `committed_dirty` 在 [`Self::commit`] 成功后填入当时收集的脏路径，
/// 供 [`History::undo`] / [`History::redo`] 在回退/重做时报告——
/// 这样调用方（GUI 顶栏的 `●` 标脏、保存路径优化、HTTP `undo` skill 等）
/// 不必各自维护「哪些事务产生了哪些 dirty 集合」的影子表。
pub struct Transaction {
    commands: Vec<Box<dyn EditCommand>>,
    /// `commit` 成功后填入；`commit` 未被调用过则 `None`。
    committed_dirty: Option<Vec<String>>,
    /// 提交序号（由 [`History::commit`] 分配）：应用层用它辨认「撤销/重做
    /// 的是不是同一条命令」（大陆生成的撤销恢复圈依赖它）。
    seq: u64,
}

impl Transaction {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            committed_dirty: None,
            seq: 0,
        }
    }

    pub fn push(&mut self, cmd: Box<dyn EditCommand>) {
        self.commands.push(cmd);
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// 顺序执行全部命令；任一失败则逆序回滚已成功的部分。
    pub fn commit(&mut self, world: &mut World) -> EditResult<Vec<String>> {
        let mut applied: Vec<&Box<dyn EditCommand>> = Vec::new();
        let mut dirty = Vec::new();
        for cmd in &self.commands {
            match cmd.apply(world) {
                Ok(paths) => {
                    dirty.extend(paths);
                    applied.push(cmd);
                }
                Err(e) => {
                    for done in applied.iter().rev() {
                        let _ = done.revert(world);
                    }
                    return Err(e);
                }
            }
        }
        self.committed_dirty = Some(dirty.clone());
        Ok(dirty)
    }

    /// 取回 `commit` 时收集的脏路径（`commit` 成功后才有值）。
    pub fn committed_dirty(&self) -> Option<&[String]> {
        self.committed_dirty.as_deref()
    }
}

impl Default for Transaction {
    fn default() -> Self {
        Self::new()
    }
}

/// Undo/Redo 栈。
///
/// 只保存**已提交**的事务。命令栈是编辑历史的权威来源，
/// 快照只是它的性能优化产物，不是替代品。
#[derive(Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// 下一个事务序号（单调递增；undo/redo 不重排）。
    next_seq: u64,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// 提交一个事务并压入 Undo 栈，清空 Redo 栈。
    pub fn commit(&mut self, world: &mut World, mut tx: Transaction) -> EditResult<Vec<String>> {
        if tx.is_empty() {
            return Ok(Vec::new());
        }
        let dirty = tx.commit(world)?;
        tx.seq = self.next_seq;
        self.next_seq += 1;
        self.undo.push(tx);
        self.redo.clear();
        Ok(dirty)
    }

    /// 已提交（在 Undo 栈上）的事务数。
    ///
    /// 应用层用它判定「撤销的是不是某一条特定命令」：大陆生成把提交后的
    /// 长度记进 stash，撤销后长度回落才恢复被消费的意图包络。
    pub fn len(&self) -> usize {
        self.undo.len()
    }

    /// Undo 栈是否为空（与 [`Self::len`] 配对，满足 clippy `len_without_is_empty`）。
    pub fn is_empty(&self) -> bool {
        self.undo.is_empty()
    }

    /// 下一次 undo 会撤的那条事务的序号（栈空为 `None`）。
    pub fn undo_top_seq(&self) -> Option<u64> {
        self.undo.last().map(|tx| tx.seq)
    }

    /// 下一次 redo 会重做的那条事务的序号（栈空为 `None`）。
    pub fn redo_top_seq(&self) -> Option<u64> {
        self.redo.last().map(|tx| tx.seq)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 回退一步。返回 `(是否实际回退, 该事务 commit 时收集的脏路径)`。
    ///
    /// **为什么返回脏路径**：撤销「新建地图 + 预填充栅格」这种事务后，
    /// `wv-io::save_world` 仍要知道「哪些目录路径需要重写 / 删除传播」——
    /// 不能要求调用方自行维护影子表。把脏路径沿 `Transaction` 一路带过来
    /// 是最不容易漂移的口径（ADR-0016 §2.3「写类 skill 经 EditCommand → History」的
    /// 隐含约束：撤销也得能精确告诉 IO 哪些文件要动）。
    pub fn undo(&mut self, world: &mut World) -> EditResult<(bool, Vec<String>)> {
        let Some(tx) = self.undo.pop() else {
            return Ok((false, Vec::new()));
        };
        for cmd in tx.commands.iter().rev() {
            cmd.revert(world)?;
        }
        let dirty = tx.committed_dirty.clone().unwrap_or_default();
        self.redo.push(tx);
        Ok((true, dirty))
    }

    /// 重做一步。返回 `(是否实际重做, 该事务 commit 时收集的脏路径)`。
    pub fn redo(&mut self, world: &mut World) -> EditResult<(bool, Vec<String>)> {
        let Some(mut tx) = self.redo.pop() else {
            return Ok((false, Vec::new()));
        };
        tx.commit(world)?;
        let dirty = tx.committed_dirty.clone().unwrap_or_default();
        self.undo.push(tx);
        Ok((true, dirty))
    }
}
