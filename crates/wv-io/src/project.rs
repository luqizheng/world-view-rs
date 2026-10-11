//! 工程目录的加载与保存。
//!
//! 目录布局见项目文档 §6。本模块只实现**阶段一**所需的最小闭环：
//! 读 `world.yaml` → 解析元数据 → 校验目录存在性 → 打印摘要。
//! 完整加载（地图/人物/事件/距离）按里程碑逐步补齐。

use std::path::{Path, PathBuf};

use wv_core::{World, WorldMeta};

use crate::error::{IoError, IoResult};

/// 一个已定位的工程。持有根目录路径与已解析的元数据。
#[derive(Debug, Clone)]
pub struct Project {
    root: PathBuf,
    meta: WorldMeta,
}

impl Project {
    /// 打开一个工程目录。
    ///
    /// 只读取 `world.yaml` 并做基本校验；不递归加载全部实体。
    /// 这样 `wv load` 在大工程上仍是常数级开销。
    pub fn open(root: impl AsRef<Path>) -> IoResult<Self> {
        let root = root.as_ref().to_path_buf();
        let index = root.join("world.yaml");
        if !index.is_file() {
            return Err(IoError::NotAProject(root.display().to_string()));
        }

        let text = std::fs::read_to_string(&index)?;
        let meta: WorldMeta = serde_yaml::from_str(&text).map_err(|source| IoError::Yaml {
            path: index.display().to_string(),
            source,
        })?;

        if meta.schema_version > crate::CURRENT_SCHEMA_VERSION {
            return Err(IoError::SchemaTooNew {
                found: meta.schema_version,
                current: crate::CURRENT_SCHEMA_VERSION,
            });
        }

        Ok(Self { root, meta })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn meta(&self) -> &WorldMeta {
        &self.meta
    }

    /// 加载完整世界。
    ///
    /// TODO(阶段一): 递归加载 maps / timeline / characters / relations / distances。
    /// 当前返回仅含元数据的骨架，以便 CLI 摘要先行可用。
    pub fn load_world(&self) -> IoResult<World> {
        Ok(World {
            meta: Some(self.meta.clone()),
            ..Default::default()
        })
    }

    /// 生成人类可读的工程摘要，供 `wv load <dir>` 打印。
    pub fn summary(&self) -> String {
        let m = &self.meta;
        format!(
            "工程: {name}\n  版本:     {ver}\n  schema:   v{schema}\n  时间轴:   {from} .. {to}\n  根地图:   {root_map}",
            name = m.name,
            ver = m.version,
            schema = m.schema_version,
            from = m.timeline_range.from,
            to = m
                .timeline_range
                .to
                .as_ref()
                .map(|t| t.to_string())
                .unwrap_or_else(|| "至今".into()),
            root_map = m.root_map,
        )
    }

    /// 校验工程内部引用完整性。
    ///
    /// 当前为骨架实现：检查各预期子目录是否存在。
    /// 悬空引用检测（`CoreError::DanglingReference`）随加载器补齐。
    pub fn validate(&self) -> IoResult<Vec<String>> {
        let expected = ["maps", "timeline", "characters", "relations", "distances"];
        let mut warnings = Vec::new();
        for dir in expected {
            if !self.root.join(dir).exists() {
                warnings.push(format!("缺少预期目录: {dir}/"));
            }
        }
        Ok(warnings)
    }
}
