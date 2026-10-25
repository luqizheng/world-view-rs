//! 字体装配：**打包字体**（Inter 拉丁 + Noto Sans SC 中文）注册进 egui 与 Bevy。
//!
//! ## 装配结果
//! - egui `Proportional`：Inter → Noto Sans SC → egui 内置——拉丁字符走 Inter，
//!   中文（含全角标点）逐字形回退到 Noto Sans SC。
//! - egui `Monospace`：系统等宽 → Noto Sans SC → Inter → 内置——数字保持等宽
//!   （§4.2 硬约束 1），中文注释回退可读。
//! - Bevy `Text2d`（画布地名标注）：Noto Sans SC 单一字体句柄（拉丁 + CJK
//!   全覆盖）。`TextFont` 只吃一个句柄，parley 的逐字形回退顺序不受本模块
//!   控制，用全覆盖字体避免回退不确定导致的方块。
//!
//! ## 变更留痕（2026-10-16 用户裁定）
//!
//! 原结论「不打包任何字体文件，只用系统字体栈」（`ART_STYLE_SPEC_v0.1` §4.2
//! 硬约束 2）被修正为「打包字体：拉丁 Inter、中文 Noto Sans SC」。理由：
//! - 设计稿锁定 Inter（web 原型 `font-family: Inter, ui-sans-serif, …`），
//!   跨机器字形必须一致；
//! - 系统中文字体（微软雅黑 / PingFang / SimHei）字形差异大，打包后跨平台可预期。
//! - 中文为什么不是 Open Sans：Open Sans 本身无 CJK 字形；Noto Sans SC 是
//!   Google 与其配套的 CJK 字体（拉丁部分同源设计），即「中文的 Open Sans」。
//! - 来源 / 许可 / 追加字重方法：`assets/fonts/README.md`；决议：ADR-0019。
//!
//! ## 已知覆盖缺口（与打包前一致，未回归）
//!
//! 面板头几何图标字符 ◈(U+25C8) ◧(U+25E7) ⬡(U+2B21)（`state::Panel::icon`）
//! 在 Inter / Noto Sans SC / egui 内置字体中均缺失，egui 以替换符 ◻(U+25FB)
//! 呈现——打包前用系统雅黑（同样缺这三个字形）也是同一表现。真修复走 SVG
//! 图标（Figma 原型即 Lucide 图标，工程已有 `assets/ui/tool_icons/` 基建）。
//!
//! ## 画布标签字体（ADR-0065，2026-10-25 用户裁定）
//!
//! 画布地名 / POI / 街道名 / 测量标注（Bevy `Text2d`，`LoadedFonts::label_handle`）
//! **默认用 Noto Sans SC**：12–16px 的屏幕字号下，无衬线体的笔画宽度足够，
//! 配合纸色晕圈仍能读清；UI 面板同样是 Inter + Noto，两处字体表现一致。
//!
//! **霞鹜文楷仍装配**（`LoadedFonts::calligraphy_handle`），但不再是默认：
//! ADR-0042 D4 曾把它定为画布标签字体，实机结果是 12px 下笔画只有 1px、
//! 被 8 向晕圈吃掉，地图上看不清 POI 名。文楷保留为**主题可选**的书法体
//! （注册进 egui 命名族 `wv_map_label`，族序 [Noto, 文楷]），不删资产——
//! 未来水墨 / 古舆图若要书法地名，按主题显式取 `calligraphy_handle` 即可。
//!
//! ## 边界（不做什么）
//! - **不画文字**——本模块只创资源 / 抢 `FontFamily` 注册位；绘制由 egui 调用。
//! - **不做字重**——egui 无合成粗体，注册的全部是 Regular；`.strong()` 只改色。
//!   设计稿 600 字重（`ART_STYLE_SPEC` §4.2）需要粗体文件 + 命名族接线，
//!   属后续任务（文件获取方法见 `assets/fonts/README.md`）。

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use std::sync::Arc;

/// 拉丁比例字体：Inter Regular——设计稿锁定字体（web 原型同款）。
///
/// 编译期嵌入（`include_bytes!`），与 `ui/widgets.rs` 图标 SVG 的
/// `egui::include_image!` 同口径：不依赖运行时工作目录，二进制自带。
const BUNDLED_LATIN: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");

