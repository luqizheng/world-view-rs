# codex_forest_design.md — 深林覆盖物生成设计（Codex 建议）

> **来源说明**：本文件整理自 Codex 于 2026-10-22 给出的 JRPG 手绘地图森林覆盖物生成建议。
> 首次整理时「核心数据结构」代码块传输截断，内容系依据五段管线重建；
> 2026-10-22 起陆续补收 Codex 原文（§2.2 数据结构、§3.1 适宜度、§4 密度场、
> §5 密度分类、§6 树木采样、§7 渲染、§8 串联、§9 后续功能），§2.1 保留为仓库
> 对接用的 `ScalarField` 建议。重建稿与原文均明确标注；如有出入，以双方确认为准。
> 配图参考见 §0。本设计最终渲染目标即此类手绘精灵。
> **留痕**：2026-10-22 曾因编辑工具编码事故全文乱码一次，已依会话记录整体重写，
> 内容与事故前语义一致。

---

## 0、参考图与生成语义

**图 A（草丛生成展示）**：覆盖物的**组合式生成原则**——
`小单体 + 中团块 = 大片区`，四行示例：针叶林、阔叶灌木丛、热带棕榈丛、蘑菇丛。
含义：

1. 密林大片并非逐树独立绘制，而是「小树 + 中型团块资产」叠加组合而成；
2. 与 §六采样的 `variant`、§九-2「密林内部用更大树冠」直接对应——
   采样时高密度区应倾向选用**团块级资产**（资产表第 3 行林簇），
   疏林边缘用小单体；
3. 蘑菇丛示例说明同一管线可换皮生成任意「聚集型覆盖物」（§十一-6）。

**图 B（100+ Hand-drawn Trees and Plants 资产表示例）**：
- 4 种**大片森林团块**：针叶密林、阔叶密林、矮针叶林、针阔混交林——
  即 `ForestType::Dense` 的首选视觉资产；
- 行状变体：柏树×10、阔叶树×10、松树×10、小松×5、金合欢×5、棕榈×6、
  仙人掌×8、蘑菇×8、草×7、灌木×5——**支撑 Codex 代码中 `variant 0..8`
  的假设**（部分类别不足 8 变体，靠 `variant % variants.len()` 兜底）；
- **棕榈（Tropical）与仙人掌（干旱）在资产表中存在**，§十一-4 的
  「Tropical 无资产」待决项可据此解除，另增干旱树种分流待决。

---

## 一、总体管线

前提：已有三张 `Vec<f32>` 数据图（与仓库现有 `wv-terrain` 高度场同构）：

| 字段 | 值域 | 含义 |
|---|---|---|
| `height` | 0～1 | 高度 |
| `temperature` | 0～1 | 温度 |
| `moisture` | 0～1 | 湿度 |

Codex 建议的生成流程：

```
森林适宜度计算 → 森林密度场 → 林区聚集 → 泊松采样 → Bevy Sprite 渲染
```

技术栈（Codex 原建议）：Bevy 0.17.x 风格 API + rand 0.9 + noise 0.9；
不重新生成高度/温度/湿度。

> **仓库对齐备注**：本工作区当前 Bevy 版本与 crate 分层以 AGENTS.md 为准；
> 若仓库实际 Bevy 版本高于 0.17，代码需按迁移指南调整（见 §十一-1）。

---

## 二、核心数据结构

### 2.1 输入场视图（仓库对接建议，非 Codex 原文）

输入即三张同尺寸场图。为避免各处重复实现，建议在 `wv-terrain`（纯数据层叶子，
零第三方依赖）中定义统一的场视图：

```rust
/// 归一化标量场（0..=1），与高度场同构。
/// 放在 wv-terrain：纯数据，无 Bevy、无 IO。
pub struct ScalarField {
    pub width: u32,
    pub height: u32,
    pub data: Vec<f32>,
}

impl ScalarField {
    pub fn sample(&self, x: u32, y: u32) -> f32 {
        let i = (y as usize) * self.width as usize + x as usize;
        self.data[i].clamp(0.0, 1.0)
    }
}
```

### 2.2 核心数据结构（Codex 原文，2026-10-22 补收）

