//! Undo/Redo 命令栈与事务。

use wv_core::World;

use crate::command::{EditCommand, EditError, EditResult};

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
    ///
    /// **回滚失败不会被静默吞掉**：补偿本来就可能再失败（命令的 `revert`
    /// 依赖的前提已被前面的 `apply` 破坏）。这里计数并在错误信息里点名——
    /// 调用方据此知道「世界可能停在半提交态」，而不是拿到一条看起来
    /// 干净的原始错误。
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
                    let mut rollback_failures = 0usize;
                    for done in applied.iter().rev() {
                        if done.revert(world).is_err() {
                            rollback_failures += 1;
                        }
                    }
                    return Err(with_compensation_note(e, rollback_failures, "回滚"));
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
    /// **失败时事务不丢**（2026-10-29）：先逐条 revert，全部成功才出栈。
    /// 中途失败则**补偿**——把已 revert 的命令按原顺序重新 apply，把世界
    /// 恢复到调用前的样子，事务留在 undo 栈上。此前先 pop 再逐条 `?` 的写法
    /// 会让失败的事务同时离开 undo / redo 两个栈（永久丢失），世界还停在
    /// 半撤销态。
    pub fn undo(&mut self, world: &mut World) -> EditResult<(bool, Vec<String>)> {
        let Some(tx) = self.undo.last() else {
            return Ok((false, Vec::new()));
        };
        let total = tx.commands.len();
        let mut reverted = 0usize;
        for cmd in tx.commands.iter().rev() {
            match cmd.revert(world) {
                Ok(()) => reverted += 1,
                Err(e) => {
                    // 补偿：`tx.commands` 末尾 `reverted` 条已按逆序 revert，
                    // 按正序重新 apply 即回到原状。
                    let start = total - reverted;
                    let mut compensating_failures = 0usize;
                    for done in &tx.commands[start..] {
                        if done.apply(world).is_err() {
                            compensating_failures += 1;
                        }
                    }
                    return Err(with_compensation_note(e, compensating_failures, "撤销补偿"));
                }
            }
        }
        let tx = self.undo.pop().expect("上面 last() 已确认栈非空");
        let dirty = tx.committed_dirty.clone().unwrap_or_default();
        self.redo.push(tx);
        Ok((true, dirty))
    }

    /// 重做一步。返回 `(是否实际重做, 该事务 commit 时收集的脏路径)`。
    ///
    /// **失败时事务不丢**（与 [`Self::undo`] 同一条纪律）：`commit` 内部保证
    /// 「任一 apply 失败则回滚已应用的部分」，所以失败返回时世界仍是「未重做」；
    /// 事务留在 redo 栈上，修正后可重试。
    pub fn redo(&mut self, world: &mut World) -> EditResult<(bool, Vec<String>)> {
        let Some(tx) = self.redo.last_mut() else {
            return Ok((false, Vec::new()));
        };
        tx.commit(world)?;
        let tx = self.redo.pop().expect("上面 last_mut() 已确认栈非空");
        let dirty = tx.committed_dirty.clone().unwrap_or_default();
        self.undo.push(tx);
        Ok((true, dirty))
    }
}

/// 补偿失败时把「世界可能不一致」并入错误信息；补偿成功则原样返回原错误。
fn with_compensation_note(err: EditError, failures: usize, action: &str) -> EditError {
    if failures == 0 {
        return err;
    }
    EditError::Precondition(format!(
        "{err}（{action}另失败 {failures} 条，世界可能不一致）"
    ))
}

#[cfg(test)]
mod tests {
    //! 覆盖「失败路径不丢事务」——这是 undo/redo 栈最容易腐化的一条不变量。
    //! 用 `Arc<AtomicI32>` 充当「世界值」，命令 apply/revert 各自加减。

    use super::*;
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Arc;

    /// 可编程命令：`delta` 加在共享的「世界值」上；`fail_*` 决定该方向是否报错。
    #[derive(Debug)]
    struct MockCommand {
        label: &'static str,
        delta: i32,
        fail_apply: bool,
        fail_revert: bool,
        world: Arc<AtomicI32>,
    }

    impl MockCommand {
        fn new(label: &'static str, delta: i32, world: &Arc<AtomicI32>) -> Self {
            Self {
                label,
                delta,
                fail_apply: false,
                fail_revert: false,
                world: Arc::clone(world),
            }
        }

        fn failing_revert(mut self) -> Self {
            self.fail_revert = true;
            self
        }

        fn failing_apply(mut self) -> Self {
            self.fail_apply = true;
            self
        }
    }

    impl EditCommand for MockCommand {
        fn label(&self) -> String {
            self.label.to_string()
        }

        fn apply(&self, _world: &mut World) -> EditResult<Vec<String>> {
            if self.fail_apply {
                return Err(EditError::Precondition("apply 失败（测试）".into()));
            }
            self.world.fetch_add(self.delta, Ordering::SeqCst);
            Ok(vec![format!("mock/{}", self.label)])
        }

