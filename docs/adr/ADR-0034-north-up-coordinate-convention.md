# ADR-0034: 上北下南——地理导入与图内坐标统一为 y 向上

- **状态**：已接受（Accepted，2026-10-18，用户对话拍板「应该是上北下南的」）
- **关联**：ADR-0007（Overture `geographic_source` 投影）、ADR-0022（城市子图外部导入投影）、`docs/MAP_EDITOR_BASELINE_v1.md` §2.15（坐标链唯一实现点）、`ART_STYLE_SPEC`（指南针 N 在上）

## 处境

三条链路各自成立，但合起来是镜像的：

| 层 | 约定 | 位置 |
|---|---|---|
| 渲染 | 世界 `+Y` = 屏幕向上 | `crates/wv-render/src/camera.rs`（`screen_to_world`） |
| 地理导入 | 纬度越大 → `y` 越小（y 向下） | `crates/wv-app/src/overture_cli.rs::project_lat_lon_to_xy`：`v = 1 - (lat-south)/range` |
| DEM 中间格式 | 行 0 = 北（GeoTIFF 行序直写） | `target/dem_gd/prep_guangdong_dem.py` |
| UV / 锚点 | `[0,1]²` 左上角为原点、y 向下 | `crates/wv-core/src/map/coord.rs`、`LatLonBbox::uv_to_lat_lon` |
| 指南针 | N 永远画在正上方 | `crates/wv-app/src/ui/compass_panel.rs` |

结果：广东整幅地图**上下颠倒**（湛江在屏幕上方、韶关在下方），而指南针、状态栏经纬度仍按「上北下南」显示——三者互相矛盾。地形、道路、POI 由同一批 lat/lon 投影而来，**相对位置正确、绝对朝向反了**；这不是旋转，`MapNode.orientation` 修不了。

## 决策

**采用「上北下南」为全局唯一约定**，在世界坐标、图内局部坐标、UV、DEM 行序四层统一为 **y 向上（北 = y 大）**：

- **D1 · 地理导入**：`project_lat_lon_to_xy` 改为 `v = (lat - south) / (north - south)`（不再取 `1 -`）。
- **D2 · DEM 中间格式**：`guangdong_dem.bin` **行 0 = 最南行**（GeoTIFF 行序写出前反向）；`build_guangdong_terrain` 直写，渲染层行 0 落在世界 `y=0`（底部）即为南。
- **D3 · UV / 锚点语义**：UV `[0,0]` = 本图**西南角**、`[1,1]` = 东北角；`world_to_uv = local / extent` 不变（数值映射不变，语义随 D1 翻转）。`IconSpec::anchor` 随导入公式自动获得新语义。
- **D4 · 经纬度反算**：`LatLonBbox::uv_to_lat_lon` 改为 `lat = south + uv[1] * (north-south)`，与 D1/D3 同源；状态栏读数与画面一致。
- **D5 · 指南针不动**：N 始终在正上方，从此与实际画面一致（不需要也不允许镜像）。
- **D6 · 手绘地图零改动**：手绘工程的图内坐标不经 lat/lon，`map_to_view = map_origin + local` 不变；此前文档写的「UV 左上原点」与实现（直接 `local → world +Y`）本就不一致，本 ADR 把文档改到与实现一致，**手绘地图的既有渲染行为不变**。

被否决的备选：

- **B · 翻转相机 / 投影**：一处改动，但文字、图标、笔刷光标、绕序全部要补偿，且与世界坐标语义永久拧着。
- **C · 声明「南在上」为约定**：与用户要求直接冲突，指南针将永远说谎。
- **只翻转广东数据、不改代码**：能出一张北在上的画面，但状态栏经纬度会反向，且下一次导入继续产出颠倒数据——把缺陷固化进数据。

## 代价

- **广东工程需重新导入**：`distances/places.yaml`（78,286 点）、`distances/edges.yaml`（84,162 边）、`map_guangdong` 与 21 个城市子图的 `icons.anchor` / `outline_polygon` 全部按新公式重算；`terrain.bin` 按 D2 重建。已完成（2026-10-18）。
- **xinhai-1911 的经纬度读数会翻转**：它的 `geographic_source` 是名义声明（地形是 Perlin 噪声、并非真实地貌），数据仍按旧约定落盘；本 ADR 只保证「新导入 + 广东工程」正确，xinhai 若要求读数正确需另行按 D1 重导。该项登记为已知偏差，不阻止合入。
- 任何在 2026-10-18 之前用 `overture apply` 导入的工程，其 y 坐标都是旧约定；要么重导，要么接受画面上下颠倒。

## 验证

- 数据级：广州（23.13°N）`y = 2127.6` > 湛江（21.27°N）`y = 451.1`；`terrain` 最南行 935/1024 为海、最北行 0/1024 为海。
- 实机：`wv gui -m examples/guangdong` 截图，韶关/梅州在上、湛江/珠海在下、海在下、指南针 N 向上，路网覆盖全省。
- 单测：`uv_to_lat_lon_maps_corners_and_center` / `uv_to_lat_lon_normalizes_reversed_bbox` / `uv_to_lat_lon_extrapolates_out_of_range_input` 按 D4 更新断言。

## 影响文件

| 文件 | 改动 |
|---|---|
| `crates/wv-app/src/overture_cli.rs` | `project_lat_lon_to_xy` 去 `1 -` |
| `crates/wv-core/src/source/geographic_source.rs` | `uv_to_lat_lon` 改 `south + v*range` + 测试/文档 |
| `crates/wv-core/src/map/coord.rs` | UV 语义文档（西南原点、y 向上） |
| `target/dem_gd/prep_guangdong_dem.py` | 写出时行序反向（row 0 = 南） |
| `crates/wv-app/examples/build_guangdong_terrain.rs` | 中间格式文档（row 0 = 南） |
| `examples/guangdong/**` | 全量重导入（places / edges / icons / outline / terrain） |
