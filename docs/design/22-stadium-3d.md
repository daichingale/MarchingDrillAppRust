# 22. 3Dスタジアムと Real View

## 1. 目的と範囲

Pyware 3D の "Real View"（観客席から実際にどう見えるかの3D表示）を機能網羅した上で、
**演出判断のための道具**として上回る設計をする。写実性（フォトリアルな芝・観客・照明）は目的ではない。
目的は次の問いに即座に答えられることである。

- 「このセットは、50ヤードライン15列目の観客からどう見えるか」
- 「奥の列は手前の列に隠れて見えないのではないか」
- 「この列は本当に一直線か、観客席から見て崩れて見えないか」

この文書が扱う範囲:

1. パラメトリックなスタジアム幾何（フィールド・観客席・プレスボックス）
2. 演者の3段階LOD表現と、身長・セクション色・向きの反映
3. 接地影と奥行き手がかり（列の整列が見えることを最優先）
4. 視点プリセットの拡張（既存3種 → 座席任意・演者視点を含む）
5. 観客席からの可視性解析と列の直線性崩れ検出（本製品の差別化）
6. フィールド表面（芝・利用者ロゴ・天候/時間帯ライティング）
7. `drill-render`（20）・GPUレンダラ（21）との境界
8. カメラ操作（軌道・注視点移動・FOV・ロール・慣性）と `23-camera-system.md` への引き渡し

この文書が扱わないこと:

- `DrawCmd` / `DisplayList` 自体の型定義（20の担当。本文書は要求のみ提示する）
- wgpu インスタンシングの実装（21の担当）
- キーフレームカメラ・複数カメラ・カメラのタイムライン編集（23の担当）
- `Performer.height_m` / `Section` / `Symbol` の正式なフィールド定義（15の担当。本文書はこれらが
  存在する前提で3D表示側の使い方だけを定義する）
- 向き（facing）の正式なモデル（11の担当。本文書は11に対する最小限のインターフェース要求のみ書く）
- 動画書き出しにおける3D Real Viewのオフラインレンダリング（`MEDIA_PIPELINE.md` P2、31の担当）

## 2. 現状

`docs/design/` には現時点で `00-conventions.md` のみが存在し、11・15・20・21・23 はまだ執筆されていない。
そのため本文書は、既存コードと `DESIGN_GAPS.md` に書かれた計画（B-1, A-5）を根拠にし、
未執筆文書への要求は「境界」として明示する（推測で仕様を埋めない）。

### 2.1 既存の3D投影 — `crates/drill-core/src/camera.rs`

- `Camera { target: [f32;3], yaw: f32, pitch: f32, distance: f32, fov_y_rad: f32, near: f32, far: f32 }`
  は軌道（orbit）カメラのみ。`position()`（camera.rs:71-81）は `target + distance * offset(yaw, pitch)` で
  眼位置を導出する一方向の変換で、「任意の観客席座標から yaw/pitch/distance を逆算する」API は無い。
- `project()`（camera.rs:87-113）は `gluLookAt` 相当のビュー行列と OpenGL 型透視投影を手書き実装。
  `drill-core` は `serde`/`serde_json` のみに依存しており（`Cargo.toml`）、この投影は純粋な行列演算で
  GPU・UI に依存しない。3D固有ロジックを `drill-core` に置く前例として踏襲する。
- プリセットは `audience_view` / `press_box` / `overhead` の3種（camera.rs:155-185）。
  いずれも `field_center(grid)` を注視点に固定し、`fit_distance` でフィールド全体が収まる距離を計算する。
  観客席の**任意の位置**・**演者視点**・**エンドゾーン視点**は無い。
- カメラに `roll`（傾き）フィールドは無い。`fov_y_rad` は構造体に存在するが、UIから変更する手段が無い
  （`main.rs` はプリセット選択時に `..Camera::default()` で毎回既定値に戻す）。

### 2.2 既存の描画 — `crates/drill-app/src/main.rs` の `draw_stadium`（292–414行）

- スタジアム構造物（観客席・プレスボックス・フィールド外周）は**一切描画されない**。描くのはフィールド
  ポリゴン・ヤードライン・ハッシュ・演者の点のみ（311–385行）。
- 演者は単一の塗りつぶし円（`painter.circle_filled`, 402行）。LODは無く、身長差・セクション色・
  向きの表現も無い。`Performer` は現状 `{ id: PerformerId, label: String, color: [u8;3] }`
  （`lib.rs:226-230`）のみで、`height_m` も `Section` も `facing` も存在しない
  （`DESIGN_GAPS.md` A-5 が導入を計画しているが未実装）。
- 深度ソートはペインターズアルゴリズム（393–394行、`order.sort_by` で降順距離ソート、O(n log n)）。
  半径は `(0.6 * focal / dist).clamp(1.5, 22.0)`（400行）で遠近を近似するのみで、影・大気遠近・
  整列補助線は無い。
- 可視性解析・列の直線性検出は存在しない。
- 操作はドラッグでyaw/pitch（299–303行）、ホイールでdistance（304–309行）のみ。注視点移動・FOV・
  ロール・慣性は無い。
- `PerformerId` は現状 `u32` の型エイリアス（`lib.rs:19`）であり、A-1が計画する安定ID構造体では
  ない。本文書は現行の `u32` 前提で書き、A-1到達後は型が変わるだけで意味論は変わらない設計にする。
- `crates/drill-core/Cargo.toml` に wgpu 等の描画依存は無い。3D幾何・解析は `drill-core` に置いても
  クレート境界を破らない。

### 2.3 性能ベースライン

`PRODUCT_QUALITY.md` より、開発機で 1,000人 × 60,000フレームの位置補間が合計 9.24ms
（1フレームあたり約 0.15µs、これは補間のみで描画を含まない）。3D描画・解析のコストはこれとは
別に見積もる必要がある（本文書 5節）。

## 3. 設計

### 3.1 スタジアム幾何

実在球場を模倣しない、パラメータで生成する汎用形状。`crates/drill-core/src/stadium.rs`（新規）に置く。

