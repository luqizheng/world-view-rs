# Overture Maps 外部资源接入指南

> **依据** `AGENTS.md` §0 / §2.1 / §4.3 / §9 + `docs/adr/ADR-0007-overture-real-geography.md`。
> **性质** **基线文档**——回答「是什么 / 怎么用」；ADR-0007 回答「为什么是这个，不是别的」。分工按 `AGENTS.md` §6.4。
> **状态** 2026-10-14 起；与 `README.md` §15-§18、`ADR-0007` 同步维护。

---

## 0. 何时读本文档

- 你想把 Overture Maps 真实地理底图引入工程（西雅图 / 北京 / 任意 bbox）；
- 你想从已下载 GeoJSON 转 `PlaceEdgeCandidate` 候选、或校验候选；
- 你想了解 CLI 子命令的完整参数表、边界与失败模式；
- 你想确认「为什么 CLI 不直写盘」与「写盘应该走哪条路径」。

**本文档不回答**：

- 「为什么选 Overture 不选 OSM PBF」——见 `ADR-0007` §背景；
- 「领域 YAML 该如何引用 OvertureId」——见 `README.md` §15；
- 「schema 升级策略」——见 `README.md` §17。

---

## 1. 概念速览

| 概念 | 定义 | 落盘位置 |
|---|---|---|
| `GeographicSourceV1` | 真实地理底图的元数据声明（theme / bbox / data_path / release）。`Option` 形式挂在 `Map.geographic_source`，`None` 即虚构地图 | 领域 YAML（`maps/<id>/index.yaml`）|
| `OvertureId` | GERS UUID 形态。**§4.4 字符集唯一豁免**——独立 newtype，不可与 `wv_core::Id` 互换 | 领域 YAML（`Place.overture_id`）|
| `PlaceEdgeCandidate` | Overture segment 转换后的中间产物（`distance_km` + `terrain` + 可选 `overture_id`）。**不**是领域模型 | 内存中（CLI 跑完后即丢）|
| `wv-overture` crate | 与 `wv-io` 同级的平行 crate，承载几何转换与索引（零运行依赖） | `crates/wv-overture/` |
| `IndexedOvertures` | 内存 hash 索引：`OvertureId → PlaceEdgeCandidate`、`bbox label → Vec<usize>` | 运行时构造，不落盘（§18「倒排」待裁）|

---

## 2. 完整工作流（端到端）

四个步骤，**前两步可省略**，后两步可独立。

### 2.1 步骤 A：bbox 切片下载（**可选**，仅首次需要）

适用：第一次把某地区引入工程、需要 Overture 最新 schema 的几何数据。

```bash
# Python 子进程触发 Overture Maps 下载（GeoJSON 中间格式）
cargo run -p wv-app -- overture fetch \
    --bbox "-122.50,47.50,-122.20,47.75" \   # WGS84 经纬度，西-南-东-北
    --theme transportation \                  # places / transportation / buildings / divisions / ...
    --out maps/seattle_modern/data/transportation.geojson \
    --release 2026-09-23.0                     # 可选，钉版（**当前未透传给 overturemaps-py**，CLI 暂作记录）
```

依赖前置：

- `pip install overturemaps`（仅首次与按需更新需要）；
- Python ≥ 3.9；
- 网络可达 `overturemaps.org` / S3。

跳过本步骤的条件：已有 GeoJSON 文件（他人分享 / 上次下载 / 离线拷贝）。

### 2.2 步骤 B：GeoJSON → PlaceEdgeCandidate 转换（**离线可做**）

适用：把已下载 GeoJSON 转成候选集合，便于步骤 D 写回领域 YAML。

**两种方式**：

- **命令行模式 A**（边下边转）——步骤 A 与 B 一次跑完；
- **命令行模式 B**（仅转换）——跳过 Python 子进程：

```bash
cargo run -p wv-app -- overture fetch \
    --in maps/seattle_modern/data/transportation.geojson \
    --out maps/seattle_modern/data/transportation.geojson \
    --emit-yaml > maps/seattle_modern/data/edges.draft.yaml
```

输出到 **stdout** 的写回草稿 YAML（**不**直写盘，理由见 §6）。

