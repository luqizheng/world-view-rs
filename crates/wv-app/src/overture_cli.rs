//! `wv overture ...` 子命令实现（ADR-0007 §决策 (5)）。
//!
//! ## 职责
//!
//! 1. 解析 CLI 参数（`--bbox` / `--theme` / `--release` / `--in` / `--out` / `--emit-yaml`）；
//! 2. 调用 `overturemaps-py` 下载 Overture bbox 切片为 GeoJSON（或跳过下载走 `--in`）；
//! 3. 走 [`wv_overture::fetch::parse_geojson_str`] 转换；
//! 4. 打印「保留 N 条 / 过滤 M 条」统计；
//! 5. （可选）输出**写回草稿** YAML，让作者以 `EditCommand`/`History` 路径落到 `distances/edges.yaml`。
//!
//! ## 不做什么
//!
//! - **不**直接写盘领域 YAML——遵循 AGENTS §7.5 V1「校验只提示不阻止」+ §3.3
//!   「System 不得改世界」，CLI 只产出「写回草稿」由作者决定怎么用；
//! - **不**持有 Python 解释器——靠 `python -m overturemaps` 子进程触发；
//! - **不**做命令解析库（`clap`）——本 CLI 简单，手写解析足够，避免 AGENTS §9「依赖默认拒绝」。

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::i18n::Locale;
use wv_overture::fetch::{
    parse_geojson_segments, parse_geojson_segments_with_connectors, parse_geojson_str,
    SegmentWithConnectors,
};
use wv_overture::index::IndexedOvertures;

/// 入口：`wv overture <子命令> <参数...>`。
///
/// 返回 `Ok(())` 表示命令成功执行完毕（即使没找到 GeoJSON 也算「命令执行 完毕」）。
/// 失败原因走 `Err(String)` 给 caller 打印。
pub fn overture_command(args: &[String], _loc: Locale) -> Result<(), String> {
    let sub = args.first().map(String::as_str).unwrap_or("");
    match sub {
        "fetch" => fetch_command(&args[1..]),
        "index" => index_command(&args[1..]),
        "apply" => apply_command(&args[1..]),
        "--help" | "-h" | "" => {
            print_overture_help();
            Ok(())
        }
        other => Err(format!("未知子命令: {other}\n\n{}", FETCH_HELP)),
    }
}

/// `wv overture fetch --bbox W,S,E,N --theme <theme> [--release TAG] --out <path>`
///
/// 数据流：
/// 1. 参数解析 + 校验（两模式互斥：`--in` 与 `--bbox/--theme` 必选其一）；
/// 2. 下载模式：调 `python -m overturemaps download ... -o <out>`；
///    转换模式：跳过下载；
/// 3. 读 `<out>`（下载落点 or `--in`）GeoJSON → `Vec<PlaceEdgeCandidate>`；
/// 4. 打印 `保留 N / 过滤 M / 错误 K 个`；
/// 5. （`--emit-yaml` 时）把 `PlaceEdgeCandidate` 序列化为**写回草稿 YAML** 写到 `--out`。
///    草稿里 `id` / `a` / `b` 是占位（`PLACE_ID:PLACE_REPLACE_ME_0` 形式），
///    作者用真 `Place.id` 替换后通过 `wv-edit::EditCommand` 落盘。
///
/// `--release` 暂不真正传给 `overturemaps`（该工具按 latest 默认），保留作未来扩展位。
fn fetch_command(args: &[String]) -> Result<(), String> {
    let parsed = FetchArgs::parse(args)?;

    // 1) 下载模式才调 Python 子进程
    if parsed.in_path.is_none() {
        run_overturemaps(&parsed)?;
    }

    // 2) 读 GeoJSON + 转 PlaceEdgeCandidate（输入路径：--in 优先，回落 --out）
    let input = parsed.in_path.as_ref().unwrap_or(&parsed.out);
    let cands = load_candidates(input)?;
    // 重新读 GeoJSON 拿总 feature 数仅为算 filtered——开销可忽略（一次 I/O）
    let total_features = count_features(input).unwrap_or(cands.len());
    let mut report = report_of(&cands);
    report.filtered = total_features.saturating_sub(cands.len());
    println!("Overture fetch 完成：");
    println!(
        "  模式:        {}",
        if parsed.in_path.is_some() {
            "仅转换"
        } else {
            "下载 + 转换"
        }
    );
    if let Some(theme) = &parsed.theme {
        println!("  主题:        {theme}");
    }
    if let Some(bbox) = parsed.bbox {
        println!("  bbox:        {bbox:?}");
    }
    println!(
        "  release:     {}",
        parsed.release.as_deref().unwrap_or("latest")
    );
    println!("  输入文件:    {}", input.display());
    println!(
        "  保留 PlaceEdgeCandidate: {}（过滤 {} 条）",
        report.kept, report.filtered
    );
    println!(
        "  覆盖 OvertureId: {}/{}",
        report.with_overture_id, report.kept
    );

    // 3) `--emit-yaml` 时输出 PlaceEdge 草稿（覆盖 GeoJSON 路径？不写到工程！
    //    我们写到 GeoJSON 路径的 `*.edges.yaml` 旁——默认是 stdout，作者可重定向)
    if parsed.emit_yaml {
        let yaml = candidates_to_edge_yaml(&cands);
        // 写到 stdout 让作者重定向（避免误覆盖工程 YAML）
        println!("\n# --- PlaceEdge 写回草稿（请用真 Place.id 替换 PLACE_ID:PLACE_REPLACE_ME_N 占位） ---");
        println!("# --- 适用 distances/edges.yaml，由 wv-edit::EditCommand 路径落盘 ---");
        println!("{yaml}");
    }
    Ok(())
}

/// fetch 子命令参数。
///
/// 两模式：
/// - **下载 + 转换**（默认）：提供 `--bbox` + `--theme` + `--out`，调 `overturemaps-py` 后转换；
/// - **仅转换**：`--in <path>` + `--out <path>`，跳过下载，直接把已下载 GeoJSON 转成 PlaceEdgeCandidate。
///
/// 字段语义：
/// - `bbox` / `theme` / `release`：仅下载模式用；提供 `--in` 时可不填；
/// - `in_path`：仅转换模式用；存在则跳过下载；
/// - `out`：两模式共用——下载模式下指向 GeoJSON 落点，转换模式下指向**已下载**的 GeoJSON；
///   两模式都打印转换统计，所以 `out` 必填。
struct FetchArgs {
    bbox: Option<Bbox>,
    theme: Option<String>,
    release: Option<String>,
    in_path: Option<PathBuf>,
    out: PathBuf,
    emit_yaml: bool,
}

/// 经纬度 bbox：west / south / east / north。
#[derive(Debug, Clone, Copy)]
struct Bbox {
    west: f64,
    south: f64,
    east: f64,
    north: f64,
}

/// 转换统计报告。
struct FetchReport {
    kept: usize,
    filtered: usize,
    with_overture_id: usize,
}

impl FetchArgs {
    /// 手写参数解析（避免引入 `clap` 等依赖）。
    ///
    /// flags：
    /// - `--bbox W,S,E,N`：下载模式必填（且与 `--in` 互斥）
    /// - `--theme <theme>`：下载模式必填
    /// - `--release <TAG>`：可选
    /// - `--in <path>`：仅转换模式必填（与 `--bbox` / `--theme` 互斥）
    /// - `--out <path>`：**两模式都必填**——下载模式下指向 GeoJSON 落点；
    ///   转换模式下指向**已下载**的 GeoJSON。
    /// - `--emit-yaml`：可选，输出 PlaceEdge 写回草稿到 stdout
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut bbox: Option<Bbox> = None;
        let mut theme: Option<String> = None;
        let mut release: Option<String> = None;
        let mut in_path: Option<PathBuf> = None;
        let mut out: Option<PathBuf> = None;
        let mut emit_yaml = false;

        let mut i = 0;
        while i < args.len() {
            let key = &args[i];
            match key.as_str() {
                "--emit-yaml" => {
                    emit_yaml = true;
                    i += 1;
                }
                _ => {
                    let val = args
                        .get(i + 1)
                        .ok_or_else(|| format!("flag `{key}` 需要一个值"))?;
                    match key.as_str() {
                        "--bbox" => {
                            bbox = Some(parse_bbox(val)?);
                            i += 2;
                        }
                        "--theme" => {
                            if val.is_empty() {
                                return Err("--theme 不能为空".into());
                            }
                            theme = Some(val.clone());
                            i += 2;
                        }
                        "--release" => {
                            release = Some(val.clone());
                            i += 2;
                        }
                        "--in" => {
                            in_path = Some(PathBuf::from(val));
                            i += 2;
                        }
                        "--out" => {
                            out = Some(PathBuf::from(val));
                            i += 2;
                        }
                        other => return Err(format!("未知 flag: {other}")),
                    }
                }
            }
        }

