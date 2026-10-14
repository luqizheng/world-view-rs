//! 数据层错误类型。

/// 数据层通用结果。
pub type CoreResult<T> = Result<T, CoreError>;

/// 数据层错误。
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("ID 校验失败: {0}")]
    Id(#[from] crate::base::id::IdError),

    #[error("引用了不存在的实体 `{referrer}` -> `{missing}`")]
    DanglingReference { referrer: String, missing: String },

    #[error("时间区间非法: from={from} to={to}（from 必须不晚于 to）")]
    InvalidSpan { from: String, to: String },

    #[error("时间点 `{0}` 超出工程时间轴范围")]
    OutOfRange(String),

    #[error("距离图中不存在地点 `{0}`")]
    UnknownPlace(String),

    #[error("路径搜索失败: {0}")]
    PathSearch(String),
}
