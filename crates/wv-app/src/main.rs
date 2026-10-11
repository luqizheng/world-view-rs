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
    // TODO(阶段二): Bevy + bevy_egui 骨架，加载工程，
    // 显示地图树 / 角色列表 / 时间轴滑块。暂无地图渲染。
    println!("GUI 骨架待实现（阶段二）");
}