        let out = out.ok_or("缺少 --out")?;

        // 两模式互斥校验
        if in_path.is_some() && (bbox.is_some() || theme.is_some()) {
            return Err("--in 与 --bbox/--theme 互斥：选一".into());
        }
        if in_path.is_none() {
            // 下载模式：--bbox / --theme 都必填
            bbox.as_ref().ok_or("下载模式缺少 --bbox")?;
            theme.as_ref().ok_or("下载模式缺少 --theme")?;
        }

        Ok(Self {
            bbox,
            theme,
            release,
            in_path,
            out,
            emit_yaml,
        })
    }
}

fn parse_bbox(s: &str) -> Result<Bbox, String> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 4 {
        return Err(format!(
            "--bbox 应为 W,S,E,N 四段（4 个经纬度），实得 {} 段",
            parts.len()
        ));
    }
    let parse_f = |s: &str, name: &str| -> Result<f64, String> {
        s.trim()
            .parse::<f64>()
            .map_err(|e| format!("--bbox {name} 解析失败: {e}"))
    };
    Ok(Bbox {
        west: parse_f(parts[0], "W")?,
        south: parse_f(parts[1], "S")?,
        east: parse_f(parts[2], "E")?,
        north: parse_f(parts[3], "N")?,
    })
}

/// 调 `python -m overturemaps download ...` 子进程。
///
/// 失败原因走 `Err`——常见：
/// - `python` 不在 PATH
/// - `overturemaps` 未安装（`pip install overturemaps`）
///
/// 调用前提：`FetchArgs::parse` 已确认 `bbox` 与 `theme` 在下载模式下非空；
/// 此函数内部直接 unwrap（`parse` 已是网关）。
fn run_overturemaps(args: &FetchArgs) -> Result<(), String> {
    let bbox = args
        .bbox
        .expect("FetchArgs::parse 应保证下载模式 bbox 已设");
    let theme = args
        .theme
        .as_ref()
        .expect("FetchArgs::parse 应保证下载模式 theme 已设");
    let bbox_str = format!("{},{},{},{}", bbox.west, bbox.south, bbox.east, bbox.north);

    // 确保父目录存在
    if let Some(parent) = args.out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("无法创建目录 {}: {e}", parent.display()))?;
        }
    }

    let mut cmd = Command::new("python");
    cmd.args(["-m", "overturemaps", "download", "--bbox", &bbox_str])
        .args(["-f", "geojson", "-t", theme])
        .arg("-o")
        .arg(&args.out);

    let status = cmd
        .status()
        .map_err(|e| format!("调用 `python -m overturemaps` 失败：{e}\n请确认 python 与 overturemaps 已安装（pip install overturemaps）"))?;

    if !status.success() {
        return Err(format!(
            "`overturemaps download` 退出码 {}，详见 stdout/stderr",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

/// 读 GeoJSON → `Vec<PlaceEdgeCandidate>`（不构建统计报告，让 caller 决定要不要 emit）。
fn load_candidates(
    path: &Path,
) -> Result<Vec<wv_overture::transportation::PlaceEdgeCandidate>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("读 {} 失败: {e}", path.display()))?;
    let fc = parse_geojson_str(&text).map_err(|e| format!("解析 {}：{e}", path.display()))?;
    Ok(parse_geojson_segments(&fc))
}

/// 由 `Vec<PlaceEdgeCandidate>` 计算 `kept` / `with_overture_id`。
/// `filtered` 由调用方事后填入（依赖总 Feature 数）。
fn report_of(cands: &[wv_overture::transportation::PlaceEdgeCandidate]) -> FetchReport {
    let kept = cands.len();
    let with_overture_id = cands.iter().filter(|c| c.overture_id.is_some()).count();
    FetchReport {
        kept,
        filtered: 0,
        with_overture_id,
    }
}

/// 读 GeoJSON FeatureCollection 的 `features.len()`（仅取顶层 features）。
///
/// 失败时（坏 JSON / 非 FeatureCollection）返回 `None`——调用方按 `cands.len()` 兜底。
fn count_features(path: &Path) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    let fc = parse_geojson_str(&text).ok()?;
    Some(fc.features.len())
}

// ---------- index 子命令（Y2 / README §18） ----------

/// `wv overture index --in <path> [--id <UUID>] [--label <name>]`
///
/// 数据流：
/// 1. 参数解析 + 校验；
/// 2. 读 GeoJSON → `Vec<PlaceEdgeCandidate>`；
/// 3. `IndexedOvertures::build`；
/// 4. 打印索引统计（candidates / by_id / by_bbox）；
/// 5. （可选）按 `--id` 反查或按 `--label` 反查。
///
/// **不**写盘——纯验证工具（README §18「何时倒排」未决，第一版只跑内存）。
fn index_command(args: &[String]) -> Result<(), String> {
    let parsed = IndexArgs::parse(args)?;
    let cands = load_candidates(&parsed.in_path)?;
    let total_features = count_features(&parsed.in_path).unwrap_or(cands.len());
    let index = IndexedOvertures::build(cands);

    println!("Overture index 完成：");
    println!("  输入文件:     {}", parsed.in_path.display());
    println!("  原始 feature: {}", total_features);
    println!("  保留 candidate: {}", index.len());
    println!("  by_id 大小:   {}", index.by_id_len());

    // 反查：按 OvertureId
    if let Some(id_str) = &parsed.lookup_id {
        let id = wv_core::OvertureId::new(id_str)
            .map_err(|e| format!("--id 非法 UUID `{id_str}`: {e}"))?;
        match index.get(&id) {
            Some(c) => println!(
                "  lookup by_id {id_str}: distance_km={:.3} terrain={}",
                c.distance_km, c.terrain
            ),
            None => println!("  lookup by_id {id_str}: 未命中"),
        }
    }

    // 反查：按 label
    if parsed.lookup_label.is_some() {
        let label = parsed.lookup_label.as_deref().unwrap_or("default");
        match index.query_bbox(label) {
            Some(indices) => println!("  lookup label `{label}`: {} 条", indices.len()),
            None => println!("  lookup label `{label}`: 未注册（CLI 第一版不预注册）"),
        }
    }

    Ok(())
}

/// index 子命令参数。
#[derive(Debug)]
struct IndexArgs {
    in_path: PathBuf,
    lookup_id: Option<String>,
    lookup_label: Option<String>,
}

impl IndexArgs {
    /// 手写参数解析。
    ///
    /// flags：
    /// - `--in <path>`：必填
    /// - `--id <UUID>`：可选
    /// - `--label <name>`：可选（默认不查；缺省时不打印 label 行）
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut in_path: Option<PathBuf> = None;
        let mut lookup_id: Option<String> = None;
        let mut lookup_label: Option<String> = None;

        let mut i = 0;
        while i < args.len() {
            let key = &args[i];
            let val = args
                .get(i + 1)
                .ok_or_else(|| format!("flag `{key}` 需要一个值"))?;
            match key.as_str() {
                "--in" => {
                    in_path = Some(PathBuf::from(val));
                    i += 2;
                }
                "--id" => {
                    lookup_id = Some(val.clone());
                    i += 2;
                }
                "--label" => {
                    lookup_label = Some(val.clone());
                    i += 2;
                }
                other => return Err(format!("未知 flag: {other}")),
            }
        }

        Ok(Self {
            in_path: in_path.ok_or("缺少 --in")?,
            lookup_id,
            lookup_label,
        })
    }
}

/// 把 `Vec<PlaceEdgeCandidate>` 序列化为 PlaceEdge 写回草稿 YAML（V3）。
///
/// **关键边界**：CLI 不知道工程里有哪些 `Place`——`a`/`b` 用占位
/// `PLACE_ID:PLACE_REPLACE_ME_<N>`。作者用真 Place.id 替换后经
/// `wv-edit::EditCommand` 落盘（AGENTS §7.5 V1「校验只提示不阻止」+ §3.3
/// 「System 不得改世界」）。
///
/// `PlaceEdge.id` 用 `overture_id` 派生（去掉连字符取首 12 位 hex），保证：
/// - 与 Overture 库可追溯；
/// - 走 §4.4 ID 字符集（仅 `[A-Za-z0-9_]`）；
/// - 不会与作者真 PlaceId 撞（用 `overture_` 前缀）。
fn candidates_to_edge_yaml(cands: &[wv_overture::transportation::PlaceEdgeCandidate]) -> String {
    let mut out = String::new();
    out.push_str("edges:\n");
    for (i, c) in cands.iter().enumerate() {
        let overture_id = c.overture_id.as_ref().map(|o| o.as_str().to_string());
        // 占位 ID：`overture_seg_<first 12 hex>`，足够稳定且不撞工程 ID。
        let seg_id = overture_id
            .as_deref()
            .and_then(|s| s.split('-').next())
            .map(|p| format!("overture_seg_{}", &p[..12.min(p.len())]))
            .unwrap_or_else(|| format!("overture_seg_unknown_{i}"));

        out.push_str(&format!("  - id: {seg_id}\n"));
        out.push_str("    # PLACE_ID:PLACE_REPLACE_ME_A ← 用真 Place.id 替换\n");
        out.push_str("    a: PLACE_ID:PLACE_REPLACE_ME_A\n");
        out.push_str("    # PLACE_ID:PLACE_REPLACE_ME_B ← 用真 Place.id 替换\n");
        out.push_str("    b: PLACE_ID:PLACE_REPLACE_ME_B\n");
        out.push_str(&format!("    distance_km: {:.3}\n", c.distance_km));
        out.push_str(&format!("    terrain: {}\n", c.terrain));
        out.push_str("    bidirectional: true\n");
        if let Some(oid) = overture_id {
            out.push_str(&format!("    # overture_id: \"{oid}\"\n"));
        }
        out.push('\n');
    }
    out
}

