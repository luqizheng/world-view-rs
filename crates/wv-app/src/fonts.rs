//! 字体装配：从**系统字体栈**解析中文字体并注册进 egui 与 Bevy `Assets<Font>`。
//!
//! ## 依据
//!
//! `docs/ART_STYLE_SPEC_v0.1.md` §4.2「字体层级（只用系统字体栈 / 已授权字体）」：
//!
//! > **字体硬约束**
//! > 2. **必须使用系统字体栈，不引入未授权字体文件**。建议栈：
//! >    - 中文：`"PingFang SC", "Microsoft YaHei", "Noto Sans CJK SC", system-ui, sans-serif`
//! >    - 数字/代码：`ui-monospace, "Cascadia Mono", "SF Mono", Consolas, monospace`
//! > 3. 中英混排：**中文字号 ≥ 12px**
//!
//! 因此本模块**不打包任何字体文件**，而是在运行时按候选列表探测系统字体路径，
//! 命中第一个可读的即注册。
//!
//! ## 为什么必需
//!
//! Bevy / egui 的内置默认字体**不含 CJK 字形**，中文会全部渲染成 □□□（豆腐块）。
//!
//! ## 注册两个字体族
//!
//! - `FontFamily::Proportional`：正文用。比例中文字体（如微软雅黑），
//!   同时把默认字体作为 fallback（保证拉丁字符与图标仍可用）。
//! - `FontFamily::Monospace`：`font.mono` 令牌用（数字/日期/天数必须等宽，
//!   见 §4.2 硬约束 1「数字必须等宽，超长年份不得跳动」）。

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::sync::Arc;

/// 中文比例字体候选（按优先级）。第一个可读者胜出。
///
/// 顺序对齐 §4.2 建议栈：PingFang SC（macOS）→ Microsoft YaHei（Windows）
/// → Noto Sans CJK SC（Linux）→ 常见回退。
#[cfg(target_os = "windows")]
const CJK_PROPORTIONAL_CANDIDATES: &[&str] = &[
    r"C:\Windows\Fonts\msyh.ttc",   // 微软雅黑（Win 8+ 标配）
    r"C:\Windows\Fonts\msyh.ttf",   // 微软雅黑（旧版单文件）
    r"C:\Windows\Fonts\simhei.ttf", // 黑体（兜底，Windows 全版本有）
    r"C:\Windows\Fonts\simsun.ttc", // 宋体（最后兜底）
];

#[cfg(target_os = "macos")]
const CJK_PROPORTIONAL_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/Library/Fonts/Arial Unicode.ttf",
];

#[cfg(all(unix, not(target_os = "macos")))]
const CJK_PROPORTIONAL_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/arphic/uming.ttc",
];

/// 等宽字体候选（`font.mono`，数字必须等宽）。
#[cfg(target_os = "windows")]
const MONO_CANDIDATES: &[&str] = &[
    r"C:\Windows\Fonts\CascadiaMono.ttf", // Cascadia Mono（Win 11 / WinTerm 附带）
    r"C:\Windows\Fonts\consola.ttf",      // Consolas（Win Vista+ 标配）
    r"C:\Windows\Fonts\lucon.ttf",        // Lucida Console（兜底）
];

#[cfg(target_os = "macos")]
const MONO_CANDIDATES: &[&str] = &[
    "/System/Library/Fonts/SFNSMono.ttf",
    "/System/Library/Fonts/Menlo.ttc",
];

#[cfg(all(unix, not(target_os = "macos")))]
const MONO_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
];

/// 已解析出的字体资源（供后续 system 读取，避免每帧重读磁盘）。
#[derive(Resource, Default)]
pub struct LoadedFonts {
    /// 是否成功注册了 CJK 字体。`false` 时 UI 会给出可操作提示。
    pub cjk_loaded: bool,
    /// 实际命中的路径（诊断用）。
    pub cjk_path: Option<String>,
    /// 同一份 CJK 字体的 **Bevy 侧**资产句柄——画布上的 `Text2d`（地名标注，
    /// `drill.rs`）用它；`None` 时只能回落 Bevy 内置字体（中文仍会是方块，
    /// egui 侧不受影响）。
    pub cjk_font: Option<Handle<Font>>,
}