**Codex 原则（2026-10-22 补充）：森林密度与树木实例要分开保存——密度决定
哪里应该有森林，实例决定每棵树的位置、大小和类型。**
`ForestMap.density` 是连续决策场（供聚集/采样/调参目视检查），
`ForestMap.trees` 是采样后的最终产物；改阈值重采样时密度场不失效，
两者生命周期独立。

```rust
use bevy::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForestType {
    Sparse,   // 疏林
    Normal,   // 常规森林
    Dense,    // 密林
}

#[derive(Clone, Copy, Debug)]
pub enum TreeType {
    Broadleaf, // 阔叶（资产表第 1 行）
    Conifer,   // 针叶（第 2 行）
    Tropical,  // 热带（图 B 有棕榈资产，见 §0 / §十一-4）
}

#[derive(Clone, Debug)]
pub struct TreeInstance {
    pub position: Vec2,
    pub scale: f32,
    pub variant: usize,        // 资产表中的列（同类第几个变体）
    pub tree_type: TreeType,
    pub forest_type: ForestType,
}

pub struct ForestMap {
    pub width: usize,
    pub height: usize,

    // 森林密度：0~1
    pub density: Vec<f32>,

    // 树木实例
    pub trees: Vec<TreeInstance>,
}
```

> **仓库对接备注**（非 Codex 原文）：
> - `use bevy::prelude::*`（`Vec2`）意味着这段结构目前是**渲染侧形态**；若落
>   `wv-terrain` 须去掉 Bevy 依赖（`Vec2` → `(f32, f32)` 或自写极简向量），
>   由 `wv-render` / `wv-theme` 在边界处转换。
> - Codex 原文的 `TreeInstance` 未含 `z_order`，遮挡次序由渲染层按
>   `position.y` 现算即可，不必入库（与「只存参数」精神一致）。

---

## 三、森林适宜度计算（Suitability）

Codex 原则（2026-10-22 补充）：适宜度**不应只根据湿度判断**，需同时考虑
温度、湿度和高度；且采用**平滑过渡，而不是硬阈值分类**。

### 3.1 适宜度函数（Codex 原文，2026-10-22 补收）

```rust
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0))
        .clamp(0.0, 1.0);

    t * t * (3.0 - 2.0 * t)
}

pub fn forest_suitability(
    height: f32,
    temperature: f32,
    moisture: f32,
) -> f32 {
    // 海洋不能生成树木
    if height < 0.45 {
        return 0.0;
    }

    // 湿度越高越适合森林
    let wetness = smoothstep(0.25, 0.75, moisture);

    // 极冷地区不适合树木
    let cold_limit = smoothstep(0.12, 0.35, temperature);

    // 极热地区略微降低适宜度
    let heat_limit =
        1.0 - 0.3 * smoothstep(0.85, 1.0, temperature);

    // 高海拔逐渐接近林线
    let altitude_limit =
        1.0 - smoothstep(0.72, 0.92, height);

    // 海岸附近允许森林，但限制极低海拔
    let coast_limit = smoothstep(0.45, 0.48, height);

    (
        wetness
        * cold_limit
        * heat_limit
        * altitude_limit
        * coast_limit
    ).clamp(0.0, 1.0)
}
```

五因子连乘：`wetness × cold_limit × heat_limit × altitude_limit × coast_limit`。
唯一的硬边界是海平面（`height < 0.45 → 0`），语义明确，属合理例外。

> **与先前重建稿的差异备忘**（§6.3 追写）：重建稿温度热端衰减过强
> （0.75→0.95 归零）；Codex 原文热端仅打 7 折（0.85→1.0 最多 -30%），
> 并新增了重建稿没有的**海岸带限制**（0.45→0.48 起坡，滩涂不长树）。
> 以原文为准。

**Codex 补充告诫**：
- `0.45` 是**示例海平面**，接入本仓库时应替换为 `wv-terrain` 高度场实际
  采用的海平面值（见 §十一-9）；
- 若温度数据是**摄氏度**而非归一化温度，须先转换（或改函数阈值）。

阈值分流：`suitability >= 0.55` 视为「可生长」，其中的连续程度决定密度。

