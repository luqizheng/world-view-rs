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
use wv_overture::fetch::{parse_geojson_segments, parse_geojson_str};
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
    println!("{FETCH_HELP}\n{INDEX_HELP}");
}

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
}
