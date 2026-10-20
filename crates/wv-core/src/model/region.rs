//! 命名区域实体（ADR-0017 §2.2 · `EntityKind::Region`）。
//!
//! 区域是作者划定的**命名多边形**——「楚河汉界」「关中平原」「武昌城垣范围」
//! 这类有名字的面状范围。它是**领域实体**（有 id、有名字、可被选中、
//! 可被引用），不是纯渲染叠加（那是 `source::measurement` 的事）。
//!
//! ## 存储形态（§4.5 每实体一文件）
//!
//! 每个 `Region` 一个 `regions/<id>.yaml`；几何内联为本图局部坐标多边形
//! （区域数量少、顶点不多，YAML 可读可 diff——P3 不需要二进制例外）。
//!
//! ## 为什么不用 `GeometryData`
//!
//! 区域语义上永远是多边形（不像 `GeometryData` 还有点列和引用变体），
//! 直接存 `Vec<f32>` 更简单、类型更精确。`GeometryData` 的变体顺序问题
//! 已修正（`Polygon` 在前），但 Region 仍保持内联——归并无收益、徒增复杂度。
//!
//! ## 时间维度（v0.1 不带）
//!
//! ADR-0017 §2.3 明确：Region v0.1 **永久可见**（只受图级
//! `MapNode.time_span` 否决）。未来加 `state_spans` 时，`label_visible_at`
//! 的调用方把 `entity_span` 从 `None` 换成实体窗口即可，渲染侧零改动。
//!
//! ## 自然区域扩展（ADR-0045）
//!
//! 区域实体在 ADR-0045 里扩成两种类别（见 [`RegionKind`]）：
//!
//! - `Area`（默认，现状语义）：作者划定的命名面。不承载覆盖物、不嵌套。
//! - `Nature`（自然区）：一片具名的自然地貌范围，可承载**覆盖物键**声明
//!   （表现层：这片深林用针叶画）、可**嵌套**（`parent` 指父区）、可由
//!   **自动分区**（biome 连通区）物化、边界可重生成。
//!
//! 四个新字段全部 `serde(default)` → **旧 YAML 零迁移**（`kind` 缺省 = `Area`，
//! 与引入前的 `Region` 语义逐位一致）。

use crate::base::{geometry::Point, id::Id};

/// 区域类别（ADR-0045 D3）。
///
/// 判别标准不是装饰，是**三处行为分叉**：`overlay` / `parent` / 洞环与重生成
/// 只在 `Nature` 生效（`Area` 携带 `overlay` 视为无效——校验面板提示，
/// V1 不阻止）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum RegionKind {
    /// 区域（默认）：作者划定的命名范围。不承载覆盖物、不嵌套。
    #[default]
    Area,
    /// 自然区（ADR-0045）：承载覆盖物键声明、可嵌套、可由自动分区物化、
    /// 边界可重生成。
    Nature,
}

impl RegionKind {
    /// 中文显示名（UI 用；YAML 仍存 snake_case 英文键）。
    pub fn label(self) -> &'static str {
        match self {
            RegionKind::Area => "区域",
            RegionKind::Nature => "自然区",
        }
    }
}

/// 命名区域：一个有名字的多边形范围，锚定在某张地图上。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Region {
    pub id: Id,
    /// 显示名（标签渲染的文本来源——标签是实体的投影，不存第二份）。
    ///
    /// **空串 = 无名**（ADR-0045 D7）：无名区域**不显示在地图上**（标签跳过）。
    /// 自动分区候选根本不落盘（命名即物化），无名的物化区域同样不显示。
    pub name: String,
    /// 所属地图（`polygon` 用该图的局部坐标）。
    pub map: Id,
    /// 区域轮廓：扁平顶点 `[x0, y0, x1, y1, ...]`，首尾不重复。
    /// 空轮廓 = 未画完（校验可提示，不阻止保存——V1）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polygon: Vec<f32>,
    /// 洞环（ADR-0045 D3）：自动分区的 biome 连通区常带洞（林中湖），
    /// 单环装不下。扁平点列，语义同 `polygon`（图内局部 wu）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<f32>>,
    /// 区域类别（ADR-0045 D3）。缺省 = `Area`（旧 YAML 零迁移）。
    #[serde(default)]
    pub kind: RegionKind,
    /// 父区域（ADR-0045 D3 / D5）：嵌套引用（仅 `Nature` 生效）。
    ///
    /// 合并两个自然区时，旧成员**变子块**（`parent` 指新区，成员保留不删）
    /// ——「合并」是「嵌套」的特例。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
    /// 覆盖物键（ADR-0045 D2）：**表现层**声明——只说「这片怎么画」，
    /// 不说「这片是什么」（那是 `cell.biome` 的本质分类）。
    ///
    /// 键是**开放字符串**，词表由主题拥有（`ThemePack`；AGENTS ADR-0001 D2
    /// 「数据承载开放集」）。当前主题无此键 → **画缺失覆盖物标记**（白底红斜线，
    /// ADR-0045 追记一），**不回退**（回退等于拿另一个变体冒充作者声明）。
    ///
    /// 仅 `kind = Nature` 生效。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<String>,
}

impl Region {
    /// 多边形质心（简单平均）——标签锚点（ADR-0017 §2.5「几何中心」）。
    ///
    /// 顶点分布不均时不是几何学重心，但作为枢轴足够（工程口径：够用、
    /// 稳定、无依赖）；空轮廓回 `None`。
    pub fn centroid(&self) -> Option<Point> {
        if self.polygon.len() < 2 {
            return None;
        }
        let n = (self.polygon.len() / 2) as f32;
        let (mut sx, mut sy) = (0.0_f32, 0.0_f32);
        for pair in self.polygon.chunks(2) {
            sx += pair[0];
            sy += pair[1];
        }
        Some(Point::new(sx / n, sy / n))
    }

