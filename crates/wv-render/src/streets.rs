//! 街道矢量层：把当前图 `LayerKind::Road` 图层的街道文档转成「路段流」。
//!
//! ## 职责
//! - 只做**取数与变换**：`World.streets`（键 = 当前图 ID）→ 本图局部坐标折线段，
//!   交给 [`crate::vector_layers`] 渲染。
//! - 取数从 `World.streets` 而非磁盘（2026-10-13 改动）：编辑立即可见，
//!   加载期一次性从 YAML 读入内存缓存。
//!
//! ## 边界（不做什么）
//! - **不做渲染决策**——线宽 / 颜色 / LOD 档位在 `vector_layers` 按
//!   `wv_core::RoadClass::lod_rank` 派生。
//! - **与 `distance.edges` 分清**——后者是城市间行程连线，不与街道层互读写。
//!
//! ## 主要导出
//! - `sync_streets_to_projection` — 当前图 / `WorldProjection` 变更时把路段对齐到 `vector_layers`。
//! - `UndoLastStreetSegment` — 撤销最近一段笔刷画的街道。
//!
//! 其余帧早退，不做逐帧克隆。

#![cfg(feature = "app")]

use bevy::prelude::*;

use wv_core::RoadClass;

use crate::layer_frame::LayerFrame;
use crate::state::{DrillState, GridConfig, LodTier, PlaybackState, ViewState, WorldProjection};

/// 当前图的街道图层缓存。
#[derive(Resource, Default)]
pub struct StreetLayerCache {
    /// 已加载的图 id；与当前图不同则整个缓存作废重载。
    pub map_id: Option<wv_core::Id>,
    /// `(Layer.source 路径, 文档)`。顺序即图层声明顺序（绘制顺序）。
    pub sources: Vec<(String, wv_core::StreetLayer)>,
}

/// 每帧对齐缓存：当前图变了、或 `World.streets` 刚被编辑改过，就从 `World`
/// 重取当前图的街道文档。
pub fn reload_street_cache(
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut cache: ResMut<StreetLayerCache>,
) {
    let current = drill.current_node(&projection.world);
    let key = current.map(|n| n.id.clone());
    // 图未变且世界未被编辑改过 → 早退（避免每帧整份克隆街道文档）。
    if cache.map_id == key && !projection.is_changed() {
        return;
    }
    cache.map_id = key;
    cache.sources.clear();

    let Some(node) = current else {
        return;
    };
    for layer in node
        .layers
        .iter()
        .filter(|l| matches!(l.kind, wv_core::map::LayerKind::Road) && l.visible)
    {
        let Some(src) = layer.source.as_deref() else {
            // 声明了 Road 图层但没写 source：提示作者补齐，不算错误。
            warn!(
                "图层 `{}`（kind: road）未声明 source，无街道几何可画",
                layer.id.as_str()
            );
            continue;
        };
        // World 里没有（文件缺失/损坏没读进来，或图是内存造的）→ 空文档
        // 兜底：手绘的第一笔会经命令在这里落进 World（V1 不阻止）。
        let doc = projection
            .world
            .street_of(&layer.id)
            .cloned()
            .unwrap_or_default();
        cache.sources.push((src.to_string(), doc));
    }
}

/// LOD 口径：越远的视野只保留越主干（`lod_rank` 越小）的街道。
///
/// ADR-0047 D3 起：`lod_rank` 先经 `RoadClass::presentation_rank_from_lod`
/// 折算成统一呈现级别，再过统一门槛 [`crate::state::rank_visible_at_tier`]。
/// 折算表（0–2→3 / 3–5→2 / 6–7→1）使本函数与旧硬编码阈值（Nation ≤ 2 /
/// Region ≤ 5 / Edit 全见）**逐档等价**——等价性由
/// `street_tier_gate_equals_legacy_thresholds` 钉死；改门槛只动
/// `rank_visible_at_tier`，不再各处散落。
pub fn street_kept_at_tier(rank: u8, tier: LodTier) -> bool {
    crate::state::rank_visible_at_tier(wv_core::RoadClass::presentation_rank_from_lod(rank), tier)
}

/// 把缓存里的全部文档展开成「路段流」：`(lod_rank, 折线段列表)`。
///
/// 折线的相邻点两两连成线段；退化折线（少于 2 点）跳过。
/// 坐标仍是**本图局部坐标**——转视图空间是渲染层的事
/// （[`GridConfig::map_to_view`]，注意不是 `raster_origin`：城市继承父图底图时
/// 栅格原点停在世界原点，用它换算整层街道会被推到视野外）。
pub fn street_streams(sources: &[(String, wv_core::StreetLayer)]) -> Vec<(u8, Vec<(Vec2, Vec2)>)> {
    let mut streams: Vec<(u8, Vec<(Vec2, Vec2)>)> = Vec::new();
    for (_, doc) in sources {
        for seg in &doc.segments {
            if seg.points.len() < 2 {
                continue;
            }
            let rank = seg.class.lod_rank();
            let entry = match streams.iter_mut().find(|(r, _)| *r == rank) {
                Some(entry) => entry,
                None => {
                    streams.push((rank, Vec::new()));
                    streams.last_mut().expect("刚 push 过，必有元素")
                }
            };
            for pair in seg.points.windows(2) {
                entry.1.push((
                    Vec2::new(pair[0].x, pair[0].y),
                    Vec2::new(pair[1].x, pair[1].y),
                ));
            }
        }
    }
    streams
}

