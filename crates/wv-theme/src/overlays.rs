//! 主题覆盖物（theme overlay）的**数据契约**：一个主题包对各类群系族的
//! 演绎参数（远带晕 + 近带符号）。
//!
//! ## 职责
//! - [`FeatureRule`] —— 一个主题包对一类群系族的演绎参数；
//! - [`FeatureOverlaySet`] —— 群系族 → 规则的映射（缺省回退是硬要求）。
//!
//! ## 边界（不做什么）
//! - **不做采样**（`forest_sites`）与**不做分带计算**（`overlay_far_blend`）——
//!   那两项是渲染运行时，留在 `wv-render`（ADR-0043 D4）；
//! - **不做时间比较**：地界由数据层求值后传入（AGENTS §3）；
//! - **不引入 `rand`**：抖动是手写 splitmix64 哈希，跨进程 / 跨平台稳定。
//!
//! 本模块是**主题包的构造面**（ADR-0023 契约）：外部主题 crate 只依赖
//! `wv-theme`，不必依赖 `wv-render`。

/// 群系族（ADR-0032 D2 规则键）。**经本模块再导出**：外部主题 crate 只依赖
/// wv-theme（ADR-0043 D2），不必直接摸 wv-terrain。
pub use wv_terrain::biome::BiomeFamily;

/// 单个群系族的演绎参数（远带晕 + 近带符号）。
///
/// **归属**：一个主题包对一族地理事实的「长相」；作者只声明事实
/// （`Place.biome`，经 [`wv_terrain::biome::family_of`] 归族），不直接写这些值。
/// v1 为代码常量，YAML 化随 ADR-0023 M3 推迟。
///
/// **贴图字段**（D12 资产契约）：写主题资产根下的相对路径，由渲染侧映射到
/// 编译期嵌入的 PNG（`feature_overlays::embedded_texture`）。`None` = 纯色路径；
/// 键未知 / 解码失败同样回退纯色——缺资产不崩、不空帧。水墨包暂留 `None`，
/// 待美术校准后接线。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureRule {
    /// 远带地界晕的 RGB（sRGB 字节）。渲染色 = 本色 × alpha，不随 palette 走。
    pub halo_color: [u8; 3],
    /// 远带晕浓度（0..=1）；近带交叉淡化时由渲染系统乘 `0.4 + 0.6×blend`。
    pub halo_alpha: f32,
    /// 近带树冠符号的 RGB（sRGB 字节）。
    pub symbol_color: [u8; 3],
    /// 近带符号的屏幕尺寸（像素）；渲染时乘 `ViewState.scale` 换世界尺寸。
    pub symbol_size_px: f32,
    /// 近带采样间距（世界单位）——同时是抖动幅度的基准（见 [`JITTER_FRACTION`]）。
    pub symbol_spacing_wu: f32,
    /// 近带符号贴图（相对主题资产根的路径；3 格横向图集）。`None` = 纯色 quad。
    pub symbol_texture: Option<&'static str>,
    /// 远带晕贴图（按地界包围盒拉伸的单张图章）。`None` = 纯色多边形。
    pub halo_texture: Option<&'static str>,
}

/// 一套主题的群系族 → 演绎规则映射（ADR-0032 D2：规则键 = 群系族）。
///
/// 族划分穷尽冻结群系表（[`BiomeFamily`]），字段**一次定型**：未来加规则
/// 内容不破此结构（外部主题 crate 的 API 稳定面，ADR-0023 D2）。
///
/// **缺省回退是硬要求**（设计稿 §6.4）：主题没给某族规则时
/// [`Self::rule_for`] 返回 `None`，渲染侧仍画地界晕（回退色）+ 地界 + 名字
/// ——事实不因换主题而消失。v1 只有森林族有内置演绎，其余族留 `None`
/// 待美术校准。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureOverlaySet {
    /// 森林族（森林/丛林/热带雨林）的规则；`None` = 本主题不演绎。
    pub forest: Option<FeatureRule>,
    /// 草原族（平原/草原/绿洲/稀树草原）的规则。
    pub grassland: Option<FeatureRule>,
    /// 干旱族（戈壁/沙漠/盐碱地）的规则。
    pub desert: Option<FeatureRule>,
    /// 湿地族（沼泽/湿地）的规则。
    pub wetland: Option<FeatureRule>,
    /// 寒带族（苔原/雪原）的规则。
    pub cold: Option<FeatureRule>,
    /// 高地族（丘陵/山地）的规则。
    pub highland: Option<FeatureRule>,
    /// 水域族（海洋/湖泊/河流）的规则。
    pub water: Option<FeatureRule>,
    /// 道路族（道路）的规则。
    pub road: Option<FeatureRule>,
}