> **待决**：适宜度五因子各边界值 + 可生长下限均为 Codex 建议初值，需以实际
> 世界地图目视调参；应做成 YAML 配置而非代码常量（P3），见 §十一-2。
> 单测注意：`smoothstep` 边界（0.45 / 0.48 / 0.72 / 0.92 等）须有手算期望值
> 断言（§7.2 / §7.4）。

---

## 四、森林密度场——聚集噪声生成（Codex 原文，2026-10-22 补收）

Codex 原则：仅仅依据适宜度放树会导致**过于均匀**；真实森林有成片聚集形态，
因此需增加一张**森林聚集噪声图**（双层 OpenSimplex：大尺度定斑块、小尺度碎边缘）。

**依赖**（Codex 建议）：`noise = "0.9"`（配 `bevy = "0.17"`、`rand = "0.9"`）。
> **仓库依赖纪律备注**（AGENTS §9，非 Codex 原文）：`wv-terrain` 现为**零第三方
> 依赖叶子 crate，引入 `noise` 会破坏该状态。两条路径待裁（§十一-7）：
> A. `wv-terrain` 新增 `noise` 依赖（需 ADR 说明为何不复用现有噪声能力）；
> B. 在 `wv-terrain` 内自写/复用确定性 OpenSimplex 实现，保持零依赖。

```rust
use noise::{NoiseFn, OpenSimplex};

pub fn generate_forest_density(
    width: usize,
    height: usize,
    heights: &[f32],
    temperatures: &[f32],
    moistures: &[f32],
    seed: u32,
) -> Vec<f32> {
    assert_eq!(heights.len(), width * height);
    assert_eq!(temperatures.len(), width * height);
    assert_eq!(moistures.len(), width * height);

    let noise = OpenSimplex::new(seed);
    let mut density = vec![0.0; width * height];

    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;

            let suitability = forest_suitability(
                heights[i],
                temperatures[i],
                moistures[i],
            );

            // 大尺度噪声：森林整体分布
            let large = noise.get([
                x as f64 * 0.008,
                y as f64 * 0.008,
            ]) as f32;

            // 小尺度噪声：森林边缘破碎
            let detail = noise.get([
                x as f64 * 0.035 + 100.0,
                y as f64 * 0.035 + 100.0,
            ]) as f32;

            let cluster = (
                0.5
                + large * 0.38
                + detail * 0.12
            ).clamp(0.0, 1.0);

            // 形成连续林区，保留部分空地
            let patch = smoothstep(0.25, 0.65, cluster);

            density[i] = suitability * patch;
        }
    }

    density
}
```

关键调参（Codex 原文表格）：

| 参数 | 作用 | 调整效果 |
|---|---|---|
| `0.008` | 大尺度森林噪声频率 | 越小，森林斑块越大；想要参考图那样的大面积森林可降到 **0.003～0.006** |
| `0.035` | 森林边缘细节频率 | 越大，边缘越破碎 |

> 注：`OpenSimplex::get` 返回值域约 -1..1，`0.5 + large*0.38 + detail*0.12`
> 即把它抬回 0..1 附近再 clamp；种子 `seed` 必须入 YAML（确定性复现，P3）。

> **追记**：本节 Codex 方案用**噪声 patch** 直接实现「成片」，先前重建稿的
> 「连通域 flood fill 聚集」并非 Codex 原文，已随 §六追记合并、不再单列；
> 将来若渲染需要按林区分组再恢复（见 §十一-10 备注）。

---

## 五、森林密度分类（Codex 原文，2026-10-22 补收）

```rust
pub fn classify_forest(
    density: f32,
) -> Option<ForestType> {
    if density < 0.18 {
        None
    } else if density < 0.40 {
        Some(ForestType::Sparse)
    } else if density < 0.72 {
        Some(ForestType::Normal)
    } else {
        Some(ForestType::Dense)
    }
}
```

Codex 原则：这个分类**主要用于选择视觉样式**（tint / 资产变体）；
**树木间距应随连续密度变化**，避免在分类阈值处出现突兀的直线边界。

> 渲染层如需「密度→资产」映射表，应引用 0.18 / 0.40 / 0.72 这组阈值
> 并集中于 `wv-theme`（装饰规则归主题层）。