const FETCH_HELP: &str = "\
用法: wv overture fetch [--bbox W,S,E,N --theme <t>] | --in <path>
            --out <path> [--release <TAG>] [--emit-yaml]

两模式：
  下载+转换（提供 --bbox + --theme）  调 overturemaps-py 下载后转换
  仅转换（提供 --in）                  跳过下载，直接转换已有 GeoJSON

参数:
  --bbox W,S,E,N   经纬度 bbox（WGS84，度），下载模式必填
  --theme <t>      Overture 主题，下载模式必填
  --in <path>      已下载 GeoJSON 路径，仅转换模式必填
  --out <path>     下载落点（下载模式）/ GeoJSON 输入路径（仅转换模式）
  --release <TAG>  可选，钉 Overture release（暂未传给 overturemaps）
  --emit-yaml      可选，输出 PlaceEdge 写回草稿 YAML 到 stdout（不写盘）

依赖: pip install overturemaps
";

const INDEX_HELP: &str = "\
用法: wv overture index --in <path> [--id <OvertureId>] [--label <name>]

从已下载 GeoJSON 构建内存索引（README §18 第一版：直读 + 内存 hash）。
打印索引统计（by_id / by_bbox 大小），并可选验证：
  --id <OvertureId>     反查该 ID 对应的 PlaceEdgeCandidate（应有则打印 distance_km）
  --label <name>        反查该 bbox label 注册的下标（需先 register_bbox；CLI 第一版
                        不做预注册，仅做存在性检查）

参数:
  --in <path>     已下载 GeoJSON 路径
  --id <uuid>     可选，反查该 OvertureId
  --label <name>  可选，反查该 bbox label（默认 'default'）
";

fn print_overture_help() {
    println!("{FETCH_HELP}\n{INDEX_HELP}\n{APPLY_HELP}");
}

// ===========================================================================
// `wv overture apply` —— 把已下载 Overture GeoJSON 转成可加载的工程目录
// ===========================================================================

/// 默认嵌入的 21 个广东地级市种子。
///
/// 字段来源：广东行政区划公开数据（民政部 + 各地市统计公报），坐标取市府
/// 近似几何中心（WGS84 经纬度）。**只**是骨架初值——作者可在 GUI 里用
/// `SetCityOutlineCommand` 后续精修轮廓（ADR-0022 D5）。
const DEFAULT_GUANGDONG_CITIES: &[CitySeed] = &[
    CitySeed {
        id: "guangzhou",
        name: "广州市",
        lat: 23.129_11,
        lon: 113.264_39,
    },
    CitySeed {
        id: "shenzhen",
        name: "深圳市",
        lat: 22.543_10,
        lon: 114.057_87,
    },
    CitySeed {
        id: "zhuhai",
        name: "珠海市",
        lat: 22.271_01,
        lon: 113.576_40,
    },
    CitySeed {
        id: "shantou",
        name: "汕头市",
        lat: 23.353_50,
        lon: 116.681_97,
    },
    CitySeed {
        id: "foshan",
        name: "佛山市",
        lat: 23.021_85,
        lon: 113.121_92,
    },
    CitySeed {
        id: "shaoguan",
        name: "韶关市",
        lat: 24.810_82,
        lon: 113.597_23,
    },
    CitySeed {
        id: "zhanjiang",
        name: "湛江市",
        lat: 21.270_99,
        lon: 110.359_38,
    },
    CitySeed {
        id: "jiangmen",
        name: "江门市",
        lat: 22.578_72,
        lon: 113.081_73,
    },
    CitySeed {
        id: "maoming",
        name: "茂名市",
        lat: 21.662_99,
        lon: 110.925_46,
    },
    CitySeed {
        id: "zhaoqing",
        name: "肇庆市",
        lat: 23.046_98,
        lon: 112.465_43,
    },
    CitySeed {
        id: "huizhou",
        name: "惠州市",
        lat: 23.111_53,
        lon: 114.416_14,
    },
    CitySeed {
        id: "meizhou",
        name: "梅州市",
        lat: 24.288_58,
        lon: 116.122_64,
    },
    CitySeed {
        id: "shanwei",
        name: "汕尾市",
        lat: 22.786_41,
        lon: 115.375_44,
    },
    CitySeed {
        id: "heyuan",
        name: "河源市",
        lat: 23.743_43,
        lon: 114.697_47,
    },
    CitySeed {
        id: "yangjiang",
        name: "阳江市",
        lat: 21.857_95,
        lon: 111.982_59,
    },
    CitySeed {
        id: "qingyuan",
        name: "清远市",
        lat: 23.681_74,
        lon: 113.056_30,
    },
    CitySeed {
        id: "dongguan",
        name: "东莞市",
        lat: 23.020_67,
        lon: 113.751_82,
    },
    CitySeed {
        id: "zhongshan",
        name: "中山市",
        lat: 22.517_58,
        lon: 113.392_73,
    },
    CitySeed {
        id: "chaozhou",
        name: "潮州市",
        lat: 23.661_81,
        lon: 116.623_00,
    },
    CitySeed {
        id: "jieyang",
        name: "揭阳市",
        lat: 23.549_84,
        lon: 116.372_83,
    },
    CitySeed {
        id: "yunfu",
        name: "云浮市",
        lat: 22.929_34,
        lon: 112.044_45,
    },
];

/// 单个城市的种子（CLI 内嵌或 `--cities` YAML 覆盖）。
#[derive(Debug, Clone)]
struct CitySeed {
    id: &'static str,
    name: &'static str,
    lat: f64,
    lon: f64,
}

/// `wv overture apply` 参数。
#[derive(Debug)]
struct ApplyArgs {
    /// 已下载的 Overture GeoJSON 路径。
    geojson: PathBuf,
    /// 目标工程目录（不存在则建）。
    target: PathBuf,
    /// 工程名（写进 `world.yaml.name`）。
    name: String,
    /// 可选：覆盖默认 21 城的 YAML（schema = `CitySeed` 列表）。
    cities: Option<PathBuf>,
    /// 逗号分隔的 road class 白名单（默认 `motorway,trunk`）。
    road_class: Vec<String>,
    /// `--skip-edges` 时只建工程骨架不导边。
    skip_edges: bool,
    /// Overture release tag（写入 `geographic_source.release`）。
    release: Option<String>,
}

impl ApplyArgs {
    /// `release` 字段直接来自 `--release` 参数（如缺省则 `None`）。
    fn release_tag(&self) -> Option<String> {
        self.release.clone()
    }
}

impl ApplyArgs {
    /// 手写解析：与 `FetchArgs::parse` 同口径（避免引入 `clap`）。
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut geojson: Option<PathBuf> = None;
        let mut target: Option<PathBuf> = None;
        let mut name: Option<String> = None;
        let mut cities: Option<PathBuf> = None;
        let mut road_class: Option<String> = None;
        let mut release: Option<String> = None;
        let mut skip_edges = false;

        let mut i = 0;
        while i < args.len() {
            let key = &args[i];
            match key.as_str() {
                "--skip-edges" => {
                    skip_edges = true;
                    i += 1;
                }
                _ => {
                    let val = args
                        .get(i + 1)
                        .ok_or_else(|| format!("flag `{key}` 需要一个值"))?;
                    match key.as_str() {
                        "--geojson" => {
                            geojson = Some(PathBuf::from(val));
                            i += 2;
                        }
                        "--target" => {
                            target = Some(PathBuf::from(val));
                            i += 2;
                        }
                        "--name" => {
                            if val.is_empty() {
                                return Err("--name 不能为空".into());
                            }
                            name = Some(val.clone());
                            i += 2;
                        }
                        "--cities" => {
                            cities = Some(PathBuf::from(val));
                            i += 2;
                        }
                        "--road-class" => {
                            road_class = Some(val.clone());
                            i += 2;
                        }
                        "--release" => {
                            release = Some(val.clone());
                            i += 2;
                        }
                        other => return Err(format!("未知 flag: {other}")),
                    }
                }
            }
        }