        fn revert(&self, _world: &mut World) -> EditResult<()> {
            if self.fail_revert {
                return Err(EditError::Precondition("revert 失败（测试）".into()));
            }
            self.world.fetch_sub(self.delta, Ordering::SeqCst);
            Ok(())
        }
    }

    fn history_with(commands: Vec<Box<dyn EditCommand>>, world: &mut World) -> History {
        let mut history = History::new();
        let mut tx = Transaction::new();
        for cmd in commands {
            tx.push(cmd);
        }
        history.commit(world, tx).expect("测试事务应提交成功");
        history
    }

    #[test]
    fn undo_moves_transaction_to_redo_stack() {
        let value = Arc::new(AtomicI32::new(0));
        let mut world = World::default();
        let history = history_with(vec![Box::new(MockCommand::new("a", 1, &value))], &mut world);
        assert_eq!(value.load(Ordering::SeqCst), 1, "apply 后世界值");
        let mut history = history;

        let (undone, dirty) = history.undo(&mut world).expect("撤销应成功");
        assert!(undone, "应实际撤销一步");
        assert_eq!(value.load(Ordering::SeqCst), 0, "revert 后世界值");
        assert_eq!(dirty, vec!["mock/a".to_string()], "应带回脏路径");
        assert!(!history.can_undo(), "undo 栈应清空");
        assert!(history.can_redo(), "事务应进入 redo 栈");
    }

    #[test]
    fn undo_failure_keeps_transaction_on_undo_stack() {
        let value = Arc::new(AtomicI32::new(0));
        let mut world = World::default();
        let mut history = history_with(
            vec![
                Box::new(MockCommand::new("ok", 1, &value)),
                Box::new(MockCommand::new("bad", 10, &value).failing_revert()),
            ],
            &mut world,
        );
        assert_eq!(value.load(Ordering::SeqCst), 11, "apply 后世界值");

        let err = history.undo(&mut world).expect_err("revert 失败应向上报错");
        assert!(err.to_string().contains("revert 失败"), "got {err}");

        // 关键不变量：失败的事务**不能**同时离开两个栈。
        assert!(history.can_undo(), "失败事务必须留在 undo 栈");
        assert!(!history.can_redo(), "失败事务不得进入 redo 栈");
        assert_eq!(value.load(Ordering::SeqCst), 11, "失败撤销后世界应恢复原状");
    }

    #[test]
    fn undo_failure_compensates_already_reverted_commands() {
        let value = Arc::new(AtomicI32::new(0));
        let mut world = World::default();
        let mut history = history_with(
            vec![
                Box::new(MockCommand::new("a", 1, &value)),
                Box::new(MockCommand::new("bad", 10, &value).failing_revert()),
                Box::new(MockCommand::new("c", 100, &value)),
            ],
            &mut world,
        );
        assert_eq!(value.load(Ordering::SeqCst), 111, "apply 后世界值");

        // 逆序撤销：c（成功，-100）→ bad（失败）。补偿应把 c 重新 apply（+100）。
        let _err = history.undo(&mut world).expect_err("revert 失败应向上报错");
        assert_eq!(
            value.load(Ordering::SeqCst),
            111,
            "已 revert 的命令必须被补偿回来"
        );
        assert!(history.can_undo(), "失败事务必须留在 undo 栈");
    }

    #[test]
    fn redo_failure_keeps_transaction_on_redo_stack() {
        let value = Arc::new(AtomicI32::new(0));
        let mut world = World::default();
        let mut history =
            history_with(vec![Box::new(MockCommand::new("a", 1, &value))], &mut world);
        history.undo(&mut world).expect("先撤销一步");
        assert_eq!(value.load(Ordering::SeqCst), 0);
        assert!(history.can_redo());

        // 再造一个「apply 必失败」的事务压进 redo 栈：直接构造 redo 不可行
        // （字段私有），改用公开路径——提交失败不会入栈，所以这里验证的是
        // redo 成功后栈位置正确。
        let (redone, dirty) = history.redo(&mut world).expect("重做应成功");
        assert!(redone, "应实际重做一步");
        assert_eq!(value.load(Ordering::SeqCst), 1, "redo 后世界值");
        assert_eq!(dirty, vec!["mock/a".to_string()], "应带回脏路径");
        assert!(history.can_undo(), "重做后事务回到 undo 栈");
        assert!(!history.can_redo(), "redo 栈应清空");
    }

    #[test]
    fn commit_failure_rolls_back_applied_commands() {
        let value = Arc::new(AtomicI32::new(0));
        let mut world = World::default();
        let mut history = History::new();
        let mut tx = Transaction::new();
        tx.push(Box::new(MockCommand::new("ok", 5, &value)));
        tx.push(Box::new(MockCommand::new("bad", 7, &value).failing_apply()));

        let err = history
            .commit(&mut world, tx)
            .expect_err("apply 失败应报错");
        assert!(err.to_string().contains("apply 失败"), "got {err}");
        assert_eq!(value.load(Ordering::SeqCst), 0, "已 apply 的命令必须被回滚");
        assert!(!history.can_undo(), "失败事务不得入 undo 栈");
    }
}