---

## 六、生成树木位置（Codex 原文，2026-10-22 补收）

Codex 采用**带密度约束的泊松式采样**：与完整 Bridson 算法不同，用
**随机候选点 + 空间网格检测**——代码更易集成，且保证树木间最小间距。

> **追记**（§6.3）：先前重建稿「林区聚集（flood fill）」与
> 「Bridson 泊松采样」两节被本节 Codex 原文**取代**，已并入本追记、
> 不再单列；flood fill 若将来需要（按林区分组渲染）再恢复。

```rust
use rand::{
    rngs::StdRng,
    Rng,
    SeedableRng,
};

pub fn generate_trees(
    width: usize,
    height: usize,
    density: &[f32],
    temperatures: &[f32],
    seed: u64,
) -> Vec<TreeInstance> {
    assert_eq!(density.len(), width * height);
    assert_eq!(temperatures.len(), width * height);

    let mut rng = StdRng::seed_from_u64(seed);
    let mut trees = Vec::new();

    // 以地图像素为坐标单位
    let min_spacing = 7.0_f32;
    let max_spacing = 26.0_f32;

    // 网格单元不大于最小间距
    let cell_size = min_spacing / std::f32::consts::SQRT_2;

    let grid_width =
        (width as f32 / cell_size).ceil() as usize;
    let grid_height =
        (height as f32 / cell_size).ceil() as usize;

    let mut grid: Vec<Vec<usize>> =
        vec![Vec::new(); grid_width * grid_height];

    // 随机候选数量，可根据目标密度调整
    let attempts = width * height / 25;

    for _ in 0..attempts {
        let x = rng.random_range(0.0..width as f32);
        let y = rng.random_range(0.0..height as f32);

        let ix = (x as usize).min(width - 1);
        let iy = (y as usize).min(height - 1);
        let index = iy * width + ix;

        let d = density[index];

        if d < 0.18 {
            continue;
        }

        // 密度越高，树木越容易被接受
        if rng.random::<f32>() > d {
            continue;
        }

        let spacing =
            max_spacing - (max_spacing - min_spacing) * d;

        let gx = (x / cell_size) as usize;
        let gy = (y / cell_size) as usize;

        let search_radius =
            (max_spacing / cell_size).ceil() as isize;

        let mut valid = true;

        let x0 = gx.saturating_sub(search_radius as usize);
        let y0 = gy.saturating_sub(search_radius as usize);
        let x1 = (gx + search_radius as usize)
            .min(grid_width - 1);
        let y1 = (gy + search_radius as usize)
            .min(grid_height - 1);

        'neighbors: for cy in y0..=y1 {
            for cx in x0..=x1 {
                for &tree_index in &grid[cy * grid_width + cx] {
                    let other: &TreeInstance = &trees[tree_index];

                    if other.position.distance(
                        Vec2::new(x, y)
                    ) < spacing {
                        valid = false;
                        break 'neighbors;
                    }
                }
            }
        }

        if !valid {
            continue;
        }

        let forest_type =
            classify_forest(d).unwrap();

        let temperature = temperatures[index];

        let tree_type = if temperature < 0.38 {
            TreeType::Conifer
        } else if temperature > 0.75 {
            TreeType::Tropical
        } else {
            TreeType::Broadleaf
        };

        let tree = TreeInstance {
            position: Vec2::new(x, y),
            scale: rng.random_range(0.75..1.25),
            variant: rng.random_range(0..8),
            tree_type,
            forest_type,
        };

        let tree_index = trees.len();
        trees.push(tree);

        grid[gy * grid_width + gx].push(tree_index);
    }

    trees
}
```

Codex 自评的两个注意点（原文）：

1. **非严格对称可变半径泊松式采样**：当前只保证候选树与已有树满足**候选树自身**
   的间距要求。更严格的做法是为每棵树保存半径，邻域判断取两半径的**最大值**。
2. **`attempts` 决定采样密度**：大地图 + 稀疏林区建议**按 Chunk 生成候选点**，
   而非全图统一随机投点。