        Ok(Self {
            geojson: geojson.ok_or("缺少 --geojson")?,
            target: target.ok_or("缺少 --target")?,
            name: name.unwrap_or_else(|| "广东省".to_string()),
            cities,
            road_class: road_class
                .unwrap_or_else(|| "motorway,trunk".to_string())
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            skip_edges,
            release,
        })
    }
}

/// `wv overture apply` 完整版：参数 → GeoJSON → World → 落盘。
///
/// 流程：
/// 1. 解析 + 校验（`--geojson` 存在 / `--target` 不含 `world.yaml` / 城市列表非空）；
/// 2. 读 GeoJSON 拿 `Vec<SegmentWithConnectors>`；
/// 3. 按 `--road-class` 白名单过滤；
/// 4. 收集 connector 端点去重 → 自动建 Place（路口）；
/// 5. 21 城市（默认 / `--cities`）→ 落成子图 + Place + outline_polygon；
/// 6. 每段 → PlaceEdge（`a` / `b` 引用两端 connector 的 Place.id）；
/// 7. `Project::create` + `Project::save_world` 全量落盘。
///
/// 纪律（AGENTS §3.3）：
/// - 不在 CLI 里塞 Bevy ECS / 不持可变工作副本；
/// - 所有写盘走 `Project::save_world`（YAML 序列化在 `wv-io`）；
/// - 城市子图用 `AddCityCommand`（验证 + 落 IconSpec + 挂 children），
///   大批量 Place / PlaceEdge 用直接 Vec 追加（与 `new_project_dialog::create_project`
///   同一口径，CLI 一次性导入不走 EditHistory 栈）。
fn apply_command(args: &[String]) -> Result<(), String> {
    let parsed = ApplyArgs::parse(args)?;

    // 1) 校验文件 / 目录
    if !parsed.geojson.is_file() {
        return Err(format!(
            "--geojson 路径不是文件或不存在: {}",
            parsed.geojson.display()
        ));
    }
    if parsed.target.join("world.yaml").is_file() {
        return Err(format!(
            "--target 已含 world.yaml，拒绝覆盖：{}",
            parsed.target.display()
        ));
    }
    let cities: Vec<(String, String, f64, f64)> = if let Some(p) = &parsed.cities {
        load_cities_from_yaml(p)?
    } else {
        DEFAULT_GUANGDONG_CITIES
            .iter()
            .map(|c| (c.id.to_string(), c.name.to_string(), c.lat, c.lon))
            .collect()
    };
    if cities.is_empty() {
        return Err("城市列表为空（默认 21 城或 --cities 至少 1 条）".into());
    }
    if parsed.road_class.is_empty() {
        return Err("--road-class 不能为空（至少 1 个 class）".into());
    }

    // 2) 读 GeoJSON → Vec<SegmentWithConnectors>
    println!("[1/5] 读取 GeoJSON: {}", parsed.geojson.display());
    let gj_text =
        std::fs::read_to_string(&parsed.geojson).map_err(|e| format!("读 GeoJSON 失败: {e}"))?;
    let fc = parse_geojson_str(&gj_text).map_err(|e| format!("GeoJSON 解析失败: {e}"))?;
    let total_features = fc.features.len();
    let all_segments = parse_geojson_segments_with_connectors(&fc);
    println!(
        "      总 features={}  含 connector 的段={}",
        total_features,
        all_segments.len()
    );

    // 3) 按 --road-class 白名单过滤
    let segments: Vec<SegmentWithConnectors> = if parsed.skip_edges {
        Vec::new()
    } else {
        all_segments
            .into_iter()
            .filter(|s| {
                parsed
                    .road_class
                    .iter()
                    .any(|c| s.road_class.eq_ignore_ascii_case(c))
            })
            .collect()
    };
    println!(
        "      过滤后保留 {} 段（road-class={}）",
        segments.len(),
        parsed.road_class.join(",")
    );

    // 4) 收集 connector 端点去重 → 自动建 Place（路口）。
    // 用 short_id 派生 Id：UUID 去掉 `-` 取前 12 位 hex，加 `place_int_` 前缀。
    // 坐标取首见的 LineString 端点（同一 connector 被多段共享，首见即代表）；
    // 路口必须锚定 map + x/y，否则道路边投影不出线段（广东省导入缺口修）。
    let mut connector_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut intersection_places: Vec<(String, String, f64, f64)> = Vec::new(); // (id, name, lon, lat)
    for s in &segments {
        if connector_seen.insert(s.connector_a.clone()) {
            intersection_places.push((
                format!("place_int_{}", short_id_from_uuid(&s.connector_a)),
                "路口".to_string(),
                s.a_lon,
                s.a_lat,
            ));
        }
        if connector_seen.insert(s.connector_b.clone()) {
            intersection_places.push((
                format!("place_int_{}", short_id_from_uuid(&s.connector_b)),
                "路口".to_string(),
                s.b_lon,
                s.b_lat,
            ));
        }
    }
    println!(
        "      路口 Place 数：{}（来自 {} 段）",
        intersection_places.len(),
        segments.len()
    );

    // 5) 构造 World
    println!(
        "[2/5] 构造 World（root map + 21 城 + {} 路口 + {} 边）",
        intersection_places.len(),
        segments.len()
    );
    let world = build_world(&parsed, &cities, &intersection_places, &segments);

    // 6) 落盘
    println!("[3/5] 写 world.yaml: {}", parsed.target.display());
    wv_io::Project::create(&parsed.target, &parsed.name)
        .map_err(|e| format!("Project::create 失败: {e}"))?;
    let mut project =
        wv_io::Project::open(&parsed.target).map_err(|e| format!("Project::open 失败: {e}"))?;
    println!("[4/5] save_world（全量）…");
    project
        .save_world(&world, &[])
        .map_err(|e| format!("save_world 失败: {e}"))?;
    // 4.5) **关键性能步** — 把 `distances/{places,edges}/<id>.yaml` × 169k
    //      个散文件合并为单文件 `distances/places.yaml` + `distances/edges.yaml`。
    //      不然 GUI 启动时 `load_world` 要逐文件 open+parse 169k 次，秒级 → 分钟级。
    //      `wv-io` 的 loader 已有回退：目录空时读单文件（`project.rs:641-648`），
    //      落盘就绪后 GUI 加载是 O(1) 次 read + 1 次 deserial。
    compact_distances_to_single_file(&parsed.target, &world.distances)?;

    // 7) 收尾统计
    println!("[5/5] 完成。");
    println!("  工程目录:    {}", parsed.target.display());
    println!("  root_map:   {}", ROOT_MAP_ID);
    println!("  城市子图:    {}", cities.len());
    println!("  路口 Place:  {}", intersection_places.len());
    println!("  PlaceEdge:   {}", segments.len());
    println!("  City:        {}", cities.len());
    println!();
    println!("下一步:");
    println!("  wv load {}", parsed.target.display());
    println!("  wv validate {}", parsed.target.display());
    Ok(())
}

/// 工程的 root map id（与 `world.yaml.root_map` 同名）。
const ROOT_MAP_ID: &str = "map_guangdong";

/// 把 `(lat, lon)` 投到 root map 的局部坐标（wu 单位）。
///
/// bbox 是 geographic_source 的经纬度范围；extent 是 root map 的 wu 范围。
/// 返回 `(x, y)`，左上角为原点。
fn project_lat_lon_to_xy(
    lat: f64,
    lon: f64,
    bbox: &wv_core::LatLonBbox,
    extent: &wv_core::map::Extent,
) -> (f32, f32) {
    let u = ((lon - bbox.west) / (bbox.east - bbox.west)).clamp(0.0, 1.0);
    // 纬度越大 → y 越大。世界坐标 +Y 向上（`camera.rs`），所以北在上、南在下
    // （ADR-0034「上北下南」）。2026-10-18 之前这里是 `1 - (lat-south)/range`，
    // 与渲染的 +Y 方向相反，整幅广东在屏幕上上下颠倒。
    let v = ((lat - bbox.south) / (bbox.north - bbox.south)).clamp(0.0, 1.0);
    (u as f32 * extent.width, v as f32 * extent.height)
}

/// 把 Overture UUID 缩短为 12 hex 字符（取 UUID 头三段）——用作 Place.id 后缀。
///
/// UUID 形如 `79907159-398d-4f15-ae7a-ac4035e08c83`，头三段是
/// `79907159398d4f15ae7aac4035e08c83` 的前 12 字符。`[A-Za-z0-9_]` 字符集 OK。
fn short_id_from_uuid(uuid: &str) -> String {
    uuid.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(12)
        .collect()
}

