//! 一次性工具：把 `target/dem_gd/guangdong_dem.bin`（真实 DEM 中间格式）写成
//! `examples/guangdong` 的 `terrain.bin` 并回填 `maps/map_guangdong/index.yaml`
//! 的 `raster:` 字段。
//!
//! 中间格式（`target/dem_gd/prep_guangdong_dem.py` 产出，seattle kinds 先例）：
//! 头 2 字节小端 u16 宽 + 2 字节 u16 高 + 行优先 i16 高度（米，海 = -350）。
//!
//! 用法：`cargo run -p wv-app --features app --example build_guangdong_terrain`
//!
//! kind 分带沿用 `wv-terrain::continent` 的高度语义（雪线 2600 / 山 1200 /
//! 丘 300 / 0 = 草原平原海岸）；负高度一律水（0 深水 / 1 浅水由深度分），
//! 与 `is_water_kind` 渲染判定一致。

use std::path::Path;

use wv_core::terrain::{TerrainCellData, TerrainData};
use wv_io::terrain_bin::write_terrain;
use wv_terrain::{
    biome::{
        BIOME_FOREST, BIOME_HILLS, BIOME_MOUNTAINS, BIOME_OCEAN, BIOME_PLAINS, BIOME_SNOWFIELD,
    },
    SURFACE_LAND, SURFACE_WATER,
};

/// 海面填充高度（prep 脚本同值）：渲染上处于 `water_deep` 区间。
const SEA_DEPTH: i16 = -350;

/// 真实高度 → (surface, biome)（`wv-terrain` 冻结表同源）。
///
/// 分带按广东实际高程分布校准（COP90 实测：p90=654 / p95=812 / p99=1139 /
/// max=1892 m）：大陆架式的「1200 m 起山」在广东只剩 0.5%，因此山带取
/// 700 m（粤北中低山）、丘陵带 350 m、森林带 80 m；雪线 2600 保留契约位。
fn classify(height: i16) -> (u8, u8) {
    if height < 0 {
        (SURFACE_WATER, BIOME_OCEAN)
    } else if height >= 2600 {
        (SURFACE_LAND, BIOME_SNOWFIELD)
    } else if height >= 700 {
        (SURFACE_LAND, BIOME_MOUNTAINS)
    } else if height >= 350 {
        (SURFACE_LAND, BIOME_HILLS)
    } else if height >= 80 {
        (SURFACE_LAND, BIOME_FOREST)
    } else {
        (SURFACE_LAND, BIOME_PLAINS)
    }
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "examples/guangdong".into());
    let dem_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "target/dem_gd/guangdong_dem.bin".into());

    let bytes = std::fs::read(&dem_path)
        .unwrap_or_else(|e| panic!("读 {dem_path:?} 失败: {e}（先跑 prep_guangdong_dem.py）"));
    assert!(bytes.len() >= 4, "dem 中间格式至少 4 字节头");
    let w = u16::from_le_bytes([bytes[0], bytes[1]]) as u32;
    let h = u16::from_le_bytes([bytes[2], bytes[3]]) as u32;
    assert_eq!(
        bytes.len(),
        4 + (w as usize) * (h as usize) * 2,
        "dem 数据长度与 {w}x{h} 不符"
    );
    println!("dem {w}x{h}");

    let cells: Vec<TerrainCellData> = bytes[4..]
        .chunks_exact(2)
        .map(|c| {
            let height = i16::from_le_bytes([c[0], c[1]]);
            let (surface, biome) = classify(height);
            TerrainCellData {
                height,
                surface,
                biome,
            }
        })
        .collect();

    let data = TerrainData {
        width: w,
        height: h,
        cells,
    };
    let root = Path::new(&root);
    let raster = write_terrain(root, "map_guangdong", &data).expect("写 terrain.bin 失败");
    println!(
        "written {} ({}x{}, checksum {:?})",
        root.join("maps/map_guangdong/terrain.bin").display(),
        raster.width,
        raster.height,
        raster.checksum
    );

    // 回填 index.yaml 的 `raster:` 字段。
    //
    // 块识别口径：**顶格** `raster:`（MapNode 字段）——layers 列表里缩进的
    // `raster:` 不算（v1 曾误插进 layers 末尾成了图层字段，serde 静默忽略）。
    // 幂等：已有顶格块（含同 checksum）→ 只更新字段值，不新增块。
    let index_path = root.join("maps/map_guangdong/index.yaml");
    let old = std::fs::read_to_string(&index_path).expect("读 index.yaml 失败");
    let raster_yaml = serde_yaml::to_string(&raster).expect("序列化 RasterRef");
    let new = set_top_level_raster(&old, &raster_yaml);
    if new != old {
        std::fs::write(&index_path, &new).expect("写 index.yaml 失败");
        println!("raster: 块已回填 {}", index_path.display());
    } else {
        println!("raster: 块已是最新的，跳过回填");
    }

    // 验证工程可加载（terrain round-trip + load_world 带出同一份数据）。
    let back = wv_io::terrain_bin::read_terrain(root, &raster).expect("回读失败");
    assert_eq!(back, data, "terrain.bin round-trip 必须无损");
    let project = wv_io::project::Project::open(root).expect("打开工程失败");
    let world = project.load_world().expect("加载世界失败");
    assert_eq!(
        world.terrain_of(&wv_core::Id::new_unchecked("map_guangdong")),
        Some(&data),
        "load_world 应带出同一份地形"
    );
    println!(
        "工程加载验证通过：{}（terrain {}x{}）",
        root.display(),
        w,
        h
    );
}

/// 把 `raster_yaml`（serde 序列化的 `RasterRef`）设为文档的**顶格** `raster:` 块。
///
/// - 已有顶格块 → 原地替换字段值（缩进 2 空格）；
/// - 没有 → 插到 `extent:` 之前（MapNode 的键序惯例）；
/// - layers 内缩进的 `raster:` 不受影响也不被误认。
fn set_top_level_raster(doc: &str, raster_yaml: &str) -> String {
    let indented: String = raster_yaml
        .lines()
        .map(|l| {
            if l.trim().is_empty() {
                String::from("\n")
            } else {
                format!("  {l}\n")
            }
        })
        .collect();

    let find_top_level = |doc: &str| -> Option<(usize, usize)> {
        let mut offset = 0;
        for line in doc.lines() {
            let start = offset;
            offset += line.len() + 1;
            if line.starts_with("raster:") {
                // 块延续 = 后续以空格开头的行。
                let mut end = offset;
                for l in doc[offset..].lines() {
                    if l.starts_with(' ') || l.trim().is_empty() {
                        end += l.len() + 1;
                    } else {
                        break;
                    }
                }
                return Some((start, end.min(doc.len())));
            }
        }
        None
    };

    if let Some((start, end)) = find_top_level(doc) {
        let mut out = String::with_capacity(doc.len() + indented.len());
        out.push_str(&doc[..start]);
        out.push_str("raster:\n");
        out.push_str(indented.trim_end_matches('\n'));
        out.push('\n');
        out.push_str(&doc[end..]);
        out
    } else {
        match doc.find("\nextent:") {
            Some(pos) => format!(
                "{}raster:\n{}{}",
                &doc[..pos + 1],
                indented,
                &doc[pos + 1..]
            ),
            None => format!("{doc}raster:\n{indented}"),
        }
    }
}