/// 中文比例字体：Noto Sans SC Regular——「中文的 Open Sans」。
///
/// 同一份字节同时注册进 egui 与 Bevy `Assets<Font>`。
const BUNDLED_CJK: &[u8] = include_bytes!("../../assets/fonts/NotoSansSC-Regular.ttf");

/// 画布标签书法体：霞鹜文楷 Regular——**可选**主题字体（ADR-0065 后非默认）。
///
/// 来源 / 许可：`assets/fonts/README.md`（GitHub Releases v1.530，OFL-1.1）。
const BUNDLED_CALLIGRAPHY: &[u8] = include_bytes!("../../assets/fonts/LXGWWenKai-Regular.ttf");

/// 等宽字体候选（`font.mono`，数字必须等宽，§4.2 硬约束 1）。
///
/// 等宽字体不打包：它是**几何约束**（数字不跳动）而非设计身份，各平台
/// 系统等宽（Consolas / SF Mono / DejaVu）都能满足，且中文等宽体积巨大。
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

/// 已解析出的字体资源（供后续 system 读取，避免每帧重建）。
#[derive(Resource, Default)]
pub struct LoadedFonts {
    /// 中文字体是否已装配。打包字体编译期嵌入，正常运行恒 `true`；
    /// 保留字段供诊断与未来字体来源替换。
    pub cjk_loaded: bool,
    /// 中文字体来源（诊断用）。
    pub cjk_path: Option<String>,
    /// Noto Sans SC 的 **Bevy 侧**资产句柄——画布上的 `Text2d`（地名标注，
    /// `drill.rs` / `streets.rs` / `measurement.rs`）用它；`None` 时只能回落
    /// Bevy 内置字体（中文会是方块，egui 侧不受影响）。
    pub cjk_font: Option<Handle<Font>>,
    /// 霞鹜文楷的 **Bevy 侧**资产句柄（ADR-0042 D4 的书法体，ADR-0065 后为
    /// **可选**而非默认）。书法体小字号可读性差，画布标签默认走 `cjk_font`；
    /// 主题若明确要手绘书法地名，显式取 [`Self::calligraphy_handle`]。
    pub calligraphy_font: Option<Handle<Font>>,
}

impl LoadedFonts {
    /// 画布标签字体句柄（默认）：**Noto Sans SC**，未装配时返回 `None`。
    ///
    /// ADR-0065：12–16px 下无衬线体比书法体清晰得多；书法体走
    /// [`Self::calligraphy_handle`]，是主题可选项。
    pub fn label_handle(&self) -> Option<Handle<Font>> {
        self.cjk_font.clone()
    }

    /// 画布标签的**书法体**句柄（霞鹜文楷）；未装配时回落默认标签字体。
    ///
    /// 仅供主题显式选用——默认标签路径（`drill` / `streets` / `measurement`）
    /// **不**调用本方法。
    pub fn calligraphy_handle(&self) -> Option<Handle<Font>> {
        self.calligraphy_font
            .clone()
            .or_else(|| self.label_handle())
    }
}

