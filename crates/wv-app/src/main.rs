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

use std::process::ExitCode;

use wv_app::i18n::{cli_locale, parse as parse_locale, set_cli_locale, Locale};
use wv_io::Project;

/// 进程启动第一步：把任意 `--language <zh|en>` 全局标记从 argv 里挑出来，
/// 设进 [`wv_app::i18n::set_cli_locale`]，剩余参数从原地后移一格。
///
/// 这个标记**必须**在所有子命令派发之前应用——一旦子命令开始 eprintln 错误
/// 信息，locale 已是最终值。`--language` 出现在位置任意（只要是 argv 的一
/// 个独立 token）都生效；缺值 / 重复 / 不在白名单内则报错并打印 help。
///
/// 整体实现走「扫一遍 args，把 `--language <v>` 切掉，剩余传给后续 dispatch」
/// —— 与主派发解耦，新加标记也按同口径加。
fn extract_language_arg(args: Vec<String>) -> Result<Vec<String>, String> {
    let loc = cli_locale();
    let t = |k: wv_app::i18n::I18nKey| -> String { wv_app::i18n::t(loc, k).to_owned() };
    let mut out = Vec::with_capacity(args.len());
    let mut set = false;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--language" {
            if set {
                return Err(t(wv_app::i18n::I18nKey::CliLanguageDup));
            }
            let val = args
                .get(i + 1)
                .ok_or_else(|| t(wv_app::i18n::I18nKey::CliLanguageMissing))?;
            let loc =
                parse_locale(val).ok_or_else(|| t(wv_app::i18n::I18nKey::CliLanguageBadTag))?;
            set_cli_locale(loc);
            set = true;
            i += 2;
        } else {
            out.push(args[i].clone());
            i += 1;
        }
    }
    if !set {
        // 兜底：未显式给也登记一下默认 zh，让 cli_locale() 始终有值，
        // 避免 [首次输出] 之前误用 .unwrap_or_default() 双重语义。
        set_cli_locale(Locale::default());
    }
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = match extract_language_arg(args) {
        Ok(a) => a,
        Err(msg) => {
            eprintln!("错误: {msg}");
            print_help();
            return ExitCode::from(2);
        }
    };
    let loc = cli_locale();
    let t = |k| wv_app::i18n::t(loc, k);
    match args.first().map(String::as_str) {
        Some("load") => {
            let Some(dir) = args.get(1) else {
                eprintln!("{}", t(wv_app::i18n::I18nKey::CliUsageLoad));
                return ExitCode::from(2);
            };
            match load(dir, loc) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{}{e}", t(wv_app::i18n::I18nKey::CliErrorPrefix));
                    ExitCode::FAILURE
                }
            }
        }
        Some("validate") => {
            let Some(dir) = args.get(1) else {
                eprintln!("{}", t(wv_app::i18n::I18nKey::CliUsageValidate));
                return ExitCode::from(2);
            };
            match validate(dir, loc) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{}{e}", t(wv_app::i18n::I18nKey::CliErrorPrefix));
                    ExitCode::FAILURE
                }
            }
        }
        Some("raster-slice") => match raster_slice(&args[1..], loc) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("{}{msg}", t(wv_app::i18n::I18nKey::CliErrorPrefix));
                ExitCode::FAILURE
            }
        },
        Some("overture") => match wv_app::overture_cli::overture_command(&args[1..], loc) {
            Ok(()) => ExitCode::SUCCESS,
            Err(msg) => {
                eprintln!("{}{msg}", t(wv_app::i18n::I18nKey::CliErrorPrefix));
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
                eprintln!("{}", t(wv_app::i18n::I18nKey::CliGuiNotCompiled));
                ExitCode::from(2)
            }
        }
        Some("mcp") => {
            // wv mcp（ADR-0083）：MCP stdio 代理——本地答 initialize/ping，
            // tools/* 转发 GUI 内嵌 HTTP 服务（默认 127.0.0.1:8732）。
            #[cfg(feature = "http")]
            {
                let mut port = wv_app::http::DEFAULT_PORT;
                let mut it = args.iter().skip(1);
                while let Some(a) = it.next() {
                    if a == "--port" {
                        match it.next().and_then(|v| v.parse::<u16>().ok()) {
                            Some(n) => port = n,
                            None => {
                                eprintln!("wv mcp: --port 需要 1..=65535 的整数");
                                return ExitCode::from(2);
                            }
                        }
                    } else {
                        eprintln!("wv mcp: 未知参数 {a}（用法：wv mcp [--port <N>]）");
                        return ExitCode::from(2);
                    }
                }
                let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
                ExitCode::from(rt.block_on(wv_app::http::proxy::run_stdio(port)) as u8)
            }
            #[cfg(not(feature = "http"))]
            {
                eprintln!(
                    "wv mcp 需要 --features http 编译（cargo run -p wv-app --features http --bin wv -- mcp）"
                );
                ExitCode::from(2)
            }
        }
        Some("gallery") => {
            #[cfg(feature = "app")]
            {
                wv_app::ui_gallery::run();
                ExitCode::SUCCESS
            }
            #[cfg(not(feature = "app"))]
            {
                eprintln!("{}", t(wv_app::i18n::I18nKey::CliGuiNotCompiled));
                ExitCode::from(2)
            }
        }
        Some("--help") | Some("-h") | None => {
            print_help();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!(
                "{}{other}",
                t(wv_app::i18n::I18nKey::CliUnknownCommandPrefix)
            );
            print_help();
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    let loc = cli_locale();
    let t = |k| wv_app::i18n::t(loc, k);
    println!(
        "{banner}\n\n\
         {usage_header}:\n\
         {load}\n\
         {validate}\n\
         {raster}\n\
         {overture}\n\
         {gui}\n\
         {gallery}\n\
         {lang}\n\
         {help}",
        banner = t(wv_app::i18n::I18nKey::CliHelpBanner),
        usage_header = t(wv_app::i18n::I18nKey::CliHelpUsageHeader),
        load = t(wv_app::i18n::I18nKey::CliHelpLoad),
        validate = t(wv_app::i18n::I18nKey::CliHelpValidate),
        raster = t(wv_app::i18n::I18nKey::CliHelpRasterSlice),
        overture = t(wv_app::i18n::I18nKey::CliHelpOvertureFetch),
        gui = t(wv_app::i18n::I18nKey::CliHelpGui),
        gallery = t(wv_app::i18n::I18nKey::CliHelpGallery),
        lang = t(wv_app::i18n::I18nKey::CliLanguageFlagHelp),
        help = t(wv_app::i18n::I18nKey::CliHelpHelp),
    );
    // mcp 子命令（ADR-0083）：i18n 键暂不加（CLI 帮助直写先例——本项目
    // CLI 文案多数走 i18n，此处一行技术性说明直写可接受）。
    println!("  wv mcp [--port <N>]        MCP stdio proxy -> wv gui's HTTP MCP (needs `wv gui` running with --features http, default port 8732)");
}

