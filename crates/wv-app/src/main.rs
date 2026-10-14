//! `wv` 命令行入口。
//!
//! ## 职责
//! - 解析 `wv load|validate|raster-slice|overture|gui` 子命令并分发。
//! - GUI 入口 `wv gui [-m <dir>]` 在 `--features app` 下由 [`run_gui`] 提供。
//!
//! ## 边界（不做什么）
//! - **不持有世界数据**——CLI 只通过 `wv_io::Project::open` 加载并打印摘要，
//!   不维护工作副本。要修改必须进 GUI。
//! - **不直接写 YAML**——校验 / 切片仍走 `wv_io::Project::save_world`。
//!
//! ## 主要导出
//! - `main()` — 入口；按子命令派发。
//! - `run_gui()` — `feature = "app"` 才编译的 GUI 启动封装。

mod overture_cli;

use std::process::ExitCode;

use overture_cli::overture_command;
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
        Some("raster-slice") => match raster_slice(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("错误: {msg}");
                ExitCode::FAILURE
            }
        },
        Some("overture") => match overture_command(&args[1..]) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("错误: {msg}");
                ExitCode::FAILURE
            }
        },
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
         wv raster-slice <工程目录> <map_id> [size]
  \x20                  从来源图栅格切出该子图覆盖区域，
  \x20                  采样落盘为 maps/<map_id>/terrain.bin（默认 512²）
         wv overture fetch --bbox W,S,E,N --theme <places|transportation>\n  \
         \x20                  [--release <TAG>] --out <GeoJSON 路径>\n  \
         \x20                  下载 Overture Maps bbox 切片（GeoJSON）\n  \
         \x20                  并按 ADR-0007 §理由（4）转为 PlaceEdgeCandidate 统计\n  \
         wv gui [-m <工程目录>]   启动图形界面（需 --features app）。\n  \
         \x20                        默认进空编辑器，不主动加载任何地图；\n  \
         \x20                        显式 `-m <目录>` 时启动后加载（失败原因在窗口内显示）\n  \
         wv --help                显示本帮助"
    );
}

/// 底图切片（README §12-24）：把来源图栅格中被子图覆盖的那块采样落盘。
fn raster_slice(args: &[String]) -> Result<(), String> {
    let Some(dir) = args.first() else {
        return Err("用法: wv raster-slice <工程目录> <map_id> [size]".into());
    };
    let Some(map_id) = args.get(1) else {
        return Err("缺少 <map_id>。用法: wv raster-slice <工程目录> <map_id> [size]".into());
    };
    let size: u32 = match args.get(2) {
        Some(s) => s.parse().map_err(|_| format!("size {s} 不是正整数"))?,
        None => 512,
    };
    let id = wv_core::Id::new(map_id).map_err(|e| format!("非法 map_id {map_id}: {e}"))?;
    let mut project = Project::open(dir).map_err(|e| e.to_string())?;
    let mut world = project.load_world().map_err(|e| e.to_string())?;
    let data = wv_io::slice_terrain(&world, &id, size).map_err(|e| e.to_string())?;
    world.set_terrain(id.clone(), data);
    project
        .save_world(&world, &[format!("maps/{map_id}/terrain.bin")])
        .map_err(|e| e.to_string())?;
    println!("已切片: maps/{map_id}/terrain.bin（{size}×{size}）");
    println!("重新打开工程后，该图即使用自己的底图（继承退场）。");
    Ok(())
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
        Ok(parsed) => {
            #[cfg(feature = "http")]
            let config = wv_app::AppConfig {
                startup_project: parsed.startup_project,
                http_server_config: parsed.http_server_config,
            };
            #[cfg(not(feature = "http"))]
            let config = wv_app::AppConfig {
                startup_project: parsed.startup_project,
            };
            wv_app::app_bootstrap::run(config)
        }
        Err(msg) => {
            eprintln!("错误: {msg}\n");
            eprintln!(
                "用法: wv gui [-m <工程目录>]\\\n           \
                 [(--http-port <N>) [--http-bind <addr>]]"
            );
            std::process::exit(2);
        }
    }
}