### 2.3 步骤 C：候选索引与反查（**离线 / CI 友好**）

适用：验证转换结果、按 `OvertureId` 反查、按 bbox label 分区。

```bash
# 统计 + UUID 反查
cargo run -p wv-app -- overture index \
    --in maps/seattle_modern/data/transportation.geojson \
    --id 50e76fae-ac7e-448a-b34d-fcb4db17b5ee

# 输出示例：
#   Overture index 完成：
#     输入文件:     maps/seattle_modern/data/transportation.geojson
#     原始 feature: 137
#     保留 candidate: 89（footway 等已被过滤）
#     by_id 大小:   89
#     lookup by_id 50e76fae-ac7e-...: distance_km=0.047 terrain=road
```

**注意**：CLI 第一版**不预注册** bbox label（`--label` 反查永远返回「未注册」）。需要时由调用方在程序内调用 `IndexedOvertures::register_bbox`。

### 2.4 步骤 D：候选 → 写回领域 YAML（**走 `wv-edit`，不走 CLI**）

**为什么 CLI 不直写盘**——按 `AGENTS.md` §3.3「禁止 System 直接改世界数据」+ §7.5 V1「校验只提示，不阻止」：

- 写盘是「**作者意图**」的领域动作，必须经 `wv_edit::EditCommand` / `History`；
- CLI 输出的 `--emit-yaml` 是**草稿**，由作者审视后决定如何落盘；
- 草稿含 `PLACE_ID:PLACE_REPLACE_ME_A/B` 占位——作者**必须**替换为真实的 `Place.id`（`AGENTS.md` §4.4 字符集）。

**写盘路径**（伪代码）：

```rust
// 在 wv-app 的「Overture 导入」交互入口（不在 CLI）：
use wv_edit::EditCommand;
use wv_core::OvertureId;

// 1. 读候选
let cands = wv_overture::fetch::parse_geojson_str(&geojson_text)?;

// 2. 在 UI 里让作者把 PLACE_REPLACE_ME_A/B 替换为真实 Id
let draft_edges = wv_overture::cli_drafts::candidates_to_edge_yaml(&cands);
// ... 作者编辑 UI ...

// 3. 走 EditCommand（不是 std::fs::write）
let mut history = EditHistory::new();
for edge_yaml in edited_yaml_blocks {
    history.push(EditCommand::AddPlaceEdge(serde_yaml::from_str(edge_yaml)?));
}
history.commit(&mut project.world)?; // 走 wv-edit 的命令队列
```

### 2.5 步骤 E：领域引用 Overture（`overture_id` 字段）

适用：`Place` 想锚定到某个 Overture POI 时。

```yaml
# maps/<id>/places/<place>.yaml
id: space_needle
name: 太空针塔
map: seattle_modern
x: 1234.5
y: 678.9
# Overture 引用（UUID 形态，§4.4 唯一豁免）：
overture_id: 2f099609-47e9-4c3e-bfeb-072ccd609a03
```

运行时按 `OvertureId` 反查几何（只读路径）：

```rust
use wv_overture::index::IndexedOvertures;
use wv_core::OvertureId;

let index = IndexedOvertures::build(candidates);
let id = OvertureId::new("2f099609-47e9-4c3e-bfeb-072ccd609a03")?;
let cand = index.get(&id); // Option<&PlaceEdgeCandidate>
```

---

## 3. 数据流图