/// 构造完整的 World（root map + 21 城子图 + Places + PlaceEdges）。
#[allow(clippy::too_many_lines)]
fn build_world(
    args: &ApplyArgs,
    cities: &[(String, String, f64, f64)],
    intersection_places: &[(String, String, f64, f64)],
    segments: &[SegmentWithConnectors],
) -> wv_core::World {
    // ---- 1) 计算 bbox（取城市 min/max 经纬度，外扩 0.5° 缓冲）---------
    let bbox = compute_bbox(cities);
    let extent_root = wv_core::map::Extent::sized(4096.0, 4096.0);
    let extent_city = wv_core::map::Extent::sized(1024.0, 1024.0);

    // ---- 2) 构造 root map ------------------------------------------------
    let mut root = wv_core::map::MapNode {
        id: wv_core::Id::new_unchecked(ROOT_MAP_ID),
        name: args.name.clone(),
        level: wv_core::map::MapLevel::World,
        extent: Some(extent_root),
        base: Some(wv_core::map::MapBase::Ocean),
        ..Default::default()
    };
    root.geographic_source = Some(wv_core::GeographicSourceV1 {
        theme: "transportation".into(),
        bbox,
        data_path: relative_geojson_path(args),
        release: args.release_tag(),
    });
    root.layers = vec![wv_core::map::Layer {
        id: wv_core::Id::new_unchecked("layer_terrain"),
        name: "地形".into(),
        kind: wv_core::map::LayerKind::Terrain,
        visible: true,
        locked: false,
        opacity: 1.0,
        z: 0,
        source: Some(format!("maps/{ROOT_MAP_ID}/terrain.bin")),
    }];

    // ---- 3) 构造 21 个城市子图 + IconSpec + children ---------------------
    let mut maps: Vec<wv_core::map::MapNode> = vec![root];
    let mut places: Vec<wv_core::distance::Place> =
        Vec::with_capacity(cities.len() + intersection_places.len());
    for (id, name, lat, lon) in cities {
        let (x, y) = project_lat_lon_to_xy(*lat, *lon, &bbox, &extent_root);
        let wu_radius = 96.0; // ≈ 城图标在 root map 上的「半径」（~2.3% extent）
        let half = wu_radius;

        // outline_polygon：围绕 (x, y) 的正方形（4 顶点顺时针）
        let poly = wv_core::map::GeometryData::Polygon(vec![
            x - half,
            y - half,
            x + half,
            y - half,
            x + half,
            y + half,
            x - half,
            y + half,
        ]);

        let icon_id_str = format!("icon_{id}");
        maps.push(wv_core::map::MapNode {
            id: wv_core::Id::new_unchecked(format!("city_{id}")),
            name: name.clone(),
            level: wv_core::map::MapLevel::City,
            parent: Some(wv_core::Id::new_unchecked(ROOT_MAP_ID)),
            children: Vec::new(),
            icons: Vec::new(), // 由 root 的 icons 引用
            layers: Vec::new(),
            extent: Some(extent_city),
            raster: None,
            geographic_source: None,
            time_span: None,
            unit_per_km: None,
            orientation: 0.0,
            base: Some(wv_core::map::MapBase::Land),
            outline_polygon: Some(poly),
        });

        // 挂 IconSpec 到 root.icons + children
        let root = maps.first_mut().expect("root 在首位");
        root.children
            .push(wv_core::Id::new_unchecked(format!("city_{id}")));
        root.icons.push(wv_core::map::IconSpec {
            id: wv_core::Id::new_unchecked(icon_id_str),
            anchor: Some([x / extent_root.width, y / extent_root.height]),
            shape: wv_core::map::IconShape::Diamond,
            size: Some(14.0),
            color: None,
            drill_into: Some(wv_core::Id::new_unchecked(format!("city_{id}"))),
        });

        // City Place（带 map + x/y + w/h 锚定）
        let place_id = format!("place_{id}");
        let place = wv_core::distance::Place {
            kind: None,
            line: None,
            id: wv_core::Id::new_unchecked(&place_id),
            name: name.clone(),
            feature: None,
            biome: None,
            envelope: None,
            map: Some(wv_core::Id::new_unchecked(ROOT_MAP_ID)),
            x: Some(x),
            y: Some(y),
            w: Some(wu_radius * 2.0),
            h: Some(wu_radius * 2.0),
            rank: Some(3),
        };
        places.push(place);
    }

    // ---- 4) 路口 Place（锚定 root map + x/y——道路边投影的坐标源）---------
    // 无 w/h：不占范围、不当城市 POI（`is_city = false`，随 LOD 淡出）。
    for (id, name, lon, lat) in intersection_places {
        let (x, y) = project_lat_lon_to_xy(*lat, *lon, &bbox, &extent_root);
        places.push(wv_core::distance::Place {
            kind: None,
            line: None,
            id: wv_core::Id::new_unchecked(id),
            name: name.clone(),
            feature: None,
            biome: None,
            envelope: None,
            map: Some(wv_core::Id::new_unchecked(ROOT_MAP_ID)),
            x: Some(x),
            y: Some(y),
            w: None,
            h: None,
            rank: Some(0),
        });
    }

    // ---- 5) PlaceEdge -----------------------------------------------------
    let mut edges: Vec<wv_core::distance::PlaceEdge> = Vec::with_capacity(segments.len());
    for s in segments {
        let a = format!("place_int_{}", short_id_from_uuid(&s.connector_a));
        let b = format!("place_int_{}", short_id_from_uuid(&s.connector_b));
        // 同一对 (a, b) 可能有多个 segment 共享——id 加序号避免撞 ID。
        let edge_id = format!(
            "edge_{}__{}_at_{}",
            a,
            b,
            s.overture_id
                .as_ref()
                .map(|o| o.as_str().to_string())
                .unwrap_or_else(|| "unk".into())
        );
        edges.push(wv_core::distance::PlaceEdge {
            rank: None,
            id: wv_core::Id::new_unchecked(edge_id),
            a: wv_core::Id::new_unchecked(a),
            b: wv_core::Id::new_unchecked(b),
            distance_km: s.distance_km,
            terrain: s.terrain.clone(),
            bidirectional: true,
        });
    }

    // ---- 6) WorldMeta ----------------------------------------------------
    let meta = wv_core::WorldMeta {
        name: args.name.clone(),
        version: "0.1".into(),
        schema_version: wv_io::CURRENT_SCHEMA_VERSION,
        root_map: wv_core::Id::new_unchecked(ROOT_MAP_ID),
        timeline_range: wv_core::TimeSpan::new(1, 2).expect("TimeSpan::new(1,2)"),
        template: None,
    };

    let distances = wv_core::distance::DistanceGraph {
        places,
        edges,
        vehicles: Vec::new(),
    };

    wv_core::World {
        meta: Some(meta),
        maps,
        timeline: None,
        characters: Vec::new(),
        events: Vec::new(),
        relations: Vec::new(),
        distances,
        seas: Vec::new(),
        situations: Vec::new(),
        units: Vec::new(),
        tracks: Vec::new(),
        narrations: Vec::new(),
        factions: Vec::new(),
        faction_layers: Vec::new(),
        faction_geometry: Vec::new(),
        place_geometry: Vec::new(),
        regions: Vec::new(),
        terrains: std::collections::BTreeMap::new(),
        streets: std::collections::BTreeMap::new(),
        measurements: std::collections::BTreeMap::new(),
        buildings: std::collections::BTreeMap::new(),
    }
}

/// 计算 bbox：城市 min/max 经纬度外扩 0.5°。
fn compute_bbox(cities: &[(String, String, f64, f64)]) -> wv_core::LatLonBbox {
    let pad = 0.5;
    let mut west = f64::INFINITY;
    let mut east = f64::NEG_INFINITY;
    let mut south = f64::INFINITY;
    let mut north = f64::NEG_INFINITY;
    for (_id, _name, lat, lon) in cities {
        west = west.min(*lon);
        east = east.max(*lon);
        south = south.min(*lat);
        north = north.max(*lat);
    }
    wv_core::LatLonBbox {
        west: (west - pad).max(-180.0),
        east: (east + pad).min(180.0),
        south: (south - pad).max(-90.0),
        north: (north + pad).min(90.0),
    }
}

/// `--geojson` 在工程目录里的相对路径（写到 `geographic_source.data_path`）。
fn relative_geojson_path(args: &ApplyArgs) -> std::path::PathBuf {
    // 取参数路径相对 target 的差；若 geojson 就在 target/overture/ 下，路径更短。
    let abs_geo = &args.geojson;
    let abs_target = &args.target;
    if let Ok(rel) = abs_geo.strip_prefix(abs_target) {
        rel.to_path_buf()
    } else {
        abs_geo.clone()
    }
}

