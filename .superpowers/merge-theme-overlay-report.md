# Merge report — `feat/theme-overlay-forest` → `main`

- Worktree: `D:\kudi-projects\worldview-rs\.worktrees\merge-theme-overlay`
- Branch: `merge/theme-overlay-forest`
- Main at merge time: `4a4bb55`
- Merge commit: `8c116167ec0ac907834048ffdd7a3f981082977e`
- Commit message: `merge: 主题覆盖物 v1 合入选择优先主线（森林=M4 意图）`

## 1. Conflict resolutions per file

| File | Resolution |
|---|---|
| `GLOSSARY.md` | Union: kept main's「连续 LOD / 提取缓存」rows and「选择工具与意图菜单」section, plus the feature's「主题覆盖物与地理事实」section. |
| `crates/wv-app/src/state.rs` | Took main: re-exports `SelectionState`; no `TerritorySketch` (it no longer exists). |
| `crates/wv-app/src/ui/map_controls.rs` | Took main entirely: no forest tile, no `TerritorySketch`, no `CityToolState` (removed on main), keeps 8 tiles with selection + continent dialogs. |
| `crates/wv-app/src/ui/widgets.rs` | Took main: wv-ui glob re-export shim + `local::{float_window, terrain_grid}`. |
| `crates/wv-ui/src/widgets/primitives.rs` | Kept main's `../../assets/...` paths (file moved to `wv-ui`) and added `ICON_TREES` pointing at `../../assets/ui/tool_icons/trees.svg`. |
| `crates/wv-ui/assets/ui/tool_icons/trees.svg` | Kept the moved asset in wv-ui (the old `wv-app/assets` copy was already gone). |
| `crates/wv-render/src/state.rs` | Took main: `SelectionState` only; `TerritorySketch` / `PendingRing` deleted with the standalone tool. |
| `crates/wv-edit/src/command.rs` | Union: command structs were already auto-merged; the appended test tails were reconstructed to keep both main's `AddPlaceEdge` / `GenerateContinent` tests and the feature's `AddFeaturePlace` / `UpdatePlaceTerritory` tests. |
| `crates/wv-app/src/app_bootstrap.rs` | Kept main's selection-tool/dialog/city registrations and added `FeaturePlaceDialog` + `FeatureOverlayEntities` + `feature_overlays::sync_feature_overlays`; removed territory-sketch registrations; moved `ui::feature_place_dialog` out of the already-full 20-system egui chain into a separate `EguiPrimaryContextPass` registration after `status_bar`. |
| `crates/wv-app/src/i18n/keys.rs` | Kept main's keys + feature dialog keys; removed unused `FeatureForestToolHint`; added `IntentMenuForest`; `ALL` count fixed at 390 (array entries verified). |
| `crates/wv-app/src/i18n/table.rs` | zh/en entries for every key; removed `FeatureForestToolHint`; added `IntentMenuForest`（森林…/Forest…）. |
| `docs/superpowers/specs/2026-10-17-theme-overlay-geo-fact-design.md` | Took the feature branch's finalized D1–D13 spec. |

## 2. M4 forest intent integration

- `crates/wv-app/src/ui/intent_menu.rs`: added a「森林」button (Lucide `trees` icon) alongside continent + city + four terrain intents. `intent_count_for` is now **7** for World/Region; its tests updated.
- Clicking the button copies the closed selection ring and `drill.current()` into `FeaturePlaceDialog::open_with(map, ring)`. It does **not** clear or close the selection.
- `crates/wv-app/src/ui/feature_place_dialog.rs` was rewritten:
  - state is `{ open, name, map: Option<Id>, ring: Vec<(f32,f32)> }` (no `TerritorySketch.pending`);
  - confirm calls `add_feature_place_now(world, edit, playback, name, FEATURE_FOREST, map, ring)`;
  - cancel / X / no-project / invalid ring clears the dialog;
  - successful create calls `SelectionState::cancel()` (intent-menu contract, spec D9).
- `add_feature_place_now` signature and pinned-map behavior are unchanged; its tests still pass.

## 3. Deleted from the feature branch and why

| Deleted | Why |
|---|---|
| `crates/wv-render/src/territory_sketch.rs` | Duplicate drawing gesture; selection-first (ADR-0025) owns polygon drawing. |
| `TerritorySketch` / `PendingRing` in `wv-render/src/state.rs` | Same reason; ring now lives in `FeaturePlaceDialog`. |
| Forest tile + toolbar-hint text in `map_controls.rs` | Toolbar must stay main's 8-tile layout; forest is an intent, not a tool. |
| `FeatureForestToolHint` i18n key (zh/en) | Unused after removing the tile. |
| `territory_sketch` module/registration/doc lines | Compile cleanup. |

All other feature work is retained: `Place.feature`, `World.place_geometry`, `place_territory_at`, YAML IO + diagnostics hints, `AddFeaturePlaceCommand` / `UpdatePlaceTerritoryCommand`, theme overlay rules + embedded textures, `FeatureOverlay` pass/system, POI suppression, overlay assets, plan/spec docs.

## 4. Gate results

| Gate | Result |
|---|---|
| `cargo test --workspace` | PASS (934 tests, 0 failed; doc-tests 0/ignored 2) |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p wv-render --features app` | PASS (352 passed) |
| `cargo app-test` | PASS (184 + 6 passed) |
| `cargo app-clippy` | PASS |
| `bash scripts/smoke-gui.sh` | PASS (5 s alive, no panic) |

## 5. Concerns / notes

1. **Spec drift**: per instruction the feature branch's finalized spec was taken verbatim; it still describes the standalone 圈地界 toolbar tool. Implementation is M4 intent. A follow-up doc touch may be desirable (spec is otherwise the D1–D13 source of truth).
2. **Pre-existing main lint debt surfaced by the app gates**: `city_intent::commit_city` had an unused `loc` parameter and 8 arguments. Fixed with `_loc` + `#[allow(clippy::too_many_arguments)]` (no behavior change).
3. **`terrain_ok` was computed but unused** in `intent_menu.rs` on main; wired it into `ui.add_enabled(terrain_ok, …)` for the four terrain buttons, realizing the documented "no own raster → no terrain intent" gate.
4. **Ring snapshot semantics**: the dialog commits the ring captured when the intent button was clicked. If the author drags selection anchors while the dialog is open, the committed territory is the snapshot (this matches the pinned-ring data-integrity requirement; it is not live-linked).
5. The merge commit does not include this report (kept untracked).
