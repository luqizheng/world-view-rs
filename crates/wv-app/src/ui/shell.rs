//! UI 样式快照 shim（ADR-0026 D3）。
//!
//! `UiShell` 本体在 [`wv_ui::shell`]；此处 `pub use` 保持
//! `crate::ui::shell::UiShell` 全部旧路径零改动。地形两 helper **留守**——
//! 它们依赖 `crate::i18n` 与 `crate::theme::colors`（世界色板，wv_render
//! re-export），不属公共控件。

#![cfg(feature = "app")]

use bevy_egui::egui;

use crate::i18n::{t, I18nKey, Locale};

pub use wv_ui::shell::UiShell;

/// 10 种地形的 i18n Key 表（与 `crate::i18n::table.rs` 的 zh/en 镜像同序）。
const TERRAIN_KEYS: [I18nKey; 10] = [
    I18nKey::TerrainKindDeepWater,
    I18nKey::TerrainKindShallowWater,
    I18nKey::TerrainKindBeach,
    I18nKey::TerrainKindGrassland,
    I18nKey::TerrainKindForest,
    I18nKey::TerrainKindHill,
    I18nKey::TerrainKindMountain,
    I18nKey::TerrainKindSnow,
    I18nKey::TerrainKindSwamp,
    I18nKey::TerrainKindRoad,
];

/// 按索引返回本地化后的地形名。索引越界时返回 `"?"`（防御性兜底，绝不在 UI 字符串里 panic）。
pub fn terrain_name_at(loc: Locale, idx: usize) -> &'static str {
    match TERRAIN_KEYS.get(idx) {
        Some(k) => t(loc, *k),
        None => "?",
    }
}

/// 地形色块索引 → `palette::CORE_TERRAIN` 同序（§4.5：色块就是地形本色）。
pub fn terrain_swatch(kind: u8) -> egui::Color32 {
    let c = crate::theme::colors::core_terrain(kind);
    let bevy::prelude::Color::Srgba(s) = c else {
        return egui::Color32::GRAY;
    };
    egui::Color32::from_rgb(
        (s.red * 255.0) as u8,
        (s.green * 255.0) as u8,
        (s.blue * 255.0) as u8,
    )
}