```rust
use crate::{GridConfig, Point};

/// A fully parametric, non-representational stadium shape built around a
/// [`GridConfig`] field. Nothing here references a real venue.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StadiumModel {
    /// Apron width beyond the playing field before the first row of seats.
    pub sideline_margin_m: f32,
    pub end_zone_margin_m: f32,
    pub stands: Vec<StandSection>,
    pub press_box: Option<PressBox>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StandId(pub u16);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StandSection {
    pub id: StandId,
    pub baseline: StandBaseline,
    pub rows: u16,
    pub row_rise_m: f32,   // vertical rise per row (rake)
    pub row_depth_m: f32,  // front-to-back depth per row
    pub front_offset_m: f32, // distance from field edge to first row
    /// 0.0 = straight grandstand; >0.0 bows toward the field. Expressed as
    /// sagitta (max inward offset at the midpoint) in meters.
    pub curvature_sagitta_m: f32,
    /// Extent along the stand's baseline, in field-grid units (yards/meters
    /// per `GridConfig::unit`), matching the field's local x (Home/Visitor)
    /// or y (EndZone) axis.
    pub extent: std::ops::Range<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StandBaseline {
    Home,      // along y = 0 side of the field
    Visitor,   // along y = grid.height side
    EndZoneNear, // along x = 0
    EndZoneFar,  // along x = grid.width
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PressBox {
    pub stand: StandId,
    pub height_above_stand_m: f32,
    pub width_fraction: f32, // fraction of the stand's extent, centered
}

impl StadiumModel {
    /// A generic default shape: a raked home stand and a lower visitor
    /// stand, no press box unless requested. Deterministic given `grid`.
    pub fn generic(grid: &GridConfig) -> Self {
        Self {
            sideline_margin_m: 3.0,
            end_zone_margin_m: 5.0,
            stands: vec![
                StandSection {
                    id: StandId(0),
                    baseline: StandBaseline::Home,
                    rows: 40,
                    row_rise_m: 0.35,
                    row_depth_m: 0.75,
                    front_offset_m: 3.0,
                    curvature_sagitta_m: 0.0,
                    extent: 0.0..grid.width,
                },
                StandSection {
                    id: StandId(1),
                    baseline: StandBaseline::Visitor,
                    rows: 20,
                    row_rise_m: 0.30,
                    row_depth_m: 0.75,
                    front_offset_m: 3.0,
                    curvature_sagitta_m: 0.0,
                    extent: 0.0..grid.width,
                },
            ],
            press_box: Some(PressBox { stand: StandId(0), height_above_stand_m: 4.0, width_fraction: 0.3 }),
        }
    }

    pub fn stand(&self, id: StandId) -> Option<&StandSection> {
        self.stands.iter().find(|s| s.id == id)
    }
}

impl StandSection {
    /// World-space seat position at eye height (~1.2 m above the seat deck),
    /// for `along_frac in 0.0..=1.0` across `extent` and `row in 0..rows`.
    /// Curvature bows the stand toward the field along its midpoint.
    pub fn seat_eye_position(&self, grid: &GridConfig, along_frac: f32, row: u16) -> [f32; 3] {
        let along_frac = along_frac.clamp(0.0, 1.0);
        let x_along = self.extent.start + (self.extent.end - self.extent.start) * along_frac;
        let bow = self.curvature_sagitta_m * (1.0 - (2.0 * along_frac - 1.0).powi(2));
        let depth = self.front_offset_m + self.row_depth_m * row as f32 + bow;
        let height = 1.2 + self.row_rise_m * row as f32;
        match self.baseline {
            StandBaseline::Home => [x_along, height, -depth],
            StandBaseline::Visitor => [x_along, height, grid.height + depth],
            StandBaseline::EndZoneNear => [-depth, height, x_along],
            StandBaseline::EndZoneFar => [grid.width + depth, height, x_along],
        }
    }
}
```

幾何は静的（`Document` の編集操作では変わらない、`GridConfig` が変わった時だけ再生成する）。
`Document` に `pub stadium: StadiumModel` を追加する（`#[serde(default)]` で v2 との後方互換を保つ）。

### 3.2 演者の3D表現とLOD

3段階LODは**画面上の高さ（px）**で選ぶ。ズームすればするほど詳細になり、1,000人が引きで
見えている時は最も軽い表現になる。

```rust
// crates/drill-core/src/stadium.rs (続き)

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerformerLod {
    /// A flat tinted disc/quad. Cheapest; used when the performer occupies
    /// only a few pixels (wide shots, 1,000-performer overhead views).
    Billboard,
    /// A schematic stick figure: torso + head + a short "nose" segment
    /// pointing along facing. Communicates orientation without needing a
    /// textured sprite.
    SimpleFigure,
    /// A section-specific 2D silhouette (brass bell wedge, drum rectangle,
    /// guard flag) billboarded to the camera and rotated in-plane by facing.
    InstrumentSilhouette,
}

/// Screen-space height thresholds, in pixels, for LOD selection. Public so
/// the app can tune them per display DPI.
pub struct LodThresholds { pub simple_figure_px: f32, pub silhouette_px: f32 }

impl Default for LodThresholds {
    fn default() -> Self { Self { simple_figure_px: 10.0, silhouette_px: 40.0 } }
}

/// `screen_height_px` = `focal_length_px * height_m / distance_to_eye_m`,
/// the same projection the app already uses for billboard radius
/// (`main.rs:400`, generalized here to use per-performer height).
pub fn choose_lod(screen_height_px: f32, thresholds: &LodThresholds) -> PerformerLod {
    if screen_height_px < thresholds.simple_figure_px {
        PerformerLod::Billboard
    } else if screen_height_px < thresholds.silhouette_px {
        PerformerLod::SimpleFigure
    } else {
        PerformerLod::InstrumentSilhouette
    }
}
```

反映するメタデータ（15が `Performer` に追加する前提。参照のみで再定義しない）:

- `Performer.height_m: f32` — ワールド空間での縦extent（足元 y=0、頭頂 y=height_m）。
  スケール差が無いと1,000人が全員同じ「棒」に見え、LODの意味が無くなる。
- `Section.color: [u8;3]` と `Performer.color: Option<[u8;3]>` — `color.unwrap_or(section.color)` で
  塗り色を決める。ドリルブックの色分けと3D表示が同一の色ソースを参照することを不変条件にする（4節）。
- `facing`（11が定義する前提。本文書からの最小要求は次の通り）:

