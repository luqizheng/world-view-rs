//! 工程诊断：把「错误」与「提示」在**类型层面**分开（ADR-0005，2026-10-11）。
//!
//! ## 为什么需要这个模块
//!
//! [`crate::project::Project::validate`] 历史上返回 `Vec<String>`，所有条目混在一起。
//! 但本项目 2026-10-11 的裁定（`AGENTS.md` §7.5 V1）要求**错误与提示必须可区分**：
//!
//! - **错误**：数据坏了（悬空引用、字段缺失）——工程打不开。
//! - **提示**：数据没坏，但可能有你没注意到的东西（相遇却未写事件）——**只提示，不阻止**。
//!
//! 把两者塞进同一个 `String` 会让 UI 无法可靠地「只显示提示、不拦保存」，
//! 也无法给提示做**回访定位**（点了要能跳到地图对应位置）。
//!
//! ## 与 `validate()` 的分工
//!
//! | 入口 | 强度 | 用途 |
//! |---|---|---|
//! | `Project::validate()` | 错误即 `Err` | CI / 命令行，加载前把关 |
//! | `Project::diagnose()` | **永不失败**，返回结构化诊断 | 编辑器校验面板：列出 + 可回访 + 不阻止保存 |
//!
//! 二者共享同一套引用校验逻辑，避免「一个说有问题、另一个说没问题」。

//! ## 2026-10-12 调整：相遇判定不再需要 maps
//!
//! 旧实现里 `EncounterInput` 带 `maps` 字段（用于取 `local_per_km` 做视界判定）；
//! 2026-10-12 用户简化方案改为「两 Track 的 Waypoint 在 (place, day) 上同时覆盖」，
//! 因此 `EncounterInput` 不再需要 maps 参数——见 [`wv_core::encounter`] 模块文档。

//! ## 2026-10-16 增列：跨级尺度一致性提示（ADR-0022 D4）
//!
//! 世界图 → 城市图跨级声明 `unit_per_km` 时，两级间的物理尺度可能不一致；
//! 给作者一条 Hint 让 ta 注意到，不阻止保存（V1 守约）。

use wv_core::encounter::{detect_encounter_hints, EncounterInput, HintSeverity};

/// 诊断级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagLevel {
    /// 提示：可能有你未注意到的事。**不阻止任何操作**。
    Hint,
    /// 错误：数据坏了。
    Error,
}

/// 一条结构化诊断。
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub level: DiagLevel,
    /// 人类可读文案。
    pub message: String,
    /// 稳定标识（供 UI 做「已读 / 已忽略」状态锚定）。
    ///
    /// **必须有**：没有稳定标识的提示，作者一旦微调数据就会「换一条新的」，
    /// 于是永远认领不掉（这正是相遇提示要求指纹不含日期的原因）。
    pub fingerprint: String,
    /// 定位信息：点击提示时跳到这里。
    pub anchor: Option<DiagAnchor>,
    /// 细分严重度（来自派生检查，如相遇提示的 `EncounterUnwritten`）。
    /// `None` 表示无细分。
    pub severity: Option<HintSeverity>,
}

/// 诊断的定位锚点。
#[derive(Debug, Clone, PartialEq)]
pub struct DiagAnchor {
    pub map: wv_core::Id,
    /// 地图局部坐标。
    pub at: wv_core::geometry::Point,
    /// 时间点（跳转时把时间轴拨到这里）。
    pub day: i64,
}

/// 诊断集合。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    /// 只取提示（编辑器的「可回访列表」用这个）。
    pub fn hints(&self) -> Vec<&Diagnostic> {
        self.items
            .iter()
            .filter(|d| d.level == DiagLevel::Hint)
            .collect()
    }

    /// 只取错误。
    pub fn errors(&self) -> Vec<&Diagnostic> {
        self.items
            .iter()
            .filter(|d| d.level == DiagLevel::Error)
            .collect()
    }

    /// 是否存在错误（编辑器据此决定能否继续保存）。
    pub fn has_errors(&self) -> bool {
        self.items.iter().any(|d| d.level == DiagLevel::Error)
    }

    /// 按稳定标识去重（同一处重复出现只保留一条）。
    pub fn dedup_by_fingerprint(&mut self) {
        self.items.sort_by(|a, b| a.fingerprint.cmp(&b.fingerprint));
        self.items.dedup_by(|a, b| a.fingerprint == b.fingerprint);
    }

    /// 合并另一组诊断（用于「悬空引用」+「相遇提示」汇成一份面板）。
    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }
}

/// 从已加载的世界派生**提示级**诊断（相遇检测）。
///
/// 这是纯提示路径：**永不产生 `DiagLevel::Error`**，也永不失败。
///
/// ## 判定口径（2026-10-12）：Waypoint (place, day) 严格重叠
///
/// 判定完全由数据决定：两 Track 的 Waypoint 在 `(place, day)` 上同时覆盖。
/// **不含任何距离阈值常量**——视野方案（ADR-0006 旧版）已撤。
pub fn derive_hints(world: &wv_core::World) -> Diagnostics {
    let hints = detect_encounter_hints(EncounterInput {
        units: &world.units,
        tracks: &world.tracks,
        events: &world.events,
    });

    let mut d = Diagnostics::default();
    for h in hints {
        // 定位锚点：用该 hint 自带的 map 与 day。
        // at 在 Waypoint (place, day) 严格判定里没有几何意义——
        // 用 (0, 0) 作为占位（无 pos 字段时不再有意义）。
        let anchor = Some(DiagAnchor {
            map: h.map.clone(),
            at: wv_core::geometry::Point { x: 0.0, y: 0.0 },
            day: h.span.from,
        });

        let unit_names: Vec<&str> = h
            .units
            .iter()
            .map(|id| {
                world
                    .units
                    .iter()
                    .find(|u| &u.id == id)
                    .map(|u| u.name.as_str())
                    .unwrap_or_else(|| id.as_str())
            })
            .collect();
        let place = h.place.as_str().to_string();
        let days = match h.span.to {
            Some(to) if to - h.span.from == 1 => format!("第 {} 天", h.span.from),
            Some(to) => format!("第 {}–{} 天", h.span.from, to - 1),
            None => "至今".to_string(),
        };

        d.items.push(Diagnostic {
            level: DiagLevel::Hint,
            message: h.severity.message(
                unit_names.first().copied().unwrap_or("?"),
                unit_names.get(1).copied().unwrap_or("?"),
                &place,
                &days,
            ),
            fingerprint: h.fingerprint,
            anchor,
            severity: Some(h.severity),
        });
    }

    // 阶段六 v0.2：事件间位移可行性提示（只提示不阻止，AGENTS §7.5 V1）。
    // 与相遇提示的分工：本检查针对**事件**而非**轨迹**——「下一事件发生时，
    // 该参与者是否还有时间从上一事件地点赶过去」；相遇提示管的是「两轨迹同
    // (place, day) 覆盖但没写事件」，两者不重叠。
    d.items.extend(detect_travel_feasibility_hints(world));

    // ADR-0022 M3 D4：跨级尺度一致性（V1 提示，不阻止）。
    d.items.extend(detect_cross_level_scale_hints(world));

    // 主题覆盖物：地界内没有对应群系格 / 孤儿地界（V1 提示，不阻止）。
    d.items.extend(detect_feature_territory_hints(world));

    // 词汇迁移（ADR-0032 D3）：Place 仍携带旧 feature 字段时的迁移/未映射提示。
    d.items.extend(detect_feature_migration_hints(world));

    d.dedup_by_fingerprint();
    d
}