/// 把系统字体注入 egui **和** Bevy 文本资产表。
///
/// **为什么是「重试式」而不是 `Startup`**（bevy_egui 0.42 回归修复）：
/// `ctx.set_fonts` 需要主上下文已创建，而 0.42 的上下文在 `PreUpdate` 的
/// `EguiPreUpdateSet::InitContexts` 才建立——`Startup` 里 `ctx_mut()` 必然
/// `Err`，被静默跳过后 egui 全程豆腐块（Bevy 侧 `Text2d` 不受影响）。
/// 故本系统注册在 `PreUpdate` 的 `InitContexts` 之后：上下文就绪的首帧执行
/// 一次，未就绪就静默下帧再试；成功后 `LoadedFonts` 出现，此后早退零开销。
///
/// 失败时**不 panic**：Bevy/egui 仍有内置默认字体（拉丁字符可用），
/// 只是中文会显示为方块 —— 此时在日志中给出明确原因与处置建议。
pub fn setup_egui_fonts(
    mut contexts: EguiContexts,
    mut font_assets: ResMut<Assets<Font>>,
    mut commands: Commands,
    loaded: Option<Res<LoadedFonts>>,
) {
    if loaded.is_some() {
        return; // 已装配过：一帧一次之后不再碰字体文件。
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return; // 上下文尚未创建：下帧再试（不会读到磁盘）。
    };
    let mut loaded = LoadedFonts::default();

    let mut fonts = egui::FontDefinitions::default();

    // ---- 中文比例字体 ----
    if let Some((path, bytes, index)) = probe_first(CJK_PROPORTIONAL_CANDIDATES) {
        let key = "wv_cjk".to_owned();
        // 同一份字节再注册进 Bevy 的 `Assets<Font>`：画布地名标注（Text2d）
        // 与 egui 面板共用一个系统字体栈，避免两套渲染方框表现不一致。
        // Bevy 0.19：`Font::from_bytes` 不再校验/解析字节（交给 fontique），
        // 因此不返回 `Result`——坏的字体要等真正排版时才暴露。
        let f = Font::from_bytes(bytes.clone());
        loaded.cjk_font = Some(font_assets.add(f));
        fonts.font_data.insert(
            key.clone(),
            Arc::new(egui::FontData {
                font: bytes.into(),
                index,
                tweak: egui::FontTweak::default(),
            }),
        );
        // 插到首位：优先用它解析中文；后面保留内置字体兜底拉丁/图标。
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, key.clone());
        }
        loaded.cjk_loaded = true;
        loaded.cjk_path = Some(path);
    } else {
        warn!(
            "未找到可用的中文字体（候选: {:?}）。UI 中文将显示为方块。\
             请安装微软雅黑 / Noto Sans CJK 后重启。",
            CJK_PROPORTIONAL_CANDIDATES
        );
    }

    // ---- 等宽字体（数字列，§4.2 硬约束 1）----
    if let Some((path, bytes, index)) = probe_first(MONO_CANDIDATES) {
        let key = "wv_mono".to_owned();
        fonts.font_data.insert(
            key.clone(),
            Arc::new(egui::FontData {
                font: bytes.into(),
                index,
                tweak: egui::FontTweak::default(),
            }),
        );
        // 等宽族首位；CJK 字体留作 fallback，保证中文注释也能显示。
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, key);
        debug!("等宽字体: {path}");
    }

    ctx.set_fonts(fonts);
    commands.insert_resource(loaded);
}

/// 按候选顺序探测，返回首个可读文件的 `(路径, 字节, face_index)`。
///
/// TTC（TrueType Collection）文件含多个 face，统一用 `index = 0`
/// （`.ttc` 的第 0 个 face 即常规字重，`msyh.ttc` / `PingFang.ttc` 均如此）。
fn probe_first(candidates: &[&str]) -> Option<(String, Vec<u8>, u32)> {
    for path in candidates {
        match std::fs::read(path) {
            Ok(bytes) if !bytes.is_empty() => {
                return Some(((*path).to_string(), bytes, 0));
            }
            Ok(_) => warn!("字体文件为空，跳过: {path}"),
            Err(_) => continue,
        }
    }
    None
}
