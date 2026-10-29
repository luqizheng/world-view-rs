//! character 命令族——人物等级列表编辑器（2026-10-30）。
//!
//! 此前人物在 GUI 里零写侧（UX 审计 2026-10-29：与事件同为「数据侧缺写」）。
//! 本族先落等级列表这一刀：`Character.state_spans` 整表替换。
//! 人物本体的创建 / 改名（UX-002 剩余实体）另行跟进，不混入本命令。

use super::*;

/// 改人物的状态片段表（等级列表编辑器的唯一写入口）。
///
/// 对话框本地改草稿（增 / 删 / 改行），确定时把**整张新表**传入——
/// 一条命令 = 一次编辑会话 = 一个撤销步。
///
/// ## 为什么整表替换而非逐行命令
///
/// 增 / 删 / 改行各自成命令会把一次会话在撤销栈里碎成 N 步，且行下标
/// 在增删之间漂移、按行寻址易错位。整表快照回滚与 `UpdateRegionCommand`
/// 的「快照整实体」同款：apply 换整表、revert 退整表。`place` / `title`
/// 等对话框不暴露的字段也随表往返——草稿从当前表克隆而来，未触字段
/// 原样带回，不存在误退。
///
/// ## V2（历史可重写）
///
/// `state_spans` 就是「已发生的历史」，作者回头改早期等级是创作常态。
/// 本命令**不做**任何区间重叠 / 顺序校验——冲突由诊断面板提示
/// （V1：只提示不阻止），不设编辑阻力。
#[derive(Debug)]
pub struct SetCharacterStateSpansCommand {
    /// 目标人物 ID。
    pub id: wv_core::Id,
    /// 新的整张状态片段表（编辑器草稿）。
    pub new_spans: Vec<wv_core::character::CharacterStateSpan>,
    before: std::sync::Mutex<Option<Vec<wv_core::character::CharacterStateSpan>>>,
}

impl SetCharacterStateSpansCommand {
    pub fn new(id: wv_core::Id, new_spans: Vec<wv_core::character::CharacterStateSpan>) -> Self {
        Self {
            id,
            new_spans,
            before: std::sync::Mutex::new(None),
        }
    }
}

impl EditCommand for SetCharacterStateSpansCommand {
    fn label(&self) -> String {
        format!("改人物「{}」的等级列表", self.id)
    }
    fn apply(&self, world: &mut World) -> EditResult<Vec<String>> {
        let c = world
            .characters
            .iter_mut()
            .find(|c| c.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Character {}", self.id)))?;
        *self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))? = Some(c.state_spans.clone());
        c.state_spans = self.new_spans.clone();
        Ok(vec!["characters".into()])
    }
    fn revert(&self, world: &mut World) -> EditResult<()> {
        let prev = self
            .before
            .lock()
            .map_err(|_| EditError::Precondition("锁中毒".into()))?
            .clone()
            .ok_or_else(|| EditError::Precondition("SetCharacterStateSpans 尚未 apply".into()))?;
        let c = world
            .characters
            .iter_mut()
            .find(|c| c.id == self.id)
            .ok_or_else(|| EditError::TargetMissing(format!("Character {}", self.id)))?;
        c.state_spans = prev;
        Ok(())
    }
}