/// 底图切片（README §12-24）：把来源图栅格中被子图覆盖的那块采样落盘。
fn raster_slice(args: &[String], loc: Locale) -> Result<(), String> {
    let t = |k| wv_app::i18n::t(loc, k);
    let Some(dir) = args.first() else {
        return Err(t(wv_app::i18n::I18nKey::CliUsageRasterSlice).to_owned());
    };
    let Some(map_id) = args.get(1) else {
        return Err(t(wv_app::i18n::I18nKey::CliUsageRasterSliceMapId).to_owned());
    };
    let size: u32 = match args.get(2) {
        Some(s) => s
            .parse()
            .map_err(|_| t(wv_app::i18n::I18nKey::CliRasterSliceBadSize).to_owned())?,
        None => 512,
    };
    let id = wv_core::Id::new(map_id)
        .map_err(|_| t(wv_app::i18n::I18nKey::CliRasterSliceBadMapId).to_owned())?;
    let mut project = Project::open(dir).map_err(|e| e.to_string())?;
    let mut world = project.load_world().map_err(|e| e.to_string())?;
    let data = wv_io::slice_terrain(&world, &id, size).map_err(|e| e.to_string())?;
    world.set_terrain(id.clone(), data);
    project
        .save_world(&world, &[format!("maps/{map_id}/terrain.bin")])
        .map_err(|e| e.to_string())?;
    // size 与 map_id 是**用户回显**字段；CLI 文案模板里留 `{map_id}` / `{size}`
    // 占位，但 v1 仍要求用户回显原文——简单做字符串替换。
    println!(
        "{}",
        t(wv_app::i18n::I18nKey::CliRasterSliceDone)
            .replace("{map_id}", map_id)
            .replace("{size}", &size.to_string())
    );
    println!("{}", t(wv_app::i18n::I18nKey::CliRasterSliceInherit));
    Ok(())
}

fn load(dir: &str, loc: Locale) -> Result<(), Box<dyn std::error::Error>> {
    let t = |k| wv_app::i18n::t(loc, k);
    let project = Project::open(dir)?;
    println!("{}", project.summary());
    let warnings = project.validate()?;
    if !warnings.is_empty() {
        println!("\n{}", t(wv_app::i18n::I18nKey::CliProjectSummaryHints));
        for w in warnings {
            println!(
                "{}{w}",
                t(wv_app::i18n::I18nKey::CliProjectSummaryHintBullet)
            );
        }
    }
    Ok(())
}