```rust
/// Minimal contract this document requires from 11-transition-model.md.
/// 11 owns the real type (likely carrying gate/curve-derived heading); this
/// signature is what 22's renderer and analyzer call.
pub trait FacingAt {
    /// Heading in radians, 0 = facing +Y (upfield, matches field north per
    /// `16-coordinates.md`'s convention), measured clockwise looking down.
    fn facing_rad(&self, set_index: usize, local_count: f32, performer: crate::PerformerId) -> f32;
}
```

11が未執筆の間、22の描画・解析コードはこのトレイトの**モック実装**（例えば「常に次セットへの移動方向を
向く」というプレースホルダ）で開発を進めてよいが、11到着後はこのモックを置き換えるだけで済むように、
呼び出し側は必ず `&dyn FacingAt` 経由にする（具体型に依存しない）。

描画（3.7節の境界を参照）:

- `Billboard`: 中心 `field_to_world(pos, height_m * 0.5)`、高さ `height_m` 相当のスクリーンサイズの円/矩形。
- `SimpleFigure`: 頭（小円）+ 胴（縦線）+ 向きを示す短い「鼻」線。3本程度の `DrawCmd::Line` で足りる。
- `InstrumentSilhouette`: セクション種別ごとの簡易シルエット（ユーザーが用意した楽器画像ではなく、
  ベクタ図形。写実素材は同梱しない — 00-conventions.mdの「模倣しないもの」に整合）。

### 3.3 接地影と被写界（奥行き手がかり）

**最優先事項**: 1,000人が重なった時に「どの列が揃っているか」が見えること。ビルボードの見た目
（カメラに正対する平面）は奥行きの手がかりにならないため、地面に落ちる影が唯一の曖昧さのない
位置情報になる。

```rust
/// A flat ground-plane ellipse under each performer, drawn *before* the
/// performer sprite in depth order. Independent of camera facing, so it is
/// the one visual element that unambiguously encodes ground position even
/// when 1,000 billboards overlap on screen.
pub struct GroundShadow {
    pub center: Point,       // same as the performer's field position
    pub radius_m: f32,       // ~0.3m, independent of height_m (footprint, not silhouette)
    pub alpha: f32,          // constant ambient-occlusion approximation, no real shadow casting
}

pub fn ground_shadow_for(pos: Point, radius_m: f32) -> GroundShadow {
    GroundShadow { center: pos, radius_m, alpha: 0.35 }
}
```

奥行き手がかりの設計:

1. **影**: 上記。人型/シルエットが重なっても影の位置で列のズレが判別できる。
2. **大気遠近**: 遠い演者ほど背景色に近づける。
   `color_faded = lerp(color, sky_tint, saturate(distance / fog_distance_m))`。
   `fog_distance_m` はカメラの `far` ではなくスタジアム規模から決める（例: `grid` 対角長の1.5倍）。
   これにより press box やオーバーヘッドからの1,000人表示でも手前と奥の区別がつく。
3. **整列オーバーレイ（トグル）**: 「同じ列に属するはずの演者」を影の位置で結ぶ細い補助線。
   グルーピングは 3.5節のアルゴリズムが使う `RowSpec` を流用する（新しい概念を増やさない）。
   影の位置をx（またはaraysの並び順）でソートして折れ線を引くと、揃っている列は直線、
   崩れている列は波打って見える — これは 3.5 の数値解析結果を**視覚的に**裏付ける役割。
4. **グリッドのフェード**: ヤードライン・ハッシュも大気遠近と同じ式で遠くを薄くし、
   コントラストで手前を強調する（現状 `draw_stadium` の一様アルファを距離依存に変更する）。

深度ソートは既存のペインターズアルゴリズム（O(n log n)、`main.rs:393-394`）を維持する。1,000人規模で
安定して動作しており、置き換える理由がない。

### 3.4 視点プリセットの拡張

既存3プリセットは `target` 固定・軌道パラメータのみ。観客席の任意位置や演者視点は「注視点から
離れた固定位置に眼を置く」必要があり、軌道パラメータでは直接表現できないため、**眼位置から
軌道パラメータを逆算するコンストラクタ**を追加する（`Camera` 自体の構造は変えない）。

```rust
// crates/drill-core/src/camera.rs に追加

impl Camera {
    /// Build a camera whose eye sits exactly at `eye`, looking at `target`.
    /// Inverse of [`Camera::position`]: recovers `yaw`/`pitch`/`distance`
    /// from a desired eye position so seat-based and performer-POV cameras
    /// still fit the existing orbit representation (needed by 23's
    /// keyframe interpolation, which interpolates yaw/pitch/distance).
    pub fn at_eye(eye: [f32; 3], target: [f32; 3], fov_y_rad: f32, near: f32, far: f32) -> Camera {
        let d = sub(eye, target);
        let distance = dot(d, d).sqrt().max(1e-4);
        let offset = [d[0] / distance, d[1] / distance, d[2] / distance];
        let pitch = offset[1].clamp(-1.0, 1.0).asin();
        let yaw = offset[0].atan2(offset[2]);
        Camera { target, yaw, pitch, distance, fov_y_rad, near, far }
    }

    /// A seat in `stand` at `row`, `along_frac` across its extent, looking
    /// at the field center. Reuses [`crate::stadium::StandSection::seat_eye_position`].
    pub fn from_seat(
        grid: &crate::GridConfig,
        stand: &crate::stadium::StandSection,
        row: u16,
        along_frac: f32,
    ) -> Camera {
        let eye = stand.seat_eye_position(grid, along_frac, row);
        Camera::at_eye(eye, field_center(grid), std::f32::consts::FRAC_PI_3, 0.1, 2000.0)
    }

    /// Eye at the near end zone, looking down the length of the field —
    /// useful for judging left/right alignment across the whole formation.
    pub fn end_zone(grid: &crate::GridConfig, near: bool) -> Camera {
        let eye = if near {
            [grid.width * 0.5, 1.7, -8.0]
        } else {
            [grid.width * 0.5, 1.7, grid.height + 8.0]
        };
        Camera::at_eye(eye, field_center(grid), std::f32::consts::FRAC_PI_3, 0.1, 2000.0)
    }

    /// What a specific performer is facing, at eye height. `heading_rad`
    /// comes from 11's `FacingAt` (3.2). `lookahead_m` is how far in front
    /// to place the look-at point (their sightline, not their head).
    pub fn performer_pov(pos: crate::Point, heading_rad: f32, lookahead_m: f32) -> Camera {
        let eye = field_to_world(pos, 1.6);
        let target = [
            eye[0] + heading_rad.sin() * lookahead_m,
            eye[1],
            eye[2] + heading_rad.cos() * lookahead_m,
        ];
        Camera::at_eye(eye, target, std::f32::consts::FRAC_PI_4, 0.1, 500.0)
    }
}
```