/// 街道名标注（Phase 5：仿 `drill.rs` 的 `PlaceLabel` 对象池）。
#[derive(Component)]
pub struct StreetLabel;

/// 中点标注行：`(本图局部中点, 段方向单位向量, 街名, 段级存在窗口)`。
pub type NamedMidpoint = (Vec2, (f32, f32), String, Option<wv_core::TimeSpan>);

/// 有名字段的中点表——标注的取数口径，纯函数可测。
///
/// 第三元 = 该段的 `time_span`（`None` = 跟随子图）。ADR-0015：段窗口外
/// 街名与街道几何同生共死，过滤在标注系统内做（`label_visible_at`）。
/// 第二元 = 中点所在折线段的行进方向（单位向量）——沿街旋转标注
/// （ADR-0073）据此算文字的屏幕倾角。
pub fn named_midpoints(sources: &[(String, wv_core::StreetLayer)]) -> Vec<NamedMidpoint> {
    let mut out = Vec::new();
    for (_, doc) in sources {
        for seg in &doc.segments {
            if let (Some(name), Some((mid, dir))) = (&seg.name, seg.polyline_midpoint_with_dir()) {
                out.push((Vec2::new(mid.x, mid.y), dir, name.clone(), seg.time_span));
            }
        }
    }
    out
}

/// 街道名文字的屏幕像素字号。
///
/// ADR-0065：旧值 9px 违反 `ART_STYLE_SPEC` §4.2 硬约束 3「中文字号 ≥ 12px」——
/// 12px 以下的中文在 1x 屏不可读，街名只能靠猜。与最小一级地名同档 12px；
/// 主次靠字色 / 层级（压在 POI 之下）区分，不靠压小字号。
const STREET_LABEL_PX: f32 = 12.0;
/// 标注层：压在街道线（0.115）之上、地名（0.4）之下。
const STREET_LABEL_Z: f32 = 0.35;