fn validate(dir: &str, loc: Locale) -> Result<(), Box<dyn std::error::Error>> {
    let t = |k| wv_app::i18n::t(loc, k);
    let project = Project::open(dir)?;
    let warnings = project.validate()?;
    if warnings.is_empty() {
        println!(
            "{}{}",
            t(wv_app::i18n::I18nKey::CliValidateOk),
            project.root().display()
        );
    } else {
        println!(
            "{}",
            t(wv_app::i18n::I18nKey::CliValidateFailPrefix)
                .replace("{n}", &warnings.len().to_string())
        );
        for w in &warnings {
            println!(
                "{}{w}",
                t(wv_app::i18n::I18nKey::CliProjectSummaryHintBullet)
            );
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
            let loc = cli_locale();
            eprintln!(
                "{}{msg}",
                wv_app::i18n::t(loc, wv_app::i18n::I18nKey::CliErrorPrefix)
            );
            eprintln!(
                "\n{}",
                wv_app::i18n::t(loc, wv_app::i18n::I18nKey::CliGuiUsage)
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
    use wv_app::i18n::I18nKey;
    let loc = cli_locale();
    let t = |k: I18nKey| -> String { wv_app::i18n::t(loc, k).to_owned() };
    let mut dir: Option<String> = None;
    // http 关闭时两个 flag 的分支直接报错返回（见下方 cfg(not(http))），收集槽
    // 不参与任何使用——按 cfg 静音，`--features app` 闸门的 -D warnings 才不误红。
    #[cfg_attr(not(feature = "http"), allow(unused_mut, unused_variables))]
    let mut port: Option<u16> = None;
    #[cfg_attr(not(feature = "http"), allow(unused_mut, unused_variables))]
    let mut bind: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-m" => {
                let Some(d) = args.get(i + 1) else {
                    return Err(t(I18nKey::CliGuiErrMDirNeeded));
                };
                if dir.is_some() {
                    return Err(t(I18nKey::CliGuiErrDirDup));
                }
                dir = Some(d.clone());
                i += 2;
            }
            "--http-port" => {
                #[cfg(feature = "http")]
                {
                    let Some(s) = args.get(i + 1) else {
                        return Err(t(I18nKey::CliGuiErrHttpPortNeeded));
                    };
                    if port.is_some() {
                        return Err(t(I18nKey::CliGuiErrHttpPortDup));
                    }
                    let parsed: u16 = s.parse().map_err(|_| t(I18nKey::CliGuiErrHttpPortParse))?;
                    if parsed == 0 {
                        return Err(t(I18nKey::CliGuiErrHttpPortZero));
                    }
                    port = Some(parsed);
                    i += 2;
                }
                #[cfg(not(feature = "http"))]
                {
                    let _ = i;
                    return Err(t(I18nKey::CliGuiErrHttpPortDisabled));
                }
            }
            "--http-bind" => {
                #[cfg(feature = "http")]
                {
                    let Some(s) = args.get(i + 1) else {
                        return Err(t(I18nKey::CliGuiErrHttpBindNeeded));
                    };
                    if bind.is_some() {
                        return Err(t(I18nKey::CliGuiErrHttpBindDup));
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
                        .map_err(|_| t(I18nKey::CliGuiErrHttpBindParse))?;
                    bind = Some(s.clone());
                    i += 2;
                }
                #[cfg(not(feature = "http"))]
                {
                    let _ = i;
                    return Err(t(I18nKey::CliGuiErrHttpBindDisabled));
                }
            }
            s if s.starts_with('-') => {
                return Err(format!(
                    "{}{s}",
                    wv_app::i18n::t(loc, I18nKey::CliGuiErrUnknownArg)
                ));
            }
            s => {
                if dir.is_some() {
                    return Err(t(I18nKey::CliGuiErrDirDup));
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
        assert_eq!(
            parsed.startup_project,
            Some("examples/xinhai-1911".to_owned())
        );
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
        let parsed = parse_gui_args(&args(&["--http-port", "9000"])).expect("--http-port 应被接受");
        assert_eq!(parsed.http_server_config.port, 9000);
        assert_eq!(parsed.startup_project, None);
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_bind_override_takes_effect() {
        let parsed =
            parse_gui_args(&args(&["--http-bind", "0.0.0.0"])).expect("--http-bind 应被接受");
        assert_eq!(parsed.http_server_config.bind, "0.0.0.0");
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_bind_with_port_works() {
        // 用户想绑 loopback:9000 → 显式 host:port 形态。
        let parsed =
            parse_gui_args(&args(&["--http-bind", "127.0.0.1:9000"])).expect("host:port 应被接受");
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
        assert_eq!(
            parsed.startup_project.as_deref(),
            Some("examples/xinhai-1911")
        );
        assert_eq!(parsed.http_server_config.port, 9000);
        assert_eq!(parsed.http_server_config.bind, "127.0.0.1");
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_port_repeated_is_rejected() {
        // LLM 客户端错传两遍时**立刻**报错（不静默覆盖）——v0.2 决策
        assert!(parse_gui_args(&args(&["--http-port", "9000", "--http-port", "9001"])).is_err());
    }

    #[cfg(feature = "http")]
    #[test]
    fn http_bind_repeated_is_rejected() {
        assert!(parse_gui_args(&args(&[
            "--http-bind",
            "0.0.0.0",
            "--http-bind",
            "127.0.0.1"
        ]))
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
