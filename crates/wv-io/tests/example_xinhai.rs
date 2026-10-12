//! 辛亥示例工程（examples/xinhai-1911）的集成测试。
//!
//! 覆盖：工程加载、公历历法换算、旁白加载与引用、诊断无错误、
//! 多地图层级（P4）与轨迹插值。

use std::path::PathBuf;

use wv_io::Project;

fn xinhai_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/xinhai-1911")
}

#[test]
fn xinhai_world_loads_with_all_eight_events() {
    let project = Project::open(xinhai_dir()).expect("辛亥示例工程应可打开");
    assert_eq!(project.meta().name, "辛亥一九一一");
    assert_eq!(project.meta().schema_version, 2);

    let world = project.load_world().expect("加载应成功");
    assert_eq!(
        world.events.len(),
        8,
        "应载入全部 8 个事件, got {}",
        world.events.len()
    );
    assert_eq!(
        world.characters.len(),
        3,
        "应载入 3 个角色, got {}",
        world.characters.len()
    );
    assert_eq!(
        world.units.len(),
        3,
        "应载入 3 个单位, got {}",
        world.units.len()
    );
    assert_eq!(
        world.tracks.len(),
        3,
        "应载入 3 条轨迹, got {}",
        world.tracks.len()
    );
    assert_eq!(
        world.situations.len(),
        3,
        "应载入 3 条事态, got {}",
        world.situations.len()
    );
    // 10 世界城 + 3 武昌要地 + 2 成都要地 = 15（任务清单逐一枚举数）。
    assert_eq!(
        world.distances.places.len(),
        15,
        "应载入 15 个地点, got {}",
        world.distances.places.len()
    );
}

#[test]
fn xinhai_calendar_renders_wuchang_uprising_as_gregorian() {
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();
    let timeline = world.timeline.as_ref().expect("axis.yaml 应已载入");

    assert_eq!(timeline.calendar.to_ymd(283), (1911, 10, 10));
    assert_eq!(
        timeline.calendar.format(283),
        "公元 1911 年 10 月 10 日",
        "武昌起义的公历显示串"
    );
}

#[test]
fn xinhai_narrations_are_loaded_and_referenced() {
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();

    assert!(
        !world.narrations.is_empty(),
        "timeline/narrations/ 应载入旁白, got {}",
        world.narrations.len()
    );

    let narration_ids: Vec<&str> = world.narrations.iter().map(|n| n.id.as_str()).collect();
    for event in &world.events {
        for nid in &event.narrations {
            assert!(
                narration_ids.contains(&nid.as_str()),
                "事件 {} 引用的旁白 {} 必须存在; 实得 {narration_ids:?}",
                event.id,
                nid
            );
        }
    }
    // 全部 8 段旁白都应被至少一个事件引用。
    let referenced: Vec<&str> = world
        .events
        .iter()
        .flat_map(|e| e.narrations.iter().map(|i| i.as_str()))
        .collect();
    assert_eq!(
        referenced.len(),
        8,
        "8 个事件各引用 1 段旁白, got {referenced:?}"
    );
}

#[test]
fn xinhai_diagnose_reports_no_errors() {
    let project = Project::open(xinhai_dir()).unwrap();
    let diags = project.diagnose().expect("diagnose 永不因提示而失败");
    assert!(
        !diags.has_errors(),
        "示例工程不应有错误级诊断: {:?}",
        diags.items
    );
}

#[test]
fn xinhai_city_submaps_are_children_of_world() {
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();

    let world_map = world
        .maps
        .iter()
        .find(|m| m.id.as_str() == "map_world")
        .expect("世界图应载入");
    let children: Vec<&str> = world_map.children.iter().map(|i| i.as_str()).collect();
    assert!(
        children.contains(&"map_city_wuchang") && children.contains(&"map_city_chengdu"),
        "父图 children 只存子图 Id（P4）, got {children:?}"
    );
    // 子图自身存在，且经 parent 反向指回父图——只存引用，不复制内容。
    for cid in ["map_city_wuchang", "map_city_chengdu"] {
        let city = world
            .maps
            .iter()
            .find(|m| m.id.as_str() == cid)
            .unwrap_or_else(|| panic!("{cid} 应作为独立地图载入"));
        assert_eq!(
            city.parent.as_ref().map(|p| p.as_str()),
            Some("map_world"),
            "{cid} 的 parent 应指回 map_world"
        );
    }
}

#[test]
fn xinhai_track_interpolates_new_army_route() {
    let project = Project::open(xinhai_dir()).unwrap();
    let world = project.load_world().unwrap();

    let track = world
        .tracks
        .iter()
        .find(|t| t.id.as_str() == "track_new_army_west")
        .expect("西调轨迹应载入");

    // 锚点日精确命中：day 258 = 武汉。
    let start = track.sample_at(258).unwrap();
    assert_eq!((start.x, start.y), (1650.0, 1700.0), "起点应为武汉坐标");
    // day 260 位于武汉(258) → 宜昌(263) 之间：u = 2/5。
    // x = 1650 + (1500-1650)*0.4 = 1590; y = 1700 + (1850-1700)*0.4 = 1760。
    let mid = track.sample_at(260).unwrap();
    assert!(
        (mid.x - 1590.0).abs() < 1e-3 && (mid.y - 1760.0).abs() < 1e-3,
        "day 260 应为武汉→宜昌之间的线性插值点 (1590, 1760), got ({}, {})",
        mid.x,
        mid.y
    );
}