/// 覆盖物地界的**事实一致性**提示（V1：只提示，不阻止保存）。
///
/// ## 口径
///
/// 1. 对每个声明了标记（`biome`，或旧 `feature` 经
///    [`crate::terrain_bin::place_feature_to_biome`] 换算）且 `map: Some(m)`
///    的地点，收集 `place_geometry` 中 `owner == place.id`、`map == m` 的
///    多边形 span；
/// 2. **该图没有自带栅格**（`world.terrain_of(m)` 为 `None`）→ 跳过：
///    继承底图的栅格不属于本图，无法判断，也不该提示（晕/描边仍可见）；
/// 3. `MapNode.extent` 缺失或非正 → 跳过（无法把地图局部坐标映射到格）；
/// 4. 环的包围盒映射到候选格范围（`x / extent.width * data.width`、
///    `y / extent.height * data.height`），逐格取**格心**的局部坐标做
///    [`wv_core::geometry::point_in_ring`]，统计群系匹配的格；
/// 5. 一个命中都没有 → [`hint`]（指纹 `feature-no-cells|{place_id}`）；
/// 6. 任何 `place_geometry` span 的 `owner` 不在 `distances.places` 里 →
///    [`hint`]（指纹 `feature-orphan-territory|{owner}`）——**不是** `Error`：
///    地界求值会静默跳过它（`place_territory_at`），校验面板负责让作者看见。
///
/// 指纹字符串保持 `feature-*` 前缀**不改**（ADR-0032 D7）：UI 的已读/忽略
/// 状态锚定在指纹上，换指纹等于重置作者的认领记录。
///
/// 同一指纹只出现一次（函数内按指纹去重；`derive_hints` 末尾还会再兜一次）。
///
/// ## 为什么是 Hint
///
/// 作者可能故意圈了一片还没刷笔刷的森林（先摆事实、后画地形，或干脆只用
/// 远带晕）。工具的职责是让他**看见**近景符号会回退到陆地格泛化布点
/// （而不是他预期的森林），不替他决定（AGENTS §7.5 V1）。
pub fn detect_feature_territory_hints(world: &wv_core::World) -> Vec<Diagnostic> {
    use std::collections::BTreeSet;

    let mut out: Vec<Diagnostic> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();

    // 孤儿地界：owner 不在 places 里。不依赖标记字段 / 栅格，逐 span 检查。
    for g in &world.place_geometry {
        if world.distances.place(&g.owner).is_some() {
            continue;
        }
        let fp = format!("feature-orphan-territory|{}", g.owner);
        if seen.insert(fp.clone()) {
            out.push(hint(
                fp,
                format!(
                    "地界引用的地点「{owner}」不存在（geometry/place 的 owner 未登记）：\
                     该地界不会被渲染。仅提示，不阻止保存。",
                    owner = g.owner
                ),
            ));
        }
    }

    for place in &world.distances.places {
        // 标记（ADR-0029/0032）：以 `biome` 为准；旧 `feature`（旧 kind 词汇）
        // 经 place 迁移表换算（D4——{0,1,5,6,7} 升级为真实群系，纠正旧
        // `migrate_v1_kind` 归 0 的假阴性）。两者皆无（或垃圾值）= 普通地点。
        let Some(biome_id) = place.biome.or_else(|| {
            place
                .feature
                .and_then(crate::terrain_bin::place_feature_to_biome)
        }) else {
            continue;
        };
        let Some(map_id) = place.map.as_ref() else {
            continue;
        };
        // 继承底图不在提示范围：只有本图自带栅格时才谈得上「刷没刷笔刷」。
        let Some(terrain) = world.terrain_of(map_id) else {
            continue;
        };
        let Some(map) = world.maps.iter().find(|m| &m.id == map_id) else {
            continue;
        };
        let Some(extent) = map.extent else {
            continue;
        };
        if extent.width <= 0.0 || extent.height <= 0.0 {
            continue;
        }
        if terrain.width == 0 || terrain.height == 0 {
            continue;
        }

        let mut checked_any_ring = false;
        let mut hit = false;
        for g in &world.place_geometry {
            if g.owner != place.id || &g.map != map_id {
                continue;
            }
            // 带洞地界（ADR-0029 M2）按外环判定——与锚点拟合 / 渲染晕「只看
            // 外环」同一取舍，洞内（湖面/草地芯）本就不该有目标群系格。
            let ring: &Vec<f32> = match &g.geometry {
                wv_core::map::GeometryData::Polygon(ring) => ring,
                wv_core::map::GeometryData::PolygonWithHoles { outer, .. } => outer,
                // Points / Ref：v0.1 不构成面，求值侧同样跳过。
                wv_core::map::GeometryData::Points(_) | wv_core::map::GeometryData::Ref(_) => {
                    continue
                }
            };
            if ring.len() < 6 {
                continue;
            }
            checked_any_ring = true;
            // 群系编号直接比较——biome 已是唯一词汇（ADR-0032），feature 兜底
            // 在上面已经迁移表换算过。
            if ring_has_feature_cell(terrain, &extent, ring, biome_id) {
                hit = true;
                break;
            }
        }
        if checked_any_ring && !hit {
            let what = format!("biome={biome_id}");
            out.push(hint(
                format!("feature-no-cells|{}", place.id),
                format!(
                    "地点「{name}」的地界内没有任何 {what} 地形格：近景覆盖物会回退到\
                     按陆地格（height > 0）泛化布点，而不是只落在声明的类别上；\
                     远看仍会显示地界与名字。仅提示，不阻止保存。",
                    name = place.name,
                ),
            ));
        }
    }

    out
}

