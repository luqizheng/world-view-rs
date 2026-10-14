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

use crate::render::layer_frame::LayerFrame;
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

/// LOD 口径：越远的视野只保留 `lod_rank` 越小（越主干）的街道。
///
/// 阈值与 [`crate::state::poi_kept_at_tier`] 同一档位体系：
/// 国家档只留 rank ≤ 2（motorway / trunk / primary），省级 ≤ 5，
/// 编辑级全保留。首版阈值可调，登记在 ADR-0010 §5。
pub fn street_kept_at_tier(rank: u8, tier: LodTier) -> bool {
    match tier {
        LodTier::Nation => rank <= 2,
        LodTier::Region => rank <= 5,
        LodTier::Edit => true,
    }
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

/// 有名字段的中点表（**本图局部坐标**）——标注的取数口径，纯函数可测。
pub fn named_midpoints(sources: &[(String, wv_core::StreetLayer)]) -> Vec<(Vec2, String)> {
    let mut out = Vec::new();
    for (_, doc) in sources {
        for seg in &doc.segments {
            if let (Some(name), Some(mid)) = (&seg.name, seg.polyline_midpoint()) {
                out.push((Vec2::new(mid.x, mid.y), name.clone()));
            }
        }
    }
    out
}

/// 街道名文字的屏幕像素基准——比地名（12px）小一号，不抢位。
const STREET_LABEL_PX: f32 = 9.0;
/// 标注层：压在街道线（0.115）之上、地名（0.4）之下。
const STREET_LABEL_Z: f32 = 0.35;

/// 街道名标注池：把缓存里**有名字**的段的中点标上名字。
///
/// LOD 口径：街道名只在编辑级视野出现——省/国家级一屏几十条街的名字
/// 会把图变成字海（Google 地图也是拉近才出街名）。
///
/// **屏幕对齐**（2026-10-14 用户裁定，与地名标注同一口径）：地图随
/// `MapNode.orientation` 旋转后，街道名文字取相机旋转的**逆**保持水平。
///
/// 图层归属：街道名是 Road 组的标注（ADR-0013）——Road 图层隐藏（YAML 声明
/// 或会话开关）时街名一并退场，透明度与 z 偏移也随 Road 组。
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_street_labels(
    mut commands: Commands,
    cache: Res<StreetLayerCache>,
    grid: Res<GridConfig>,
    view: Res<ViewState>,
    layers: Res<LayerFrame>,
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
            With<crate::render::label_halo::LabelHalo>,
            Without<StreetLabel>,
        ),
    >,
) {
    // 期望标注集：有名字的段 × 折线中点（本图局部 → 视图空间，走 `map_to_view`）。
    // 图层不可见 → wanted 为空 → 池全隐藏（尾部的「多余的隐藏」路径）。
    let tier = crate::state::lod_tier(view.scale);
    let mut wanted: Vec<(Vec2, String)> = Vec::new();
    if layers.road.visible && tier == LodTier::Edit {
        for (mid, name) in named_midpoints(&cache.sources) {
            wanted.push((grid.map_to_view(mid), name));
        }
    }
    // 无变化帧也照常对齐：位置/字号随视图每帧都可能变，池对齐本身极轻。

    let font_size = STREET_LABEL_PX * view.scale.max(0.05);
    let label_z = STREET_LABEL_Z + layers.road.z_offset;
    let label_rot = camera
        .single()
        .map(|t| t.rotation.inverse())
        .unwrap_or(Quat::IDENTITY);
    let cjk = fonts.cjk_font.clone();
    let label_color = match view.mode {
        crate::state::ViewMode::GoogleMap => crate::theme::colors_google::GOOGLE_LABEL_FILL,
        // 古舆图 / 暮色：深褐墨（暮色为浅米），不再回退纯白（ADR-0014）。
        _ => crate::theme::palette::colors(view.map_palette).label_fill,
    }
    .with_alpha(layers.road.opacity);
    // 晕边色（ADR-0014 M2）：与地名同口径（Google 白晕兜底）。
    let halo_color = match view.mode {
        crate::state::ViewMode::GoogleMap => bevy::color::Color::WHITE,
        _ => crate::theme::palette::colors(view.map_palette).halo_fill,
    }
    .with_alpha(layers.road.opacity);
    let halo_offset = crate::render::label_halo::HALO_PX * view.scale.max(0.05);

    let mut iter = q.iter_mut();
    for (pos, name) in &wanted {
        let name: &str = name.as_str();
        if let Some((e, mut tf, mut text, mut style, mut color, mut vis)) = iter.next() {
            tf.translation = pos.extend(label_z);
            tf.rotation = label_rot;
            *text = Text2d::new(name);
            style.font_size = font_size.into();
            if let Some(f) = &cjk {
                style.font = f.clone().into();
            }
            *color = TextColor(label_color);
            *vis = Visibility::Visible;
            // 晕边子实体随字同步（8 向纸色副本）。
            if let Ok(children) = children_q.get(e) {
                crate::render::label_halo::sync_halo(
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
                    rotation: label_rot,
                    ..Default::default()
                },
                Visibility::Visible,
            ));
            let (text_owned, font_ref, fs, hc) =
                (name.to_string(), cjk.as_ref(), font_size, halo_color);
            cmd.with_children(move |c| {
                for _ in 0..crate::render::label_halo::HALO_DIRS.len() {
                    c.spawn(crate::render::label_halo::halo_bundle(
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
    ui: Res<crate::render::camera::PointerOverUi>,
    grid: Res<GridConfig>,
    mut projection: ResMut<WorldProjection>,
    drill: Res<DrillState>,
    mut editor: ResMut<StreetEditorState>,
    mut edit: ResMut<crate::edit::editing::EditHistory>,
) {
    if !editor.enabled {
        return;
    }
    // 悬在 UI 上 / 播放中只读 / 极低缩放禁画（与地形笔刷同口径）。
    // 无工程锁同口径（2026-10-14）：没打开任何工程时不允许落笔。
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
    let origin = crate::render::drill::map_origin(&projection.world, node, &grid);
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
    let cmd = wv_edit::AddStreetSegmentCommand::new(layer.id.clone(), seg);
    let mut tx = wv_edit::Transaction::new();
    tx.push(Box::new(cmd));
    match edit.history.commit(&mut projection.world, tx) {
        Ok(dirty) => {
            edit.record_dirty(dirty);
            projection.dirty = true;
            editor.last_click = Some(cursor);
            editor.drawn += 1;
        }
        Err(e) => bevy::log::warn!("街道编辑命令失败: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wv_core::{Point, RoadClass, StreetLayer, StreetSegment};
    use wv_edit::EditCommand as _;

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
                },
            ]),
        )];
        let mids = named_midpoints(&sources);
        assert_eq!(mids.len(), 1, "无名段不产生标注");
        assert_eq!(mids[0].1, "户部巷");
        assert!(
            (mids[0].0.x - 4.0).abs() < 1e-4 && (mids[0].0.y - 5.0).abs() < 1e-4,
            "中点应为 (4, 5)，实得 {:?}",
            mids[0].0
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
            unit_per_km: None,
            orientation: 0.0,
        };
        let mut city = wv_core::map::MapNode {
            orientation: 0.0,
            level: wv_core::map::MapLevel::City,
            parent: Some(wv_core::Id::new_unchecked("map_world")),
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
            unit_per_km: None,
            orientation: 0.0,
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

        // 手绘一笔：走 wv-edit 命令改 World（渲染层不许直改），再触发 change。
        let mut projection = app_world.resource_mut::<crate::state::WorldProjection>();
        let cmd = wv_edit::AddStreetSegmentCommand::new(
            wv_core::Id::new_unchecked("roads"),
            StreetSegment::straight(RoadClass::Residential, 10.0, 10.0, 40.0, 10.0),
        );
        cmd.apply(&mut projection.world).expect("命令应成功");
        // `Mut` 的借用活到语句末尾即释放（无需显式 drop，它也不实现 Drop）。

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

    /// 手绘交互闭环：两次左键点击经 `commit_street_click` 落成一直段
    /// （走 `EditHistory`，脏路径入 `dirty_set`），右键结束连线。
    #[test]
    fn street_clicks_commit_segments_through_the_history() {
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
                unit_per_km: None,
                orientation: 0.0,
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
        app_world.insert_resource(crate::render::camera::PointerOverUi::default());
        app_world.insert_resource(GridConfig::default());
        app_world.insert_resource(StreetEditorState {
            enabled: true,
            class: RoadClass::Primary,
            ..StreetEditorState::default()
        });
        app_world.insert_resource(crate::edit::editing::EditHistory::default());

        let mut schedule = Schedule::default();
        schedule.add_systems(commit_street_click);

        // 第一次点击：只起笔。
        app_world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        schedule.run(&mut app_world);
        assert_eq!(app_world.resource::<StreetEditorState>().drawn, 0);
        assert!(app_world
            .resource::<StreetEditorState>()
            .last_click
            .is_some());

        // 松开再按第二次（换落点）：`clear()` 只清 just_*，`pressed` 还挂着，
        // 必须先 `release` 才能让下一次 `press` 重新产生 just_pressed。
        let mut buttons = app_world.resource_mut::<ButtonInput<MouseButton>>();
        buttons.release(MouseButton::Left);
        buttons.clear();
        app_world.resource_mut::<ViewState>().cursor_world = Some(Vec2::new(150.0, 180.0));
        app_world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        schedule.run(&mut app_world);

        {
            let editor = app_world.resource::<StreetEditorState>();
            assert_eq!(
                editor.drawn, 1,
                "第二次点击应提交 1 段（若为 0 则系统提前退出）"
            );
            let projection = app_world.resource::<crate::state::WorldProjection>();
            let doc = projection
                .world
                .street_of(&wv_core::Id::new_unchecked("roads"))
                .expect("命令应建出街道文档");
            assert_eq!(doc.len(), 1, "两次点击应落 1 段，实际 {}", doc.len());
            let s = &doc.segments[0];
            assert_eq!(s.class, RoadClass::Primary);
            assert_eq!(s.points.len(), 2);
            assert_eq!(s.points[0], wv_core::geometry::Point::new(100.0, 200.0));
            assert_eq!(s.points[1], wv_core::geometry::Point::new(150.0, 180.0));
        }
        let edit = app_world.resource::<crate::edit::editing::EditHistory>();
        assert!(
            edit.dirty_set.contains("streets/roads"),
            "脏路径应进 dirty_set，实际 {:?}",
            edit.dirty_set
        );

        // 右键结束连线：下一落点不再接段。
        app_world.resource_mut::<ButtonInput<MouseButton>>().clear();
        app_world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        schedule.run(&mut app_world);
        assert!(app_world
            .resource::<StreetEditorState>()
            .last_click
            .is_none());
    }
}