/// 把 `distances/{places,edges}/<id>.yaml` × N 个散文件合并为单文件 bulk YAML。
///
/// ## 为什么
/// `wv-io::Project::save_world` 默认按「每实体一个 YAML」写入（§4.5 原则）；
/// 对 Overture 量级（81k places + 87k edges = 169k 文件），GUI 启动时
/// `load_world` 要逐个 open + parse + close，秒级操作滑到分钟级，**打不开窗口**。
///
/// `wv-io::Project::load_world` 已支持回退：`distances/places/` 目录空时
/// 读单文件 `distances/places.yaml`（`project.rs:641-648`）。所以合并策略是：
/// 1. 写 bulk 单文件（顶 YAML 列表 = `Vec<T>` 的 serde 形态）；
/// 2. 删 `distances/{places,edges,vehicles}/` 目录——下次 load 走回退路径。
///
/// ## 不做
/// - **不**改 `wv-io::save_world` 的全局行为——per-entity 仍是 Overture 之外
///   的默认（作者手工建工程、几十个 Place 的常态）；只在大批量导入时用本函数。
fn compact_distances_to_single_file(
    target: &Path,
    distances: &wv_core::distance::DistanceGraph,
) -> Result<(), String> {
    let distances_dir = target.join("distances");
    if !distances_dir.is_dir() {
        return Err(format!(
            "distances/ 目录不存在：{}",
            distances_dir.display()
        ));
    }

    // 1) places 单文件
    let places_yaml =
        serde_yaml::to_string(&distances.places).map_err(|e| format!("序列化 places 失败: {e}"))?;
    let places_path = distances_dir.join("places.yaml");
    std::fs::write(&places_path, &places_yaml)
        .map_err(|e| format!("写 {} 失败: {e}", places_path.display()))?;

    // 2) edges 单文件
    let edges_yaml =
        serde_yaml::to_string(&distances.edges).map_err(|e| format!("序列化 edges 失败: {e}"))?;
    let edges_path = distances_dir.join("edges.yaml");
    std::fs::write(&edges_path, &edges_yaml)
        .map_err(|e| format!("写 {} 失败: {e}", edges_path.display()))?;

    // 3) vehicles（空数组也写——避免 loader 走回退时 N/A）
    let vehicles_yaml = serde_yaml::to_string(&distances.vehicles)
        .map_err(|e| format!("序列化 vehicles 失败: {e}"))?;
    let vehicles_path = distances_dir.join("vehicles.yaml");
    std::fs::write(&vehicles_path, &vehicles_yaml)
        .map_err(|e| format!("写 {} 失败: {e}", vehicles_path.display()))?;

    // 4) 删 per-entity 目录——下次 load_world 走「目录空 → 单文件」回退。
    //    remove_dir_all 在目录不存在时返回 Err，用 `.ok()` 吞掉「已删」的边缘场景。
    std::fs::remove_dir_all(distances_dir.join("places")).ok();
    std::fs::remove_dir_all(distances_dir.join("edges")).ok();
    std::fs::remove_dir_all(distances_dir.join("vehicles")).ok();

    Ok(())
}

/// 从 `--cities` YAML 加载城市列表。
///
/// schema（简化版，仅本 CLI 用）：
/// ```yaml
/// cities:
///   - id: guangzhou
///     name: 广州市
///     lat: 23.12911
///     lon: 113.26439
/// ```
fn load_cities_from_yaml(path: &Path) -> Result<Vec<(String, String, f64, f64)>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("读 --cities YAML 失败 {}: {e}", path.display()))?;
    #[derive(serde::Deserialize)]
    struct YamlCity {
        id: String,
        name: String,
        lat: f64,
        lon: f64,
    }
    #[derive(serde::Deserialize)]
    struct YamlDoc {
        cities: Vec<YamlCity>,
    }
    let doc: YamlDoc =
        serde_yaml::from_str(&text).map_err(|e| format!("解析 --cities YAML 失败: {e}"))?;
    Ok(doc
        .cities
        .into_iter()
        .map(|c| (c.id, c.name, c.lat, c.lon))
        .collect())
}

const APPLY_HELP: &str = "\
用法: wv overture apply --geojson <path> --target <dir>
            [--name <name>] [--cities <yaml>]
            [--road-class motorway,trunk] [--skip-edges]

把已下载 Overture GeoJSON 转成可加载工程目录：
  - 21 个广东地级市子图（默认；可 --cities 覆盖）
  - 主要公路 PlaceEdge（motorway+trunk；按 --road-class 调整）
  - 路口 Place（Overture connector_id 去重，按频次 top-N 截断）

参数:
  --geojson <path>     已下载 Overture GeoJSON 路径（必填）
  --target <dir>       目标工程目录（必填；已含 world.yaml 则拒绝）
  --name <name>        工程名（默认「广东省」）
  --cities <yaml>      覆盖默认 21 城的 YAML
  --road-class <list>  逗号分隔的 road class 白名单（默认 motorway,trunk）
  --skip-edges         只建工程骨架不导边

