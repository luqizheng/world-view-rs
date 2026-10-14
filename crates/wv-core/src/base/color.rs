//! 主题无关的颜色值。
//!
//! 本模块只解决一个问题：**YAML 里写的是字符串，数据层该拿什么类型接住它？**
//!
//! 选择「`#[serde(transparent)]` 新类型包 `String`」，而不是 `r/g/b` 三字段结构，
//! 是因为颜色的可读性优先于可分解性——作者写 `color: "#7AB8FF"` 比写
//! `color: {r: 122, g: 184, b: 255}` 更接近他在 HTML / CSS 里的习惯，
//! 而 YAML diff 也更短。需要分量时走 [`Color::to_rgb`]。
//!
//! **数据层不校验颜色是否合法**。解析失败返回 `None`，由渲染层回退到主题色；
//! 把「颜色写错了」变成加载失败会违反 `AGENTS.md` §7.5V1（校验只提示，不阻止）——
//! 作者写错一个十六进制不该连工程都打不开。

use serde::{Deserialize, Serialize};

/// 一个颜色值。YAML 里就是一个带引号的十六进制字符串：`color: "#7AB8FF"`。
///
/// **作者注意**：YAML 里 `#` 是注释起始符，所以颜色值**必须加引号**。
/// 不加引号时它会被当成注释整体丢弃，该字段表现为「未声明」——
/// 渲染层按缺省色处理，不报错（§7.5V1）。
///
/// 之所以是透明序列化而非 `r/g/b` 字段：见本模块文档。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Color(pub String);

impl Color {
    /// 原样返回底层字符串。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 解析为 sRGB 八位分量。
    ///
    /// 接受 `#RRGGBB` 与 `#RRGGBBAA`，也宽容地接受不带 `#` 前缀的 6 / 8 位形式。
    /// 带 alpha 时只取前三个分量——`wv-core` 的颜色没有透明度概念
    /// （透明度是图层属性，见 [`crate::map::Layer::opacity`]）。
    ///
    /// 返回 `None` 表示「解析不出来」，**不是错误**：调用方负责回退。
    pub fn to_rgb(&self) -> Option<[u8; 3]> {
        let s = self.0.trim().trim_start_matches('#');
        if s.len() != 6 && s.len() != 8 {
            return None;
        }
        let mut out = [0u8; 3];
        for (i, byte) in out.iter_mut().enumerate() {
            let hex = &s[i * 2..i * 2 + 2];
            *byte = u8::from_str_radix(hex, 16).ok()?;
        }
        Some(out)
    }
}

impl std::ops::Deref for Color {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::Color;
    use std::ops::Deref;

    #[test]
    fn six_digit_hex_parses_to_srgb_bytes() {
        assert_eq!(Color("#7AB8FF".to_string()).to_rgb(), Some([122, 184, 255]),);
    }

    #[test]
    fn eight_digit_hex_keeps_rgb_and_drops_alpha() {
        // 第四个字节是 alpha，`to_rgb` 明确丢弃它。
        assert_eq!(Color("#00000000".to_string()).to_rgb(), Some([0, 0, 0]),);
        assert_eq!(
            Color("#FFFFFF80".to_string()).to_rgb(),
            Some([255, 255, 255]),
        );
    }

    #[test]
    fn hash_prefix_is_optional() {
        // 缺 `#` 也照收——YAML 里不带引号的裸十六进制会被当成普通字符串读进来。
        assert_eq!(
            Color("F2C14E".to_string()).to_rgb(),
            Some([0xF2, 0xC1, 0x4E])
        );
    }

    #[test]
    fn three_digit_shorthand_is_not_accepted() {
        // 有意不支持 `#FFF` 简写：YAML 里三字符太容易和路径片段、代号混淆。
        assert_eq!(Color("#FFF".to_string()).to_rgb(), None);
        assert_eq!(Color("#7AB8F".to_string()).to_rgb(), None);
        assert_eq!(Color("#7AB8FFFFF".to_string()).to_rgb(), None);
    }

    #[test]
    fn non_hex_bytes_return_none_instead_of_panic() {
        // 这是 `AGENTS.md` §7.5V1 的边界：解析失败只能是 `None`，不能 panic、
        // 不能让工程打不开。
        assert_eq!(Color("#GGGGGG".to_string()).to_rgb(), None);
        assert_eq!(Color("#12".to_string()).to_rgb(), None);
        assert_eq!(Color("".to_string()).to_rgb(), None);
        assert_eq!(Color("#".to_string()).to_rgb(), None);
        assert_eq!(Color("not-a-color".to_string()).to_rgb(), None);
    }

    #[test]
    fn whitespace_around_the_value_is_tolerated() {
        assert_eq!(
            Color("  #A0B0C0 ".to_string()).to_rgb(),
            Some([0xA0, 0xB0, 0xC0]),
        );
    }

    #[test]
    fn as_str_and_deref_and_display_all_yield_the_raw_string() {
        let c = Color("#123456".to_string());
        assert_eq!(c.as_str(), "#123456");
        assert_eq!(c.deref(), "#123456");
        assert_eq!(c.to_string(), "#123456");
        // `Eq`/`Hash` 是按字符串判等的，同串同哈希。
        let mut seen = std::collections::HashSet::new();
        assert!(seen.insert(c.clone()));
        assert!(!seen.insert(Color("#123456".to_string())));
    }
}