/// 街道名标注池：把缓存里**有名字**的段的中点标上名字。
///
/// LOD 口径：街道名只在编辑级视野出现——省/国家级一屏几十条街的名字
/// 会把图变成字海（Google 地图也是拉近才出街名）。
///
/// **沿街旋转 + 直立翻转**（ADR-0073）：文字沿中点所在段的方向倾斜，
/// 朝左（|倾角| > 90°）的段翻 180°，永远头朝上、从左往右读——与水平
/// 时期的「取相机旋转的逆」在零朝向 + 水平段时同值，零视觉回归。
/// 占位上街名最后落位（`.after(update_sea_labels)`），让位给地名与面标签。
///
/// 图层归属（ADR-0017 §2.9 Q3 全接管）：
/// - ADR-0013 §2.2：街名随 Road 图层（`layers.road.visible`）
/// - ADR-0017 Q1：标签走独立渲染组（`layers.label.visible`）
/// - 两者合取——任一关闭即街名消失。
/// - LodTier 过滤**去除**（Q3：不再只在 Edit 级别显示）。
/// - 时间过滤（ADR-0015）：图级窗口（`MapNode.time_span`）∧ 段级窗口
///   （`StreetSegment.time_span`）合取，经 `label_visible_at` 单一求值——
///   街道几何不显示的时段，街名一并退场。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_street_labels(
    mut commands: Commands,
    recipe: Res<crate::recipe::RenderRecipe>,
    cache: Res<StreetLayerCache>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
    playback: Res<crate::state::PlaybackState>,
    // 统一占位池（ADR-0073）：街名最后落位，让位给地名 / 面标签 / 事件。
    mut occupancy: ResMut<crate::label_collision::LabelOccupancy>,
    fonts: Res<crate::theme::fonts::LoadedFonts>,
    camera: Query<&Transform, With<Camera2d>>,
    mut q: Query<
        (
            Entity,
            &mut Transform,
            &mut Text2d,
            &mut TextFont,
            &mut TextColor,
            &mut Visibility,
        ),
        (With<StreetLabel>, Without<Camera2d>),
    >,
    children_q: Query<&bevy::ecs::hierarchy::Children, With<StreetLabel>>,
    mut halo_q: Query<
        (&mut Transform, &mut Text2d, &mut TextFont, &mut TextColor),
        (
            With<crate::label_halo::LabelHalo>,
            Without<bevy::prelude::Camera2d>,
            Without<StreetLabel>,
        ),
    >,
) {
    // 期望标注集：有名字的段 × 折线中点 + 段方向（本图局部 → 视图空间，走
    // `map_to_view`）。图层不可见 → wanted 为空 → 池全隐藏（尾部的「多余的
    // 隐藏」路径）。
    let mut wanted: Vec<(Vec2, (f32, f32), String)> = Vec::new();
    // ADR-0017 §2.9 Q3：街名走 Label 渲染组 + Road 图层（合取）。
    // 去掉 LodTier::Edit 限制——Q3 拍板"全接管"，所有级别都显示街名。
    if layers.road.visible
        && layers.label.visible
        && recipe.enabled(crate::recipe::PassId::StreetLabels)
    {
        // ADR-0015 时间过滤：图级窗口（当前图）∧ 段级窗口（每段自带）。
        let t = playback.current_day;
        let map_span = drill
            .current_node(&projection.world)
            .and_then(|node| node.time_span.as_ref());
        for (mid, dir, name, seg_span) in named_midpoints(&cache.sources) {
            if wv_core::label_visible_at(map_span, seg_span.as_ref(), t) {
                wanted.push((grid.map_to_view(mid), dir, name));
            }
        }
    }
    // 无变化帧也照常对齐：位置/字号随视图每帧都可能变，池对齐本身极轻。

    let unit_scale = view.scale.max(0.05);
    let cam_rot = camera
        .single()
        .map(|t| t.rotation)
        .unwrap_or(Quat::IDENTITY);
    // 相机 x 轴的世界方向角：screen→world = 相机旋转（与拾取同一条已验证
    // 变换链），街段的世界方向角减去它 = 街段在屏幕上的倾角。
    let cam_fwd = cam_rot.mul_vec3(Vec3::X);
    let cam_yaw = cam_fwd.y.atan2(cam_fwd.x);

    // ADR-0065 → ADR-0073：街名纳入统一占位池，沿段方向旋转 + 直立翻转
    // （朝左的段翻 180°，文字永远头朝上从左往右读）。候选位只有段中点
    // 原位——街名必须钉在路上，没有备选方位；预算内放不下就让位。
    let mut candidates: Vec<crate::label_collision::LabelCandidate> = Vec::new();
    for (pos, dir, name) in wanted.drain(..) {
        let phi = crate::label_collision::upright_angle(dir.1.atan2(dir.0) - cam_yaw);
        let priority = crate::label_collision::priority_for(1, &name);
        let mut cand = crate::label_collision::LabelCandidate::new(
            crate::label_collision::LabelPass::Street,
            pos,
            name,
            STREET_LABEL_PX,
            priority,
            None,
        );
        cand.angle = Some(phi);
        candidates.push(cand);
    }
    let filtered =
        crate::label_collision::filter_collisions(&mut candidates, unit_scale, &mut occupancy);

    let font_size = STREET_LABEL_PX * unit_scale;
    // ADR-0017 §2.9 Q3：街名走 Label 渲染组的 z 和 opacity。
    let label_z = STREET_LABEL_Z + layers.label.z_offset;
    let cjk = fonts.label_handle();
    let label_color = match view.mode {
        crate::state::ViewMode::GoogleMap => crate::theme::colors_google::GOOGLE_LABEL_FILL,
        // 古舆图 / 暮色：深褐墨（暮色为浅米），不再回退纯白（ADR-0016）。
        _ => view.active_theme.palette.label_fill,
    }
    .with_alpha(layers.label.opacity);
    // 晕边色（ADR-0016 M2）：与地名同口径（Google 白晕兜底）。
    let halo_color = match view.mode {
        crate::state::ViewMode::GoogleMap => bevy::color::Color::WHITE,
        _ => view.active_theme.palette.halo_fill,
    }
    .with_alpha(layers.label.opacity);
    let halo_offset = crate::label_halo::halo_offset_world(STREET_LABEL_PX, unit_scale);

    let mut iter = q.iter_mut();
    for c in &filtered {
        let pos = c.pos;
        let name: &str = c.name.as_str();
        // 沿街文字实体旋转 = rot_z(屏幕倾角) · 相机旋转——倾角已含相机角
        //（见 cam_yaw），该构造保证文字在屏幕上与街段平行；零朝向 +
        // 水平段时退化为与旧「相机旋转的逆」同值。
        let rot = Quat::from_rotation_z(c.angle.unwrap_or(0.0)) * cam_rot;
        if let Some((e, mut tf, mut text, mut style, mut color, mut vis)) = iter.next() {
            tf.translation = pos.extend(label_z);
            tf.rotation = rot;
            *text = Text2d::new(name);
            style.font_size = font_size.into();
            if let Some(f) = &cjk {
                style.font = f.clone().into();
            }
            *color = TextColor(label_color);
            *vis = Visibility::Visible;
            // 晕边子实体随字同步（8 向纸色副本）。
            if let Ok(children) = children_q.get(e) {
                crate::label_halo::sync_halo(
                    children,
                    &mut halo_q,
                    name,
                    font_size,
                    cjk.as_ref(),
                    halo_color,
                    halo_offset,
                );
            }
        } else {
            let mut cmd = commands.spawn((
                StreetLabel,
                Text2d::new(name.to_string()),
                TextFont {
                    font_size: font_size.into(),
                    font: cjk.clone().unwrap_or_default().into(),
                    ..Default::default()
                },
                TextColor(label_color),
                Transform {
                    translation: pos.extend(label_z),
                    rotation: rot,
                    ..Default::default()
                },
                Visibility::Visible,
            ));
            let (text_owned, font_ref, fs, hc) =
                (name.to_string(), cjk.as_ref(), font_size, halo_color);
            cmd.with_children(move |c| {
                for _ in 0..crate::label_halo::HALO_DIRS.len() {
                    c.spawn(crate::label_halo::halo_bundle(
                        &text_owned,
                        font_ref,
                        fs,
                        hc,
                    ));
                }
            });
        }
    }
    // 池里多出来的实体隐藏（复用不 despawn，与 PlaceLabel 同哲学）。
    for (_, mut _tf, mut _text, mut _style, mut _color, mut vis) in iter {
        *vis = Visibility::Hidden;
    }
}