    /// 是否自然区（ADR-0045 D3）。
    pub fn is_nature(&self) -> bool {
        self.kind == RegionKind::Nature
    }

    /// 是否无名（ADR-0045 D7）——无名区域**不显示在地图上**。
    pub fn is_unnamed(&self) -> bool {
        self.name.trim().is_empty()
    }

    /// 生效的覆盖物键（ADR-0045 D2/D3）。
    ///
    /// 只有 `kind = Nature` 才认 `overlay`；`Area` 携带 `overlay` 视为无效
    /// （校验面板提示，V1 不阻止）——泛用面挂覆盖物会引入无意义的组合爆炸。
    pub fn declared_overlay(&self) -> Option<&str> {
        if !self.is_nature() {
            return None;
        }
        self.overlay.as_deref().filter(|k| !k.trim().is_empty())
    }

    /// 区域深度（用于「最深优先」的覆盖物解析，ADR-0045 D8）。
    ///
    /// 深度 = 沿 `parent` 链上溯的层数（自身为 0）。链上出现环时按最大步数
    /// 收敛（脏数据不 panic，V1）。
    pub fn depth(&self, regions: &[Region]) -> usize {
        let mut depth = 0usize;
        let mut cur = self.parent.clone();
        let mut guard = 0usize;
        while let Some(pid) = cur {
            guard += 1;
            if guard > regions.len() + 1 {
                break;
            }
            let Some(p) = regions.iter().find(|r| r.id == pid) else {
                break;
            };
            depth += 1;
            cur = p.parent.clone();
        }
        depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(polygon: Vec<f32>) -> Region {
        Region {
            id: Id::new_unchecked("region_guanzhong"),
            name: "关中平原".into(),
            map: Id::new_unchecked("map_world"),
            polygon,
            holes: Vec::new(),
            kind: RegionKind::Area,
            parent: None,
            overlay: None,
        }
    }

    fn nature(id: &str, name: &str, parent: Option<&str>, overlay: Option<&str>) -> Region {
        Region {
            id: Id::new_unchecked(id),
            name: name.into(),
            map: Id::new_unchecked("map_world"),
            polygon: vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0],
            holes: Vec::new(),
            kind: RegionKind::Nature,
            parent: parent.map(Id::new_unchecked),
            overlay: overlay.map(str::to_string),
        }
    }

    #[test]
    fn centroid_is_the_vertex_average_of_the_polygon() {
        // 方形四顶点 (0,0)(4,0)(4,4)(0,4) → 质心 (2,2)。
        let r = region(vec![0.0, 0.0, 4.0, 0.0, 4.0, 4.0, 0.0, 4.0]);
        let c = r.centroid().expect("四边形有质心");
        assert!(
            (c.x - 2.0).abs() < 1e-4 && (c.y - 2.0).abs() < 1e-4,
            "质心应为 (2, 2)，实得 ({}, {})",
            c.x,
            c.y
        );
    }

    #[test]
    fn centroid_is_none_for_empty_polygon() {
        assert!(region(vec![]).centroid().is_none(), "空轮廓无锚点");
        assert!(region(vec![3.0]).centroid().is_none(), "半对顶点无锚点");
    }

    #[test]
    fn default_kind_is_area_and_old_yaml_still_loads() {
        // 旧 YAML 只有 4 个字段 → kind 缺省 = Area，语义逐位一致（零迁移）。
        let old_yaml = "id: region_a\nname: 甲\nmap: map_world\npolygon: [0,0,1,0,1,1]\n";
        let r: Region = serde_yaml::from_str(old_yaml).expect("旧 YAML 应可读");
        assert_eq!(r.kind, RegionKind::Area);
        assert!(!r.is_nature());
        assert!(r.holes.is_empty());
        assert!(r.parent.is_none());
        assert!(r.overlay.is_none());
    }

    #[test]
    fn area_region_ignores_declared_overlay() {
        // Area 携带 overlay 视为无效（ADR-0045 D3）——declared_overlay 回 None。
        let mut r = region(vec![0.0, 0.0, 1.0, 0.0]);
        r.overlay = Some("conifer".into());
        assert!(r.declared_overlay().is_none());
    }

    #[test]
    fn nature_region_exposes_its_overlay_key() {
        let r = nature("nr_a", "迷幻深林", None, Some("conifer"));
        assert_eq!(r.declared_overlay(), Some("conifer"));
    }

    #[test]
    fn empty_overlay_key_counts_as_no_declaration() {
        let r = nature("nr_b", "空白", None, Some("  "));
        assert!(r.declared_overlay().is_none(), "空白键不算声明");
    }

    #[test]
    fn unnamed_region_is_detected_by_empty_name() {
        let mut r = nature("nr_c", "老君山", None, None);
        assert!(!r.is_unnamed());
        r.name = String::new();
        assert!(r.is_unnamed());
        r.name = "   ".into();
        assert!(r.is_unnamed(), "纯空白也算无名");
    }

    #[test]
    fn depth_counts_parent_chain() {
        let parent = nature("nr_p", "山脉", None, None);
        let child = nature("nr_c", "山峰", Some("nr_p"), None);
        let regions = vec![parent.clone(), child.clone()];
        assert_eq!(parent.depth(&regions), 0);
        assert_eq!(child.depth(&regions), 1);
    }

    #[test]
    fn depth_survives_parent_cycle_without_panicking() {
        // 脏数据：parent 指向自己 → 不能死循环。
        let mut r = nature("nr_loop", "环", Some("nr_loop"), None);
        r.parent = Some(Id::new_unchecked("nr_loop"));
        let regions = vec![r];
        assert!(regions[0].depth(&regions) <= 2, "环上深度应收敛");
    }
}
