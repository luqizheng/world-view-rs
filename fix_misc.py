import io

# ============ I2: faction label anchor hole-aware ============
p = 'crates/wv-render/src/drill.rs'
s = io.open(p, encoding='utf-8').read()
old = """                // factions_at 已按 GeometrySpan.span 过滤（时间维度），
                // 此处只需过滤图级窗口。
                let factions = wv_core::factions_at(&projection.world, &current.id, t);
                for (faction, coords) in factions {
                    // 计算多边形质心（简单平均）。
                    if coords.len() >= 2 {
                        let n = (coords.len() / 2) as f32;
                        let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
                        for pair in coords.chunks(2) {
                            sx += pair[0];
                            sy += pair[1];
                        }
                        let center = Vec2::new(sx / n, sy / n);"""
new = """                // faction_territories_at 已按 GeometrySpan.span 过滤（时间维度），
                // 并带出洞环（ADR-0028 D3）：国中国的宿主标签不能落在洞里。
                let territories =
                    wv_core::faction_territories_at(&projection.world, &current.id, t);
                for territory in territories {
                    let faction = territory.faction;
                    let coords = territory.vertices;
                    // 计算多边形质心（简单平均）。
                    if coords.len() >= 6 {
                        let n = (coords.len() / 2) as f32;
                        let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
                        for pair in coords.chunks(2) {
                            sx += pair[0];
                            sy += pair[1];
                        }
                        let mut center = Vec2::new(sx / n, sy / n);
                        // 洞感知（终审 I2）：质心落在洞里 → 沿「质心 → 最远外环
                        // 顶点」的中点挪出去，否则环形宿主（如被切出国中国的余部）
                        // 的名字会画在内国头顶上。
                        let in_hole = territory
                            .holes
                            .iter()
                            .any(|h| wv_core::geometry::point_in_ring(h, center.x, center.y));
                        if in_hole {
                            let mut far = (coords[0], coords[1], -1.0f32);
                            for pair in coords.chunks_exact(2) {
                                let d = (pair[0] - center.x).powi(2)
                                    + (pair[1] - center.y).powi(2);
                                if d > far.2 {
                                    far = (pair[0], pair[1], d);
                                }
                            }
                            center = Vec2::new((center.x + far.0) / 2.0, (center.y + far.1) / 2.0);
                        }"""
assert old in s
s = s.replace(old, new)
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)

# ============ M1: query_at_point 走 point_in_geometry ============
p2 = 'crates/wv-core/src/model/faction.rs'
t = io.open(p2, encoding='utf-8').read()
old2 = """    let mut out: Vec<(Id, Id, Option<String>)> = Vec::new();
    for t in faction_territories_at(world, map, day) {
        // 洞感知（ADR-0028 D3）：外环内但落在洞里 = 不在领地。
        if !crate::geometry::point_in_ring(t.vertices, x, y) {
            continue;
        }
        if t.holes.iter().any(|h| crate::geometry::point_in_ring(h, x, y)) {
            continue;
        }"""
new2 = """    let mut out: Vec<(Id, Id, Option<String>)> = Vec::new();
    for t in faction_territories_at(world, map, day) {
        // 洞感知（ADR-0028 D3）：统一走 `point_in_geometry`（唯一判定入口）。
        let geometry = if t.holes.is_empty() {
            crate::map::GeometryData::Polygon(t.vertices.to_vec())
        } else {
            crate::map::GeometryData::PolygonWithHoles {
                outer: t.vertices.to_vec(),
                holes: t.holes.iter().map(|h| h.to_vec()).collect(),
            }
        };
        if !crate::geometry::point_in_geometry(&geometry, x, y) {
            continue;
        }"""
assert old2 in t
t = t.replace(old2, new2)

# 文档同步
old3 = """//! ## 待决 §12-8：带洞多边形
//!
//! **v0.1 仅支持无洞单环多边形**（`GeometryData::Polygon(Vec<f32>)`），
//! 扁平坐标、首尾不重复。如要表达「环形的飞地」请拆成多个 polygon。
//! 这是 §12-8 待裁问题的第一版落地——文档已记录，等真用上再升级为
//! `PolygonWithHoles` 变体。"""
new3 = """//! ## §12-8（带洞多边形）：已由 ADR-0028 D3 落地
//!
//! 原 v0.1 只支持无洞单环；势力划分引入国中国后，宿主余部必须是环形飞地，
//! 因此 [`crate::map::GeometryData`] 增加了 `PolygonWithHoles` 变体
//! （2026-10-18，ADR-0028）。点包含判定统一走
//! [`crate::geometry::point_in_geometry`]（洞内 = 不在领地）。"""
assert old3 in t
t = t.replace(old3, new3)