`ViewpointPreset` を `drill-app` 側の選択状態として持つ（`Camera` 自体は「今のカメラ」の値であって
プリセットの列挙ではない。既存の `Camera::press_box(&grid)` 等と同じパターンを踏襲）:

```rust
// crates/drill-app/src/view/stadium3d.rs (新規, C-1のツリーに沿う)
pub enum ViewpointPreset {
    AudienceRow { stand: drill_core::stadium::StandId, row: u16 },
    PressBox,
    Overhead,
    EndZone { near: bool },
    CustomSeat { stand: drill_core::stadium::StandId, row: u16, along_frac: f32 },
    PerformerPov { performer: drill_core::PerformerId },
}
```

### 3.5 見え方の解析（差別化機能）

Pywareに無い機能。2つの解析を提供する。どちらも `drill-core` の純粋関数として実装し、
`ScanScratch`（`DESIGN_GAPS.md` A-4）と同じ「再利用スクラッチ・ヒープ確保ゼロ」パターンに従う。
新規モジュール `crates/drill-core/src/visibility.rs`。

#### 3.5.1 観客席からの遮蔽（occlusion）判定

演者を「肩幅半径 `shoulder_radius_m` の鉛直円柱」として近似する。観客の眼 `eye` から対象演者
`target` への視線（レイ）上、`eye` により近い他の演者がレイを角度的に遮っていれば遮蔽とみなす。

```rust
pub struct OcclusionResult {
    pub target: crate::PerformerId,
    pub blocked_by: Vec<crate::PerformerId>, // ordered nearest-to-eye first
    /// 1.0 = fully visible, 0.0 = fully blocked by the nearest occluder's
    /// angular footprint (not a physically exact fraction, a usable proxy).
    pub visible_fraction: f32,
}

/// Reused across calls: no heap allocation once warmed up (same discipline
/// as `editing::ScanScratch`).
#[derive(Default)]
pub struct VisibilityScratch {
    /// Ground-plane uniform grid over performer positions, cell width =
    /// `shoulder_radius_m * 4`, so a ray only needs to visit the handful of
    /// cells it actually crosses (DDA walk) instead of testing all N
    /// performers against every target.
    cell_of: Vec<u32>,
    bucket_head: Vec<i32>,
    bucket_next: Vec<i32>,
    candidate_ids: Vec<u32>,
}

/// For one seat `eye`, test visibility of every performer in `positions`
/// against every other performer as a potential occluder.
///
/// Complexity: naive O(n^2) (~1,000 x 1,000 = 1e6 pair tests for the full
/// cast); with the grid in `VisibilityScratch` restricting occluder
/// candidates to cells the ray crosses (~20 cells for a field-scale ray),
/// it drops to ~O(n * k) with k ~ 20, i.e. ~20,000 tests for 1,000
/// performers from one seat. See section 5 for the real-time budget this
/// implies.
pub fn visibility_from_seat(
    positions: &[crate::Point],
    heights_m: &[f32],       // parallel to `positions`, from Performer.height_m
    eye: [f32; 3],
    shoulder_radius_m: f32,
    scratch: &mut VisibilityScratch,
    out: &mut Vec<OcclusionResult>,
);
```

判定の核（アルゴリズム）:

1. 対象 `P` の視線点を `sight = field_to_world(P.pos, P.height_m * 0.85)`（頭のやや下、胸〜首の高さ）とする。
2. レイ方向 `dir = sight - eye`、距離 `dist_p = |dir|`。
3. 候補遮蔽者 `O`（`VisibilityScratch` のグリッドでレイが通過するセルに属する演者のみ）について:
   - `t = dot(O.pos_world - eye, dir) / dot(dir, dir)` を計算。`t` が `(0, 1)` の範囲外（`O` が `eye` より
     遠い、または `P` より遠い）なら遮蔽対象から除外。
   - `O` の眼からの距離 `dist_o = t * dist_p` における遮蔽者の**見かけの角半径**
     `angular_radius = atan2(shoulder_radius_m, dist_o)` を求める。
   - `O` とレイの**角度差** `angle = acos(dot(normalize(O.pos_world - eye), normalize(dir)))` を求める。
   - `angle < angular_radius` なら `O` は `P` を遮蔽する。
4. 遮蔽者が1人でもいれば `blocked_by` に追加し、最も近い遮蔽者の `angular_radius / angle` 比から
   `visible_fraction` を近似する。

#### 3.5.2 列の直線性が観客席から見て崩れる箇所の検出

「列」は演者IDの順序付き集合として与える（ハッシュ上に整列させたセクションの並び等、呼び出し側が
`Set` の座標から抽出する。抽出ロジック自体は 13/14 の担当範囲なので、ここでは既に得られた
`RowSpec` を入力として受け取るところから設計する）。