/// 词汇迁移提示（ADR-0032 D3/D7 · V1 只提示，不阻止保存）。
///
/// Place 仍携带旧 `feature` 字段只在两种情形发生：
/// 1. **旧工程加载后、地点数据首次回写前**——加载迁移已把群系写入 `biome`，
///    feature 原值作为提示证据驻留内存；作者保存地点（`distances/places`
///    脏区被写入）后旧字段从 YAML 消失、重载后本提示随之消散（指纹可认领）。
///    注意只保存地形等其他子树**不会**重写 places（`save_world` 的脏区路由），
///    提示会保留到下一次地点回写或重新打开工程；
/// 2. 值在旧词汇表（0–9）外——无法迁移；无 biome 时按普通地点处理（迁移
///    窗口内 POI 抑制仍读 `feature`，点标暂不出现，见 ADR-0032 代价）。
///
/// 指纹 = `feature-migrated|{place_id}` / `feature-unmapped|{place_id}`，
/// 不含内容值——同一地点反复出现是同一条，认领状态跨会话稳定。
pub fn detect_feature_migration_hints(world: &wv_core::World) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for place in &world.distances.places {
        let Some(f) = place.feature else {
            continue;
        };
        match crate::terrain_bin::place_feature_to_biome(f) {
            // 旧词汇有效：迁移提示；并存时以作者手写的 biome 为胜。
            Some(mapped) => {
                let target = place.biome.unwrap_or(mapped);
                out.push(hint(
                    format!("feature-migrated|{}", place.id),
                    format!(
                        "地点「{name}」使用旧词汇 feature={f}（ADR-0032 已收口）：以 biome={target} 为准；\
                         下次保存地点（或重新打开工程）后 feature 键从 YAML 移除，本提示随之消失。\
                         仅提示，不阻止保存。",
                        name = place.name,
                    ),
                ));
            }
            // 词汇表外：无等价群系。若作者已写 biome，旧键只是待清理的外来值。
            None => {
                let tail = match place.biome {
                    Some(b) => format!("已按 biome={b} 展示；保存后旧键移除"),
                    None => "已按普通地点处理（迁移窗口内仍不显示点标）；如需标记请改用 biome 字段"
                        .to_string(),
                };
                out.push(hint(
                    format!("feature-unmapped|{}", place.id),
                    format!(
                        "地点「{name}」的 feature={f} 不在旧词汇表（0–9）内，{tail}。\
                         仅提示，不阻止保存。",
                        name = place.name,
                    ),
                ));
            }
        }
    }
    out
}

/// 环的包围盒内是否存在**格心落在环内且群系匹配**的地形格。
///
/// 只扫包围盒覆盖的格（含 1 格余量），避免大地图每次诊断全图遍历；
/// 精确判定仍交给 [`wv_core::geometry::point_in_ring`]。
fn ring_has_feature_cell(
    terrain: &wv_core::terrain::TerrainData,
    extent: &wv_core::map::Extent,
    ring: &[f32],
    biome: u8,
) -> bool {
    let (tw, th) = (terrain.width as usize, terrain.height as usize);
    if tw == 0 || th == 0 || extent.width <= 0.0 || extent.height <= 0.0 {
        return false;
    }

    // 包围盒（非有限坐标视为退化环，与渲染侧 `ring_bounds` 同口径）。
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for p in ring.chunks_exact(2) {
        if !p[0].is_finite() || !p[1].is_finite() {
            return false;
        }
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
    }
    if !(min_x <= max_x && min_y <= max_y) {
        return false;
    }

    // 地图局部包围盒 → 候选格范围（格心口径：格 g 覆盖 [g, g+1) 个格宽）。
    // 两侧各放宽 1 格：包围盒边界上的格心由 `point_in_ring` 最终裁决。
    let scale_x = terrain.width as f32 / extent.width;
    let scale_y = terrain.height as f32 / extent.height;
    // 坐标 × 缩放可能溢出 f32（极端有限值 → ±inf），`as i64` 饱和到
    // `i64::MIN` / `i64::MAX`；再用 `saturating_*` 放宽 1 格，确保畸形 YAML
    // 永远不会在 debug 构建触发溢出 panic。
    let gx0 = ((min_x * scale_x).floor() as i64)
        .saturating_sub(1)
        .clamp(0, tw as i64 - 1) as usize;
    let gx1 = ((max_x * scale_x).ceil() as i64)
        .saturating_add(1)
        .clamp(0, tw as i64 - 1) as usize;
    let gy0 = ((min_y * scale_y).floor() as i64)
        .saturating_sub(1)
        .clamp(0, th as i64 - 1) as usize;
    let gy1 = ((max_y * scale_y).ceil() as i64)
        .saturating_add(1)
        .clamp(0, th as i64 - 1) as usize;

    for gy in gy0..=gy1 {
        let cy = (gy as f32 + 0.5) / terrain.height as f32 * extent.height;
        for gx in gx0..=gx1 {
            let Some(cell) = terrain.cells.get(gy * tw + gx) else {
                continue;
            };
            if cell.biome != biome {
                continue;
            }
            let cx = (gx as f32 + 0.5) / terrain.width as f32 * extent.width;
            if wv_core::geometry::point_in_ring(ring, cx, cy) {
                return true;
            }
        }
    }
    false
}

/// 跨级尺度一致性提示（ADR-0022 D4 · 2026-10-16 接受）。
///
/// ## 口径
///
/// 世界图 → 城市子图跨级声明 `unit_per_km` 时，预期物理宽度（伪代码）：
///
/// ```text
/// expected_world_extent_w = (city_local_extent_w / city.unit_per_km) / world.unit_per_km
/// declared_world_extent_w  = city.extent.width
/// ratio = declared / expected
/// ```
///
/// | ratio 区间 | 提示 |
/// |---|---|
/// | \|ratio − 1.0\| ≤ 0.1 | 一致，不发提示 |
/// | 0.1 < \|ratio − 1.0\| ≤ 1.0 | 「两级间尺度有 X% 偏差」 |
/// | ratio < 0.5 或 > 2.0 | 「两级尺度明显不匹配」 |
///
/// 缺省策略：
/// - `world.unit_per_km = None` → 世界未声明尺度，**跳过**所有跨级检查
///   （无法换算「城市在世界上应该多大」）。
/// - `city.unit_per_km = None` → 城市未声明，跳过该城。
/// - `city.extent = None` → 城市未声明幅面，跳过。
///
/// ## 与 P6（非破坏）/ V1（只提示）的关系
///
/// 这是**纯提示**，指纹 = `cross-level-scale:{city_id}` 稳定可认领；
/// 不阻止保存——作者可能故意（艺术夸张、戏剧比例尺）。
pub fn detect_cross_level_scale_hints(world: &wv_core::World) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for city in &world.maps {
        let Some(city_extent) = city.extent else {
            continue;
        };
        let Some(city_per_km) = city.unit_per_km else {
            continue;
        };
        if city_per_km <= 0.0 {
            continue;
        }
        let Some(world_per_km) = world
            .maps
            .iter()
            .find(|m| m.parent.is_none())
            .and_then(|m| m.unit_per_km)
        else {
            continue;
        };
        if world_per_km <= 0.0 {
            continue;
        };

        let city_world_w = city_extent.width / city_per_km / world_per_km;
        if city_world_w <= 0.0 {
            continue;
        }
        let declared_world_w = city_extent.width;
        let ratio = declared_world_w / city_world_w;

        let deviation = (ratio - 1.0).abs();
        let severity_msg = if !(0.5..=2.0).contains(&ratio) {
            format!(
                "城市「{}」与父图两级尺度明显不匹配（ratio={:.2}）：城市本地 {:.0} wu × {} m/wu ≈ {:.1} km，\
                 在世界上应占 {:.1} wu，当前声明 {:.0} wu。",
                city.id,
                ratio,
                city_extent.width,
                city_per_km,
                city_extent.width / city_per_km,
                city_world_w,
                declared_world_w,
            )
        } else if deviation > 0.1 {
            format!(
                "城市「{}」与父图两级间尺度有 {:.0}% 偏差（ratio={:.2}）：城市本地 {:.0} wu 约 {:.1} km，\
                 在世界上应占 {:.1} wu，当前声明 {:.0} wu。",
                city.id,
                deviation * 100.0,
                ratio,
                city_extent.width,
                city_extent.width / city_per_km,
                city_world_w,
                declared_world_w,
            )
        } else {
            continue;
        };

        out.push(Diagnostic {
            level: DiagLevel::Hint,
            message: severity_msg,
            fingerprint: format!("cross-level-scale:{}", city.id),
            anchor: Some(DiagAnchor {
                map: city.id.clone(),
                at: wv_core::geometry::Point { x: 0.0, y: 0.0 },
                day: 0,
            }),
            severity: None,
        });
    }
    out
}