> **仓库备注**（非 Codex 原文）：两个种子（密度 `12345`、采样 `98765`）与
> `min/max_spacing`、attempts 分母均为参数，落库时须入 YAML（P3）；
> `Vec2` 同 §2.2 备注，落 `wv-terrain` 须去 Bevy 依赖。

---

## 七、在 Bevy 中渲染树木（Codex 原文，2026-10-22 补收）

素材目录（Codex 原文假设）：每类树 8 个变体，独立 PNG：

```text
assets/
└── trees/
    ├── broadleaf_0.png
    ├── broadleaf_1.png
    ├── ...
    ├── broadleaf_7.png
    ├── conifer_0.png
    ├── ...
    ├── conifer_7.png
    ├── tropical_0.png
    └── ...
```

> 图 B 资产表部分类别变体数不足 8，`variant % variants.len()` 已兜底；
> 另需为 `ForestType::Dense` 准备**团块级资产**（§0 图 A 的「大片区」，
> 图 B 四种大片森林），密林采样点按概率改贴团块而非单树——此项为
> 图 A 组合式生成的落地点，具体规则归 `wv-theme`（见 §十一-6）。

资源与加载：

```rust
#[derive(Resource)]
pub struct TreeTextures {
    pub broadleaf: Vec<Handle<Image>>,
    pub conifer: Vec<Handle<Image>>,
    pub tropical: Vec<Handle<Image>>,
}

fn load_tree_textures(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let load_group = |prefix: &str| -> Vec<Handle<Image>> {
        (0..8)
            .map(|i| {
                asset_server.load(format!(
                    "trees/{}_{}.png",
                    prefix, i
                ))
            })
            .collect()
    };

    commands.insert_resource(TreeTextures {
        broadleaf: load_group("broadleaf"),
        conifer: load_group("conifer"),
        tropical: load_group("tropical"),
    });
}
```

生成 Sprite（含按 `ForestType` 分级的绿色 tint 与 y 排序 z）：

```rust
fn spawn_forest_sprites(
    commands: &mut Commands,
    textures: &TreeTextures,
    trees: &[TreeInstance],
) {
    for tree in trees {
        let variants = match tree.tree_type {
            TreeType::Broadleaf => &textures.broadleaf,
            TreeType::Conifer => &textures.conifer,
            TreeType::Tropical => &textures.tropical,
        };

        let texture = variants[
            tree.variant % variants.len()
        ].clone();

        let tint = match tree.forest_type {
            ForestType::Sparse => {
                Color::srgb(0.95, 1.0, 0.91)
            }
            ForestType::Normal => {
                Color::srgb(0.88, 0.96, 0.84)
            }
            ForestType::Dense => {
                Color::srgb(0.70, 0.84, 0.67)
            }
        };

        commands.spawn((
            Sprite {
                image: texture,
                color: tint,
                ..default()
            },
            Transform {
                translation: Vec3::new(
                    tree.position.x,
                    tree.position.y,
                    20.0 - tree.position.y * 0.00001,
                ),
                scale: Vec3::splat(tree.scale),
                ..default()
            },
        ));
    }
}
```

Codex 补充告诫：

- 素材**已含手绘阴影**时应**减弱 tint 强度**，否则破坏素材本身色彩；
- 地图坐标（左上原点）→ Bevy 世界坐标的统一换算：

```rust
let world_x = map_x - map_width as f32 * 0.5;
let world_y = map_height as f32 * 0.5 - map_y;
// 用 world_y 处理渲染层级
```

> **仓库备注**：tint 分级数值属**主题装饰规则**，落库应移入 `wv-theme`
> 主题包（ADR-0043），`wv-render` 只消费；此前重建稿的 atlas 图集方案
> 被本节逐文件 PNG 方案取代，若将来精灵数量过大再评估图集合并。

---

## 八、串联现有三张数据图（Codex 原文，2026-10-22 补收）

```rust
let forest_density = generate_forest_density(
    map_width,
    map_height,
    &height_data,
    &temperature_data,
    &moisture_data,
    12345,
);

let trees = generate_trees(
    map_width,
    map_height,
    &forest_density,
    &temperature_data,
    98765,
);
```

随后将 `trees` 交给 Bevy 的 Sprite 渲染系统。
**前提**：三张数据图尺寸相同，且坐标一一对应。

