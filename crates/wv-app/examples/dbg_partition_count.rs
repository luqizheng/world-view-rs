//! 临时诊断：湿地/沼泽格的高度分布（用户：右下角海绵 height 不对）。
use wv_terrain::biome;

fn main() {
    let path = "examples/work-view-test/maps/world_main/terrain.bin";
    let bytes = std::fs::read(path).expect("read terrain.bin");
    let data: wv_core::terrain::TerrainData =
        wv_io::terrain_bin::decode_terrain(&bytes).expect("decode");
    println!(
        "decoded {}x{} cells={}",
        data.width,
        data.height,
        data.cells.len()
    );
    let mut stats: std::collections::BTreeMap<u8, (usize, i16, i16, i64)> =
        std::collections::BTreeMap::new();
    for c in data.cells.iter() {
        let e = stats.entry(c.biome).or_insert((0, i16::MAX, i16::MIN, 0));
        e.0 += 1;
        e.1 = e.1.min(c.height);
        e.2 = e.2.max(c.height);
        e.3 += c.height as i64;
    }
    let names = [
        "NONE", "森林", "平原", "草原", "戈壁", "沙漠", "沼泽", "湿地", "苔原", "雪原", "丛林",
        "雨林", "丘陵", "山地", "海洋", "湖泊", "河流", "绿洲", "稀树", "盐碱", "道路",
    ];
    for (b, (n, lo, hi, sum)) in &stats {
        let name = names.get(*b as usize).copied().unwrap_or("?");
        println!(
            "biome {b:2} {name:6} 格数={n:6} 高度=[{lo}, {hi}] 平均={:.0}",
            sum / (*n.max(&1)) as i64
        );
    }
    let mut quad: Vec<(i16, u8)> = Vec::new();
    for (i, c) in data.cells.iter().enumerate() {
        let gx = i as u32 % data.width;
        let gy = i as u32 / data.width;
        if gx > 512
            && gy > 384
            && (c.biome == biome::BIOME_WETLAND || c.biome == biome::BIOME_SWAMP)
        {
            quad.push((c.height, c.biome));
        }
    }
    quad.sort();
    if quad.is_empty() {
        println!("右下象限无湿地/沼泽格");
    } else {
        println!(
            "右下象限湿地/沼泽格 {} 个，高度范围 [{}, {}]",
            quad.len(),
            quad.first().unwrap().0,
            quad.last().unwrap().0
        );
    }
}