/// 构造一条提示级诊断（供其他派生检查复用，如「孤立事件」等后续检查）。
pub fn hint(fingerprint: impl Into<String>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        level: DiagLevel::Hint,
        message: message.into(),
        fingerprint: fingerprint.into(),
        anchor: None,
        severity: None,
    }
}

/// 阶段六 v0.2：事件间位移可行性提示。
///
/// ## 规则
///
/// 1. 把 `world.events` 按 `span.from` 升序；
/// 2. 对每对**相邻**事件 `prev` / `next`，若 `participants` 交集非空且
///    两事件都有 `location.place` 且二者不同；
/// 3. 计算 `gap = next.span.from - prev.span.to`（半开：`prev.to` 那天
///    `next` 才能开始，所以 `prev.span.to == None`（开放区间）即跳过；
///    `gap <= 0` 也跳过——事件在时间上不正向或同日同时）；
/// 4. 跑 `DistanceGraph::shortest_journey(prev.place, next.place)` 拿
///    `total_days`（最少可行天数）；
/// 5. 若 `gap < total_days` → 提示「预算时间不足：<X>→<Y> 需 ≥N 天，仅给 M 天」。
///
/// ## 为什么是 hint 不是 error
///
/// 参照 `AGENTS.md` §7.5 V1：作者可能有意压缩（小说里常见的"星夜兼程"）。
/// 工具的职责是让作者**看见**这个差距，不替他决定。
///
/// ## 与相遇提示的分工
///
/// 相遇提示（[`wv_core::encounter::detect_encounter_hints`]）按**轨迹**的
/// `(place, day)` 覆盖判定「该处应该有事件」；本检查按**事件序列**判定
/// 「前后事件之间路够不够走」。两者不重叠：前者是**缺事件**，后者是**事件间距过紧**。
///
/// ## 单元测试
///
/// 见本模块 `tests::travel_feasibility_*`——直接喂 `wv_core::Event` + 极简
/// `DistanceGraph`，不依赖示例工程。
fn detect_travel_feasibility_hints(world: &wv_core::World) -> Vec<Diagnostic> {
    use std::collections::BTreeSet;

    if world.events.len() < 2 {
        return Vec::new();
    }

    // 按 span.from 升序。稳定排序：from 相同时保留 YAML 顺序（不做稳定性
    // 假设对正确性无影响——同 from 的两事件地点若不同，是另一类冲突，非本检查目标）。
    let mut order: Vec<usize> = (0..world.events.len()).collect();
    order.sort_by_key(|&i| world.events[i].span.from);

    let mut out: Vec<Diagnostic> = Vec::new();
    for w in order.windows(2) {
        let (pi, ni) = (w[0], w[1]);
        let (Some(prev), Some(next)) = (world.events.get(pi), world.events.get(ni)) else {
            continue;
        };
        // 参与者交集。
        let prev_p: BTreeSet<&wv_core::Id> = prev.participants.iter().collect();
        let next_p: BTreeSet<&wv_core::Id> = next.participants.iter().collect();
        let common: Vec<&wv_core::Id> = prev_p.intersection(&next_p).copied().collect();
        if common.is_empty() {
            continue;
        }
        // 双方都得有 location.place。
        let (Some(prev_loc), Some(next_loc)) = (prev.location.as_ref(), next.location.as_ref())
        else {
            continue;
        };
        // 同地不动不算。
        if prev_loc.place == next_loc.place {
            continue;
        }
        // 半开：[prev.from, prev.to)。prev.to == None 表示至今仍在持续——
        // 既然 next 还在 prev 期间发生就矛盾，先不报本检查。
        let Some(prev_to) = prev.span.to else {
            continue;
        };
        let gap = next.span.from - prev_to;
        if gap <= 0 {
            continue;
        }
        // 跑最短路径。不通（NoPath / NoUsableVehicle）→ 不报本检查：
        // 这是另一类问题（地图不连通），留给后续检查。
        let Ok(journey) = world
            .distances
            .shortest_journey(&prev_loc.place, &next_loc.place, &[])
        else {
            continue;
        };
        if journey.total_days.is_finite() && (gap as f32) < journey.total_days {
            // 指纹稳定：被检查的「两事件 id + 共同参与者集合」。
            // 参与者排序以 BTreeSet 的天然序为准，确定性。
            let participants: String = common
                .iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let prev_id = prev.id.as_str();
            let next_id = next.id.as_str();
            let fp = format!("travel_feasibility|{prev_id}|{next_id}|{participants}");
            // 文案：报告实际差距（不是给死数字）。
            let deficit = journey.total_days - gap as f32;
            let place_a = prev_loc.place.as_str();
            let place_b = next_loc.place.as_str();
            let prev_from = prev.span.from;
            let next_from = next.span.from;
            let total = journey.total_days;
            let message = format!(
                "参与者预算时间不足：第 {prev_from}–{prev_to_end} 天的「{place_a}」到第 {next_from} 天的「{place_b}」 \
                 至少需 {total:.1} 天，预算仅 {gap} 天（差 {deficit:.1} 天）",
                prev_to_end = prev_to - 1,
            );
            out.push(hint(fp, message));
        }
    }
    out
}