old4 = """/// - 只取 `GeometryData::Polygon` 变体（其他变体 v0.1 不画，跳过）；"""
new4 = """/// - 只取多边形变体（`Polygon` / `PolygonWithHoles` 取外环；`Points`/`Ref` 跳过）；"""
assert old4 in t
t = t.replace(old4, new4)
io.open(p2, 'w', encoding='utf-8', newline='\n').write(t)

# ============ M2: earcut 桥可见性用当前环 + 兜底扇形 ============
p3 = 'crates/wv-render/src/faction_overlays.rs'
r = io.open(p3, encoding='utf-8').read()
old5 = """    let flat_ring: Vec<f32> = ring.iter().flat_map(|p| [p[0], p[1]]).collect();
    for hole in &holes {
        if hole.is_empty() {
            continue;
        }"""
new5 = """    for hole in &holes {
        if hole.is_empty() {
            continue;
        }
        // 可见性测试必须用**当前**（已桥接过的）环：第二、第三个洞的桥不能
        // 穿过先前洞的零宽走廊（终审 M2）。
        let flat_ring: Vec<f32> = ring.iter().flat_map(|p| [p[0], p[1]]).collect();"""
assert old5 in r
r = r.replace(old5, new5)

old6 = """        if !clipped {
            // 数值兜底：残余顶点扇形收尾（不 panic，V1 精神）。
            for w in idx.windows(3) {
                tris.push(ring[w[0]]);
                tris.push(ring[w[1]]);
                tris.push(ring[w[2]]);
            }
            break;
        }"""
new6 = """        if !clipped {
            // 数值兜底：以 idx[0] 为轴的**真扇形**收尾（不 panic，V1 精神）。
            // 逐窗口连发是条带不是扇形，会产出重叠三角形。
            let pivot = ring[idx[0]];
            for w in idx.windows(2).skip(1) {
                tris.push(pivot);
                tris.push(ring[w[0]]);
                tris.push(ring[w[1]]);
            }
            break;
        }"""
assert old6 in r
r = r.replace(old6, new6)
io.open(p3, 'w', encoding='utf-8', newline='\n').write(r)

# ============ M7: 其余陈旧注释 ============
p4 = 'crates/wv-core/src/base/geometry.rs'
g = io.open(p4, encoding='utf-8').read()
old7 = """//! 几何辅助。几何编码方式仍是 §13 待裁问题；本模块现提供无洞单环的
//! 点包含谓词 [`point_in_ring`]。"""
new7 = """//! 几何辅助。几何编码方式仍是 §13 待裁问题；本模块提供单环点包含
//! [`point_in_ring`] 与洞感知的唯一入口 [`point_in_geometry`]（ADR-0028 D3）。"""
assert old7 in g
g = g.replace(old7, new7)
io.open(p4, 'w', encoding='utf-8', newline='\n').write(g)

p5 = 'crates/wv-edit/src/command.rs'
c = io.open(p5, encoding='utf-8').read()
old8 = "/// - `outline_polygon`（若 `Some` 且为 `Polygon` 变体）所有顶点 += delta"
new8 = "/// - `outline_polygon`（`Polygon` / `PolygonWithHoles`，含洞环）所有顶点 += delta"
assert old8 in c
c = c.replace(old8, new8)
io.open(p5, 'w', encoding='utf-8', newline='\n').write(c)

# push_cmd 文档错位：把「内部：单条命令包成事务提交…」还给 push_cmd 本身
p6 = 'crates/wv-app/src/edit/faction_panel_actions.rs'
a = io.open(p6, encoding='utf-8').read()
old9 = """/// 内部：单条命令包成事务提交，失败时 warn 不抛。
/// 从宿主（或空势力）划分出一个新势力（ADR-0028 / M1 Task 6）。"""
new9 = """/// 从宿主（或空势力）划分出一个新势力（ADR-0028 / M1 Task 6）。"""
assert old9 in a
a = a.replace(old9, new9)
old10 = """fn push_cmd(
    world: &mut WorldProjection,"""
new10 = """/// 内部：单条命令包成事务提交，失败时 warn 不抛。
fn push_cmd(
    world: &mut WorldProjection,"""
assert old10 in a
a = a.replace(old10, new10)
# divide_faction_now 文档补 day 参数
old11 = """/// - `cell_size`：图内一格的世界单位（wu → 格坐标）。"""
new11 = """/// - `cell_size`：图内一格的世界单位（wu → 格坐标）；
/// - `day`：钉住的生效日（与打开对话框那一刻一致，M6）。"""
assert old11 in a
a = a.replace(old11, new11)
io.open(p6, 'w', encoding='utf-8', newline='\n').write(a)
print("ok")