依赖: wv-overture fetch 已成功跑过（提供 --geojson）
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bbox_accepts_well_formed_input() {
        let b = parse_bbox("-122.50,47.50,-122.20,47.75").unwrap();
        assert!((b.west - -122.50).abs() < 1e-9);
        assert!((b.south - 47.50).abs() < 1e-9);
        assert!((b.east - -122.20).abs() < 1e-9);
        assert!((b.north - 47.75).abs() < 1e-9);
    }

    #[test]
    fn parse_bbox_rejects_wrong_segment_count() {
        assert!(parse_bbox("1,2,3").is_err());
        assert!(parse_bbox("1,2,3,4,5").is_err());
    }

    #[test]
    fn parse_bbox_rejects_non_numeric() {
        assert!(parse_bbox("a,b,c,d").is_err());
        assert!(parse_bbox("1,2,3,not_a_number").is_err());
    }

    #[test]
    fn fetch_args_parses_all_required_flags() {
        let args = vec![
            "--bbox".to_string(),
            "-122.5,47.5,-122.2,47.75".into(),
            "--theme".into(),
            "transportation".into(),
            "--out".into(),
            "data/seattle_segments.geojson".into(),
        ];
        let parsed = FetchArgs::parse(&args).expect("合法参数应通过");
        assert!(parsed.bbox.is_some(), "下载模式应有 bbox");
        assert!((parsed.bbox.unwrap().west - -122.5).abs() < 1e-9);
        assert_eq!(parsed.theme.as_deref(), Some("transportation"));
        assert!(parsed.in_path.is_none());
        assert_eq!(parsed.out.to_str(), Some("data/seattle_segments.geojson"));
        assert!(parsed.release.is_none());
    }

    #[test]
    fn fetch_args_rejects_missing_required_flag_in_download_mode() {
        // 下载模式（无 --in），三标志缺一即失败
        let no_bbox = vec![
            "--theme".into(),
            "places".into(),
            "--out".into(),
            "x".into(),
        ];
        assert!(FetchArgs::parse(&no_bbox).is_err(), "--bbox 必填");

        let no_theme = vec![
            "--bbox".into(),
            "-122.5,47.5,-122.2,47.75".into(),
            "--out".into(),
            "x".into(),
        ];
        assert!(FetchArgs::parse(&no_theme).is_err(), "--theme 必填");

        let no_out = vec![
            "--bbox".into(),
            "-122.5,47.5,-122.2,47.75".into(),
            "--theme".into(),
            "places".into(),
        ];
        assert!(FetchArgs::parse(&no_out).is_err(), "--out 必填");
    }

    #[test]
    fn fetch_args_accepts_in_only_mode_without_bbox() {
        let v = vec![
            "--in".to_string(),
            "already_downloaded.geojson".into(),
            "--out".into(),
            "x".into(),
        ];
        let parsed = FetchArgs::parse(&v).expect("仅转换模式合法");
        assert_eq!(
            parsed.in_path.as_ref().and_then(|p| p.to_str()),
            Some("already_downloaded.geojson")
        );
        assert!(parsed.bbox.is_none());
        assert!(parsed.theme.is_none());
    }

    #[test]
    fn fetch_args_rejects_in_with_bbox_as_mutually_exclusive() {
        // 两模式不能同时启用（防止命令语义混乱）
        let v = vec![
            "--in".into(),
            "x.geojson".into(),
            "--bbox".into(),
            "-122.5,47.5,-122.2,47.75".into(),
            "--theme".into(),
            "places".into(),
            "--out".into(),
            "y".into(),
        ];
        assert!(FetchArgs::parse(&v).is_err(), "--in 与 --bbox 互斥");
    }

    #[test]
    fn fetch_args_rejects_in_with_theme_only() {
        let v = vec![
            "--in".into(),
            "x.geojson".into(),
            "--theme".into(),
            "places".into(),
            "--out".into(),
            "y".into(),
        ];
        assert!(FetchArgs::parse(&v).is_err(), "--in 与 --theme 也互斥");
    }

    #[test]
    fn fetch_args_rejects_in_without_out() {
        let v = vec!["--in".into(), "x.geojson".into()];
        assert!(FetchArgs::parse(&v).is_err(), "--out 必填");
    }

    #[test]
    fn fetch_args_rejects_flag_without_value() {
        let v = vec!["--bbox".into()];
        assert!(FetchArgs::parse(&v).is_err(), "--bbox 缺值应被拒绝");
    }

    #[test]
    fn fetch_args_rejects_unknown_flag() {
        let v = vec!["--unknown".into(), "x".into()];
        assert!(FetchArgs::parse(&v).is_err());
    }

    #[test]
    fn fetch_args_accepts_optional_release() {
        let v = vec![
            "--bbox".into(),
            "-122.5,47.5,-122.2,47.75".into(),
            "--theme".into(),
            "transportation".into(),
            "--release".into(),
            "2026-09-23.0".into(),
            "--out".into(),
            "x".into(),
        ];
        let parsed = FetchArgs::parse(&v).unwrap();
        assert_eq!(parsed.release.as_deref(), Some("2026-09-23.0"));
    }

    #[test]
    fn fetch_args_accepts_emit_yaml_flag() {
        let v = vec![
            "--in".into(),
            "x.geojson".into(),
            "--out".into(),
            "y".into(),
            "--emit-yaml".into(),
        ];
        let parsed = FetchArgs::parse(&v).unwrap();
        assert!(parsed.emit_yaml, "--emit-yaml 应被解析");
    }

    #[test]
    fn fetch_args_defaults_emit_yaml_to_false() {
        let v = vec![
            "--in".into(),
            "x.geojson".into(),
            "--out".into(),
            "y".into(),
        ];
        let parsed = FetchArgs::parse(&v).unwrap();
        assert!(!parsed.emit_yaml, "默认不 emit");
    }

    // ---- V3: candidates_to_edge_yaml 测试 ----

    fn cand_with_id(id: &str) -> wv_overture::transportation::PlaceEdgeCandidate {
        wv_overture::transportation::PlaceEdgeCandidate {
            distance_km: 0.137,
            terrain: "road".into(),
            overture_id: wv_core::OvertureId::new(id).ok(),
        }
    }

    #[test]
    fn yaml_uses_overture_id_first_segment_for_edge_id() {
        let cands = vec![cand_with_id("50e76fae-ac7e-448a-b34d-fcb4db17b5ee")];
        let yaml = candidates_to_edge_yaml(&cands);
        // UUID 第一个段是 `50e76fae`（8 字符）—— 测试拼写而非绕序。
        assert!(yaml.contains("id: overture_seg_50e76fae"));
        assert!(yaml.contains("PLACE_ID:PLACE_REPLACE_ME_A"));
        assert!(yaml.contains("PLACE_ID:PLACE_REPLACE_ME_B"));
        assert!(yaml.contains("distance_km: 0.137"));
        assert!(yaml.contains("terrain: road"));
    }

    #[test]
    fn yaml_falls_back_to_unknown_index_when_overture_id_invalid() {
        let mut c = cand_with_id("bad");
        c.overture_id = None;
        let cands = vec![c];
        let yaml = candidates_to_edge_yaml(&cands);
        assert!(
            yaml.contains("overture_seg_unknown_0"),
            "无 UUID 应降级为 unknown_0：got {yaml}"
        );
    }

    #[test]
    fn yaml_emits_one_block_per_candidate() {
        let cands = vec![
            cand_with_id("11111111-2222-3333-4444-555555555555"),
            cand_with_id("22222222-3333-4444-5555-666666666666"),
            cand_with_id("33333333-4444-5555-6666-777777777777"),
        ];
        let yaml = candidates_to_edge_yaml(&cands);
        // 每个 edge 有 A + B 两个 PLACE_ID 占位（行内 + 注释共 2 次），故 A 出现 2N 次，B 出现 2N 次。
        assert_eq!(yaml.matches("PLACE_ID:PLACE_REPLACE_ME_A").count(), 6);
        assert_eq!(yaml.matches("PLACE_ID:PLACE_REPLACE_ME_B").count(), 6);
        assert_eq!(yaml.matches("id: overture_seg_").count(), 3);
    }

    // ---- Y2: index 子命令参数解析 ----

    #[test]
    fn index_args_requires_in() {
        let v = vec!["--id".into(), "abc".into()];
        assert!(IndexArgs::parse(&v).is_err(), "--in 必填");
    }

    #[test]
    fn index_args_parses_in_only() {
        let v = vec!["--in".into(), "x.geojson".into()];
        let p = IndexArgs::parse(&v).unwrap();
        assert_eq!(p.in_path.to_str(), Some("x.geojson"));
        assert!(p.lookup_id.is_none());
        assert!(p.lookup_label.is_none());
    }

    #[test]
    fn index_args_parses_all_flags() {
        let v = vec![
            "--in".into(),
            "x.geojson".into(),
            "--id".into(),
            "50e76fae-ac7e-448a-b34d-fcb4db17b5ee".into(),
            "--label".into(),
            "viewport_seattle".into(),
        ];
        let p = IndexArgs::parse(&v).unwrap();
        assert_eq!(
            p.lookup_id.as_deref(),
            Some("50e76fae-ac7e-448a-b34d-fcb4db17b5ee")
        );
        assert_eq!(p.lookup_label.as_deref(), Some("viewport_seattle"));
    }

    #[test]
    fn index_args_rejects_unknown_flag() {
        let v = vec!["--in".into(), "x".into(), "--zoom".into(), "5".into()];
        assert!(IndexArgs::parse(&v).is_err());
    }

    // --- apply subcommand tests ------------------------------------------

    #[test]
    fn apply_args_requires_geojson_and_target() {
        assert!(ApplyArgs::parse(&[]).is_err());
        let only_geo = vec!["--geojson".into(), "x.geojson".into()];
        assert!(ApplyArgs::parse(&only_geo).is_err(), "缺 --target 应拒");
    }

    #[test]
    fn apply_args_defaults_name_and_road_class() {
        let v = vec![
            "--geojson".into(),
            "x.geojson".into(),
            "--target".into(),
            "out".into(),
        ];
        let a = ApplyArgs::parse(&v).expect("parse ok");
        assert_eq!(a.name, "广东省");
        assert_eq!(
            a.road_class,
            vec!["motorway".to_string(), "trunk".to_string()]
        );
        assert!(!a.skip_edges);
    }

    #[test]
    fn apply_args_parses_road_class_list() {
        let v = vec![
            "--geojson".into(),
            "x.geojson".into(),
            "--target".into(),
            "out".into(),
            "--road-class".into(),
            "motorway, trunk , primary".into(),
        ];
        let a = ApplyArgs::parse(&v).expect("parse ok");
        assert_eq!(
            a.road_class,
            vec![
                "motorway".to_string(),
                "trunk".to_string(),
                "primary".to_string(),
            ]
        );
    }

    #[test]
    fn apply_args_parses_skip_edges_flag() {
        let v = vec![
            "--geojson".into(),
            "x.geojson".into(),
            "--target".into(),
            "out".into(),
            "--skip-edges".into(),
        ];
        let a = ApplyArgs::parse(&v).expect("parse ok");
        assert!(a.skip_edges);
    }

    #[test]
    fn apply_args_rejects_empty_name() {
        let v = vec![
            "--geojson".into(),
            "x.geojson".into(),
            "--target".into(),
            "out".into(),
            "--name".into(),
            "".into(),
        ];
        assert!(ApplyArgs::parse(&v).is_err());
    }

    #[test]
    fn apply_args_rejects_unknown_flag() {
        let v = vec![
            "--geojson".into(),
            "x.geojson".into(),
            "--target".into(),
            "out".into(),
            "--bogus".into(),
        ];
        assert!(ApplyArgs::parse(&v).is_err());
    }

    #[test]
    fn default_guangdong_cities_has_21_unique_ids() {
        assert_eq!(DEFAULT_GUANGDONG_CITIES.len(), 21, "广东应恰好 21 个地级市");
        let mut seen = std::collections::HashSet::new();
        for c in DEFAULT_GUANGDONG_CITIES {
            assert!(seen.insert(c.id), "城市 id 重复: {}", c.id);
            assert!(!c.id.is_empty());
            assert!(!c.name.is_empty());
            // 字符集 §4.4：[A-Za-z0-9_]
            for ch in c.id.chars() {
                assert!(
                    ch.is_ascii_alphanumeric() || ch == '_',
                    "id 含非法字符: {ch}"
                );
            }
        }
    }

    #[test]
    fn default_guangdong_cities_lie_within_province_bbox() {
        // bbox = 109.5,20.2,117.3,25.5（与 GeoJSON 下载 bbox 一致）
        for c in DEFAULT_GUANGDONG_CITIES {
            assert!(
                (109.5..=117.3).contains(&c.lon),
                "{} lon {} 越界",
                c.id,
                c.lon
            );
            assert!(
                (20.2..=25.5).contains(&c.lat),
                "{} lat {} 越界",
                c.id,
                c.lat
            );
        }
    }

    // 以下三条 2026-10-26 用户裁定暂时忽略：受限执行环境（agent 派生进程）
    // 写盘被安全软件门控，`os error 5` 假失败（AGENTS §2.2 环境约束），
    // 每轮门禁必红且无法在会话内根治。正常桌面终端可 `--ignored` 复核。
    #[test]
    #[ignore = "受限环境 temp 写盘 os error 5 假失败（AGENTS §2.2）；桌面终端 --ignored 可跑"]
    fn apply_command_skeleton_rejects_missing_geojson_file() {
        // --geojson 指向不存在的文件 → 拒绝（不在静默创建）。
        let dir =
            std::env::temp_dir().join(format!("wv_apply_test_missing_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("project");
        let v = vec![
            "--geojson".into(),
            dir.join("nope.geojson").to_string_lossy().into(),
            "--target".into(),
            target.to_string_lossy().into(),
        ];
        let err = apply_command(&v).expect_err("missing geojson 应拒");
        assert!(err.contains("--geojson"), "错误信息应提 --geojson：{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[ignore = "受限环境 temp 写盘 os error 5 假失败（AGENTS §2.2）；桌面终端 --ignored 可跑"]
    fn apply_command_skeleton_rejects_existing_target() {
        // --target 已含 world.yaml → 拒绝覆盖。
        let dir =
            std::env::temp_dir().join(format!("wv_apply_test_existing_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 1) 先用一个有效 geojson（哪怕是空 FeatureCollection）骗过 --geojson 校验
        let geojson = dir.join("fake.geojson");
        std::fs::write(&geojson, r#"{"type":"FeatureCollection","features":[]}"#).unwrap();
        // 2) target 里放一个 world.yaml 模拟「已建过工程」
        let target = dir.join("project");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("world.yaml"), "name: x\nschema_version: 2\n").unwrap();

        let v = vec![
            "--geojson".into(),
            geojson.to_string_lossy().into(),
            "--target".into(),
            target.to_string_lossy().into(),
        ];
        let err = apply_command(&v).expect_err("已含 world.yaml 应拒");
        assert!(err.contains("world.yaml"), "错误信息应提 world.yaml：{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 端到端集成：拿一份手工 Overture GeoJSON fixture → apply → 验目录结构。
    ///
    /// 测的是「apply 全流程能写出可加载的工程目录」：
    /// - world.yaml 写入且 schema_version = 2
    /// - maps/map_<root>/index.yaml 写入且含 `geographic_source`
    /// - 21 个 city_<id>/index.yaml 都存在
    /// - distances/places/place_<id>.yaml 写入（至少 21 条城市 Place）
    /// - distances/edges/edge_*.yaml 写入（motorway 段）
    /// - 重开 `Project::open` 成功 = 序列化 round-trip OK
    #[test]
    #[ignore = "受限环境 temp 写盘 os error 5 假失败（AGENTS §2.2）；桌面终端 --ignored 可跑"]
    fn apply_command_writes_loadable_guangdong_like_project() {
        // 用一个最小但合法的 Overture segment GeoJSON fixture：
        // 2 段 motorway 直线（绕开百度深圳一带），覆盖 4 个 connector。
        let fixture = r#"{
            "type": "FeatureCollection",
            "features": [
                {
                    "id": "11111111-1111-1111-1111-111111111111",
                    "type": "Feature",
                    "geometry": {
                        "type": "LineString",
                        "coordinates": [[113.0, 23.0], [113.5, 23.5]]
                    },
                    "properties": {
                        "subtype": "road",
                        "class": "motorway",
                        "connectors": [
                            {"connector_id": "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa", "at": 0.0},
                            {"connector_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", "at": 1.0}
                        ]
                    }
                },
                {
                    "id": "22222222-2222-2222-2222-222222222222",
                    "type": "Feature",
                    "geometry": {
                        "type": "LineString",
                        "coordinates": [[113.5, 23.5], [114.0, 24.0]]
                    },
                    "properties": {
                        "subtype": "road",
                        "class": "trunk",
                        "connectors": [
                            {"connector_id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb", "at": 0.0},
                            {"connector_id": "cccccccc-cccc-cccc-cccc-cccccccccccc", "at": 1.0}
                        ]
                    }
                }
            ]
        }"#;

        let dir = std::env::temp_dir().join(format!("wv_apply_e2e_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let geojson = dir.join("fixture.geojson");
        std::fs::write(&geojson, fixture).unwrap();
        let target = dir.join("proj");
        std::fs::create_dir_all(&target).unwrap();

        let v = vec![
            "--geojson".into(),
            geojson.to_string_lossy().into(),
            "--target".into(),
            target.to_string_lossy().into(),
        ];
        apply_command(&v).expect("apply 应成功");

        // 1) world.yaml 存在且 schema_version = 2
        let world_yaml = target.join("world.yaml");
        assert!(world_yaml.is_file(), "world.yaml 应落盘");
        let wtext = std::fs::read_to_string(&world_yaml).unwrap();
        assert!(
            wtext.contains("schema_version: 2"),
            "schema_version 必须为 2"
        );

        // 2) root map + 21 city submaps
        let root_index = target.join("maps/map_guangdong/index.yaml");
        assert!(root_index.is_file(), "root map index.yaml 应在");
        let rtext = std::fs::read_to_string(&root_index).unwrap();
        assert!(
            rtext.contains("geographic_source"),
            "root 应带 geographic_source"
        );
        assert!(
            rtext.contains("theme: transportation"),
            "theme 应为 transportation"
        );

        for c in DEFAULT_GUANGDONG_CITIES {
            let city_idx = target.join(format!("maps/city_{}/index.yaml", c.id));
            assert!(city_idx.is_file(), "city_{} index.yaml 应在", c.id);
        }

        // 3) 21 个城市 Place — 走 bulk 单文件（compact pass 已合并）
        let places_yaml = target.join("distances/places.yaml");
        assert!(places_yaml.is_file(), "places.yaml 单文件应落盘");
        let ptext = std::fs::read_to_string(&places_yaml).unwrap();
        for c in DEFAULT_GUANGDONG_CITIES {
            assert!(
                ptext.contains(&format!("id: place_{}", c.id)),
                "places.yaml 应含 place_{}",
                c.id
            );
        }
        // 4 个路口 Place 也要在，且**锚定 root map 带坐标**（道路边投影源）
        assert!(
            ptext.contains("place_int_aaaaaaaa"),
            "应含 4 个路口 place_int_"
        );
        {
            let blocks: Vec<&str> = ptext
                .split("- id: ")
                .filter(|b| b.starts_with("place_int_"))
                .collect();
            assert_eq!(
                blocks.len(),
                3,
                "fixture 去重后 3 个路口，got {}",
                blocks.len()
            );
            for b in &blocks {
                assert!(b.contains("x:"), "路口必须有 x 坐标：{b}");
                assert!(b.contains("y:"), "路口必须有 y 坐标：{b}");
                assert!(
                    b.contains(&format!("map: {ROOT_MAP_ID}")),
                    "路口必须锚定 root map：{b}"
                );
            }
        }
        // 关键性能断言：per-entity 目录**不应**存在
        assert!(
            !target.join("distances/places").exists(),
            "distances/places/ 目录应被 compact 删掉（性能关键）"
        );

        // 4) 至少 2 条 PlaceEdge 走 bulk 单文件
        let edges_yaml = target.join("distances/edges.yaml");
        assert!(edges_yaml.is_file(), "edges.yaml 单文件应落盘");
        let etext = std::fs::read_to_string(&edges_yaml).unwrap();
        let edge_count = etext
            .lines()
            .filter(|l| l.trim_start().starts_with("- id: "))
            .count();
        assert!(
            edge_count >= 2,
            "应至少 2 条边（fixture 给了 2 段），实得 {edge_count}"
        );
        assert!(
            !target.join("distances/edges").exists(),
            "distances/edges/ 目录应被 compact 删掉"
        );

        // 5) 重开 Project::open = 序列化 round-trip OK（loader 走单文件回退）
        let project = wv_io::Project::open(&target).expect("重开应成功");
        assert_eq!(project.meta().name, "广东省");
        assert_eq!(project.meta().schema_version, 2);
        // summary() 应含 root_map = map_guangdong
        let summary = project.summary();
        assert!(
            summary.contains("map_guangdong"),
            "summary 应提 root map: {summary}"
        );
        // 关键：load_world 也得能拉出所有 Place / Edge 验证 round-trip
        let world = project.load_world().expect("load_world");
        // fixture 有 3 个独立 connector（a/b/c，b 在两段间共享 → 去重 = 3）
        let loaded_place_count = world
            .distances
            .places
            .iter()
            .filter(|p| p.id.as_str().starts_with("place_int_"))
            .count();
        assert_eq!(
            loaded_place_count, 3,
            "应回收到 3 个路口 place_int（fixture 给了 3 个独立 connector，b 共享）"
        );
        assert_eq!(
            world.distances.edges.len(),
            2,
            "应回收到 2 条边（fixture 给了 2 段）"
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