```rust
pub struct RowSpec {
    pub members: Vec<crate::PerformerId>,   // performance order along the row
    pub expected_shape: RowShape,
}

pub enum RowShape { Straight, Arc { radius_m: f32 } }

pub struct RowFlatnessReport {
    /// Max perpendicular deviation from the best-fit line/arc, in field
    /// meters. Camera-independent ground truth.
    pub field_space_residual_m: f32,
    /// Max perpendicular deviation from the *expected* projected line, in
    /// screen pixels, as seen from the given seat. A row can be perfect in
    /// field space (residual ~0) yet read as crooked from an oblique seat
    /// once foreshortening is applied — this is the number that matters
    /// for the designer standing at that seat.
    pub screen_space_deviation_px: f32,
    /// Ratio of farthest-to-nearest eye distance among the row's members.
    /// High values mean the row runs toward/away from the seat, so an
    /// audience member's depth perception (much weaker than left/right)
    /// will struggle to judge its straightness at all, independent of how
    /// straight it actually is.
    pub depth_spread_ratio: f32,
    pub flags: Vec<RowFlag>,
}

pub enum RowFlag {
    /// Wrong even in field coordinates — a design error, not a viewing angle.
    CrookedInField { residual_m: f32 },
    /// Fine in field coordinates but crosses the pixel threshold from this seat.
    CrookedFromSeat { deviation_px: f32 },
    /// depth_spread_ratio exceeds threshold: alignment will be hard to judge live.
    Telescoped { depth_spread_ratio: f32 },
    /// One or more interior members are occluded from this seat (3.5.1).
    PartiallyOccluded { members: Vec<crate::PerformerId> },
}

pub fn analyze_row_from_seat(
    positions: &[crate::Point],
    heights_m: &[f32],
    row: &RowSpec,
    camera: &crate::camera::Camera,
    viewport: (f32, f32),
    crooked_px_threshold: f32,   // suggested default 4.0 px at 1920x1080, see 9
    telescoped_ratio_threshold: f32, // suggested default 2.5
    scratch: &mut VisibilityScratch,
) -> RowFlatnessReport;
```

計算量: 列サイズ `m`（典型的にセクション規模、十数〜百人程度）について、最小二乗フィッティングは
`O(m)`。遮蔽判定は列の各メンバーに対し 3.5.1 と同じ `O(k)`（`k` ≈ 20）なので `O(m * k)`。
1,000人ドキュメント全体を全列×全座席で回すと大きくなるため、5節でリアルタイム/オフラインの
使い分けを規定する。

### 3.6 フィールド表面

```rust
// crates/drill-core/src/stadium.rs (続き)

/// A user-supplied logo placed on the turf. The image itself is never
/// bundled with the product (00-conventions.md: 模倣しないもの); this only
/// records placement. Decoding/security limits are 51's responsibility —
/// referenced here as a boundary requirement (max texture dimensions, max
/// file size, format allowlist).
///
/// `asset_path` is a project-relative path string for now. 41
/// (永続化・プロジェクトコンテナ・復旧) is expected to introduce a proper
/// `AssetRef`/`AssetState::{Present,Missing}` type for all externally
/// referenced files (audio, images); once that lands, `FieldLogo` should
/// switch to it instead of a bare `String` so a moved/deleted logo file
/// degrades the same way a missing audio file does (`PRODUCT_QUALITY.md`:
/// 「音声・画像が欠落してもドリル本体を開ける」).
pub struct FieldLogo {
    pub asset_path: String,          // project-relative; see note above
    pub anchor: crate::Point,        // field-space center
    pub size_m: (f32, f32),
    pub rotation_deg: f32,
}

pub struct Lighting {
    pub sun_elevation_deg: f32,  // 0 = horizon, 90 = overhead
    pub sun_azimuth_deg: f32,
    pub ambient: f32,            // 0..1, fill light with no directionality
    pub sky_tint: [f32; 3],      // used for fog/atmospheric blending (3.3)
    pub fog_density: f32,
}

pub enum Weather { Clear, Overcast, NightLights }

impl Lighting {
    pub fn preset(weather: Weather) -> Self {
        match weather {
            Weather::Clear => Self { sun_elevation_deg: 55.0, sun_azimuth_deg: 200.0, ambient: 0.35, sky_tint: [0.55, 0.65, 0.75], fog_density: 0.015 },
            Weather::Overcast => Self { sun_elevation_deg: 90.0, sun_azimuth_deg: 0.0, ambient: 0.75, sky_tint: [0.6, 0.6, 0.62], fog_density: 0.03 },
            Weather::NightLights => Self { sun_elevation_deg: 35.0, sun_azimuth_deg: 0.0, ambient: 0.2, sky_tint: [0.05, 0.06, 0.1], fog_density: 0.05 },
        }
    }
}
```

芝は写真ではなく、5ヤード間隔ごとに明度を交互にするモウイングストライプを手続き的に生成する
（`GridConfig.major_line_interval` から導出、既存のヤードライン計算と同じ刻みを再利用する）。
`Lighting` は物理シミュレーションではなく、方向性のある明暗差と色温度でおおまかな時間帯・天候を
伝えるだけに留める（写実性より判断への寄与を優先する本文書の方針に合わせる）。

### 3.7 20 (`drill-render`) / 21 (GPUレンダラ) との境界

`drill-render` も `21-gpu-renderer.md` もまだ存在しないため、ここでは**要求**のみを列挙する
（00-conventions.mdの「他文書の担当範囲へ踏み込まない」に従い、`DrawCmd` 自体は定義しない）。

**現状の問題として先に指摘すべきこと**: 現在の3D投影（`main.rs:316-320` の `proj` クロージャ、
392–406行の深度ソートと描画）は `drill-app` の中で行われている。これは00-conventions.mdの
クレート境界表「`drill-app` は egui/wgpu の表示と入力変換のみ。業務ロジックを持たない」に反する
（射影・深度ソート・LOD選択は業務ロジック）。20が導入されたら、`Scene`（`Document` + `count` +
`Camera` + `RenderOptions`）を受けて `DisplayList` を返す `drill_render::build()` の中に
この投影・深度ソート・LOD選択・遮蔽注釈を移し、`drill-app` 側は投影済みの2D座標を持つ
`DrawCmd` を描くだけにする。これを20への要求の前提とする。

20（DisplayList）への要求:

- `Scene` は `camera: Option<Camera>` と `viewport: (f32, f32)` を持てること（2Dモードでは `None`）。
- LOD 0/1（Billboard, SimpleFigure）は投影後の2D座標を使う既存の `DrawCmd::Dot` / `DrawCmd::Line` /
  `DrawCmd::Text` で表現できる想定であり、新しい `DrawCmd` 種別を**必須では要求しない**。
- 地面・観客席・プレスボックスの静止ジオメトリ（`StadiumModel` 由来、`Document` の編集では変わらない）
  向けに `DrawCmd::Mesh { vertices: Range<u32>, indices: Range<u32>, transform: [[f32;4];4], color: Rgba }`
  を要求する。毎フレーム再構築せず、`StadiumModel`/`grid` が変わった時だけ再生成してキャッシュする。