```
┌─────────────────┐   bbox 切片下载      ┌────────────────────────┐
│ Overture S3     │  ────────────────►  │ maps/<id>/data/*.geojson │
│ (GeoParquet)    │   CLI step A+B      │  (GeoJSON, 资源层)     │
└─────────────────┘                     └───────────┬────────────┘
                                                     │ CLI step B (parse_geojson_str)
                                                     ▼
┌────────────────────────────────────────────────────────────────┐
│  wv-overture（运行时转换器 / 索引）                            │
│  parse_geojson_str()                                           │
│    → Vec<PlaceEdgeCandidate>                                   │
│       { distance_km, terrain, overture_id }                    │
│  IndexedOvertures::build()                                    │
│    → { by_id: HashMap<OvertureId, usize>, by_bbox: HashMap }  │
│  CLI --emit-yaml                                               │
│    → stdout 写回草稿（含 PLACE_REPLACE_ME_* 占位）             │
└───────────────────────────────┬────────────────────────────────────┘
                                │ 由作者编辑（替换占位）
                                ▼
┌────────────────────────────────────────────────────────────────┐
│  领域模型（YAML 落盘形式）                                       │
│  PlaceEdge { distance_km, terrain }            # 仅参数          │
│  Place.overture_id: OvertureId (UUID)          # 仅引用         │
│                                                                 │
│  ✅ 领域 YAML 里**没有几何**（AGENTS §4.3）                       │
│  ✅ 仅有 custom 字段与引用                                      │
└────────────────────────────────────────────────────────────────┘

反向流（运行时只读）：
  Place.overture_id → IndexedOvertures.get(id) → PlaceEdgeCandidate → 渲染 / 校验
```

---

## 4. CLI 子命令参考

### 4.1 `wv overture fetch`

```
用法: wv overture fetch [--bbox W,S,E,N --theme <t>] | --in <path>
            --out <path> [--release <TAG>] [--emit-yaml]
```

| 模式 | 必填 | 作用 |
|---|---|---|
| 下载模式 | `--bbox` + `--theme` + `--out` | 调 `overturemaps-py` 下载后转换 |
| 仅转换模式 | `--in` + `--out` | 跳过下载，直接转换已有 GeoJSON |

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `--bbox W,S,E,N` | 经纬度字符串 | 下载模式 | WGS84 度，西-南-东-北 |
| `--theme <t>` | 字符串 | 下载模式 | Overture 主题（自由字符串，由 Overture schema 定义有效值）|
| `--in <path>` | 路径 | 仅转换模式 | 已下载 GeoJSON 路径 |
| `--out <path>` | 路径 | 两种模式都要 | 下载落点 / GeoJSON 输入路径（两种模式共用同一参数）|
| `--release <TAG>` | 字符串 | 否 | Overture release tag（**当前 CLI 暂未透传给 overturemaps-py**，作记录用）|
| `--emit-yaml` | flag | 否 | 输出 PlaceEdge 写回草稿 YAML 到 stdout |

### 4.2 `wv overture index`

```
用法: wv overture index --in <path> [--id <OvertureId>] [--label <name>]
```

| 参数 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `--in <path>` | 路径 | 是 | 已下载 GeoJSON 路径 |
| `--id <UUID>` | 字符串 | 否 | 反查该 OvertureId 对应的 candidate（应命中则打印 distance_km / terrain）|
| `--label <name>` | 字符串 | 否 | 反查该 bbox label 注册的下标（CLI 第一版不预注册，永远返回「未注册」）|

输出包含：输入路径、原始 feature 数、保留 candidate 数、`by_id` 大小，以及可选的 `--id` / `--label` 反查结果。

---

## 5. 失败模式 / 边界情况

| 情况 | 表现 | 应对 |
|---|---|---|
| `bbox` 越界（西 > 东 / 南 > 北）| `flag \`--bbox\` 需要一个值` 或参数解析错误 | CLI 校验即拒；如需修正检查经纬度顺序 |
| `overturemaps` 未安装 | Python 子进程退出码非零，错误透传 | `pip install overturemaps`（或跳过下载，走 `--in` 模式）|
| 离线环境（无网络）| 同上 | 用 `--in` 模式转换本地 GeoJSON；CI 用此模式 |
| GeoJSON 坏 JSON / 非 FeatureCollection | `FetchError::ParseFailed` | 重下 / 换工具 |
| 单个 feature 缺 `id` / 坏 UUID | 该 feature 跳过（不阻断整批） | 日志可见 filtered 数；如需全量通过需修源数据 |
| `subtype: footway` / `path` / `cycleway` 等 | **默认过滤**（ADR-0007 F 决议）| 想要步行拓扑须手动编辑 domain YAML 改 `terrain`（不走 CLI）|
| Overture schema 升级 | `OVERTURE_SCHEMA_VERSION` 常量与转换表可能失效 | 跑回归（`segment_length_m` 137 m 用例）；如失败去 `wv-overture/src/lib.rs` 更新版本号与转换表 |