/// 构造一条错误级诊断。
pub fn error(fingerprint: impl Into<String>, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        level: DiagLevel::Error,
        message: message.into(),
        fingerprint: fingerprint.into(),
        anchor: None,
        severity: None,
    }
}

// ====== ADR-0027 · 势力层 / 归属边诊断（V1 只提示，不阻止保存） =============

/// 势力层 / 归属边 / 声称实控的结构化诊断。
///
/// 全部进入 `diagnose()`（编辑器校验面板），**不进入** `validate()` 的保存闸门：
/// - 悬空 `layer` / `membership.target` / `seat` → Error（数据坏了，但工程仍能打开）
/// - 重复 `FactionLayer.id` → Error
/// - `Membership.span.to < from` → Error
/// - 归属边成环 → Hint（层级奇怪但仍是作者可表达的世界）
pub fn derive_faction_diagnostics(world: &wv_core::World) -> Diagnostics {
    use std::collections::HashSet;

    let mut d = Diagnostics::default();
    let layer_ids: HashSet<&str> = world.faction_layers.iter().map(|l| l.id.as_str()).collect();
    let faction_ids: HashSet<&str> = world.factions.iter().map(|f| f.id.as_str()).collect();
    let place_ids: HashSet<&str> = world
        .distances
        .places
        .iter()
        .map(|p| p.id.as_str())
        .collect();

    // 重复层 id（不同文件声明同一个 id）。
    let mut seen = HashSet::new();
    for l in &world.faction_layers {
        if !seen.insert(l.id.as_str()) {
            d.items.push(error(
                format!("faction_layer:dup:{}", l.id),
                format!("势力层 id 重复：{}（多个文件声明了同一个 id）", l.id),
            ));
        }
    }

    for f in &world.factions {
        if let Some(layer) = &f.layer {
            if !layer_ids.contains(layer.as_str()) {
                d.items.push(error(
                    format!("faction:{}:layer:{}", f.id, layer),
                    format!(
                        "势力「{}」引用了不存在的势力层「{}」，将回落默认层",
                        f.name, layer
                    ),
                ));
            }
        }
        if let Some(seat) = &f.seat {
            if !place_ids.contains(seat.as_str()) {
                d.items.push(error(
                    format!("faction:{}:seat:{}", f.id, seat),
                    format!("势力「{}」的驻地引用了不存在的地点「{}」", f.name, seat),
                ));
            }
        }
        for m in &f.memberships {
            if !faction_ids.contains(m.target.as_str()) {
                d.items.push(error(
                    format!("faction:{}:membership:{}", f.id, m.target),
                    format!("势力「{}」的归属边指向不存在的势力「{}」", f.name, m.target),
                ));
            }
            if let Some(to) = m.span.to {
                if to < m.span.from {
                    d.items.push(error(
                        format!(
                            "faction:{}:membership:{}:{}:inverted",
                            f.id, m.target, m.span.from
                        ),
                        format!(
                            "势力「{}」→「{}」的归属区间倒置（from {} > to {}）",
                            f.name, m.target, m.span.from, to
                        ),
                    ));
                }
            }
        }
    }

    for cycle in wv_core::membership_cycles(world) {
        let path: Vec<&str> = cycle.iter().map(|id| id.as_str()).collect();
        d.items.push(hint(
            format!("faction:cycle:{}", path.join(">")),
            format!(
                "势力归属成环：{}（层级可表达，但检查是否误连）",
                path.join(" → ")
            ),
        ));
    }

    d
}

// ====== ADR-0022 M3 D4 · 跨级一致性 V1 提示 =================================

#[cfg(test)]
mod cross_level_scale_tests {
    use super::*;
    use wv_core::map::{Extent, MapLevel, MapNode};
    use wv_core::Id;

    fn world_with(per_km: Option<f32>) -> wv_core::World {
        let mut w = wv_core::World::default();
        w.maps.push(MapNode {
            id: Id::new_unchecked("map_world"),
            name: "世界".into(),
            level: MapLevel::World,
            unit_per_km: per_km,
            ..MapNode::default()
        });
        w
    }

    fn city_with(parent: &str, width: f32, per_km: Option<f32>) -> MapNode {
        MapNode {
            id: Id::new_unchecked("map_city"),
            name: "城".into(),
            level: MapLevel::City,
            parent: Some(Id::new_unchecked(parent)),
            extent: Some(Extent::sized(width, width)),
            unit_per_km: per_km,
            ..MapNode::default()
        }
    }

