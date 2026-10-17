//! egui 图像解码器装配（工具栏 SVG 渲染前置）。
//!
//! egui 0.36 的 loader 链（`egui/src/load.rs` 顶部文档）共三级：
//! ```text
//!   ImageSource::load
//!     ├► Context::include_bytes   （BytesLoader：取原始字节）
//!     ├► Context::try_load_image  （ImageLoader：字节 → 像素）★ 解码
//!     └► try_load_texture         （TextureLoader：像素 → 纹理）
//! ```
//!
//! **修正（2026-10-15，推翻本节旧结论）**：旧版注解说「bevy_egui 不注册
//! DefaultBytesLoader / DefaultTextureLoader，需手动装」——误诊。两者都是
//! `Loaders::default()` 的出厂件；真正的缺口是 **ImageLoader：egui core
//! 一个解码器都不带**（出厂 `image: Vec::new()`），SVG 字节永远解不成像素，
//! `Image` 只能画红三角占位（用户截图所见）。旧代码手动 push 的两个 loader
//! 实为重复注册（`add_*_loader` 不去重），一并移除。
//!
//! SVG 解码器只在官方配套 crate `egui_extras`（`svg` feature → resvg；
//! `install_image_loaders` 在该 feature 下仅装 `SvgLoader`，且
//! `is_loader_installed` 幂等）。依赖审视见根 Cargo.toml 注释。
//!
//! 时序：bevy_egui 0.42 的主上下文在 `PreUpdate` 的 `InitContexts` 才建立，
//! `Startup` 调 `ctx_mut()` 必然 `Err`。**与 `setup_egui_fonts` 同口径**：
//! 注册在 `PreUpdate` 链 `InitContexts` 之后，重试式 + 资源 marker 防重入。
//!
//! 历史来源：拆自原 `crate::widgets`。

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

/// 已安装图像解码器的 marker 资源（零大小）。
#[derive(Default, Resource)]
pub struct EguiImageLoadersInstalled;

/// 在 egui 上下文上安装 SVG 解码器（纯 egui 操作，单测可直调）。
pub fn install_svg_decoder(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

/// 主 egui 上下文就绪后安装图像解码器。
///
/// 上下文未就绪时静默下帧再试；装好后插入 marker；marker 存在则早退零开销。
pub fn setup_egui_image_loaders(
    mut commands: Commands,
    mut contexts: EguiContexts,
    installed: Option<Res<EguiImageLoadersInstalled>>,
) {
    if installed.is_some() {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    install_svg_decoder(ctx);
    commands.insert_resource(EguiImageLoadersInstalled);
}

// ── 测试 ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod svg_decoder_tests {
    use super::install_svg_decoder;
    use bevy_egui::egui;

    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#000000" stroke-width="2"><rect x="4" y="4" width="16" height="16"/></svg>"##;

    fn bytes_source() -> egui::ImageSource<'static> {
        egui::ImageSource::Bytes {
            uri: std::borrow::Cow::Borrowed("bytes://test/pen.svg"),
            bytes: egui::load::Bytes::from(SVG),
        }
    }

    /// 红三角根因的回归测试：出厂 egui 上下文没有任何 `ImageLoader`，
    /// `bytes://….svg` 解不出像素（egui 画红三角占位）；装上
    /// `egui_extras` 的 `SvgLoader` 后必须一步到位解码成纹理。
    #[test]
    fn svg_bytes_stay_undecoded_until_svg_loader_installed() {
        let ctx = egui::Context::default();

        // 未装解码器：必须报 NoMatchingImageLoader（旧版红三角的确切来源）。
        let before = bytes_source().load(
            &ctx,
            Default::default(),
            egui::load::SizeHint::Scale(1.0.into()),
        );
        match &before {
            // 出厂 `image` 列表为空 → NoImageLoaders；若哪天 egui 出厂带了解码器
            // 但不认 SVG，会落 NoMatchingImageLoader——两者都是「没解码」的证据。
            Err(
                egui::load::LoadError::NoImageLoaders
                | egui::load::LoadError::NoMatchingImageLoader { .. },
            ) => {}
            Err(e) => panic!("出厂上下文应报缺解码器，got {e}"),
            Ok(poll) => panic!(
                "出厂上下文不应解码就绪，got {}",
                if matches!(poll, egui::load::TexturePoll::Ready { .. }) {
                    "Ready"
                } else {
                    "Pending"
                }
            ),
        }

        install_svg_decoder(&ctx);
        let after = bytes_source().load(
            &ctx,
            Default::default(),
            egui::load::SizeHint::Scale(1.0.into()),
        );
        let ready = match after {
            Ok(ready @ egui::load::TexturePoll::Ready { .. }) => ready,
            Ok(_) => panic!("装上 SvgLoader 后应直接解码就绪，而非 Pending"),
            Err(e) => panic!("装上 SvgLoader 后应解码成功，got {e}"),
        };
        let egui::load::TexturePoll::Ready { texture, .. } = ready else {
            unreachable!("上面已筛出 Ready");
        };
        let size = texture.size;
        assert_eq!(
            (size.x, size.y),
            (24.0, 24.0),
            "SVG 应按 viewBox 点数解码，got {size:?}"
        );
    }
}