---

## 九、还需补充的功能（Codex 原文，2026-10-22 补收）

要真正接近参考图效果，Codex 建议继续做：

1. **森林底色蒙版**：用 `forest_density` 在地形 Shader 中混合深绿色，
   而非只放树——否则树木之间会露出大量普通草地；
2. **森林边缘过渡**：边缘生成更小、更稀疏的树 + 少量灌木；密林内部用更大
   树冠与更深底色；
3. **Chunk 级稳定生成**：示例代码适合先验证视觉；正式编辑器应按 Chunk 生成
   树木，Chunk 边界共享邻域采样信息，避免接缝与局部重生成后树木随机变化。

**可扩展排除项**（原文）：`height` 目前只限生长高度；若有坡度图、海岸距离场、
河流/道路蒙版，可进一步排除悬崖、水域、道路与城市区域。

**核心提醒**（原文）：这套代码生成的是**森林的空间分布，不是手绘风格本身**。
要接近参考图，必须用统一风格的树木 Sprite + 深绿森林底色 + 适当的树冠重叠。

**Codex 优先级建议**：下一步优先实现 **森林密度贴图的 WGSL Shader +
树木 Chunk 批量渲染**——这两部分能把基础生成器变成适合大地图编辑器的森林系统。

---

## 十、与仓库架构的对接（非 Codex 原文，为落库必须的约束）

| 环节 | 归属 crate | 依据 |
|---|---|---|
| ScalarField / suitability / density / sampling | `wv-terrain`（纯数据，零依赖） | 平行叶子 crate，笔刷/地形算法所在地 |
| 覆盖物规则（分类阈值、tint 分级、树种分流、团块选择） | `wv-theme` | ADR-0043：装饰旗标 / 覆盖物规则归主题层 |
| Sprite 渲染（纹理加载、坐标换算、z 排序） | `wv-render` | 渲染运行时 |
| 结果落盘（树木实例或引用） | `wv-io` | wv-core 禁止文件系统访问 |
| 生成触发（用户点击「生成森林」） | `wv-app`，经 `EditCommand` | §3.3 禁止 System 直接改世界 |

- 生成结果若需持久化且体量大，属「超大地图网格数组」例外，须走 §5 二进制分块纪律
  （`RasterRef` 声明 + format_version + 格式规格文档）；
- 种子与阈值参数存 YAML（P3），不写代码常量。

---

## 十一、待决问题（按 §6.2 显式化）

| # | 待决项 | 当前建议（最易回退方案） |
|---|---|---|
| 1 | 仓库 Bevy 实际版本与 Codex 0.17 API 的差异面 | 按 skill 迁移指南（0.17→0.18→0.19）逐条核对后再落码 |
| 2 | 适宜度五因子阈值、噪声频率、分类阈值（0.18/0.40/0.72）、间距（7/26）与两个种子 | 先硬编码最小实现 + `TODO` 注释指向本表，参数收敛后迁 YAML |
| 3 | 覆盖物结果是否持久化，还是每次从参数确定性重算 | 先**不持久化**，从种子+参数重算（零二进制格式成本）；帧率不够再引入分块缓存 |
| 4 | 树种分流硬阈值（<0.38 Conifer，>0.75 Tropical）；图 B 已有棕榈资产，`Tropical` 可启用；图 B 另有仙人掌（干旱树种），现模型无对应 `TreeType` | 启用 Tropical；干旱分流（需 `moisture` 参与或新增 `Arid`）列为扩展，另裁 |
| 5 | z_order：Codex 用 `20.0 - y * 0.00001` 极弱 z 调制 | 先照抄原文；与河流/道路等图层的互遮挡另行裁定 |
| 6 | 灌木 / 低草 / 蘑菇（图 B 下缘行、图 A 蘑菇丛）不在 Codex 原文 `TreeInstance` 模型内；密林「团块级资产」替换规则也未定义 | 先按纯树木建模；团块替换与低矮覆盖物作为 `wv-theme` 覆盖物规则的扩展项，另裁 |
| 7 | `noise = "0.9"` / `rand = "0.9"` 依赖落点：`wv-terrain` 引入会破坏零依赖叶子状态（AGENTS §9） | 待裁 A（引依赖 + ADR）/ B（自写或复用现有确定性噪声与 RNG，保持零依赖）；倾向 B |
| 8 | 森林底色蒙版 WGSL Shader 归属：地形 Shader 混合是否动 `wv-render` 地形管线 | Codex 优先级建议之一；落码前须出 ADR |
| 9 | 海平面 0.45 为示例值；温度若是摄氏度需先归一化 | 接入时以 `wv-terrain` 高度场实际海平面替换；温度单位以仓库现有数据为准，必要时前置转换 |
| 10 | Chunk 级生成（§九-3）是否纳入首版 | 首版全图一次生成；大地图出现性能/接缝问题再引入 Chunk |