/// 街道手绘编辑器的**会话状态**（ADR-0010 第 3 项的 GUI 侧）。
///
/// 只存「本会话怎么画」，不存任何街道数据——街道文档的唯一归宿是
/// `World.streets`（经 `wv-edit` 命令），这里连缓存都不留（P6/P7 同款边界）。
#[derive(Resource, Default)]
pub struct StreetEditorState {
    /// 是否处于画街道模式（开启时左键不再拖动地图）。
    pub enabled: bool,
    /// 新段的类别。
    pub class: RoadClass,
    /// 连线的上一个落点（**世界坐标**）；`None` = 尚未起笔 / 已结束连线。
    pub last_click: Option<Vec2>,
    /// 本次会话已提交的段数（面板显示用）。
    pub drawn: usize,
    /// 新段的街道名（空串 = 无名）。落进下一笔 `StreetSegment.name`。
    pub name: String,
}

/// 当前图上可编辑的街道图层（`kind: road` 且可见、未锁定、有 source）。
///
/// 取「第一个」；多入口歧义已登记 README §12-22（同口径）。
pub fn target_road_layer(node: &wv_core::map::MapNode) -> Option<&wv_core::map::Layer> {
    node.layers.iter().find(|l| {
        matches!(l.kind, wv_core::map::LayerKind::Road)
            && l.visible
            && !l.locked
            && l.source.is_some()
    })
}

