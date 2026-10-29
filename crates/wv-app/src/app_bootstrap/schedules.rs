//! 全部 `add_systems` 注册：Startup / PreUpdate / Update / Last / egui pass。
//!
//! 注册顺序与原 `run()` 内完全一致；每条 `app.add_systems` 各自独立，
//! 执行顺序只由 `.chain()` / `.after()` 决定（拆成多条调用不改调度语义）。

use bevy::prelude::*;

use crate::edit::editing;
use crate::edit::nature_region_actions;
use crate::ui;
use wv_render::brush_cursor;
use wv_render::buildings;
use wv_render::camera;
use wv_render::cellmap_bridge;
use wv_render::faction_overlays;
use wv_render::feature_overlays;
use wv_render::layer_frame;
use wv_render::map_frame;
use wv_render::place_line;
use wv_render::region_outline;
use wv_render::streets;
use wv_render::terrain_decor;
use wv_render::terrain_view;
use wv_render::vector_layers;
use wv_render::world_projection;

use super::startup::{setup_camera, setup_terrain_texture_after_world};
use super::window::{reveal_window_once_first_frame_presented, sync_window_title_with_locale};

/// 把所有系统挂进 `App` 的各条 schedule。
pub(super) fn register(app: &mut App) {
    app.add_systems(
        Startup,
        // 字体装配不在这里：bevy_egui 0.42 的主上下文要等 PreUpdate 的
        // InitContexts 才创建，Startup 时 ctx_mut() 必然 Err（曾导致 egui
        // 全程豆腐块）。见 fonts.rs 文档与下方 PreUpdate 注册。
        //
        // `apply_env_view_mode` 必须在烘焙之前：`WV_MODE` 一旦给定，首帧纹理
        // 就该用目标模式的着色器（否则会先闪一帧水彩）。
        (
            crate::debug_probe::apply_env_view_mode,
            // `WV_STRIP` 剥离探针（ADR-0088 Task 4）：启动读一次，先于首帧
            // 场同步（Startup 全部先于 Update），首帧 uniform 即带掩码。
            crate::debug_probe::apply_env_strip,
            setup_camera,
            setup_terrain_texture_after_world,
        )
            .chain(),
    );
    // `WV_DRILL` 图级探针（ADR-0073 验收）：等后台工程加载完成后钻到指定图
    // ——城市级街名 / 标签摆位截图必须「开机即在城市图」，双击手势没法从
    // 命令行模拟。探针自带「未设置即退出」，缺省零开销。
    // `WV_SCALE` 机位探针（ADR-0088 Task 4 对比帧）：等钻入过渡结束后每帧
    // 幂等钉死缩放（同机位两档），同样「未设置即退出」。
    app.add_systems(Update, crate::debug_probe::apply_env_drill);
    app.add_systems(Update, crate::debug_probe::apply_env_scale);
    // 主题包同步（ADR-0023 M2）：`ViewState.theme_pack_id` → `ViewState.active_theme`。
    // **必须先于所有消费点**——挂在 PreUpdate 最前；渲染消费点在 Update（PreUpdate
    // 一定先于 Update），唯一同在 PreUpdate 的消费者（`poll_open_project_dialog`）
    // 由 ui_shell 侧显式 `.before()` 保证。
    app.add_systems(PreUpdate, wv_render::theme::switch::sync_active_theme_pack);
    app.add_systems(
        PreUpdate,
        // 必须排在 bevy_egui 建上下文之后：否则首帧 ctx_mut() 对未初始化
        // 上下文调用会 panic（bevy_egui 0.32 lib.rs:437 的 expect）。
        // 字体重试式装配同口径：上下文就绪的首帧执行一次（fonts.rs 文档）。
        // 图标 loader 同口径：bytes:// URI 没这两个 loader 全部变红三角。
        (
            crate::theme::fonts::setup_egui_fonts,
            crate::ui::setup_egui_image_loaders,
            camera::poll_egui_wants_input,
        )
            .after(bevy_egui::EguiPreUpdateSet::InitContexts),
    );
    app.add_systems(
        Update,
        (
            // 主链已顶满 20 参上限，拆一层嵌套元组（内层先 chain 成一个整体）。
            //
            // 2026-10-24 再拆一层：M1/M2 加入 `track_edit_cell_hover`、
            // `paint_edit_cell_on_drag` 后内层达 23 个
            // 系统，超出 `all_tuples` 的 20 上限（报「chain 的 trait bounds
            // 不满足」）。按「输入/编辑」与「纹理/几何」两个语义段拆成
            // 两个各自 chain 的子链，外层再 chain 保证跨段顺序不变。
            (
                (
                    // 输入 → 视图状态
                    // 朝向先行：把 MapNode.orientation 写进相机旋转，同帧的缩放锚点、
                    // 平移与拾取换算读 Transform.rotation 时拿到的就是新值（帧内自洽）。
                    //
                    // 每个系统都套 `perf_timed!`（`WV_PERF=1` 才计时，默认两次布尔
                    // 判断即返回）：2026-10-23「鼠标移动 120→40 FPS」要把 17ms 落到
                    // 具体 system 名上。整段摘除 = 把宏换回裸 system。
                    wv_render::perf_timed!("apply_map_orientation", camera::apply_map_orientation),
                    wv_render::perf_timed!("camera_zoom", camera::camera_zoom),
                    wv_render::perf_timed!("camera_pan", camera::camera_pan),
                    wv_render::perf_timed!("camera_pan_keys", camera::camera_pan_keys),
                    wv_render::perf_timed!("brush_radius_scroll", camera::brush_radius_scroll),
                    wv_render::perf_timed!("track_cursor_world", camera::track_cursor_world),
                    wv_render::perf_timed!("collect_brush_stroke", camera::collect_brush_stroke),
                    // 编辑（笔触 → 命令 → History）
                    wv_render::perf_timed!(
                        "commit_stroke_on_release",
                        editing::commit_stroke_on_release
                    ),
                    // 点选工具（ADR-0057 补记 6）：点击 → cell_pick → CellSelection
                    // （无修饰 = 替换 / Shift = 并 / Alt = 差）。EditCell 只画不选。
                    wv_render::perf_timed!(
                        "commit_point_select_on_click",
                        editing::commit_point_select_on_click
                    ),
                    // 图钉拖动 + 双击属性（ADR-0074 追记三）：按住挪钉
                    // （松手一次性 UpdatePlaceCommand）、双击弹 Inspector
                    // 地点表单（仅 CellMap；浏览族双击归下钻）。
                    wv_render::perf_timed!(
                        "drag_place_pin_on_hold",
                        editing::drag_place_pin_on_hold
                    ),
                    // 地标放置（ADR-0071）：Landmark 激活时单击地图创建点状
                    // Place。与点选互斥由 dispatch 的 SelectMapTool 分支保证
                    // （placing 开时 point_select 必关），故两者先后无耦合。
                    wv_render::perf_timed!("commit_landmark_click", editing::commit_landmark_click),
                    // hover 准星（[ADR-0052] D2 写入侧；ADR-0057：EditCell ∨
                    // PointSelect 共用）：每帧追踪光标压住的格。
                    // 必须排在 `track_cursor_world` 之后（要读 cursor_world）。
                    wv_render::perf_timed!("track_edit_cell_hover", editing::track_edit_cell_hover)
                        .after(wv_render::camera::track_cursor_world),
                    // 单格连续绘制（[ADR-0052] D3）：拖动累积格 + 松手提交。
                    // 排在 `track_cursor_world` 之后（要读 cursor_world）。
                    wv_render::perf_timed!(
                        "paint_edit_cell_on_drag",
                        crate::edit::cell_stroke::paint_edit_cell_on_drag
                    )
                    .after(wv_render::camera::track_cursor_world),
                    // （泛洪选择系统已随 ADR-0086 退役：点选工具的默认点击
                    // 即泛洪（ADR-0074 追记四），在 `commit_point_select_on_click`
                    // 内执行，不再需要第二个入口。）
                    // 派生运算（[ADR-0052] D6 · M3）：平滑 / 量化 → 1 条批命令。
                    // 排在面板绘制之后的下一帧消费请求（面板在 EguiPrimaryContextPass，
                    // 早于它的 Update 系统只会看到上一帧排的请求——一帧延迟与工具
                    // 切换同量级，用户不可感）。
                    wv_render::perf_timed!(
                        "apply_derived_op",
                        crate::edit::derived_op::apply_derived_op
                    ),
                )
                    .chain(),
                (
                    // 候选点选（2026-10-22 内容矩阵）：在 cell 点选之后跑——
                    // 读 selected 判定是否落进未命名候选。
                    wv_render::perf_timed!(
                        "pick_candidate_on_click",
                        crate::edit::nature_region_actions::pick_candidate_on_click
                    ),
                    // 群系分界线：创建/拖动/提交（在 cell 点选之后，互斥工具态）。
                    wv_render::perf_timed!(
                        "biome_boundary_input",
                        crate::edit::biome_boundary::biome_boundary_input
                    ),
                    // 候选过期的编辑信号（在提交路径之后跑，读 History 形状）。
                    wv_render::perf_timed!(
                        "sync_candidate_invalidation",
                        crate::edit::nature_region_actions::sync_candidate_invalidation
                    ),
                    // 区域边界拖拽（在 cell 点选之后；同为输入系统）。
                    wv_render::perf_timed!(
                        "region_adjust_input",
                        crate::edit::region_adjust::region_adjust_input
                    ),
                    // 自然区选择手势（2026-10-24 用户裁定）拆到主链**之后**单独注册：
                    // 左键点在自然区内 ⇒ 选中；右键**单击**（位移 <4px）⇒ 取消选中。
                    // 这两条必须排在 ①`drill_pick`（空白单击会清选中）与
                    // ②`region_adjust_input`（拽锚点优先）之后，否则会被覆盖或抢走。
                    // 见下方「自然区选择」。
                    // 批量选区（2026-10-20）：多边形闭合 → 栅格化贴合方格
                    wv_render::perf_timed!("sync_region_cells", editing::sync_region_cells)
                        .after(camera::track_cursor_world),
                    wv_render::perf_timed!("commit_street_click", streets::commit_street_click),
                    // 撤销 / 重做不再在这里单独监听 Ctrl+Z：快捷键统一由
                    // `ui_shell::hotkeys` 收（那里已经处理了「输入框聚焦时失效」），
                    // 两处都听会导致一次按键应用两次。
                    wv_render::perf_timed!("flush_terrain_texture", editing::flush_terrain_texture),
                    // 离开 Browse 自动暂停（2026-10-27「播放器只在 Browse」裁定
                    // 的行为侧）：必须排在 advance_playback 之前——切走的那一帧
                    // 时间就不许再走。ui/panel_timeline 只门控呈现，这里门控行为。
                    ui::pause_playback_off_browse,
                    // 播放推进（时间 → 当前时刻）
                    wv_render::perf_timed!("advance_playback", world_projection::advance_playback),
                    // 纹理先于网格几何：`grid.cell_size` 取自纹理的来源图与格数。
                    wv_render::perf_timed!(
                        "rebuild_texture_on_mode_change",
                        terrain_view::rebuild_texture_on_mode_change
                    ),
                    // 后台合成落地（M3）：必须在重建触发之后、网格几何之前——
                    // 它写 tex.size，`sync_grid_config` 同帧要读到。
                    wv_render::perf_timed!(
                        "land_pending_terrain_synth",
                        terrain_view::land_pending_terrain_synth
                    ),
                    // 网格几何（`raster_origin` / `map_origin` 两套原点）必须先于所有
                    // 消费它的系统：投影里的地点、街道、街道名、地图外框都按 `GridConfig`
                    // 换算，顺序反了就会拿上一帧的原点画一帧——钻取那一帧最明显。
                    wv_render::perf_timed!("sync_grid_config", terrain_view::sync_grid_config),
                )
                    .chain(),
            )
                .chain(),
            // 换级淡化：旧底图淡出精灵逐帧降透明、归零回收（ADR-0009 §2⑦）。
            wv_render::perf_timed!("tick_basemap_fadeout", terrain_view::tick_basemap_fadeout),
            // 图层帧（ADR-0013）：先于一切消费它的渲染系统求值。
            wv_render::perf_timed!("resolve_layer_frame", layer_frame::resolve_layer_frame),
            // 投影（时间 → 渲染资源）
            wv_render::perf_timed!("compute_projection", world_projection::compute_projection),
            // 等值线矢量叠加（ADR-0012 / plan v0.1 M2）：先于 chunk 层读
            // `tex.chunks_dirty`（chunk 系统会消费清零），触发重算。
            wv_render::perf_timed!("sync_isoline_overlay", terrain_view::sync_isoline_overlay),
            // 编辑态逐格网格（plan v0.1 D4）：与等值线同管线族，先于 chunk 层。
            wv_render::perf_timed!("sync_grid_lines", terrain_view::sync_grid_lines),
            // 地形分块 + mip + 视口拣选（优化项 3a/3b/3c）。
            // `should_render_field` 为真时整层早退（ADR-0088 模式路由，与
            // field 侧同消费一个函数）——见函数开头的底图载体路由。
            wv_render::perf_timed!("sync_terrain_chunks", cellmap_bridge::sync_terrain_chunks),
            // 地形场渲染（ADR-0053 D1，Phase 1 接线）：**必须排在 chunk 层之后**。
            // 两个理由：① `field_render_on_chunk_entities` 要报**帧末**的块数，
            // 而块的 spawn / despawn 由 chunk 系统在本帧完成（排在前面只能读到
            // 上一帧的残留，与组件口径对不上）；② 编辑信号取自变更戳
            // （`WorldProjection` / `TerrainTexture` 的 `is_changed`），不抢
            // `tex.chunks_dirty`——那个旗标照旧归 chunk 层消费。
            // 独占性不依赖顺序：场开着时 chunk 层自己早退。
            wv_render::perf_timed!("sync_terrain_field", cellmap_bridge::sync_terrain_field),
            // 派生诊断刷新：在 flush 之后、UI 之前；面板本帧即可读新值。
            wv_render::perf_timed!(
                "refresh_project_diagnostics",
                editing::refresh_project_diagnostics
            ),
            // 候选区预览输入（ADR-0045 追记四）：光标格 → biome 连通区环。
            wv_render::perf_timed!(
                "sync_candidate_input",
                nature_region_actions::sync_candidate_input
            ),
            // 街道图层缓存：当前图变化时重读（ADR-0010），先于 sync_vector_layers。
            wv_render::perf_timed!("reload_street_cache", streets::reload_street_cache),
            // 矢量叠加层（道路 + POI）：在 compute_projection 之后跑。
            wv_render::perf_timed!("sync_vector_layers", vector_layers::sync_vector_layers),
            // 街道名标注：消费同一份 StreetLayerCache，在街道画完之后对齐标签。
            // **必须 `.after(DrillLabelChain)`**（P0-4 排序缺口，2026-10-29）：
            // 统一标签占位池的落位次序是「地名→…→海名→街名」（ADR-0073），
            // 该约束在游戏路径由 plugins.rs:80 声明，本编辑器主链此前缺失——
            // 街名与海名对占位池的竞争顺序不定，摆位帧间可漂移。
            wv_render::perf_timed!("update_street_labels", streets::update_street_labels)
                .after(wv_render::label_collision::DrillLabelChain),
            // 势力多边形叠加层（S12 v0.1）：在矢量层之后、地图外框之前。
            wv_render::perf_timed!(
                "sync_faction_overlays",
                faction_overlays::sync_faction_overlays
            ),
            // 事实地界覆盖层（theme overlay）：地盘随 (图, 日, 缩放) 求值，
            // 排在势力层之后、地图外框之前（z 由各层常量决定，与顺序无关）。
            wv_render::perf_timed!(
                "sync_feature_overlays",
                feature_overlays::sync_feature_overlays
            ),
            wv_render::perf_timed!("sync_terrain_decor", terrain_decor::sync_terrain_decor),
            // 地图外框 + 1/4 幅面参考网格（外框需地形尺寸）。
            wv_render::perf_timed!("sync_map_frame", map_frame::sync_map_frame),
        )
            .chain(),
    );
    // per-system 计时汇总（`WV_PERF=1` 才有输出）：`Last` 里跑，本帧所有
    // Update 系统都已收工——单帧超 4ms 立刻打印，每 5s 打 top5 + 最差帧。
    app.add_systems(Last, wv_render::perf::report_frame);
    // 主链已顶满 Bevy 系统元组上限：新三件单独成链，显式排序——
    // 候选输入在主链（diagnostics 之后天然有序），轮廓/预览消费它。
    app.add_systems(
        Update,
        (
            // 区域轮廓 + 底色覆盖 + 候选区预览（ADR-0045 追记四）：区域的
            // 地理边界此前只有标签没有轮廓——作者看不出圈了哪块地。
            wv_render::perf_timed!("sync_region_outlines", region_outline::sync_region_outlines)
                .after(map_frame::sync_map_frame),
            // 作者小河的落笔 + 预览（ADR-0046）：左键加点 / 右键收线。
            wv_render::perf_timed!(
                "commit_place_line_click",
                place_line::commit_place_line_click
            ),
            wv_render::perf_timed!(
                "sync_place_line_preview",
                place_line::sync_place_line_preview
            )
            .after(place_line::commit_place_line_click),
        )
            .chain(),
    );
    // 区域覆盖物解析（ADR-0045 D8/D9/D10）：**必须早于装饰采样**（装饰要按
    // region 的声明换覆盖物变体 / 颜色），缺失标记紧随其后。
    app.add_systems(
        Update,
        cellmap_bridge::sync_region_overlay_index
            .after(terrain_view::sync_grid_config)
            .before(terrain_decor::sync_terrain_decor),
    );
    app.add_systems(
        Update,
        cellmap_bridge::sync_missing_overlay_markers
            .after(cellmap_bridge::sync_region_overlay_index),
    );
    // 主链已顶满 Bevy 系统元组的 20 参上限（all_tuples，bevy_ecs
    // function_system.rs），笔刷光标单独注册、显式排在光标取世界坐标之后。
    app.add_systems(
        Update,
        wv_render::perf_timed!("sync_brush_cursor", brush_cursor::sync_brush_cursor)
            .after(camera::track_cursor_world),
    );
    // CellMap 编辑域渲染系统（[ADR-0040] 选中高亮 / [ADR-0052] D2 hover
    // 准星 · D3 待落笔触 · D8-A 白闪）。
    //
    // **为什么在这里单独装配（2026-10-24 修复）**：`wv-render` 的
    // `EditorOverlayPlugin` 注册了这批系统，但本 crate 从未装配该插件
    // （渲染主链按 ADR-0020 直装）。后果是 ADR-0040 的单格金色 outline
    // 从未在线上出现，M1 的 hover 准星与 M2 的待落笔触色块同样画不出来。
    // 按 AGENTS §2.1「装配点留在组合根」，这里显式注册。
    //
    // 顺序：区域 → 笔触 → 选中 → hover → 闪视觉，与 ADR-0052 §9.5 的
    // 绘制序（region → stroke → hover）一致；闪视觉最后
    // （z 最高，250ms 短促）。ADR-0057：旧 sync_cell_selection（单格 outline）
    // 退役，选区统一由 sync_cell_region 承担。
    //
    // **必须 `.after(sync_grid_config)`**（2026-10-24 真机调试）：
    // 这批系统全靠 `GridConfig::raster_origin` / `cell_size` 把格坐标换算成
    // 世界坐标。`sync_grid_config` 在主链里每帧刷新它（`raster_origin` 取决于
    // 当前图的图帧原点），未排序时 cellmap 系统可能先跑、读到**默认值**
    // （origin = (0,0)、cell_size = 8）→ 描边画到离视野几千世界单位的地方，
    // 屏幕上看不到任何东西。真机症状：读数条 hover 有值、地图上无准星。
    app.add_systems(
        Update,
        (
            cellmap_bridge::sync_cell_region,
            cellmap_bridge::sync_cell_stroke,
            cellmap_bridge::sync_cell_hover.after(cellmap_bridge::sync_cell_region),
            cellmap_bridge::sync_cell_flash,
        )
            // **`.chain()`**（P0-4 排序缺口，2026-10-29）：上面注释声明的
            // 「区域 → 笔触 → hover → 闪视觉」此前只是文档——stroke/flash 与
            // region 之间无序，靠 z 常量兜底才没出视觉 bug。显式定序。
            .chain()
            .after(terrain_view::sync_grid_config)
            .after(camera::track_cursor_world),
    );
    // 选中图钉高亮环 + gizmo 线宽（[ADR-0074] 追记三 followup）——同上：
    // `EditorOverlayPlugin` 注册了它们但本 crate 从未装配该插件，须在此
    // 显式注册（2026-10-24 单格 outline 不可见的同一坑二次踩）。
    app.add_systems(
        Update,
        (
            wv_render::world_projection::sync_gizmo_line_width,
            wv_render::world_projection::draw_pin_highlight
                .after(wv_render::world_projection::sync_gizmo_line_width),
        )
            .after(camera::track_cursor_world),
    );
    // 编辑意图翻译（ADR-0020 §2.5）：渲染 commit_* 产意图 → 翻译器提交 wv-edit
    // → 渲染 apply_*_receipts 回填本地。必须晚于两个 commit 系统（同帧闭环）。
    app.add_systems(
        Update,
        (
            crate::edit::intent_translator::translate_edit_intents,
            wv_render::streets::apply_street_receipts,
            wv_render::measurement::apply_measurement_receipts,
        )
            .chain()
            .after(wv_render::streets::commit_street_click)
            .after(wv_render::measurement::commit_measurement_click),
    );
    // 纸面氛围层（ADR-0016 M2：雾霭 + 纸张噪点）：独立注册，排在主链
    // `sync_map_frame` 之后（同帧即可，只要不先于地形 chunk 出现）。
    app.add_systems(
        Update,
        cellmap_bridge::sync_atmosphere.after(map_frame::sync_map_frame),
    );
    // 自然区选择手势（2026-10-24 用户裁定）：左键点在自然区内 ⇒ 选中该区域；
    // 右键**单击**（松手位移 <4px）⇒ 取消选中。
    //
    // 单独注册而不是塞进主链：这两条要挂在 `drill_pick`（空白单击清选中）与
    // `region_adjust_input`（拽锚点优先）**之后**——主链是一整条 `.chain()`，
    // 混进 `.after()` 会让整条链的调度约束求不出来（实测编译失败）。
    app.add_systems(
        Update,
        (
            wv_render::perf_timed!(
                "select_nature_region_on_click",
                crate::edit::region_select::select_nature_region_on_click
            )
            .after(wv_render::drill::drill_pick)
            .after(crate::edit::region_adjust::region_adjust_input),
            // 右键取消选中：只读按键 + 光标，不依赖任何输入系统的先后。
            wv_render::perf_timed!(
                "cancel_selection_on_right_click",
                crate::edit::region_select::cancel_selection_on_right_click
            ),
        ),
    );
    // 2.5D 建筑挤出（ADR-0016 §6）：独立注册，消费主链产出的 LayerFrame。
    app.add_systems(
        Update,
        buildings::sync_building_overlays.after(vector_layers::sync_vector_layers),
    );
    app.add_systems(
        Update,
        (
            // 测量：先 reload 缓存，再 commit 点击，最后 render。
            // reload 在 `world_projection::is_changed` 后即可（缓存早退）。
            wv_render::measurement::reload_measurement_cache.after(camera::track_cursor_world),
            wv_render::measurement::commit_measurement_click
                .after(wv_render::measurement::reload_measurement_cache),
            // 线段 POI 钉点交互（ADR-0070 D4）：同一条光标坐标链——
            // `cursor_world` 就绪后才读（cell_stroke / selection 同款排序）。
            crate::edit::line_poi_tool::line_poi_tool_input.after(camera::track_cursor_world),
        ),
    );
    // 选择工具（pen 方法）：变图清空 → 点击采集 → 拖动移锚，同一条光标坐标链。
    app.add_systems(
        Update,
        (
            wv_render::selection::sync_selection_map_change,
            // Esc 清空统一选区（ADR-0057 补记 3）：**必须排在手势处理之前**——
            // 拖绘 / 套索进行中时它跳过（Esc 归手势），空闲帧才清选区。
            // 排在其后会看到「手势已被本帧清掉」而顺手清选区（一键两效）。
            editing::clear_cell_selection_on_escape
                .after(wv_render::selection::sync_selection_map_change)
                .before(wv_render::selection::commit_selection_click)
                .before(wv_render::selection::commit_selection_shape_drag)
                .before(crate::edit::cell_stroke::paint_edit_cell_on_drag),
            wv_render::selection::commit_selection_click
                .after(wv_render::selection::sync_selection_map_change),
            wv_render::selection::drag_selected_anchor
                .after(wv_render::selection::commit_selection_click),
            // 框选形状（ADR-0043）：按住拖拽的那一套手势。与上面三个系统读同
            // 一批资源、各自在函数开头按形状早退（互斥，不会同时改状态）。
            wv_render::selection::commit_selection_shape_drag
                .after(wv_render::selection::sync_selection_map_change),
        )
            .after(camera::track_cursor_world),
    );
    app.add_systems(
        Update,
        (
            // 选中区域装饰（阶梯描边 + 实心白方块锚点）：**合批 mesh**，
            // 2026-10-24 从 `draw_map_gizmos` 迁出——逐条 gizmo 在大区域上是
            // 上万条/帧（用户症状：点选自然区后 FPS 掉到 20 多）。
            // 排在地图绘制之前：它只写 mesh 资产与实体，不依赖本帧 gizmos。
            wv_render::perf_timed!(
                "sync_region_decor",
                wv_render::region_decor::sync_region_decor
            ),
            // 地图绘制必须在所有浮动面板**之前**执行，
            // 保证地图是最底层、面板浮在其上（基线 §1.11 R1）。
            wv_render::perf_timed!("draw_map_gizmos", world_projection::draw_map_gizmos),
            // 测量顶置渲染——与 gizmos 一起画。
            wv_render::measurement::render_measurements,
            // 线段 POI 预览 / 选中高亮（ADR-0070 D4）：同层 gizmo 即帧即弃。
            crate::edit::line_poi_tool::draw_line_poi_preview,
            // 选择工具（草稿折线 + 橡皮筋 / 闭合环 + 选中锚点）。
            wv_render::selection::draw_selection,
        )
            // **`.chain()`**（P0-4 排序缺口，2026-10-29）：六者全写 `Gizmos`
            // （互斥资源），调度器此前任意定序、gizmo 提交序帧间不定。
            // 数组序即绘制层序：区域装饰（mesh 非路 gizmo）→ 地图 → 笔刷
            // 准星 → 测量 → line-POI 预览 → 选区。
            .chain(),
    );
    app.add_systems(
        // bevy_egui 0.42 起 pass 是独立调度（在 PostUpdate 的 EndPass 里
        // `Context::run_ui` 内执行）：egui 首帧字体在该 pass 前就绪，
        // 放 `Update` 会在第一帧 panic「No fonts available until first call
        // to Context::run()」。世界系统留在 `Update`，先跑后画。
        bevy_egui::EguiPrimaryContextPass,
        (
            // 全部浮动面板（基线 §1.11 R1–R4：地图铺满，面板浮其上）。
            //
            // **必须 `.chain()`**：这些系统都要 `ResMut<PanelState>` 与
            // `ResMut<ViewState>`，Bevy 无法并行调度同时可变借用同一资源的系统
            // （否则运行期 panic：`could not access system parameter`）。
            ui::top_bar,
            // topOptionBar 工具选项条（2026-10-27）：贴 topbar 正下方——egui
            // top Panel 按注册序堆叠，必须在 `ui::top_bar` 之后画才落对位。
            ui::top_option_bar,
            // Google 式贴边导航控件先画，浮动面板仍可在需要时覆盖它。
            ui::map_controls,
            // （2026-10-25 用户裁定：地图区左上角的面包屑浮层删除——顶栏
            // 身份区已有同内容面包屑，浮层重复且挡地图。）
            ui::panel_map_tree,
            ui::panel_layers,
            ui::panel_brush,
            // 2026-10-25 恢复（Figma `3:20823`）：overview + 视口框 + Fit region。
            ui::panel_minimap,
            ui::panel_timeline,
            ui::panel_event,
            ui::panel_character,
            // 2026-10-22 统一 Inspector：panel_situation / panel_faction /
            // panel_cell_editor 均已退役——内容分别成为 Inspector 的
            // 「事态」「势力层」「属性（Cell）」页签。
            ui::panel_region,
            // 地标面板（ADR-0071）：放置 toggle + 当前图点状地点列表。
            // 同链纪律：读 ResMut<PanelState>，必须 chain 在其余面板之间。
            ui::panel_landmark,
            // v0.3：Inspector 整体右移后绘在最后（右上角，离顶栏最近），
            // 避免被左列 / 中列面板遮挡。
            ui::panel_inspector,
            // 2026-10-22 统一 Inspector：Cell 编辑器的**内容**已迁入 Inspector
            // 的「属性」页签，独立浮窗不再注册（枚举项暂留，退役见 M4）。
            ui::panel_validation,
            // 「河流和道路」（ADR-0070 D4）：线段 POI 的新建设置 + 选中属性
            // 编辑，随工具开关（分发器裁决显隐）。
            ui::panel_line_poi,
            // 「新建地图」对话框（CommandId::NewMap 打开；居中模态窗）。
            ui::new_map_dialog,
            // 「生成大陆」属性面板（工具条 / 命令面板打开；居中模态窗）。
            ui::continent_dialog_system,
            // 「新建工程」对话框（CommandId::NewProject / 欢迎页打开；
            // 与 new_map_dialog 同族的居中模态窗，排在状态栏之前——
            // 确认创建写的回执本帧就要被状态栏读到）。
            ui::new_project_dialog,
            // 「新建工程」apply 系统：消费 UI 系统上一帧塞进
            // `NewProjectDialog::pending_apply` 的意图，做实际落盘 + 打开。
            // 拆出来的目的是让 UI 系统跟 apply 系统都在 Bevy 0.19 的 16 系统
            // 参数上限内（详见 `new_project_dialog` 文档）。
            ui::new_project_apply_create,
            // 状态栏最后画：它读得到的 `ShellNotice` 已是本帧最终值。
            ui::status_bar,
        )
            .chain(),
    );
    // 候选上下文镜像（2026-10-22 验收）：主链已顶满 20 元组，单独注册。
    // （2026-10-25 用户裁定：群系图例浮层删除——地形设计器面板已含群系
    // swatch，图例信息重复。）
    app.add_systems(
        Update,
        (crate::edit::nature_region_actions::sync_candidate_context,),
    );
    // 「新建地理事实」命名对话框（选择工具 M4「森林」意图打开；居中模态窗）。
    // 主链已顶满 20 元组上限，单独注册并排在状态栏之后。
    app.add_systems(
        bevy_egui::EguiPrimaryContextPass,
        (
            ui::feature_place_dialog.after(ui::status_bar),
            // 应用设置（顶栏 settings-2）：与状态栏同帧、压在所有面板之上
            // （`DialogWindow` 走 `Order::Foreground`）。
            ui::settings_dialog.after(ui::status_bar),
            // UX-002「登记事件」对话框（时间轴按钮 / 命令面板打开）：同族
            // 居中模态窗，压在状态栏之后。
            ui::event_create_dialog::event_create_dialog_system.after(ui::status_bar),
            // UX-001 关闭确认框：必须压在一切之上（挡住「正在丢弃工作」的
            // 误触路径），且在 dispatch（Update）之后读当帧保存结果。
            crate::ui_shell::close_confirm::close_confirm_dialog.after(ui::status_bar),
        ),
    );
    // 「地图属性」对话框（CommandId::MapProperties 打开；居中模态窗）。
    // 主链顶满，同上单独注册。
    app.add_systems(
        bevy_egui::EguiPrimaryContextPass,
        ui::map_properties_dialog.after(ui::status_bar),
    );
    // 选区意图菜单（底部居中浮动条；spec 2026-10-17 §4）。ego pass 主链已到
    // Bevy 元组上限（20），单独注册、排在 map_controls 之后即可（只读选区）。
    app.add_systems(
        bevy_egui::EguiPrimaryContextPass,
        ui::intent_menu.after(ui::map_controls),
    );
    // 「生成城市」对话框（意图菜单打开；居中模态窗）。与菜单同链尾段，
    // 排在其后（先画菜单、后画它弹出的对话框，同帧即可）。
    app.add_systems(
        bevy_egui::EguiPrimaryContextPass,
        ui::city_intent::city_intent_dialog_system.after(ui::intent_menu),
    );

    // 界面外壳（主题 / 快捷键 / 命令面板 / 布局持久化）与地图钻取（v0.2 §B）。
    // 两者各自成模块并各自注册，`gui.rs` 只负责调用顺序（见各处模块文档）。
    crate::ui_shell::register(app);
    wv_render::drill::register(app);
    // 隐藏起步的揭示（见 Window 配置注释）：帧 1 的 Update 翻转 `visible`，
    // 本帧 `Last` 里 bevy_winit 的 `changed_windows` 调 `set_visible(true)`——
    // 此时帧 0 已完整 present，窗口亮起即是有内容的画面。
    app.add_systems(Update, reveal_window_once_first_frame_presented);
    // 窗口标题跟随界面语言（见 [`sync_window_title_with_locale`]）。
    app.add_systems(Update, sync_window_title_with_locale);
    // 渲染配方求值（ADR-0021 D1）：PreUpdate 单点求值 RenderRecipe——已迁移的
    // gate（buildings / faction_overlays / atmosphere）读 `recipe.enabled(...)`，
    // **必须**挂本系统，否则 recipe 停留在 Default 全关、三个 pass 永不渲染。
    wv_render::recipe::register_recipe_system(app);
    // ADR-0020 §2.6：钻入标记界面色由应用层灌（渲染只读 DrillMarkerPalette）；
    // 悬停气泡（egui 浮窗）从 wv-render 链尾移回应用层，仍排海名标签之后。
    app.add_systems(
        Update,
        crate::render_bridge::sync_drill_marker_palette
            .before(wv_render::drill::draw_drill_markers),
    );
    app.add_systems(
        Update,
        ui::hover_tooltip::hover_tooltip_system.after(wv_render::drill::update_sea_labels),
    );

    // 切蛋糕 pen/编辑（ADR-0079 T11–T13）：**必须进 `EguiPrimaryContextPass`**
    // ——bevy_egui 0.42 起画 egui 的系统一律在该调度内执行；挂裸 `Update`
    // 会在 pass 外画（形状上屏但 widget 不进交互快照，整窗点不动——
    // 2026-10-28 实机抓的回归，pen 弹框按钮全灭即此）。
    app.add_systems(
        bevy_egui::EguiPrimaryContextPass,
        (
            crate::ui::partition_pen::partition_pen_dialog,
            crate::ui::partition_edit::partition_edit_hud,
            crate::ui::partition_edit::partition_cut_gesture,
            crate::ui::partition_edit::partition_drag_gesture,
            crate::ui::partition_edit::partition_assign_gesture,
            crate::ui::partition_edit::partition_assign_dialog,
        ),
    );
}