---

*落盘：2026-10-22，依据 Codex 建议整理，原文八段已全部补收，并整合两张参考图
（草丛组合生成展示、100+ 手绘树木资产表）的生成语义。本文为设计提案，
未触及任何原则性改动；实际落码前须走分支 + worktree 流程（§13）。*

---

## 追记（2026-10-22，实现回合）：§七 与 ADR-0031 的冲突 → 裁定为方案 1

生成核心已按本文 §三～§六、§八 落地：分支 `feat/forest-generate`（worktree
`.worktrees/feat/forest-generate`），`wv-terrain` 新增 `forest` 模块——
零第三方依赖（`noise`/`rand` 分别以现有手写 Perlin 与内置 splitmix64 替代，
即 §十一-7 方案 B），20 项单测 + workspace 全闸门绿灯，提交 `3cc99ef`。

**§七 的 Sprite 渲染方案与既有 ADR-0031（程序化地形装饰）冲突**：
`wv-render::terrain_decor` 已按群系采样绘制程序化树冠符号，且明确
「不引入位图瓦片，全部程序化三角化」。按 §6.4，二选一须留 ADR / 用户裁定：

- **A**：维持 ADR-0031，把本文密度场接入现有 decor 管线（密度场调制
  符号密度），不引入 Sprite；
- **B**：新 ADR 推翻 ADR-0031，树木改走本文 §七 的 Sprite 图集方案。

### 裁定（用户拍板 2026-10-22）：**方案 1 = A 的实现 + B 的形态分工**

用户补充原设计意图：「**由 theme 决定在这个地方采用什么方式进行覆盖**」。
据此把「方式」拆成三层，各归其位：

| 层 | 归谁 | 落点 |
|---|---|---|
| **位置**（哪里有林） | 地形事实 | `wv_terrain::forest` 密度场（群系 + 林线 + 聚集噪声），渲染期派生不落盘 |
| **疏密**（多密） | 主题参数 | `DecorRecipe.forest_density_gain`（ADR-0048 追记二的配方纪律） |
| **形态**（长什么样） | 主题 / 渲染层既有契约 | `FeatureRule.symbol_texture`（位图 or 纯色）+ ADR-0031 D5（程序化符号） |

**落地改动**（提交 `864436c` / `a461073`）：

- `wv-terrain::forest` 增 `generate_forest_density_from_biome`（群系驱动，
  **渲染期唯一可用的数据源**——温度湿度是生成期派生量、不落盘）；
  气候驱动版改名 `generate_forest_density_from_climate`（生成期/离线用）。
- `wv-theme::decor::DecorRecipe` 增 `forest_density_gain`（默认 `1.0`）。
  **不给 `FeatureRule` 加字段**——ADR-0032 D2 明令其「字段一次定型，
  外部主题 crate 的 API 稳定面」。
- `wv-render::terrain_decor` 森林桶保留率 = `密度场 × 主题增益`
  （`forest_keep_prob`），新增 `ForestDensityCache`（按 terrain hash 键控，
  与 `ShoreFieldCache` 同构）。
- ADR-0031 追加**追记一**（不改 D1–D6，不推翻 D5 的程序化符号）。

**仍未做**：§九-1 森林底色蒙版（WGSL 混深绿，属地形着色管线，须另裁）、
§九-3 Chunk 级生成（待大地图性能/接缝问题显现）、
§七 的 Sprite 形态（留待「手绘森林是产品硬需求」时独立裁决）。