- ロゴ・芝テクスチャ向けに `DrawCmd::TexturedQuad { corners: [Vec2;4], uv: [Vec2;4], texture: TextureRef }`
  を要求する。
- `DrawCmd::Trail`（B-1で既に要求済み）は3Dでも同じ意味で使えること（射影済み2D点列）。

21（GPUレンダラ）への要求:

- LOD 0（Billboard）は1,000演者を**1回のインスタンス描画呼び出し**で処理できるインスタンスレイアウトを
  要求する: `PerformerInstance { center: [f32;3], size: f32, color: [u8;4], facing_rad: f32 }`。
  22はこのレイアウトの**必要フィールド**だけを定義し、パイプライン自体は21が実装する。
  1,000インスタンス程度はCPU側のペインターズソート＋GPU側インスタンス描画の組み合わせでも
  16.6ms予算内に収まる規模だが、4,000人（上限規模）でも劣化するだけで壊れないことを21に要求する。
- `DrawCmd::Mesh`（観客席・地面）は静的なので頂点バッファの使い回し（B-3のJob基盤とは無関係、
  単に「毎フレーム再アップロードしない」）を21に要求する。

23（キーフレームカメラ）への要求:

- `Camera` のyawは周期角なので、キーフレーム間の補間は最短経路（`shortest_angle(a, b)`）で行う
  必要がある。単純な線形補間では ±πをまたぐ時に逆回転する。23はこれを踏まえた
  `Camera::lerp(a: &Camera, b: &Camera, t: f32) -> Camera` を持つこと。
- 3.4節の `Camera::at_eye` / `from_seat` / `performer_pov` はキーフレームの**始点・終点を作る**
  ためのコンストラクタとして23から呼ばれる想定。23はこれらの戻り値をそのままキーフレームの
  値として保持できる（`Camera` の構造がキーフレーム補間にそのまま使えるように3.4節を設計した）。

### 3.8 カメラ操作

既存: ドラッグでyaw/pitch回転、ホイールでdistanceズーム（`main.rs:299-309`）。追加する操作:

```rust
// crates/drill-core/src/camera.rs に追加

impl Camera {
    /// Right-handed basis at the current eye: (right, up, forward-into-scene).
    /// Exposed so the app can translate `target` along the view plane
    /// (pan) without duplicating the view-matrix math that already exists
    /// in `view_matrix`.
    pub fn basis(&self) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let eye = self.position();
        let f = normalize(sub(self.target, eye));
        let up = [0.0, 1.0, 0.0];
        let s = normalize(cross(f, up));
        let u = cross(s, f);
        (s, u, f)
    }

    /// Move `target` (and therefore the whole orbit) by `dx`/`dy` screen-ish
    /// units along the current right/up basis, scaled by distance so pan
    /// speed feels consistent whether zoomed in or out.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let (right, up, _) = self.basis();
        let scale = self.distance * 0.001;
        for i in 0..3 {
            self.target[i] += (right[i] * dx - up[i] * dy) * scale;
        }
    }
}
```

- `roll: f32` フィールドを `Camera` に追加する（`#[serde(default)]` でv2との後方互換を保つ）。
  `view_matrix` の `up = [0,1,0]` を `up = rotate_around(f, [0,1,0], roll)` に変更する。
  既定値0では既存の全テスト（camera.rs:235-372）の結果は変わらない。
- FOV: `fov_y_rad` を `20°..=90°`（`std::f32::consts` 由来のラジアン範囲）にクランプしつつ、
  Ctrl+ホイール等の修飾キー入力で変更できるようにする（`drill-app` 側のUI変更、`drill-core` には
  クランプ関数 `Camera::set_fov_deg(&mut self, deg: f32)` を追加する）。
- 慣性: `angular_velocity: Vec2` と `zoom_velocity: f32` は**ドキュメントの一部ではないUI一時状態**
  なので `drill-core::camera::Camera` には入れない。`drill-app/src/view/stadium3d.rs`
  （C-1のツリー）に `CameraInertia { angular_velocity: [f32;2], zoom_velocity: f32, damping: f32 }`
  を置き、毎フレーム `camera.yaw += angular_velocity[0] * dt; angular_velocity *= damping.powf(dt*60.0)`
  のように減衰させる。ドラッグ中は速度をドラッグ量から更新し、離した後は慣性で減衰させる。

## 4. 不変条件

1. `Camera::position()` は常に `target` から厳密に `distance` の距離にある（既存テスト
   `position_is_distance_from_target` を維持。`at_eye`/`from_seat`/`performer_pov` で作った
   カメラも同じ不変条件を満たす — テストで検証する）。
2. `Camera::at_eye(eye, target, ...).position()` は `eye` に一致する（往復精度 `1e-3` 以内）。
3. 演者の塗り色は常に `color.unwrap_or(section.color)` の一箇所からのみ導出される。2D表示・
   3D表示・ドリルブック・カウントシートが**同じ関数**を呼ぶ（別々に色ロジックを持たない）。
4. `visibility_from_seat` / `analyze_row_from_seat` は同じ `(positions, heights, eye, params)`
   から常に同じ結果を返す（決定論、00-conventions.md 不変条件5）。
5. `VisibilityScratch` は初回呼び出し後、2回目以降の呼び出しでヒープ確保しない
   （`editing::ScanScratch` と同じ検証方法: ポインタ比較）。
6. `roll = 0.0` のとき、既存の `view_matrix` の出力は変更前と完全に一致する（回帰）。
7. スタジアム形状（`StadiumModel`）は `Document` の編集操作（`Edit::apply`）では変化しない。
   `grid` が変わった時のみ、明示的な再生成関数を通す。
8. 3D表示はドキュメントを変更しない（`main.rs:291` の既存コメント「Editing stays in 2D」を維持し、
   `draw_stadium` 相当の関数はどのリファクタ後も `&self` ではなく最小限の `&mut self`
   （カメラ状態のみ）に留める）。

## 5. 性能

基準規模: 演者1,000人。3Dビューは2Dビューと排他表示（`main.rs:64-67, 1437-1440` の
`ViewMode`）なので、3D表示中は16.6ms予算のほぼ全体を3D側の投影・LOD選択・描画に使ってよいが、
再生中は `positions_at`（既存 9.24ms/60,000フレーム = 1フレーム約0.15µs、無視できる）・
タイムラインUI・オーディオクロック読み取りも同じフレームで走る。3D固有処理に**14ms**を予算とし、
残り2.6msを他処理に残す配分とする。

