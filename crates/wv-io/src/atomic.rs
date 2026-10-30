//! 原子写盘：先写同目录 `<name>.tmp`，再 rename 覆盖目标。
//!
//! ## 为什么（崩溃恢复的另一半）
//!
//! `std::fs::write` 是「打开并截断 → 写入」两步。崩溃 / 断电发生在两步之间时，
//! 目标文件就变成 0 字节或半截内容。对 `world.yaml` 这类「打开工程的总入口」
//! 而言，这等于**整个工程打不开**——比 UX-001 里「丢最后 60 秒编辑」严重得多，
//! 而且自动保存越勤，踩中这个窗口的机会越多。
//!
//! 同一目录内的 `rename` 是原子的：读者要么看到旧内容、要么看到新内容，不会
//! 看到半截。于是「崩溃时最坏损失」被钉在「上一个完整版本」而不是「一个坏文件」。
//!
//! ## 边界
//!
//! - **不是 fsync**：本函数挡住的是进程级崩溃（任务管理器杀进程 / panic）与
//!   写入中断；断电时还差一层 `File::sync_all`。本仓库的落盘温度（作者每几秒
//!   一次编辑）不值得每笔都 fsync，先不引入。
//! - 失败即清理：rename 失败时删掉自己留下的 `.tmp`，不给作者留垃圾。
//! - 同目录 `.tmp` 不会被实体目录的删除传播扫到（那里只认 `.yaml` 扩展名），
//!   也不会被 `RasterRef` 之类按名字引用的读取路径看到。

use std::path::{Path, PathBuf};

use crate::error::IoResult;

/// 目标的同目录临时路径：`world.yaml` → `world.yaml.tmp`。
///
/// 用 `OsString` 拼接而不是 `with_extension`——后者会把 `world.yaml` 变成
/// `world.tmp`，与作者手动打开的文件名撞车。
fn tmp_sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

/// 原子写文本（YAML 等）。
pub(crate) fn write_text(path: &Path, text: &str) -> IoResult<()> {
    write_bytes(path, text.as_bytes())
}

/// 原子写字节（地形 / 街道 / 建筑二进制）。
///
/// 先写临时文件再 `rename`：写一半失败时目标文件仍是**上一个完整版本**。
pub(crate) fn write_bytes(path: &Path, bytes: &[u8]) -> IoResult<()> {
    let tmp = tmp_sibling(path);
    std::fs::write(&tmp, bytes)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("wv_atomic_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }

    /// 覆盖写：内容换成新的，且不留下 `.tmp` 垃圾。
    #[test]
    fn atomic_write_replaces_content_and_leaves_no_temp_file() {
        let dir = scratch_dir("replace");
        let target = dir.join("world.yaml");
        std::fs::write(&target, "old").expect("先放旧内容");
        write_text(&target, "new").expect("原子写");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件：{leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 写一半失败（这里用「tmp 路径被一个目录占住」注入）：旧文件必须原样保住。
    ///
    /// 这条钉住的正是崩溃恢复的底线——**失败不能把好文件赔进去**。
    #[test]
    fn failed_write_keeps_the_previous_content_intact() {
        let dir = scratch_dir("failure");
        let target = dir.join("world.yaml");
        std::fs::write(&target, "good").expect("先放好内容");
        // 占住 `<target>.tmp`：`fs::write` 到这里必然失败。
        std::fs::create_dir(dir.join("world.yaml.tmp")).expect("占住临时路径");
        let result = write_text(&target, "new");
        assert!(result.is_err(), "tmp 写不进去时必须报错，不能假装成功");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "good",
            "写失败时旧内容必须原样保留"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 二进制同款：`write_bytes` 覆盖写后内容一致。
    #[test]
    fn atomic_write_bytes_replaces_binary_content() {
        let dir = scratch_dir("bytes");
        let target = dir.join("terrain.bin");
        write_bytes(&target, &[1, 2, 3]).expect("首次写");
        write_bytes(&target, &[9, 9]).expect("覆盖写");
        assert_eq!(std::fs::read(&target).unwrap(), vec![9, 9]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