    #[test]
    fn ratio_one_emits_no_hint() {
        // world 4 km/wu × city 1024 wu × 50 m/wu（=51.2 km） → 应占 12.8 wu
        // 但城市声明 1024 wu（未校正），ratio = 80 → 远大于 2，应发「明显不匹配」。
        // 这里先验证「正确换算」场景：城市声明 12.8 wu、unit_per_km = 800 m/wu（512 wu/km），
        // 物理宽度 12.8 × 800 = 10240 m = 10.24 km；world 1 km/wu → 应占 10.24 wu。
        // 调整到 ratio=1.0：city 1024 wu，unit 0.001 km/wu = 1 m/wu，world 1 km/wu，
        // 预期 1024 / 0.001 / 1 = 1_024_000 wu。声明 1024 wu → ratio ≈ 1/1024 → 应发「明显」。
        // 所以直接测 ratio=1：city 1000 wu、unit 1.0 km/wu、world 1.0 km/wu
        // 预期 1000/1/1 = 1000 wu。声明 1000 wu → ratio=1 → 无提示。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: Some(1.0),
            ..city_with("map_world", 1000.0, Some(1.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert!(
            hints.is_empty(),
            "ratio=1.0 不应发提示，实得 {} 条",
            hints.len()
        );
    }

    #[test]
    fn ratio_within_ten_percent_emits_no_hint() {
        // city 1000 wu、unit 1.0；预期 1000 wu。声明 1050 wu → ratio = 1.05（差 5%）→ 无提示。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: Some(1.0),
            ..city_with("map_world", 1050.0, Some(1.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert!(hints.is_empty(), "5% 偏差 ≤ 10% 阈值，应无提示");
    }

    #[test]
    fn ratio_50_percent_emits_mild_hint() {
        // ratio = city.unit_per_km × world.unit_per_km = 0.5 × 1.0 = 0.5
        // → 「两级间尺度有 50% 偏差」。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: Some(0.5),
            ..city_with("map_world", 1000.0, Some(0.5))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert_eq!(hints.len(), 1, "应发 1 条提示");
        assert_eq!(hints[0].level, DiagLevel::Hint);
        assert!(hints[0].message.contains("50%"), "应含百分比");
    }

    #[test]
    fn ratio_five_x_emits_severe_hint() {
        // ratio = city.unit_per_km × world.unit_per_km = 5.0 × 1.0 = 5.0
        // → 「两级尺度明显不匹配」。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: Some(5.0),
            ..city_with("map_world", 1000.0, Some(5.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert_eq!(hints.len(), 1, "应发 1 条明显不匹配提示");
        assert!(
            hints[0].message.contains("明显不匹配"),
            "应标记为严重，实得：{}",
            hints[0].message
        );
    }

    #[test]
    fn missing_world_per_km_skips_all_checks() {
        // world.unit_per_km = None → 跳过所有城市。
        let mut w = world_with(None);
        w.maps.push(MapNode {
            unit_per_km: Some(1.0),
            ..city_with("map_world", 5000.0, Some(1.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert!(hints.is_empty(), "world 未声明尺度应跳过");
    }

    #[test]
    fn missing_city_per_km_skips_city() {
        // 城市未声明 unit_per_km → 跳过该城（自身元数据不足）。
        let mut w = world_with(Some(1.0));
        w.maps.push(MapNode {
            unit_per_km: None,
            ..city_with("map_world", 5000.0, Some(1.0))
        });
        let hints = detect_cross_level_scale_hints(&w);
        assert!(hints.is_empty(), "城市未声明尺度应跳过");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_and_errors_are_distinguished_by_level() {
        let mut d = Diagnostics::default();
        d.items.push(hint("f1", "相遇未写事件"));
        d.items.push(error("f2", "悬空引用"));
        assert_eq!(d.hints().len(), 1);
        assert_eq!(d.errors().len(), 1);
        assert!(d.has_errors());
    }

    #[test]
    fn hints_alone_do_not_count_as_errors() {
        // 核心裁定：只有提示时，作者**可以正常保存**（AGENTS §7.5 V1）。
        let mut d = Diagnostics::default();
        d.items.push(hint("f1", "相遇未写事件"));
        d.items.push(hint("f2", "相遇未写事件 2"));
        assert!(!d.has_errors(), "只有提示时不得阻断保存");
    }

    #[test]
    fn dedup_keeps_one_entry_per_fingerprint() {
        let mut d = Diagnostics::default();
        d.items.push(hint("dup", "a"));
        d.items.push(hint("dup", "a"));
        d.items.push(hint("other", "b"));
        d.dedup_by_fingerprint();
        assert_eq!(d.items.len(), 2);
    }

    #[test]
    fn dedup_is_order_independent() {
        let mut a = Diagnostics::default();
        a.items.push(hint("x", "1"));
        a.items.push(hint("y", "2"));
        let mut b = Diagnostics::default();
        b.items.push(hint("y", "2"));
        b.items.push(hint("x", "1"));
        a.dedup_by_fingerprint();
        b.dedup_by_fingerprint();
        assert_eq!(a, b, "去重后必须与输入顺序无关（确定性）");
    }

    #[test]
    fn derive_hints_on_empty_world_is_empty_and_ok() {
        let w = wv_core::World::default();
        let d = derive_hints(&w);
        assert!(d.items.is_empty());
        assert!(!d.has_errors(), "提示路径永不产生错误");
    }

    // -----------------------------------------------------------------------
    // 阶段六 v0.2：事件间位移可行性提示单元测试
    // -----------------------------------------------------------------------

    use wv_core::distance::{DistanceGraph, Place, PlaceEdge, ScheduledJourney, Vehicle};
    use wv_core::event::{Event, EventLocation};
    use wv_core::time::TimeSpan;
    use wv_core::Id;

    /// 极简图：A → B，单条边 100 km plain，单车 speed 10 km/h 跑 8h → 12.5 km/天。
    /// A → B 至少需 100/12.5 = 8 天。
    fn fixture_graph() -> DistanceGraph {
        let a = Id::new_unchecked("p_a");
        let b = Id::new_unchecked("p_b");
        DistanceGraph {
            places: vec![
                Place {
                    id: a.clone(),
                    name: "A".into(),
                    map: Some(Id::new_unchecked("m")),
                    x: Some(0.0),
                    y: Some(0.0),
                    w: None,
                    h: None,
                    feature: None,
                    biome: None,
                    envelope: None,
                    rank: None,
                },
                Place {
                    id: b.clone(),
                    name: "B".into(),
                    map: Some(Id::new_unchecked("m")),
                    x: Some(0.0),
                    y: Some(0.0),
                    w: None,
                    h: None,
                    feature: None,
                    biome: None,
                    envelope: None,
                    rank: None,
                },
            ],
            edges: vec![PlaceEdge {
                id: Id::new_unchecked("e_ab"),
                a: a.clone(),
                b: b.clone(),
                distance_km: 100.0,
                terrain: "plain".into(),
                bidirectional: true,
            }],
            vehicles: vec![Vehicle {
                id: Id::new_unchecked("v_walk"),
                name: "步行".into(),
                instant: false,
                // 40 km/日（5 km/h × 8 h，ADR-0024 单位修正后）。100 km 边 → 2.5 天；
                // 下面的预算用例以 2 天（缺口）与 50 天（充足）区分提示与否。
                speed_kmh: Some(5.0),
                daily_hours: Some(8.0),
                terrain_modifiers: Default::default(),
                requires_endpoints: false,
                requirements: vec![],
            }],
        }
    }

    fn ev(id: &str, from: i64, to: i64, place: &str, who: &[&str]) -> Event {
        Event {
            id: Id::new_unchecked(id),
            name: id.into(),
            span: TimeSpan { from, to: Some(to) },
            location: Some(EventLocation {
                place: Id::new_unchecked(place),
                x: None,
                y: None,
                w: None,
                h: None,
            }),
            participants: who.iter().map(|s| Id::new_unchecked(*s)).collect(),
            narrative: None,
            outcome: None,
            effects: vec![],
            narrations: vec![],
        }
    }

    /// 构造一个最小可工作 `World`：`distances` + `events` 都给好。
    fn world_with(distances: DistanceGraph, events: Vec<Event>) -> wv_core::World {
        wv_core::World {
            distances,
            events,
            ..Default::default()
        }
    }

    /// 预算 < 路径最少 → 出 hint。
    #[test]
    fn travel_feasibility_hints_when_budget_below_path_min() {
        let w = world_with(
            fixture_graph(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_b", &["x"])],
        );
        // 直查 A* 拿真实 total_days，让断言用真实值。
        let j = w
            .distances
            .shortest_journey(&Id::new_unchecked("p_a"), &Id::new_unchecked("p_b"), &[])
            .unwrap();
        let expected_total = j.total_days;
        let d = derive_hints(&w);
        let travel: Vec<&Diagnostic> = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .collect();
        assert_eq!(travel.len(), 1, "应出一条位移提示");
        let msg = &travel[0].message;
        let total_str = format!("{:.1}", expected_total);
        assert!(
            msg.contains(&total_str),
            "应报最少需 {total_str} 天：got {msg}"
        );
        assert!(msg.contains("2"), "应报预算 2 天：got {msg}");
        assert_eq!(travel[0].level, DiagLevel::Hint);
    }

    /// 预算 ≥ 路径最少 → 不出 hint。
    #[test]
    fn travel_feasibility_silent_when_budget_covers_path_min() {
        // fixture 中 A→B 实际需 2.5 天（5 km/h × 8 h = 40 km/日，100 km），gap 50 足够。
        let w = world_with(
            fixture_graph(),
            vec![
                ev("e1", 1, 5, "p_a", &["x"]),
                ev("e2", 55, 56, "p_b", &["x"]),
            ],
        );
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "预算充足时不应出提示");
    }

    /// 同参与者集合为空 → 不出 hint。
    #[test]
    fn travel_feasibility_skipped_when_no_common_participant() {
        let w = world_with(
            fixture_graph(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_b", &["y"])],
        );
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "无共同参与者不应出提示");
    }

    /// 带洞地界按**外环**做 feature-no-cells 判定（ADR-0029 M2）：洞不参与，
    /// 外环内无目标群系格 → 提示；格刷对 → 提示消失。
    #[test]
    fn marker_with_holes_geometry_is_checked_by_outer_ring() {
        use wv_core::map::{Extent, GeometryData, GeometrySpan};
        use wv_core::terrain::{TerrainCellData, TerrainData};

        fn marker_world() -> wv_core::World {
            let map = Id::new_unchecked("m");
            let mut w = wv_core::World::default();
            w.maps.push(wv_core::map::MapNode {
                id: map.clone(),
                name: "M".into(),
                level: wv_core::map::MapLevel::World,
                parent: None,
                children: Vec::new(),
                layers: Vec::new(),
                icons: Vec::new(),
                extent: Some(Extent::sized(64.0, 64.0)),
                raster: None,
                geographic_source: None,
                time_span: None,
                unit_per_km: None,
                orientation: 0.0,
                base: None,
                outline_polygon: None,
            });
            w.set_terrain(
                map.clone(),
                TerrainData {
                    width: 64,
                    height: 64,
                    cells: vec![
                        TerrainCellData {
                            height: 100,
                            surface: 1,
                            biome: 3,
                        };
                        64 * 64
                    ],
                },
            );
            let place_id = Id::new_unchecked("place_ring_woods");
            w.distances.places.push(Place {
                id: place_id.clone(),
                name: "环湖林地".into(),
                map: Some(map.clone()),
                x: None,
                y: None,
                w: None,
                h: None,
                feature: None,
                biome: Some(1),
                envelope: None,
                rank: None,
            });
            w.place_geometry.push(GeometrySpan {
                map,
                owner: place_id,
                span: TimeSpan { from: 0, to: None },
                geometry: GeometryData::PolygonWithHoles {
                    outer: vec![10.0, 10.0, 30.0, 10.0, 30.0, 30.0, 10.0, 30.0],
                    holes: vec![vec![18.0, 18.0, 22.0, 18.0, 22.0, 22.0, 18.0, 22.0]],
                },
                relation: None,
            });
            w
        }

        // 外环内全是草原（biome 3），声明的却是森林（biome 1）→ 应提示。
        let w = marker_world();
        let d = derive_hints(&w);
        assert!(
            d.items
                .iter()
                .any(|x| x.fingerprint == "feature-no-cells|place_ring_woods"),
            "带洞地界不得被 feature-no-cells 静默跳过，got {:?}",
            d.items
        );

        // 对照：外环内刷成森林 → 提示消失（洞内保持草原不影响）。
        let mut w2 = marker_world();
        let mut data = w2.terrain_of(&Id::new_unchecked("m")).cloned().unwrap();
        for y in 10..30 {
            for x in 10..30 {
                data.cells[y * 64 + x].biome = 1;
            }
        }
        w2.set_terrain(Id::new_unchecked("m"), data);
        let d2 = derive_hints(&w2);
        assert!(
            !d2.items
                .iter()
                .any(|x| x.fingerprint == "feature-no-cells|place_ring_woods"),
            "外环内有目标群系格时不应提示，got {:?}",
            d2.items
        );
    }

    /// 同地点 → 不出 hint。
    #[test]
    fn travel_feasibility_skipped_when_same_place() {
        let w = world_with(
            fixture_graph(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_a", &["x"])],
        );
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "同地点不算位移");
    }

    /// 路径不通（图中无连边）→ 不报本检查，留给其他检查。
    #[test]
    fn travel_feasibility_silent_when_no_path() {
        let w = world_with(
            DistanceGraph::default(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_b", &["x"])],
        );
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "路径不可达时不应出本类提示");
    }

    /// `prev.span.to == None`（开放区间）→ 跳过——next 与仍持续的 prev 重叠是另一类矛盾。
    #[test]
    fn travel_feasibility_skipped_when_prev_to_open() {
        let mut e1 = ev("e1", 1, 5, "p_a", &["x"]);
        e1.span.to = None;
        let w = world_with(fixture_graph(), vec![e1, ev("e2", 7, 8, "p_b", &["x"])]);
        let d = derive_hints(&w);
        let travel = d
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert_eq!(travel, 0, "开放区间跳过本检查");
    }

    /// 指纹由 prev/next id + 共同参与者集合组成，确定性、不含日期（AGENTS §6.1 措辞思路：
    /// 日期一变提示就失效，作者认领不掉——本检查同理用稳定 id）。
    #[test]
    fn travel_feasibility_fingerprint_is_stable_under_time_edit() {
        let w = world_with(
            fixture_graph(),
            vec![ev("e1", 1, 5, "p_a", &["x"]), ev("e2", 7, 8, "p_b", &["x"])],
        );
        let d1 = derive_hints(&w);
        // 把 next 的 day 挪到 55 → gap=50，预算充足 → 提示应消失。
        let mut w2 = w.clone();
        w2.events[1].span.from = 55;
        let d2 = derive_hints(&w2);
        let fp1: String = d1
            .items
            .iter()
            .find(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .map(|x| x.fingerprint.clone())
            .unwrap();
        let count2 = d2
            .items
            .iter()
            .filter(|x| x.fingerprint.starts_with("travel_feasibility|"))
            .count();
        assert!(
            fp1.contains("e1") && fp1.contains("e2") && fp1.contains("x"),
            "指纹应含 prev/next id 与共同参与者：got {fp1}"
        );
        assert_eq!(count2, 0, "挪日期后预算充足，提示应消失");
    }

    /// _use ScheduledJourney：避免未使用导入 lint。
    #[allow(dead_code)]
    fn _ensure_journey_in_scope(_: &ScheduledJourney) {}

    /// 阶段六 v0.1：xinhai-1911 工程的距离图应正确加载三层（places/edges/vehicles），
    /// 且武汉→成都走「长江＋陆路」可解（A* 返非空）。
    ///
    /// 守住「加载层必须把距离三个分量都读全」这一收敛点——旧实现只读单文件 places.yaml，
    /// edges/vehicles 始终为空，路径搜索退化。
    #[test]
    fn xinhai_loads_full_distance_graph_and_path_finds_jiangnan_route() {
        let path = std::path::Path::new("../examples/xinhai-1911");
        if !path.is_dir() {
            // 在子 crate 工作目录外运行（如 IDE unit test）允许跳过。
            return;
        }
        let project = crate::project::Project::open(path).expect("工程加载失败");
        let world = project.load_world().expect("加载世界失败");
        let g = &world.distances;
        assert!(
            !g.places.is_empty(),
            "places 应从目录加载；空 = 加载器未生效"
        );
        assert!(!g.edges.is_empty(), "edges 应从目录加载；空 = 旧实现未替换");
        assert!(
            !g.vehicles.is_empty(),
            "vehicles 应从目录加载；空 = 旧实现未替换"
        );
        // 武汉 → 成都：waterway + plain，A* 应至少给出一条路径。
        let wuhan = wv_core::id::Id::new_unchecked("place_wuhan");
        let chengdu = wv_core::id::Id::new_unchecked("place_chengdu");
        let journey = g
            .shortest_journey(&wuhan, &chengdu, &[])
            .expect("武汉→成都应有路径");
        assert!(
            journey.total_days.is_finite() && journey.total_days > 0.0,
            "总天数应有限且 > 0：got {}",
            journey.total_days
        );
        // 武汉到宜昌是水路，工具轮船应被采用。
        let uses_steamboat = journey
            .legs
            .iter()
            .any(|s| s.vehicle.as_str() == "veh_steamboat");
        assert!(
            uses_steamboat,
            "长江段应被 veh_steamboat 承担，实际工具列表 = {:?}",
            journey
                .legs
                .iter()
                .map(|s| s.vehicle.as_str())
                .collect::<Vec<_>>()
        );
    }

    /// 阶段 S12 v0.1：xinhai-1911 工程应加载 factions + faction_geometry 两层目录，
    /// 且 `factions_at` 在关键日期能筛出应有的势力。
    ///
    /// 辛亥年（day 283，武昌起义）前后是关键断面：早期只有清廷，晚期清 + 革命并存。
    #[test]
    fn xinhai_loads_factions_and_geometry_at_key_days() {
        let path = std::path::Path::new("../examples/xinhai-1911");
        if !path.is_dir() {
            return;
        }
        let project = crate::project::Project::open(path).expect("工程加载失败");
        let world = project.load_world().expect("加载世界失败");
        // 两层目录都应非空。
        assert!(world.factions.len() >= 2, "示例工程应至少 2 个势力");
        assert!(
            world.faction_geometry.len() >= 2,
            "示例工程应至少 2 段势力几何"
        );
        // 第 100 天：只有清廷。
        let map = wv_core::id::Id::new_unchecked("map_world");
        let early = wv_core::factions_at(&world, &map, 100);
        assert_eq!(early.len(), 1, "起义前应只有清廷");
        assert_eq!(early[0].0.id.as_str(), "faction_qing");
        // 第 283 天：武昌起义，清 + 革命都生效。
        let mid = wv_core::factions_at(&world, &map, 283);
        assert_eq!(mid.len(), 2, "起义当日双方都生效");
        // 第 300 天：清廷 [1, 285) 已结束，革命军（开放）仍在。
        let late = wv_core::factions_at(&world, &map, 300);
        assert_eq!(late.len(), 1, "清廷结束日之后只剩革命军");
        assert_eq!(late[0].0.id.as_str(), "faction_rev");
    }
}

#[cfg(test)]
mod faction_diag_tests {
    use super::*;
    use wv_core::{Color, Faction, FactionColorMode, FactionLayer, Id, Membership, TimeSpan};

    fn faction(id: &str) -> Faction {
        Faction::new(Id::new_unchecked(id), id, Color("#000000".into()))
    }

    fn membership(target: &str, from: i64, to: Option<i64>) -> Membership {
        Membership {
            target: Id::new_unchecked(target),
            kind: None,
            span: TimeSpan { from, to },
        }
    }

    fn layer(id: &str) -> FactionLayer {
        FactionLayer {
            id: Id::new_unchecked(id),
            name: id.into(),
            z: 0,
            color_mode: FactionColorMode::Own,
            visible: true,
        }
    }

    #[test]
    fn dangling_layer_target_and_seat_are_errors() {
        let mut w = wv_core::World::default();
        let mut f = faction("faction_a");
        f.layer = Some(Id::new_unchecked("layer_missing"));
        f.seat = Some(Id::new_unchecked("place_missing"));
        f.memberships.push(membership("faction_missing", 1, None));
        w.factions.push(f);
        let d = derive_faction_diagnostics(&w);
        assert_eq!(
            d.errors().len(),
            3,
            "layer / target / seat 各一条，got {:?}",
            d.items
        );
        assert!(d.hints().is_empty(), "不应有提示，got {:?}", d.hints());
    }

    #[test]
    fn duplicate_layer_id_is_an_error() {
        let mut w = wv_core::World::default();
        w.faction_layers.push(layer("layer_admin"));
        w.faction_layers.push(layer("layer_admin"));
        let d = derive_faction_diagnostics(&w);
        assert_eq!(d.errors().len(), 1, "got {:?}", d.items);
        assert!(d.errors()[0].fingerprint.contains("dup"));
    }

    #[test]
    fn inverted_membership_span_is_an_error() {
        let mut w = wv_core::World::default();
        w.factions.push(faction("faction_a"));
        let mut b = faction("faction_b");
        b.memberships.push(membership("faction_a", 10, Some(5)));
        w.factions.push(b);
        let d = derive_faction_diagnostics(&w);
        assert!(
            d.errors()
                .iter()
                .any(|e| e.fingerprint.contains("inverted")),
            "got {:?}",
            d.items
        );
    }

    #[test]
    fn membership_cycle_is_a_hint_not_an_error() {
        let mut w = wv_core::World::default();
        let mut a = faction("faction_a");
        a.memberships.push(membership("faction_b", 1, None));
        let mut b = faction("faction_b");
        b.memberships.push(membership("faction_a", 1, None));
        w.factions.extend([a, b]);
        let d = derive_faction_diagnostics(&w);
        assert!(
            d.hints().iter().any(|h| h.fingerprint.contains("cycle")),
            "got {:?}",
            d.items
        );
        assert!(
            d.errors().is_empty(),
            "环本身不产生 Error，got {:?}",
            d.errors()
        );
    }
}
