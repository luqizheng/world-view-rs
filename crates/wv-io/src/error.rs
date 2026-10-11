//! IO 层错误类型。

pub type IoResult<T> = Result<T, IoError>;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("文件系统错误: {0}")]
    Fs(#[from] std::io::Error),

    #[error("YAML 解析失败于 `{path}`: {source}")]
    Yaml {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("数据层错误: {0}")]
    Core(#[from] wv_core::CoreError),

    #[error("工程目录 `{0}` 不存在或缺少 world.yaml")]
    NotAProject(String),

    #[error("schema 版本 {found} 超出本版本支持范围（当前 {current}），需要迁移")]
    SchemaTooNew { found: u32, current: u32 },

    #[error(
        "schema 版本过旧: 工程为 v{found}，当前为 v{current}。\
         v1 的时间模型是「纪元+年+日」字段式，v2 改为天数计数器，\
         无自动迁移。请手动把 `{path}` 中的时间字段改为天数计数器（如 `from: 100805`），\
         并把 `schema_version` 改为 {current}。"
    )]
    SchemaTooOld {
        found: u32,
        current: u32,
        path: String,
    },

    #[error("二进制分块格式非法: {0}")]
    Binary(String),
}