impl FeatureOverlaySet {
    /// 按群系族取规则。
    ///
    /// 族无规则返回 `None`，走「回退色晕 + 地界 + 名字」的回退，不静默消失。
    /// 写一条族规则即覆盖同族全部群系——森林族一条规则同时服务森林、丛林、
    /// 热带雨林三种 `biome`（旧数字键「写 1 漏 10/11」的碎裂就此消除）。
    pub fn rule_for(&self, family: BiomeFamily) -> Option<&FeatureRule> {
        match family {
            BiomeFamily::Forest => self.forest.as_ref(),
            BiomeFamily::Grassland => self.grassland.as_ref(),
            BiomeFamily::Desert => self.desert.as_ref(),
            BiomeFamily::Wetland => self.wetland.as_ref(),
            BiomeFamily::Cold => self.cold.as_ref(),
            BiomeFamily::Highland => self.highland.as_ref(),
            BiomeFamily::Water => self.water.as_ref(),
            BiomeFamily::Road => self.road.as_ref(),
        }
    }
}

// ---------------------------------------------------------------------------
// 区域覆盖物规则（ADR-0045 D2 / D10）
// ---------------------------------------------------------------------------

/// 区域覆盖物规则（ADR-0045 D10）：主题为「自然区域声明的覆盖物键」提供的长相。
///
/// **归属**：区域声明的是**表现层**——「这片深林用针叶画」，不是「这片是什么」
/// （那是 `cell.biome` 的本质分类）。故本规则只管**怎么画**，不管**是什么**。
///
/// **键是开放字符串**（AGENTS ADR-0001 D2「数据承载开放集」；ADR-0032 的碎裂教训
/// ——枚举化会裂成「写 1 漏 10/11」）。词表由主题拥有，数据侧只存字符串。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionOverlayRule {
    /// 变体键（渲染层映射到程序化符号变体：`conifer` / `broadleaf` / `sparse` /
    /// `boulder` / …）。
    ///
    /// **未知变体不回退到「什么都不画」**：渲染层回退到该格群系派生的默认形状
    /// （事实不消失），只保留本规则的颜色 / 尺寸 / 间距。
    pub variant: &'static str,
    /// 符号颜色（sRGB 字节）——覆盖该族默认墨色。
    pub color: [u8; 3],
    /// 符号不透明度（0..=1）；`None` = 用该族默认 alpha。
    pub alpha: Option<f32>,
    /// 符号尺寸倍率（相对默认尺寸；`1.0` = 不变）。
    pub size_scale: f32,
    /// 采样间距倍率（相对默认间距；`< 1.0` = 更密）。
    pub spacing_scale: f32,
}

/// 一套主题的「区域覆盖物键 → 规则」表（ADR-0045 D10）。
///
/// **缺省回退是硬要求**：当前主题没有某键时 [`Self::rule_for`] 返回 `None`，
/// 渲染侧**不静默回退到 cell 派生默认**——那等于拿另一个变体冒充作者声明
/// （ADR-0045 追记一）——而是画**缺失覆盖物标记**（白底红斜线）。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RegionOverlaySet {
    rules: &'static [(&'static str, RegionOverlayRule)],
}

impl RegionOverlaySet {
    /// 空表（主题不提供任何区域覆盖物）。
    pub const EMPTY: RegionOverlaySet = RegionOverlaySet { rules: &[] };

    /// 由静态表构造（外部主题 crate 的构造面）。
    pub const fn new(rules: &'static [(&'static str, RegionOverlayRule)]) -> Self {
        Self { rules }
    }

    /// 按覆盖物键取规则。
    ///
    /// 键不存在 → `None`（调用方据此画缺失覆盖物标记；**不静默回退**）。
    pub fn rule_for(&self, key: &str) -> Option<&RegionOverlayRule> {
        self.rules.iter().find(|(k, _)| *k == key).map(|(_, r)| r)
    }

    /// 全部键（诊断面板列「本主题支持哪些覆盖物」用）。
    pub fn keys(&self) -> impl Iterator<Item = &'static str> {
        self.rules.iter().map(|(k, _)| *k)
    }

    /// 规则条数。
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}