内訳（1,000人、静止フレームの再描画。ドラッグ中の再投影も同じコスト）:

| 処理 | 計算量 | 見積り |
|---|---|---|
| 深度ソート | O(n log n) | 1,000人で ~0.05ms（既存実装のまま） |
| LOD選択（画面高さ計算） | O(n) | ~0.02ms |
| 影・大気遠近の色計算 | O(n) | ~0.05ms |
| CPU側投影（20導入前の暫定経路） | O(n) | ~0.1ms |
| GPUインスタンス描画（21導入後、Billboard LOD） | O(n) だが1回のdraw call | 数百µs〜1ms程度（GPU依存、上限規模4,000人でも1回のdraw call） |
| 観客席・プレスボックスのメッシュ描画 | 静的、フレーム毎の再構築なし | ほぼ0（キャッシュ済みバッファ） |

3D固有処理の合計は14ms予算に対して十分な余裕がある。ボトルネックになり得るのは
**GPU未導入の暫定経路**（現状のegui painterでの個別描画呼び出し、`circle_filled` を1,000回）であり、
これは21が入るまでの間の一時的な制約として明記する（6節フォールバックも参照）。

### 5.1 可視性解析の計算量（リアルタイム/オフラインの切り分け）

- **1点の観客席 × 1,000演者の遮蔽判定**: ナイーブ実装は `O(n^2)` = 約100万ペア判定。
  `VisibilityScratch` の空間グリッドでレイが通過するセルのみ候補にすると、対象1人あたり
  候補遮蔽者は概ね20人程度に収まり、1,000対象で **約20,000回の角度判定**（3.5.1）。
  1回の判定は三角関数数回・ベクトル演算数回なので、20,000回で**1ms未満**と見積もる。
  これは**単一座席**を選んでいる間のインタラクティブ表示（ドラッグ停止後・カメラ変更のたびに
  1回）としてリアルタイムに近い頻度（毎フレームではなく、カメラ操作が止まった時にデバウンスして
  実行、`DESIGN_GAPS.md` A-4の掃引衝突検査と同じデバウンス方針）で実行してよい。
- **全座席サンプル（観客席解析レポート）**: 代表座席を50点程度サンプルし、各座席で全列
  （典型的に十数列、列あたり十〜百人）を解析すると、50座席 × 20,000判定 = 100万回。
  これは**1フレームでは走らせない**。`DESIGN_GAPS.md` B-3 の `Job<T>` 基盤に乗せ、
  バックグラウンドスレッドで実行し進捗をポーリングする「観客席解析ジョブ」として提供する。
  実行契機は明示的なボタン操作、または `document_revision` が変化してからのデバウンス
  （A-4の衝突走査と同じパターン）。UIスレッドをブロックしない。
- 上限規模4,000人では同じ比率で4倍、単一座席判定でも数ms規模になり得るため、単一座席の
  インタラクティブ解析も**メインスレッドではなくJobで**実行し、結果が揃うまで前回の結果を
  表示し続ける設計にする（6節）。

## 6. 失敗モードと安全性

- **3D表示が使えない環境（GPU無し・ドライバ不良・wgpu初期化失敗）**: eframeの `Renderer::Wgpu`
  自体が起動できない場合はアプリ全体が起動しない既存の制約があるため、これは本文書の範囲外
  （43-application-structureの担当）。本文書が扱うのは「wgpuは動くがインスタンシング
  パイプライン（21）がまだ無い、または特定GPUで不安定」なケースで、**Tier1フォールバック**
  として現行の `draw_stadium` 相当（egui painterでの個別描画、投影は`drill-render`経由に
  移行済み）を維持し続ける。21が使えない環境では自動的にTier1へ落とす
  （`RenderOptions.prefer_gpu_instancing: bool` を`drill-render`側に持たせ、初期化失敗時に
  falseへフォールバックする）。
- **動画書き出し時の3D Real View（P2、31の担当）**: オフラインレンダリングはヘッドレス環境
  （CI・サーバー）で動く必要があるため、GPUに依存しないソフトウェアラスタライズ経路が別途
  必要になる。本文書は要求のみ残す: `drill-render::build()` の出力（`DisplayList`）は
  GPU/CPUどちらのバックエンドでも同じ結果を再現できる決定論的な中間表現であること
  （既にB-1で要求済みの性質を3Dでも維持する）。
- **信頼できない入力（ロゴ画像）**: `FieldLogo.asset` はユーザー提供画像。デコードは
  51-securityの規約に従い、サイズ上限・フォーマット許可リスト・パストラバーサル拒否を適用する
  （具体的な上限値は51が定める。ここではデコードをUIスレッドで行わないことだけを要求する
  — 00-conventions.md不変条件6）。
- **NaN/Inf**: `Camera::at_eye` は `target == eye` のとき（`distance ≈ 0`）`normalize` が
  ゼロ除算になり得る。`distance.max(1e-4)` で下限を設け、`offset` の各成分が有限であることを
  呼び出し前提として保証する。`pitch = offset[1].clamp(-1.0, 1.0).asin()` は `asin` の定義域外
  入力（浮動小数誤差で `1.0` をわずかに超える）によるNaNを防ぐ。
- **列解析のゼロ除算**: `RowSpec.members` が0〜1人の場合、最小二乗フィッティングが定義できない。
  `analyze_row_from_seat` はこの場合 `field_space_residual_m: 0.0` で早期リターンし、
  パニックしない。
- **視認性解析のパニック禁止経路**: `heights_m` の長さが `positions` と一致しない場合、
  添字パニックではなく空の `Vec` を返す（00-conventions.md 不変条件「パニック禁止経路」）。

## 7. テスト計画

### 単体テスト

- `Camera::at_eye(eye, target, ...).position()` が `eye` に一致する（往復精度チェック、
  `target == eye` を含む縮退ケースがパニックしないことも）。
- `Camera::from_seat` / `Camera::end_zone` / `Camera::performer_pov` それぞれについて、
  既存の `position_is_distance_from_target` と同じ形の距離検証。