/// `wv gui` 参数解析的产物。
#[cfg(feature = "app")]
#[derive(Debug)]
struct GuiArgs {
    startup_project: Option<String>,
    /// 仅 `--features http` 时填入；其余构建下保持 `()` 形态。
    #[cfg(feature = "http")]
    http_server_config: wv_app::http_integration::HttpServerConfig,
}

/// 解析 `wv gui` 的参数。
///
/// ## 接受的 flag
/// - `-m <dir>`：规范形式，启动后加载该工程；
/// - `<dir>`（位置参数）：旧用法兼容，等价于 `-m <dir>`；
/// - `--http-port <N>`（仅 `--features http`）：覆盖 MCP server 端口；
/// - `--http-bind <addr>`（仅 `--features http`）：覆盖 MCP server 绑定地址。
///
/// ## 失败模式（携带给用户看的原因）
/// - 未知 flag / `-m` 缺值 / 目录重复指定
/// - `--http-port` 非 1..=65535 的整数 / `--http-bind` 不是合法 IPv4:port
/// - 同一 flag 重复指定
///
/// ## 设计取舍
/// 单遍扫描（不重复 `args.clone()`），重复 flag 立刻报错（避免后写的
/// 静默覆盖前写的——LLM 客户端若错传两遍 `--http-port` 我们希望用户
/// 第一眼就发现）。
#[cfg(feature = "app")]
fn parse_gui_args(args: &[String]) -> Result<GuiArgs, String> {
    let mut dir: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut bind: Option<String> = None;
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
            "--http-port" => {
                #[cfg(feature = "http")]
                {
                    let Some(s) = args.get(i + 1) else {
                        return Err(
                            "`--http-port` 需要一个 1..=65535 的整数，例如 `--http-port 9000`"
                                .to_owned(),
                        );
                    };
                    if port.is_some() {
                        return Err("`--http-port` 重复指定".to_owned());
                    }
                    let parsed: u16 = s
                        .parse()
                        .map_err(|_| format!("`--http-port {s}` 不是 1..=65535 的整数"))?;
                    if parsed == 0 {
                        return Err("`--http-port` 必须 ≥ 1".to_owned());
                    }
                    port = Some(parsed);
                    i += 2;
                }
                #[cfg(not(feature = "http"))]
                {
                    let _ = i;
                    return Err(
                        "`--http-port` 需要 `--features http` 才会生效".to_owned(),
                    );
                }
            }
            "--http-bind" => {
                #[cfg(feature = "http")]
                {
                    let Some(s) = args.get(i + 1) else {
                        return Err(
                            "`--http-bind` 需要一个 IPv4 地址，例如 `--http-bind 0.0.0.0`"
                                .to_owned(),
                        );
                    };
                    if bind.is_some() {
                        return Err("`--http-bind` 重复指定".to_owned());
                    }
                    // 用 std::net::SocketAddr 解析（要求 host:port 形态）。
                    // 端口可省：默认 0（让 OS 选），与 axum 的 TcpListener::bind 一致。
                    let probe = if s.contains(':') {
                        s.to_owned()
                    } else {
                        format!("{s}:0")
                    };
                    probe
                        .parse::<std::net::SocketAddr>()
                        .map_err(|_| {
                            format!("`--http-bind {s}` 不是合法的 IPv4:port 形态")
                        })?;
                    bind = Some(s.clone());
                    i += 2;
                }
                #[cfg(not(feature = "http"))]
                {
                    let _ = i;
                    return Err(
                        "`--http-bind` 需要 `--features http` 才会生效".to_owned(),
                    );
                }
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

    // 组装最终结果：port / bind 缺省走 [`HttpServerConfig::default()`]。
    #[cfg(feature = "http")]
    let http_server_config = {
        let mut cfg = wv_app::http_integration::HttpServerConfig::default();
        if let Some(p) = port {
            cfg.port = p;
        }
        if let Some(b) = bind {
            cfg.bind = b;
        }
        cfg
    };
    Ok(GuiArgs {
        startup_project: dir,
        #[cfg(feature = "http")]
        http_server_config,
    })
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
        let parsed = got.expect("no args should parse cleanly");
        assert_eq!(parsed.startup_project, None, "got {parsed:?}");
        #[cfg(feature = "http")]
        assert_eq!(
            parsed.http_server_config.port,
            wv_app::http_integration::HttpServerConfig::default().port,
            "no --http-port → 默认端口 8732"
        );
    }

    #[test]
    fn m_flag_loads_the_given_directory() {
        let got = parse_gui_args(&args(&["-m", "examples/minimal-world"]));
        let parsed = got.expect("valid -m should parse");
        assert_eq!(
            parsed.startup_project,
            Some("examples/minimal-world".to_owned())
        );
    }

    #[test]
    fn positional_directory_still_works_as_legacy_form() {
        let got = parse_gui_args(&args(&["examples/xinhai-1911"]));
        let parsed = got.expect("positional dir should parse");
        assert_eq!(parsed.startup_project, Some("examples/xinhai-1911".to_owned()));
    }

    #[test]
    fn m_flag_without_value_is_rejected_with_usage_hint() {
        assert!(parse_gui_args(&args(&["-m"])).is_err());
    }

    #[test]
    fn duplicate_directory_is_rejected() {
        assert!(parse_gui_args(&args(&["-m", "a", "-m", "b"])).is_err());
    }

    #[test]
    fn unknown_flag_is_rejected() {
        assert!(parse_gui_args(&args(&["--project", "a"])).is_err());
    }

    // ====== HTTP CLI 覆盖（v0.2 增量）======

    #[cfg(feature = "http")]
    #[test]
    fn http_port_override_takes_effect() {
        let parsed = parse_gui_args(&args(&["--http-port", "9000"]))
            .expect("--http-port 应被接受");
        assert_eq!(parsed.http_server_config.port, 9000);
        assert_eq!(parsed.startup_project, None);
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_bind_override_takes_effect() {
        let parsed = parse_gui_args(&args(&["--http-bind", "0.0.0.0"]))
            .expect("--http-bind 应被接受");
        assert_eq!(parsed.http_server_config.bind, "0.0.0.0");
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_bind_with_port_works() {
        // 用户想绑 loopback:9000 → 显式 host:port 形态。
        let parsed = parse_gui_args(&args(&["--http-bind", "127.0.0.1:9000"]))
            .expect("host:port 应被接受");
        assert_eq!(parsed.http_server_config.bind, "127.0.0.1:9000");
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_port_out_of_range_is_rejected() {
        // 65536 越界；非数字也算
        assert!(parse_gui_args(&args(&["--http-port", "65536"])).is_err());
        assert!(parse_gui_args(&args(&["--http-port", "0"])).is_err());
        assert!(parse_gui_args(&args(&["--http-port", "abc"])).is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_bind_garbage_is_rejected() {
        assert!(parse_gui_args(&args(&["--http-bind", "not-an-ip"])).is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_port_and_bind_together_with_m_flag() {
        // 典型 LLM 客户端配置：`-m 工程 --http-port 9000 --http-bind 127.0.0.1`
        let parsed = parse_gui_args(&args(&[
            "-m",
            "examples/xinhai-1911",
            "--http-port",
            "9000",
            "--http-bind",
            "127.0.0.1",
        ]))
        .expect("全 flag 共存应能解析");
        assert_eq!(parsed.startup_project.as_deref(), Some("examples/xinhai-1911"));
        assert_eq!(parsed.http_server_config.port, 9000);
        assert_eq!(parsed.http_server_config.bind, "127.0.0.1");
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_port_repeated_is_rejected() {
        // LLM 客户端错传两遍时**立刻**报错（不静默覆盖）——v0.2 决策
        assert!(parse_gui_args(&args(&["--http-port", "9000", "--http-port", "9001"]))
            .is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_bind_repeated_is_rejected() {
        assert!(parse_gui_args(&args(&["--http-bind", "0.0.0.0", "--http-bind", "127.0.0.1"]))
            .is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_port_without_value_is_rejected_with_usage_hint() {
        assert!(parse_gui_args(&args(&["--http-port"])).is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_bind_without_value_is_rejected_with_usage_hint() {
        assert!(parse_gui_args(&args(&["--http-bind"])).is_err());
    }
}