/// 街道手绘：左键逐点连线（上一落点 → 本次点击 = 一直段），右键结束连线。
///
/// 与地形笔刷同一条纪律：本系统只生成 `AddStreetSegmentCommand` 交给
/// `EditHistory`，不直接改 `World`（AGENTS §3.3）。点取自
/// `ViewState.cursor_world`（`track_cursor_world` 每帧无条件刷新，便于测试）；
/// 世界 → 本图局部走 `coord::resolve_frame_in`，与标记/栅格同一条坐标链。
#[allow(clippy::too_many_arguments)]
pub fn commit_street_click(
    buttons: Res<ButtonInput<MouseButton>>,
    view: Res<ViewState>,
    playback: Res<PlaybackState>,
    ui: Res<crate::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    projection: Res<WorldProjection>,
    drill: Res<DrillState>,
    mut editor: ResMut<StreetEditorState>,
    mut intents: ResMut<crate::editing_bridge::EditIntents>,
    mut acks: ResMut<StreetCommitAcks>,
) {
    if !editor.enabled {
        return;
    }
    // 悬在 UI 上 / 播放中只读 / 极低缩放禁画（与地形笔刷同口径）。
    // 无工程闸门同口径（2026-10-14）：没打开任何工程时不允许落笔。
    if ui.wants_pointer || playback.playing || view.scale < 0.15 {
        return;
    }
    if projection.project_dir.is_none() {
        return;
    }
    if buttons.just_pressed(MouseButton::Right) {
        editor.last_click = None;
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(cursor) = view.cursor_world else {
        return;
    };

    // 目标图层：当前图第一个「可见、未锁定、有 source」的 Road 图层。
    let Some(node) = drill.current_node(&projection.world) else {
        return;
    };
    let Some(layer) = target_road_layer(node) else {
        warn!("当前图没有可编辑的街道图层（需要 kind: road 且声明 source 且未锁定）");
        return;
    };

    // 视图（根帧）坐标 → 本图局部：走 `drill::map_origin` 这唯一的图帧实现点
    // （解析不到时它退回栅格原点，等价于「本图就是栅格那张图」的旧行为）。
    let origin = crate::drill::map_origin(&projection.world, node, &grid);
    let local = cursor - origin;

    // 首点只起笔不画段。
    let Some(prev_world) = editor.last_click else {
        editor.last_click = Some(cursor);
        return;
    };
    let prev_local = prev_world - origin;

    let seg = wv_core::StreetSegment::straight(
        editor.class,
        prev_local.x,
        prev_local.y,
        local.x,
        local.y,
    );
    let mut seg = seg;
    // 面板里填了名字 → 本段命名；空串 = 无名（不产生标注）。
    let trimmed = editor.name.trim().to_string();
    if !trimmed.is_empty() {
        seg.name = Some(trimmed);
    }
    let seq = intents.push_street(crate::editing_bridge::StreetSegmentIntent {
        layer_id: layer.id.clone(),
        segment: seg,
    });
    acks.0.push((seq, cursor));
}

/// 街道提交的挂起回执队列：`(seq, 本笔光标世界坐标)`。
///
/// `commit_street_click` 写、`apply_street_receipts` 读。渲染层不持有
/// `EditHistory`（ADR-0020 §2.5），「提交成功」由应用层翻译器经
/// `crate::editing_bridge::EditReceipts` 回执后，这里才推进本地状态。
#[derive(Resource, Default)]
pub struct StreetCommitAcks(Vec<(u64, Vec2)>);

/// 消费编辑回执：**提交成功才**推进连线锚点与计数——失败不动本地，
/// 与拆 crate 前行为一致（ADR-0020 §2.5 / 闸门 8）。
/// 必须排在 `wv-app` 翻译器之后跑（顺序由 app_bootstrap 钉死）。
pub fn apply_street_receipts(
    mut receipts: ResMut<crate::editing_bridge::EditReceipts>,
    mut acks: ResMut<StreetCommitAcks>,
    mut editor: ResMut<StreetEditorState>,
) {
    for (seq, cursor) in std::mem::take(&mut acks.0) {
        if !receipts.take(seq) {
            continue; // 提交失败 / 未回执：与旧行为一致，不推进锚点。
        }
        editor.last_click = Some(cursor);
        editor.drawn += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::{Point, RoadClass, StreetLayer, StreetSegment};

    fn doc_of(segments: Vec<StreetSegment>) -> wv_core::StreetLayer {
        let mut doc = StreetLayer::new();
        for s in segments {
            doc.push(s);
        }
        doc
    }

    #[test]
    fn named_midpoints_picks_only_named_segments() {
        let sources = vec![(
            "roads.yaml".to_string(),
            doc_of(vec![
                StreetSegment::straight(RoadClass::Primary, 0.0, 0.0, 10.0, 0.0),
                StreetSegment {
                    class: RoadClass::Residential,
                    points: vec![Point::new(0.0, 5.0), Point::new(8.0, 5.0)],
                    name: Some("户部巷".into()),
                    time_span: None,
                },
            ]),
        )];
        let mids = named_midpoints(&sources);
        assert_eq!(mids.len(), 1, "无名段不产生标注");
        assert_eq!(mids[0].2, "户部巷");
        assert!(
            (mids[0].0.x - 4.0).abs() < 1e-4 && (mids[0].0.y - 5.0).abs() < 1e-4,
            "中点应为 (4, 5)，实得 {:?}",
            mids[0].0
        );
    }

    #[test]
    fn named_midpoints_carries_each_segment_time_span() {
        // ADR-0015：段级窗口随中点一起交给标注系统过滤。
        let sources = vec![(
            "roads.yaml".to_string(),
            doc_of(vec![
                StreetSegment {
                    class: RoadClass::Primary,
                    points: vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
                    name: Some("无名无窗".into()),
                    time_span: None,
                },
                StreetSegment {
                    class: RoadClass::Residential,
                    points: vec![Point::new(0.0, 5.0), Point::new(10.0, 5.0)],
                    name: Some("清末新路".into()),
                    time_span: Some(wv_core::TimeSpan::new(1850, 1912).unwrap()),
                },
            ]),
        )];
        let mids = named_midpoints(&sources);
        assert_eq!(mids.len(), 2);
        assert!(mids[0].3.is_none(), "无窗段携带 None");
        assert_eq!(
            mids[1].3,
            Some(wv_core::TimeSpan::new(1850, 1912).unwrap()),
            "有窗段携带原窗口"
        );
        // 过滤语义与 label_visible_at 一致：窗口外（1912 起）不可见。
        assert!(
            !wv_core::label_visible_at(None, mids[1].3.as_ref(), 1912),
            "段窗口 [1850,1912)：t=1912 不可见（半开不含右）"
        );
        assert!(
            wv_core::label_visible_at(None, mids[1].3.as_ref(), 1900),
            "段窗口 [1850,1912)：t=1900 可见"
        );
    }

    #[test]
    fn nation_tier_keeps_only_major_street_ranks() {
        assert!(street_kept_at_tier(
            RoadClass::Motorway.lod_rank(),
            LodTier::Nation
        ));
        assert!(street_kept_at_tier(
            RoadClass::Primary.lod_rank(),
            LodTier::Nation
        ));
        assert!(!street_kept_at_tier(
            RoadClass::Residential.lod_rank(),
            LodTier::Nation
        ));
        assert!(street_kept_at_tier(
            RoadClass::Residential.lod_rank(),
            LodTier::Region
        ));
        assert!(street_kept_at_tier(
            RoadClass::Path.lod_rank(),
            LodTier::Edit
        ));
    }

    /// ADR-0047 D3 等价性：折算 + 统一门槛的结果必须与**旧硬编码阈值**
    /// （Nation ≤ 2 / Region ≤ 5 / Edit 全见）在全部 8 个 lod 档 × 3 视野档
    /// 上逐位一致——这是「街道呈现行为零变化」的客观证据。
    #[test]
    fn street_tier_gate_equals_legacy_thresholds() {
        let legacy = |rank: u8, tier: LodTier| match tier {
            LodTier::Nation => rank <= 2,
            LodTier::Region => rank <= 5,
            LodTier::Edit => true,
        };
        for lod in 0..=7u8 {
            for tier in [LodTier::Edit, LodTier::Region, LodTier::Nation] {
                assert_eq!(
                    street_kept_at_tier(lod, tier),
                    legacy(lod, tier),
                    "lod {lod} × {tier:?}：新折算门槛必须与旧阈值同值"
                );
            }
        }
    }

    #[test]
    fn polylines_flatten_into_adjacent_segment_pairs() {
        let docs = vec![(
            "maps/city/roads.yaml".to_string(),
            doc_of(vec![StreetSegment {
                class: RoadClass::Residential,
                points: vec![
                    Point::new(0.0, 0.0),
                    Point::new(4.0, 0.0),
                    Point::new(4.0, 4.0),
                ],
                name: None,
                time_span: None,
            }]),
        )];
        let streams = street_streams(&docs);
        assert_eq!(streams.len(), 1, "单一类别应聚成一档");
        let (rank, segments) = &streams[0];
        assert_eq!(*rank, RoadClass::Residential.lod_rank());
        assert_eq!(segments.len(), 2, "3 点折线应产生 2 段");
        assert_eq!(segments[0], (Vec2::ZERO, Vec2::new(4.0, 0.0)));
        assert_eq!(segments[1], (Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0)));
    }

    #[test]
    fn degenerate_polylines_and_empty_docs_are_skipped() {
        let docs = vec![
            (
                "a.yaml".to_string(),
                doc_of(vec![StreetSegment {
                    class: RoadClass::Path,
                    points: vec![Point::new(1.0, 1.0)],
                    name: None,
                    time_span: None,
                }]),
            ),
            ("b.yaml".to_string(), StreetLayer::new()),
        ];
        assert!(street_streams(&docs).is_empty(), "无有效折线时应产出空流");
    }

    #[test]
    fn classes_group_into_distinct_rank_buckets() {
        let docs = vec![(
            "c.yaml".to_string(),
            doc_of(vec![
                StreetSegment::straight(RoadClass::Motorway, 0.0, 0.0, 1.0, 0.0),
                StreetSegment::straight(RoadClass::Service, 0.0, 0.0, 1.0, 0.0),
                StreetSegment::straight(RoadClass::Track, 0.0, 0.0, 1.0, 0.0),
            ]),
        )];
        let streams = street_streams(&docs);
        // Motorway=0 一档；Service 与 Track 同为 rank 6，聚一档。
        assert_eq!(streams.len(), 2, "应按 rank 聚成 2 档，实际 {streams:?}");
        assert_eq!(streams[0].0, RoadClass::Motorway.lod_rank());
        assert_eq!(streams[1].1.len(), 2, "service 与 track 应同档");
    }

    /// 端到端：钻入带 Road 图层的城市后，reload 应从 `World.streets` 取出
    /// 该图层的文档；缓存按当前图 id 判重，同图且世界未变时不重复取。
    #[test]
    fn reload_reads_declared_street_layers_for_the_current_map_from_the_world() {
        // 内存世界：map_world(根) → map_city(带 Road 图层)。
        let mut world = wv_core::World::default();
        let mut root = wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked("map_world"),
            name: "世界".into(),
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: vec![wv_core::Id::new_unchecked("map_city")],
            icons: Vec::new(),
            layers: Vec::new(),
            extent: Some(wv_core::map::Extent::sized(4096.0, 4096.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        };
        let mut city = wv_core::map::MapNode {
            orientation: 0.0,
            level: wv_core::map::MapLevel::City,
            parent: Some(wv_core::Id::new_unchecked("map_world")),
            base: None,
            ..root.clone()
        };
        city.id = wv_core::Id::new_unchecked("map_city");
        city.name = "城市".into();
        city.children = Vec::new();
        city.layers = vec![wv_core::map::Layer {
            id: wv_core::Id::new_unchecked("roads"),
            name: "街道".into(),
            kind: wv_core::map::LayerKind::Road,
            visible: true,
            locked: false,
            opacity: 1.0,
            z: 0,
            source: Some("maps/map_city/roads.yaml".into()),
        }];
        root.children = vec![city.id.clone()];
        world.maps = vec![root, city];

        // wv-io 加载时读进来的街道文档（磁盘文件在 wv-io 侧已另有 round-trip
        // 与工程级测试，这里聚焦缓存取数口径）。
        let mut doc = StreetLayer::new();
        doc.push(StreetSegment::straight(
            RoadClass::Primary,
            0.0,
            0.0,
            10.0,
            0.0,
        ));
        world.set_streets(wv_core::Id::new_unchecked("roads"), doc);

        // Bevy 侧资源 + 单系统调度。
        let mut app_world = bevy::prelude::World::default();
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&world);
        assert!(
            drill.enter(&world, &wv_core::Id::new_unchecked("map_city")),
            "应能钻入 map_city"
        );
        let projection = crate::state::WorldProjection {
            world,
            project_dir: Some(".".into()),
            ..crate::state::WorldProjection::default()
        };
        app_world.insert_resource(projection);
        app_world.insert_resource(drill);
        app_world.insert_resource(StreetLayerCache::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(reload_street_cache);
        schedule.run(&mut app_world);
        {
            let cache = app_world.resource::<StreetLayerCache>();
            assert_eq!(
                cache.sources.len(),
                1,
                "应取出 1 份街道文档，实际 {:?}",
                cache.sources
            );
            assert_eq!(cache.sources[0].0, "maps/map_city/roads.yaml");
            assert_eq!(cache.sources[0].1.len(), 1);
            assert_eq!(cache.sources[0].1.segments[0].class, RoadClass::Primary);
        }
        // 同图且世界未变：不重复取（sources 不翻倍）。
        schedule.run(&mut app_world);
        let cache = app_world.resource::<StreetLayerCache>();
        assert_eq!(cache.sources.len(), 1, "同图重跑不应重复加载");
    }

    /// 编辑闭环：手绘一笔（`World.streets` 被改 → `WorldProjection` 触发
    /// change 检测）后，缓存应立刻带上新段——街道编辑器所见即所得的根基。
    #[test]
    fn editor_drawn_segment_reaches_the_cache_on_world_change() {
        let mut world = wv_core::World::default();
        let city = wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked("map_city"),
            name: "城市".into(),
            // ensure_root 无 meta 时退回「第一个 level: world 的图」，单图
            // 测试必须把级别设为 World，否则 drill.current() 恒为 None。
            level: wv_core::map::MapLevel::World,
            parent: None,
            children: Vec::new(),
            icons: Vec::new(),
            layers: vec![wv_core::map::Layer {
                id: wv_core::Id::new_unchecked("roads"),
                name: "街道".into(),
                kind: wv_core::map::LayerKind::Road,
                visible: true,
                locked: false,
                opacity: 1.0,
                z: 0,
                source: Some("maps/city/roads.yaml".into()),
            }],
            extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: None,
            outline_polygon: None,
            trashed: false,
            trashed_at: None,
            comment: None,
        };
        world.maps = vec![city];

        let mut app_world = bevy::prelude::World::default();
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&world);
        let projection = crate::state::WorldProjection {
            world,
            ..crate::state::WorldProjection::default()
        };
        app_world.insert_resource(projection);
        app_world.insert_resource(drill);
        app_world.insert_resource(StreetLayerCache::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(reload_street_cache);
        schedule.run(&mut app_world);
        assert!(
            app_world
                .resource::<StreetLayerCache>()
                .sources
                .iter()
                .all(|(_, d)| d.is_empty()),
            "初始无街道文档，缓存应为空文档兜底"
        );

        // 手绘一笔：把街道文档直接放进 World 的 streets 表（本测试关注缓存联动，
        // 不经编辑链；「意图 → History 提交」的测试在 wv-app::edit::intent_translator）。
        app_world
            .resource_mut::<crate::state::WorldProjection>()
            .world
            .streets
            .insert(
                wv_core::Id::new_unchecked("roads"),
                doc_of(vec![StreetSegment::straight(
                    RoadClass::Residential,
                    10.0,
                    10.0,
                    40.0,
                    10.0,
                )]),
            );

        schedule.run(&mut app_world);
        let cache = app_world.resource::<StreetLayerCache>();
        assert_eq!(cache.sources.len(), 1);
        assert_eq!(
            cache.sources[0].1.len(),
            1,
            "手绘的一笔应立刻出现在缓存里，实际 {:?}",
            cache.sources
        );
    }

    /// 手绘交互闭环（渲染侧契约）：两次左键点击经 `commit_street_click` 产出
    /// **编辑意图**；应用层回执（模拟 `wv-app::edit::intent_translator` 提交成功）
    /// 之后，本地状态（连线锚点 / 段计数）才推进；右键结束连线。
    /// 「意图 → `History.commit` → `dirty_set`」那半段在
    /// `wv-app::edit::intent_translator` 的测试里覆盖。
    #[test]
    fn street_clicks_emit_intents_and_receipts_advance_local_state() {
        let world = wv_core::World {
            maps: vec![wv_core::map::MapNode {
                id: wv_core::Id::new_unchecked("map_city"),
                name: "城市".into(),
                level: wv_core::map::MapLevel::World,
                parent: None,
                children: Vec::new(),
                icons: Vec::new(),
                layers: vec![wv_core::map::Layer {
                    id: wv_core::Id::new_unchecked("roads"),
                    name: "街道".into(),
                    kind: wv_core::map::LayerKind::Road,
                    visible: true,
                    locked: false,
                    opacity: 1.0,
                    z: 0,
                    source: Some("maps/city/roads.yaml".into()),
                }],
                extent: Some(wv_core::map::Extent::sized(1024.0, 1024.0)),
                raster: None,
                geographic_source: None,
                time_span: None,
                unit_per_km: None,
                orientation: 0.0,
                base: None,
                outline_polygon: None,
                trashed: false,
                trashed_at: None,
                comment: None,
            }],
            ..wv_core::World::default()
        };

        let mut app_world = bevy::prelude::World::default();
        let mut drill = crate::state::DrillState::default();
        drill.ensure_root(&world);
        app_world.insert_resource(crate::state::WorldProjection {
            world,
            project_dir: Some("/test/project".into()),
            ..crate::state::WorldProjection::default()
        });
        app_world.insert_resource(drill);
        app_world.insert_resource(ButtonInput::<MouseButton>::default());
        app_world.insert_resource(ViewState {
            scale: 1.0,
            cursor_world: Some(Vec2::new(100.0, 200.0)),
            ..ViewState::default()
        });
        app_world.insert_resource(PlaybackState::default());
        app_world.insert_resource(crate::camera::PointerOverUi::default());
        app_world.insert_resource(GridConfig::default());
        app_world.insert_resource(StreetEditorState {
            enabled: true,
            class: RoadClass::Primary,
            ..StreetEditorState::default()
        });
        app_world.insert_resource(crate::editing_bridge::EditIntents::default());
        app_world.insert_resource(crate::editing_bridge::EditReceipts::default());
        app_world.insert_resource(StreetCommitAcks::default());

        // 渲染侧两个系统分两个 schedule：真实 app 里翻译器（wv-app）夹在
        // commit 与 apply 之间同帧跑；测试用「先 commit 轮、手动翻译回执、
        // 再 apply 轮」复现同一时序（若合并一个 schedule，apply 会在回执
        // 出现前把挂起 ack 消费掉——那不是真实时序）。
        let mut commit_schedule = Schedule::default();
        commit_schedule.add_systems(commit_street_click);
        let mut apply_schedule = Schedule::default();
        apply_schedule.add_systems(apply_street_receipts);

        // 第一次点击：只起笔，不产意图。
        app_world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        commit_schedule.run(&mut app_world);
        assert_eq!(app_world.resource::<StreetEditorState>().drawn, 0);
        assert!(app_world
            .resource::<StreetEditorState>()
            .last_click
            .is_some());
        assert!(
            app_world
                .resource_mut::<crate::editing_bridge::EditIntents>()
                .drain_streets()
                .is_empty(),
            "首点只起笔，不应产意图"
        );

        // 松开再按第二次（换落点）：`clear()` 只清 just_*，`pressed` 还挂着，
        // 必须先 `release` 才能让下一次 `press` 重新产生 just_pressed。
        let mut buttons = app_world.resource_mut::<ButtonInput<MouseButton>>();
        buttons.release(MouseButton::Left);
        buttons.clear();
        app_world.resource_mut::<ViewState>().cursor_world = Some(Vec2::new(150.0, 180.0));
        app_world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        commit_schedule.run(&mut app_world);

        // 翻译器还没跑（它在 wv-app）：本地状态此刻**必须**没动。
        assert_eq!(
            app_world.resource::<StreetEditorState>().drawn,
            0,
            "回执前不得推进计数"
        );

        // 模拟应用层翻译器：取出意图、核对内容、回执成功，再跑回执系统。
        let drained = app_world
            .resource_mut::<crate::editing_bridge::EditIntents>()
            .drain_streets();
        assert_eq!(drained.len(), 1, "两次点击应产 1 条意图");
        let (seq, intent) = &drained[0];
        assert_eq!(intent.layer_id, wv_core::Id::new_unchecked("roads"));
        assert_eq!(intent.segment.class, RoadClass::Primary);
        assert_eq!(intent.segment.points.len(), 2);
        assert_eq!(
            intent.segment.points[0],
            Point::new(100.0, 200.0),
            "段起点 = 首笔光标"
        );
        assert_eq!(
            intent.segment.points[1],
            Point::new(150.0, 180.0),
            "段终点 = 第二笔光标"
        );
        app_world
            .resource_mut::<crate::editing_bridge::EditReceipts>()
            .commit(*seq);
        apply_schedule.run(&mut app_world);

        {
            let editor = app_world.resource::<StreetEditorState>();
            assert_eq!(
                editor.drawn, 1,
                "回执后应推进 1 段（若为 0 则回执链没接上）"
            );
            assert_eq!(
                editor.last_click,
                Some(Vec2::new(150.0, 180.0)),
                "锚点应推进到第二笔"
            );
        }

        // 右键结束连线：下一落点不再接段（右键分支在 commit 系统里）。
        app_world.resource_mut::<ButtonInput<MouseButton>>().clear();
        app_world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        commit_schedule.run(&mut app_world);
        assert!(app_world
            .resource::<StreetEditorState>()
            .last_click
            .is_none());
    }
}
