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
//! ## 地图标签书法体（2026-10-20 用户拍板，ADR-0042 D4）
//!
//! **霞鹜文楷**（LXGW WenKai，OFL-1.1）注册为**命名族** `wv_map_label`：
//! 只给画布上的地名 / 街道名 / 测量标注（Bevy `Text2d`，`LoadedFonts::label_handle`）
//! 用——**UI 面板仍是 Inter + Noto**（楷体做界面正文可读性差）。canvas 标签
//! 是参考图手绘风的最后一块：书法体地名 + 程序化插画 + 纸纹。
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

/// 画布标签书法体：霞鹜文楷 Regular——地名 / 街道名 / 测量标注的字体身份。
///
/// 来源 / 许可：`assets/fonts/README.md`（GitHub Releases v1.530，OFL-1.1）。
const BUNDLED_LABEL: &[u8] = include_bytes!("../../assets/fonts/LXGWWenKai-Regular.ttf");

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
    /// 霞鹜文楷的 **Bevy 侧**资产句柄（ADR-0042 D4）——画布标签的书法体。
    /// `None` = 尚未装配；消费方走 [`Self::label_handle`] 自动回落 `cjk_font`。
    pub label_font: Option<Handle<Font>>,
}

impl LoadedFonts {
    /// 画布标签字体句柄：文楷优先，未装配时回落 Noto（同样全覆盖中文）。
    pub fn label_handle(&self) -> Option<Handle<Font>> {
        self.label_font.clone().or_else(|| self.cjk_font.clone())
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

    // ---- 画布标签书法体：霞鹜文楷（ADR-0042 D4）----
    // Bevy 侧：Text2d 消费 `label_handle`；egui 侧：注册**命名族**
    // `wv_map_label`（[文楷, Noto 回退]），未来 egui 侧标签（label_halo 族）
    // 可按族取用。**不**进 Proportional——UI 面板保持 Inter + Noto。
    let label_key = "wv_label".to_owned();
    let label_font = font_assets.add(Font::from_bytes(BUNDLED_LABEL.to_vec()));
    loaded.label_font = Some(label_font);
    fonts.font_data.insert(
        label_key.clone(),
        Arc::new(egui::FontData::from_static(BUNDLED_LABEL)),
    );
    fonts.families.insert(
        egui::FontFamily::Name("wv_map_label".into()),
        vec![label_key, cjk_key.clone()],
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
