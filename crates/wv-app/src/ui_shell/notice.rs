//! 状态栏回执资源。

use bevy::prelude::*;

/// 一次用户可见的操作结果回执（成功 / 失败都显示）。
///
/// **为什么要有它**：v0.2 新增了六条「打开 / 另存 / 导出 / 关闭 / 设置」类命令，
/// 其中一部分在当前架构下**做不到**（见 `dispatch_project_command`）。让这些命令
/// 静默无反应会违反 §5.3「错误必须给出人类可读的原因与下一步」与 D5 零隐藏功能——
/// 用户会以为程序坏了。故一律回执。
#[derive(Resource, Debug, Default)]
pub struct ShellNotice {
    /// 最近一条回执文案。
    pub text: Option<String>,
    /// 是否为错误（决定状态栏里的徽标类型）。
    pub is_error: bool,
    /// 生成该回执的帧号：状态栏只显示最近若干秒，避免永久占位。
    pub frame: u64,
}

impl ShellNotice {
    /// 记一条成功回执（`ui_shell` 之外的编辑链 / 对话框也用，故 pub）。
    pub fn info(&mut self, text: impl Into<String>, frame: u64) {
        self.text = Some(text.into());
        self.is_error = false;
        self.frame = frame;
    }

    /// 记一条失败回执（同上 pub：笔刷提交失败等路径要给人话说明）。
    pub fn error(&mut self, text: impl Into<String>, frame: u64) {
        self.text = Some(text.into());
        self.is_error = true;
        self.frame = frame;
    }

    /// 回执是否还在展示窗口内（约 6 秒 @60fps）。
    pub fn is_fresh(&self, now: u64) -> bool {
        self.text.is_some() && now.saturating_sub(self.frame) < 360
    }
}

/// 撤销栈逐出回执（UX-034 / round-2 审计）。
///
/// `History` 在 wv-edit（零 Bevy 依赖），只能计数不能发通知——这里每帧
/// 轮询 [`wv_edit::History::take_trimmed`]，把「最早事务被顶掉」转成一条
/// 状态栏回执。没有它，「撤销不到更早」就是静默的能力损失，用户只会
/// 觉得撤销坏了。取数即清零：同一批逐出只提示一次。
pub fn notify_history_trimmed(
    mut edit: ResMut<crate::edit::editing::EditHistory>,
    mut notice: ResMut<ShellNotice>,
    loc: Res<crate::i18n::CurrentLocale>,
    time: Res<Time>,
) {
    if edit.history.take_trimmed() > 0 {
        notice.info(
            crate::i18n::t(loc.0, crate::i18n::I18nKey::HistoryTrimmedNotice),
            (time.elapsed_secs() * 60.0) as u64,
        );
    }
}
