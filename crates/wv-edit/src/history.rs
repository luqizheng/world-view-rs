//! Undo/Redo 命令栈与事务。

use wv_core::World;

use crate::command::{EditCommand, EditResult};

/// 一次事务：一组要么全成功、要么全回滚的命令。
pub struct Transaction {
    commands: Vec<Box<dyn EditCommand>>,
}

impl Transaction {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: Box<dyn EditCommand>) {
        self.commands.push(cmd);
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// 顺序执行全部命令；任一失败则逆序回滚已成功的部分。
    pub fn commit(&self, world: &mut World) -> EditResult<Vec<String>> {
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
        Ok(dirty)
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
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    /// 提交一个事务并压入 Undo 栈，清空 Redo 栈。
    pub fn commit(&mut self, world: &mut World, tx: Transaction) -> EditResult<Vec<String>> {
        if tx.is_empty() {
            return Ok(Vec::new());
        }
        let dirty = tx.commit(world)?;
        self.undo.push(tx);
        self.redo.clear();
        Ok(dirty)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 回退一步。
    pub fn undo(&mut self, world: &mut World) -> EditResult<bool> {
        let Some(tx) = self.undo.pop() else {
            return Ok(false);
        };
        for cmd in tx.commands.iter().rev() {
            cmd.revert(world)?;
        }
        self.redo.push(tx);
        Ok(true)
    }

    /// 重做一步。
    pub fn redo(&mut self, world: &mut World) -> EditResult<bool> {
        let Some(tx) = self.redo.pop() else {
            return Ok(false);
        };
        tx.commit(world)?;
        self.undo.push(tx);
        Ok(true)
    }
}
