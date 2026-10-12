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
         wv gui [-m <工程目录>]   启动图形界面（需 --features app）。\n  \
         \x20                        默认进空编辑器，不主动加载任何地图；\n  \
         \x20                        显式 `-m <目录>` 时启动后加载（失败原因在窗口内显示）\n  \
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
    // 需求（用户裁定）：启动**不主动加载**任何地图。裸 `wv gui` 进空编辑器，
    // 只有显式给出工程目录才加载——规范形式 `wv gui -m <工程目录>`，
    // 位置参数 `<工程目录>` 保留兼容（旧用法 `wv gui examples/...` 不破坏）。
    // 环境变量 / 自动寻根等隐式加载一律移除：隐式行为正是「打开的不是
    // 用户想打开的工程」这类事故的根源（曾命中 target/ 下的陈旧拷贝）。
    let args: Vec<String> = std::env::args().skip(2).collect();
    match parse_gui_args(&args) {
        Ok(dir) => wv_app::gui::run(wv_app::AppConfig {
            startup_project: dir,
        }),
        Err(msg) => {
            eprintln!("错误: {msg}\n");
            eprintln!("用法: wv gui [-m <工程目录>]");
            std::process::exit(2);
        }
    }
}

/// 解析 `wv gui` 的参数。
///
/// - `-m <dir>`：规范形式，启动后加载该工程；
/// - `<dir>`（位置参数）：旧用法兼容，等价于 `-m <dir>`；
/// - 无参数：`Ok(None)`——空编辑器启动，不加载任何工程。
///
/// 返回 `Err` 时携带给用户看的原因（未知参数 / `-m` 缺值 / 目录重复指定）。
#[cfg(feature = "app")]
fn parse_gui_args(args: &[String]) -> Result<Option<String>, String> {
    let mut dir: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-m" => {
                let Some(d) = args.get(i + 1) else {
                    return Err(
                        "`-m` 需要一个工程目录参数，例如 `wv gui -m examples/minimal-world`"
                            .to_owned(),
                    );
                };
                if dir.is_some() {
                    return Err("工程目录重复指定".to_owned());
                }
                dir = Some(d.clone());
                i += 2;
            }
            s if s.starts_with('-') => return Err(format!("未知参数: {s}")),
            s => {
                if dir.is_some() {
                    return Err("工程目录重复指定".to_owned());
                }
                dir = Some(s.to_owned());
                i += 1;
            }
        }
    }
    Ok(dir)
}

#[cfg(all(test, feature = "app"))]
mod gui_args_tests {
    use super::parse_gui_args;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_args_yields_empty_editor_without_any_project() {
        let got = parse_gui_args(&[]);
        assert_eq!(got, Ok(None), "got {got:?}");
    }

    #[test]
    fn m_flag_loads_the_given_directory() {
        let got = parse_gui_args(&args(&["-m", "examples/minimal-world"]));
        assert_eq!(
            got,
            Ok(Some("examples/minimal-world".to_owned())),
            "got {got:?}"
        );
    }

    #[test]
    fn positional_directory_still_works_as_legacy_form() {
        let got = parse_gui_args(&args(&["examples/xinhai-1911"]));
        assert_eq!(
            got,
            Ok(Some("examples/xinhai-1911".to_owned())),
            "got {got:?}"
        );
    }

    #[test]
    fn m_flag_without_value_is_rejected_with_usage_hint() {
        let got = parse_gui_args(&args(&["-m"]));
        assert!(got.is_err(), "expected Err, got {got:?}");
    }

    #[test]
    fn duplicate_directory_is_rejected() {
        let got = parse_gui_args(&args(&["-m", "a", "-m", "b"]));
        assert!(got.is_err(), "expected Err, got {got:?}");
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let got = parse_gui_args(&args(&["--project", "a"]));
        assert!(got.is_err(), "expected Err, got {got:?}");
    }
}