---

## 6. 与 ADR-0007 / README §15-§18 的对应关系

| 本文位置 | 对应 ADR | 对应 README |
|---|---|---|
| §1 概念 | §决策 (1)(2)(3) + §后果「UUID 例外条款」| §15 (a)(b)(c)(d)(e)(f)|
| §2.4 写盘路径 | §决策 (6)「不开 P3 例外口子」| §16 |
| §2.5 反查路径 | §理由 (1) | §18（部分关闭）|
| §5 schema 升级 | §17 待裁 | §17（决议：内部常量，不入领域）|

---

## 7. 验证（最小复现）

以下序列可在离线环境跑（**仅步骤 B + C，不需 Python / 网络**）：

```bash
# 1. 准备最小 GeoJSON（西雅图某 residential segment + 一个 footway）
mkdir -p _probe
cat > _probe/min.geojson <<'EOF'
{"type":"FeatureCollection","features":[
  {"id":"50e76fae-ac7e-448a-b34d-fcb4db17b5ee","type":"Feature",
   "geometry":{"type":"LineString","coordinates":[[-122.3493,47.6205],[-122.3491,47.6201]]},
   "properties":{"subtype":"road","class":"residential"}},
  {"id":"11111111-2222-3333-4444-555555555555","type":"Feature",
   "geometry":{"type":"LineString","coordinates":[[-122.349,47.6205],[-122.349,47.6207]]},
   "properties":{"subtype":"road","class":"footway"}}
]}
EOF

# 2. 索引（验证转换 + UUID 反查）
cargo run -p wv-app -- overture index \
    --in _probe/min.geojson \
    --id 50e76fae-ac7e-448a-b34d-fcb4db17b5ee

# 期望输出：
#   Overture index 完成：
#     输入文件:     _probe/min.geojson
#     原始 feature: 2
#     保留 candidate: 1   ← footway 被过滤
#     by_id 大小:   1
#     lookup by_id 50e76fae-ac7e-...: distance_km=0.047 terrain=road

# 3. 写回草稿（验证 --emit-yaml 输出格式）
cargo run -p wv-app -- overture fetch \
    --in _probe/min.geojson \
    --out _probe/min.geojson \
    --emit-yaml

# 期望输出（每个 feature 一个 PlaceEdge 块）：
#   - id: overture_seg_50e76fae
#     from: PLACE_ID:PLACE_REPLACE_ME_A    # ← 作者需替换
#     to:   PLACE_ID:PLACE_REPLACE_ME_B
#     distance_km: 0.047
#     terrain: road
#   # 注：真实写盘应走 wv-edit::EditCommand::AddPlaceEdge，不直写 yaml

# 4. 清理
rm -rf _probe
```

---

## 8. 待决（沿 README §18 转发）

- **倒排索引**：何时把 `IndexedOvertures` 落盘到工程目录（预计 N > 10 万 candidate 时）；
- **SPI 多源**：是否引入 `GeographicSource` trait 抽象 OSM / Mapbox / 自建库（当前第一版仅 Overture 一源）；
- **Parquet 直读**：跳过 GeoJSON 中间格式，直接读 Overture S3 上的 GeoParquet（依赖选型待定，参见 §5「schema 升级」与 ADR-0007 §17）；
- **`overturemaps-py` 透传 `--release`**：当前 CLI 暂作记录，未传给 Python 子进程——是否实装取决于「钉版」是否真为第一版必需。

---

## 9. 相关文档索引

- [ADR-0007 Overture 真实地理底图](adr/ADR-0007-overture-real-geography.md)——决策与备选被否决
- [README §15 真实地理底图引入路径](../README.md)——决议 checklist
- [README §16-§18](../README.md)——资源 / schema / 检索三段待裁
- [AGENTS.md §4.3 只存引用](../AGENTS.md)——领域不重存原则
- [AGENTS.md §7.5 V1 校验只提示](../AGENTS.md)——CLI 写草稿不直写盘的依据