/// 把打包字体注入 egui **和** Bevy 文本资产表。
///
/// **为什么是「重试式」而不是 `Startup`**（bevy_egui 0.42 回归修复）：
/// `ctx.set_fonts` 需要主上下文已创建，而 0.42 的上下文在 `PreUpdate` 的
/// `EguiPreUpdateSet::InitContexts` 才建立——`Startup` 里 `ctx_mut()` 必然
/// `Err`，被静默跳过后 egui 全程豆腐块（Bevy 侧 `Text2d` 不受影响）。
/// 故本系统注册在 `PreUpdate` 的 `InitContexts` 之后：上下文就绪的首帧执行
/// 一次，未就绪就静默下帧再试；成功后 `LoadedFonts` 出现，此后早退零开销。
pub fn setup_egui_fonts(
    mut contexts: EguiContexts,
    mut font_assets: ResMut<Assets<Font>>,
    mut commands: Commands,
    loaded: Option<Res<LoadedFonts>>,
) {
    if loaded.is_some() {
        return; // 已装配过：一帧一次之后不再碰字体资产。
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return; // 上下文尚未创建：下帧再试（不会读磁盘）。
    };
    let mut loaded = LoadedFonts::default();
    let mut fonts = egui::FontDefinitions::default();

    // ---- 拉丁：Inter ----
    // `from_static` 借用编译期字节（零拷贝）；egui 逐字形回退时它排最前，
    // 所有它含有的字符（拉丁、希腊、西里尔、常见符号）都走它。
    let latin_key = "wv_latin".to_owned();
    fonts.font_data.insert(
        latin_key.clone(),
        Arc::new(egui::FontData::from_static(BUNDLED_LATIN)),
    );

    // ---- 中文：Noto Sans SC ----
    // 同一份字节再注册进 Bevy 的 `Assets<Font>`：画布地名标注（Text2d）
    // 与 egui 面板共用同一字体，避免两处渲染表现不一致。
    // Bevy 0.19：`Font::from_bytes` 不再校验/解析字节（交给 fontique），
    // 因此不返回 `Result`——坏的字体要等真正排版时才暴露。
    let cjk_key = "wv_cjk".to_owned();
    loaded.cjk_font = Some(font_assets.add(Font::from_bytes(BUNDLED_CJK.to_vec())));
    loaded.cjk_loaded = true;
    loaded.cjk_path = Some("assets/fonts/NotoSansSC-Regular.ttf（编译期嵌入）".to_owned());
    fonts.font_data.insert(
        cjk_key.clone(),
        Arc::new(egui::FontData::from_static(BUNDLED_CJK)),
    );

    // ---- 画布标签书法体：霞鹜文楷（可选，ADR-0042 D4 / ADR-0065）----
    // Bevy 侧：默认标签走 Noto（上面装配的 `cjk_font`），文楷只经
    // `calligraphy_handle` 供主题显式选用。egui 侧仍注册**命名族**
    // `wv_map_label`，但**族序改成 [Noto, 文楷]**——与默认标签字体一致。
    // **不**进 Proportional——UI 面板保持 Inter + Noto。
    let label_key = "wv_label".to_owned();
    loaded.calligraphy_font = Some(font_assets.add(Font::from_bytes(BUNDLED_CALLIGRAPHY.to_vec())));
    fonts.font_data.insert(
        label_key.clone(),
        Arc::new(egui::FontData::from_static(BUNDLED_CALLIGRAPHY)),
    );
    fonts.families.insert(
        egui::FontFamily::Name("wv_map_label".into()),
        vec![cjk_key.clone(), label_key],
    );

    // ---- 等宽字体（系统探测）----
    let mono_key = probe_first(MONO_CANDIDATES).map(|(path, bytes, index)| {
        let key = "wv_mono".to_owned();
        fonts.font_data.insert(
            key.clone(),
            Arc::new(egui::FontData {
                font: bytes.into(),
                index,
                tweak: egui::FontTweak::default(),
            }),
        );
        debug!("等宽字体: {path}");
        key
    });

    // ---- 族顺序（逐字形回退，一次成型）----
    // Proportional = [Inter, Noto Sans SC, …内置]：拉丁 Inter 优先，中文回退
    // Noto Sans SC；egui 内置（Ubuntu-Light 等）兜底罕见符号。
    let proportional = fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default();
    proportional.insert(0, cjk_key.clone());
    proportional.insert(0, latin_key.clone());

    // Monospace = [系统等宽, Noto Sans SC, Inter, …内置]：数字等宽最优先
    // （等宽在前，Inter 不参与数字渲染）；中文注释回退 Noto Sans SC。
    let monospace = fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default();
    monospace.insert(0, latin_key);
    monospace.insert(0, cjk_key);
    if let Some(key) = mono_key {
        monospace.insert(0, key);
    }

    ctx.set_fonts(fonts);
    commands.insert_resource(loaded);
}

/// 按候选顺序探测系统字体，返回首个可读文件的 `(路径, 字节, face_index)`。
///
/// 候选均为单 face TTF，`index` 恒 0（.ttc 第 0 个 face 即常规字重，
/// 曾用于系统 CJK 探测；打包字体化后本函数只服务等宽候选）。
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
