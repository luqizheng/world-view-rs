//! 文字晕边（ADR-0014 M2）：地名 / 街名 `Text2d` 的纸色描边晕。
//!
//! ## 职责
//! - [`LabelHalo`] —— 晕边子实体标记（挂在标注实体之下，每标注 8 向副本）。
//! - [`attach_halo`] / [`sync_halo`] —— 池函数的挂载 / 每帧同步辅助。
//!
//! ## 为什么是「子实体 × 8」而不是 shader 描边
//! `Text2d` 无原生描边（drill.rs 历史注释已预留 halo TODO）；自定义 WGSL 是
//! 全仓第一个 shader，成本高。8 向偏移副本是经典纸面做法：晕色 = 该主题纸面
//! 色（[`PaletteColors::halo_fill`]），在绿地 / 水面 / 等值线密集处把文字「托」出
//! 来。子实体继承父标注的屏幕对齐旋转——偏移量定义在**父局部系**（即屏幕系），
//! 地图怎么转晕都贴着字走。
//!
//! ## 边界（不做什么）
//! - **不池化晕边**——晕边随父标注生灭（父池已防 spawn 抖动，子实体只是
//!   跟随者）；幅面内标注量级（城市点 + 编辑级街名）× 8 仍远小于地形纹理预算。

#![cfg(feature = "app")]

use bevy::prelude::*;
use core::f32::consts::FRAC_1_SQRT_2;

/// 晕边子实体标记。
#[derive(Component)]
pub struct LabelHalo;

/// 晕边偏移（屏幕像素基准；每帧乘 `view.scale` 转世界单位）。
pub const HALO_PX: f32 = 1.1;
/// 晕边相对父标注的 Z 后撤（同 Z 绘制序不定，后撤保证晕在下、字在上）。
pub const HALO_LOCAL_Z: f32 = -0.005;
/// 8 向偏移（4 正交 + 4 对角，对角为 1/√2 归一化字面量——`normalize` 非 const）。
/// 正交留对角缝、对角留正交缝，8 向才闭合。
pub const HALO_DIRS: [Vec2; 8] = [
    Vec2::X,
    Vec2::NEG_X,
    Vec2::Y,
    Vec2::NEG_Y,
    Vec2::new(FRAC_1_SQRT_2, FRAC_1_SQRT_2),
    Vec2::new(FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
    Vec2::new(-FRAC_1_SQRT_2, FRAC_1_SQRT_2),
    Vec2::new(-FRAC_1_SQRT_2, -FRAC_1_SQRT_2),
];

/// 单个晕边子实体的组件束（池的 spawn 分支在 `with_children` 闭包里循环 spawn）。
///
/// 返回具体元组而非走 `ChildBuilder` 参数——后者在 Bevy 版本间是别名
/// （`RelatedSpawner<ChildOf>`），不把版本细节写进公开签名。
pub fn halo_bundle(
    text: &str,
    font: Option<&Handle<Font>>,
    font_size: f32,
    halo_color: Color,
) -> (LabelHalo, Text2d, TextFont, TextColor, Transform) {
    (
        LabelHalo,
        Text2d::new(text.to_string()),
        TextFont {
            font_size: font_size.into(),
            font: font.cloned().unwrap_or_default().into(),
            ..Default::default()
        },
        TextColor(halo_color),
        Transform::default(),
    )
}

/// 每帧同步一个标注的晕边子实体（池的逐标注同步分支调用）。
///
/// 偏移量写在**父局部系**：父变换已含屏幕对齐旋转，局部 x/y 即屏幕方向，
/// 地图旋转不影响晕的屏幕观感。`halo_color` 与字号随主题 / 缩放每帧对齐。
pub fn sync_halo<F>(
    children: &Children,
    halos: &mut Query<(&mut Transform, &mut Text2d, &mut TextFont, &mut TextColor), F>,
    text: &str,
    font_size: f32,
    font: Option<&Handle<Font>>,
    halo_color: Color,
    offset_world: f32,
) where
    F: bevy::ecs::query::QueryFilter,
{
    let mut synced = 0usize;
    for child in children.iter() {
        let Ok((mut tf, mut txt, mut style, mut color)) = halos.get_mut(child) else {
            continue;
        };
        if synced < HALO_DIRS.len() {
            let d = HALO_DIRS[synced] * offset_world;
            tf.translation = Vec3::new(d.x, d.y, HALO_LOCAL_Z);
            *txt = Text2d::new(text.to_string());
            style.font_size = font_size.into();
            if let Some(f) = font {
                style.font = f.clone().into();
            }
            *color = TextColor(halo_color);
        }
        synced += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 8 向偏移覆盖 4 正交 + 4 对角，且模长一致（晕圈均匀的前提）。
    #[test]
    fn halo_dirs_cover_eight_even_directions() {
        assert_eq!(HALO_DIRS.len(), 8);
        let len = HALO_DIRS[0].length();
        for d in HALO_DIRS {
            assert!((d.length() - len).abs() < 1e-6, "方向 {d:?} 模长不齐");
        }
        // 正交 + 对角各半。
        let axis = HALO_DIRS
            .iter()
            .filter(|d| d.x.abs() < 1e-6 || d.y.abs() < 1e-6)
            .count();
        assert_eq!(axis, 4, "应有 4 个正交方向");
    }
}
