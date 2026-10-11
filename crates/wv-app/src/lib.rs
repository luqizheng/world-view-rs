//! # wv-app —— World View 应用层
//!
//! Bevy 渲染与 UI。**默认不编译**，需 `--features app`。
//!
//! 职责边界（§5.1 数据与渲染分离）：
//! - 本层**不持有**世界数据。世界数据是 `wv-core` 的类型，
//!   本层读取它、把投影结果写入 Bevy 资源。
//! - 本层**不直接改**世界。所有修改经 `wv-edit` 的命令。
//! - 本层**不直接读盘**。加载经 `wv-io`。

/// 应用层持有的一次性配置。
pub struct AppConfig {
    /// 要打开的工程目录。
    pub project_dir: String,
}