- `roll = 0.0` で `view_matrix` の出力が既存実装とビット単位で一致する回帰テスト。
- `StandSection::seat_eye_position` の `curvature_sagitta_m = 0.0` のとき、y座標一定・
  along_fracに対し線形であることの性質テスト。
- `choose_lod` の閾値境界（`simple_figure_px` ちょうど、`silhouette_px` ちょうど）。

### ゴールデン/シナリオテスト

- 3人の演者を手で配置した最小シナリオ（1人が別の2人の間、観客席の眼から見て一直線）で
  `visibility_from_seat` が期待通り遮蔽を検出する。
- 意図的に1人だけ0.5m前にずらした「列」で `analyze_row_from_seat` が
  `CrookedInField` を検出し、揃った列では検出しないことを両方テストする。
- 列は field 空間で完璧だが、極端に浅い角度の座席（`StandSection::seat_eye_position` の
  `row = 0`、列がその座席から見て真正面に伸びる配置）で `Telescoped` フラグが立つケース。

### プロパティテスト

- ランダムな `yaw`/`pitch`/`distance` の `Camera` について `at_eye(cam.position(), cam.target, ...)`
  が元の `yaw`/`pitch`/`distance` を（周期性・ジンバルロックを考慮した許容誤差で）復元する。

### ベンチマーク（`crates/drill-core/benches/`）

- 1,000演者・1座席の `visibility_from_seat`: 初回呼び出し後、2回目以降で `VisibilityScratch`
  内部バッファのポインタが変化しないこと（ゼロ確保）と、実行時間が5節の見積り（1ms未満）に
  収まることを計測する。
- 4,000演者（上限規模）で同ベンチを再実行し、パニックせず、時間が比例的に劣化するだけであることを
  記録する（劣化してよいが壊れてはいけない、00-conventions.md）。

### ストレス

- `StadiumModel::generic` を1,000通りの `GridConfig`（幅・高さをランダムに変えたもの）に対して
  生成し、`stands` の `extent` が常に `grid` の範囲内に収まること（NaN/Infが出力に混入しないこと）。

## 8. 実装タスク

Codexに渡す粒度（1タスク=1〜3時間）。依存関係を明示する。

**直列（土台、Wave 0相当）**

1. `crates/drill-core/src/stadium.rs` 新設: `StadiumModel` / `StandSection` / `PressBox` /
   `StadiumModel::generic` / `seat_eye_position`。単体テスト込み。（他タスクの前提）
2. `camera.rs` に `Camera::at_eye` / `pan` / `basis` / `roll` フィールド追加。既存テスト回帰確認。
   （3, 4, 5の前提）

**並行可能（タスク1・2完了後）**

3. `Camera::from_seat` / `Camera::end_zone` / `Camera::performer_pov`（タスク1・2に依存）。
4. `crates/drill-core/src/visibility.rs` 新設: `VisibilityScratch` / `visibility_from_seat`
   （タスク1と独立、`Point`/`PerformerId`のみに依存）。
5. `RowSpec` / `RowFlatnessReport` / `analyze_row_from_seat`（タスク4に依存、遮蔽判定を内部で使う）。
6. `LodThresholds` / `choose_lod` / `GroundShadow` / `Lighting` / `FieldLogo`（他タスクと独立）。
7. `Document.stadium: StadiumModel` フィールド追加とスキーマv2互換の `#[serde(default)]`
   （タスク1に依存、A-7のマイグレーション基盤と合流）。

**依存: 20/21の到着待ち（本文書の範囲外だが22側の受け皿）**

8. `drill-app` の `draw_stadium` を `drill-render::build()` 呼び出し + 投影済み `DisplayList`
   描画へ置き換え（20のDisplayList実装後。3.7節の要求を満たす形で）。
9. `PerformerInstance` レイアウトでのGPUインスタンス描画統合（21実装後）。
10. `CameraInertia`（慣性）を `drill-app/src/view/stadium3d.rs` に実装（C-1の分解が先行する場合は
    そのツリーに配置、していない場合は `main.rs` 内に一時的に置いて後で移動）。

**Job基盤待ち**

11. 「観客席解析レポート」ジョブ（`DESIGN_GAPS.md` B-3の `Job<T>` 実装後、50座席サンプルを
    バックグラウンドで解析しレポートを返す）。

## 9. 未決事項

- **11-transition-model.md 未執筆**: `facing` の正式な型・API（`FacingAt` トレイトの実体）が
  決まっていない。11執筆時にはゲート・カーブ経路から向きをどう導出するか（移動方向そのものか、
  ドリルデザイナーが明示指定するオーバーライドを許すか）を確認し、本文書の `FacingAt` 契約と
  整合させる必要がある。
- **15（演者・セクション）未執筆**: `Performer.height_m` のデフォルト値・単位・入力UI、
  `Section.color` のデフォルトパレットは15が決める。22はこれらが存在する前提でしか書けない。
- **20/21のスケジュール**: `DrawCmd::Mesh` / `DrawCmd::TexturedQuad` をどちらが先に実装するかで
  タスク8・9の着手順が変わる。20が先行する想定で書いたが、21が先行する場合はタスク9を
  タスク8より前に倒す必要がある。
- **画素しきい値の妥当性**: `crooked_px_threshold`（既定4.0px @ 1920x1080）・
  `telescoped_ratio_threshold`（既定2.5）は暫定値。実際のドリルデザイナーによる試用
  （どの程度の見た目のズレを「崩れている」と感じるか）でチューニングが必要。
  解像度非依存にするため、将来的にはpx基準を視野角基準（度）に変更する可能性がある。
- **観客席の曲率**: `curvature_sagitta_m` を持たせたが、実際のスタジアム形状としてどこまで
  曲げるべきかはデザイン判断であり未検証。直線グランドスタンドのみで発売可能かも含め検討事項。
- **ロゴ画像のセキュリティ上限**: 具体的な最大解像度・最大ファイルサイズ・許可フォーマットは
  51-securityが決定する。本文書はデコードをUIスレッド外で行うことのみを要求している。
- **`InstrumentSilhouette` の図形セット**: どのセクション種別にどのベクタ図形を割り当てるかは
  未定義。15の `Section`/`PerformerKind` 語彙が固まってから対応表を作る。
