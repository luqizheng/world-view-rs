//! `wv` 命令行入口。
//!
//! 阶段一交付物：`wv load <dir>` 打印世界摘要。
//! GUI 入口在开启 `app` feature 后由 [`run_gui`] 提供。

use std::process::ExitCode;

use wv_io::Project;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("load") => {
            let Some(dir) = args.get(1) else {
                eprintln!("用法: wv load <工程目录>");
                return ExitCode::from(2);
            };
            match load(dir) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("错误: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("validate") => {
            let Some(dir) = args.get(1) else {
                eprintln!("用法: wv validate <工程目录>");
                return ExitCode::from(2);
            };
            match validate(dir) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("错误: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("gui") => {
            #[cfg(feature = "app")]
            {
                run_gui();
                ExitCode::SUCCESS
            }
            #[cfg(not(feature = "app"))]
            {
                eprintln!("GUI 未编译。请使用 `cargo run -p wv-app --features app -- gui`");
                ExitCode::from(2)
            }
        }
        Some("--help") | Some("-h") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("未知命令: {other}\n");
            print_help();
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    println!(
        "wv —— World View 世界编辑器\n\n\
         用法:\n  \
         wv load <工程目录>       加载工程并打印摘要\n  \
         wv validate <工程目录>   校验工程结构与引用完整性\n  \
         wv gui                   启动图形界面（需 --features app）\n  \
         wv --help                显示本帮助"
    );
}

fn load(dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let project = Project::open(dir)?;
    println!("{}", project.summary());
    let warnings = project.validate()?;
    if !warnings.is_empty() {
        println!("\n提示:");
        for w in warnings {
            println!("  - {w}");
        }
    }
    Ok(())
}

fn validate(dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let project = Project::open(dir)?;
    let warnings = project.validate()?;
    if warnings.is_empty() {
        println!("校验通过: {}", project.root().display());
    } else {
        println!("校验发现 {} 项问题:", warnings.len());
        for w in &warnings {
            println!("  - {w}");
        }
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(feature = "app")]
fn run_gui() {
    // 工程目录解析优先级：
    //   1. 显式参数 `wv gui <目录>`
    //   2. 环境变量 `WV_PROJECT`（便于在 IDE / 任意目录下启动）
    //   3. 从可执行文件位置向上寻找工程根（解决在 `target/debug` 下启动时
    //      相对路径 `examples/minimal-world` 解析失败的问题）
    //   4. 当前工作目录下的 `examples/minimal-world`（最后兜底）
    let args: Vec<String> = std::env::args().collect();
    let project_dir = args
        .get(2)
        .cloned()
        .or_else(|| std::env::var("WV_PROJECT").ok())
        .or_else(resolve_default_project)
        .or_else(|| Some("examples/minimal-world".to_string()));
    wv_app::gui::run(wv_app::AppConfig { project_dir });
}

/// 从可执行文件位置向上回溯，寻找含 `examples/minimal-world/world.yaml` 的工程根。
///
/// 覆盖两种典型布局：
/// - `target/debug/wv.exe` → 上溯两级到工程根
/// - `target/release/wv.exe` → 同上
/// - 任意 `cargo run` 的工作目录（直接命中当前目录）
#[cfg(feature = "app")]
fn resolve_default_project() -> Option<String> {
    const REL: &str = "examples/minimal-world";
    const MARKER: &str = "examples/minimal-world/world.yaml";

    // 先试当前工作目录。
    if std::path::Path::new(MARKER).is_file() {
        return Some(REL.to_string());
    }

    // 再从可执行文件所在目录逐级上溯。
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?;
    for _ in 0..6 {
        let candidate = dir.join(MARKER);
        if candidate.is_file() {
            return Some(dir.join(REL).to_string_lossy().into_owned());
        }
        dir = dir.parent()?;
    }
    None
}