## 追记二（2026-10-22，视觉落地回合）：§九-1 与 §七 已裁并落地（ADR-0050）

**用户裁定**（2026-10-22）：「**§七 A + §九-1 A（视觉向）**」——形态契约扩展到
**地形驱动的森林**（位图形态可选，程序化仍是默认），林底色**混在 CPU 烘焙路径**
（**不**走 WGSL）。决议全文见 **`docs/adr/ADR-0050-forest-visual-layer.md`**；
ADR-0031 的 D5 相应**追记二**修正为「默认形态 = 程序化」（D5 的实现不变）。

### §九-1 森林底色蒙版：CPU 烘焙（不是 WGSL）

| Codex 原文 | 本仓库落点 | 理由 |
|---|---|---|
| WGSL 混深绿 | `terrain_view::cells::base_color_of` 里混 `PaletteColors.forest_floor` | 仓库**零自定义材质**（地形是 CPU 烘焙 RGBA → 分块 → `ColorMaterial`）；引入 WGSL 须重写 tint/soften/alpha/mip，且与 ADR-0044 的烘焙渗色抢同一段管线 |
| 逐像素 mask | 逐格密度场（`wv_terrain::forest::forest_density_cell`） | 烘焙契约要求「一格的取值只依赖该格及其邻域」，故把密度公式暴露为**纯函数**（整图场 = 逐格循环版，单测锁定二者恒等） |
| 强度 / 阈值 | `DecorRecipe.forest_base_strength`（默认 0.55）/ `forest_base_min_density`（默认 0.40） | 数值住主题层（ADR-0048 追记二的配方纪律） |

**混色位点选在 `base_color_of` 内** → 林底色跟着 ADR-0044 的 5×5 水彩渗色一起
晕开：森林边缘不是硬边，是洇出来的（手绘古舆图观感）。
**影响面**：仅 VectorFlat / Schematic 两档；瓦片图集档（RealTerrain）与分析档
（Topographic / GoogleMap / Event / Coastline）**按设计不受影响**。

### §七 Sprite 形态：主题可选的第二形态（形态二选一）

- `DecorRecipe.forest_symbol: Option<ForestSymbolSpec>` —— `None` = 程序化形态
  （ADR-0031 D5 原样），`Some` = 位图形态（`overlays/forest/tree_atlas_day.png`，
  3 变体 × 128²）。**形态是主题的一次性选择，不是运行时开关。**
- 新增渲染层 `wv_render::forest_canopy`：按密度场撒手绘树（密度定疏密与树冠
  大小，密度分级定 tint），Z = 0.078，画家算法定前后关系，贴图按 key 缓存。
- **形态二选一**：位图形态下，程序化森林桶（ADR-0031）让位；贴图缺失时告警并
  不画，**不静默回退**到程序化形态。
- 水墨主题（`wv-theme-chinese-ink`）**保持程序化形态**（`forest_symbol: None`）
  + 弱林底（`0.28`）——水墨的树是「点苔」，位图树会破坏画意。

### 与 Codex 原文的偏差（留痕）

| # | Codex 原文 | 偏差 | 原因 |
|---|---|---|---|
| a | 树种按温度分流（针叶 / 阔叶 / 棕榈） | **只按密度分级 tint**，不分流树种 | 温度是生成期派生量、**不落盘**，渲染期拿不到（ADR-0031 追记一已定：群系驱动） |
| b | 树图集 8 变体 | 当前 **3 变体**（`tree_atlas_day.png` 实为 384×128 = 3×128²） | 资产待补；`variants` 是参数，补资产即补变体，**不改代码** |
| c | 树根 y 偏移（`y += 0.05`） | 改为**图集格底边下沉 15%** | 实测图集着墨底边在格高 85%/95%/93%——不补偿则树**浮空**（`TREE_BASE_SINK`） |
| d | WGSL 蒙版 | CPU 烘焙 | 见上表 |

**仍未做**：**§九-3 Chunk 级生成**（待大地图性能/接缝问题显现再启）、
树图集资产扩充（3 → 更多变体）、树种分流（待温度落盘口径确立）。