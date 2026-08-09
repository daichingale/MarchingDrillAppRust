# 14. フォーメーション生成とシェイプツール

## 1. 目的と範囲

現在の `crates/drill-core/src/shapes.rs` は「点群を生成する純関数の集合」に留まっており、
一度セットへ焼き込んだ図形はただの `Vec<Point>` に戻ってしまう。半径を後から直すことも、
セクションのまとまりを保ったまま配置し直すこともできない。本書は Pyware 3D 上位互換に必要な
シェイプツール一式を、次の観点で再設計する。

- 図形を「編集可能なオブジェクト」としてドキュメントに残す（パラメータを保持し、再生成できる）。
- 直線・円弧・円・ブロック・螺旋・ベジェに加え、楕円・放物線・正弦波・星形・多角形・十字・
  自由描画パス・テキスト配置を追加する。
- カーブ上の配置を**弧長**基準の等間隔にする（現状のパラメータ均等は誤り）。
- 図形間モーフィング、対称性ツール、最小二乗フィッティング、セクション制約付き割り当て、
  優先順位付きスナップを設計する。

**本書が扱わないこと**

- `Edit` enum 自体の最終定義、`SetId` の導入（[doc 10](10-document-model.md) 相当。本書は
  `DESIGN_GAPS.md` A-1 で提案された形を前提として参照するのみで、確定はしない）。
- `Section` / `SectionId` の最終フィールド定義（doc 15。本書は不透明なキーとして使うのみ）。
- `DrillError` の全バリアント一覧と `Locale` 文言（doc 42。本書は必要なバリアントを提案するのみ）。
- 掃引衝突検査そのもの（doc 13）。図形適用後の衝突チェックは doc 13 の `scan_transition` を
  そのまま呼び出す想定で、ここでは呼び出し方を規定しない。
- ルート・ゲート・イージング（doc 11）。図形は「セットの座標」を作るだけで、セット間の移動軌跡
  には関与しない。
- フォントのアウトライン抽出そのもの（外部フォントライブラリの選定は doc 43/52）。本書は
  「抽出済みの輪郭点列を受け取る」境界までを設計する。

## 2. 現状

`crates/drill-core/src/shapes.rs`（全369行、テスト14件）にあるもの:

- `block` / `block_fit` / `circle` / `spiral` / `bezier` / `polyline`。すべて `Vec<Point>` を
  返す純関数で、`count == 0` は空、`count == 1` は妥当な1点を返す規約が統一されている
  （doc冒頭コメント 1-6行目）。
- `polyline`（140-188行目）は累積弧長テーブルを作り、目標弧長を線形探索で辿って補間している。
  **これは正しく弧長等間隔になっている**（テスト `polyline_spacing_is_uniform_by_arc_length`,
  342-357行目で検証済み）。

**バグ**: `bezier`（104-138行目）と `spiral`（80-102行目）は、doc コメントが
"Points sampled evenly in the parameter `t`" と自ら明記している通り、**弧長ではなくパラメータ
`t` を均等割りしている**。De Casteljau 補間や螺旋の半径増加により `t` に対する速度
`|d/dt curve(t)|` は一定でないため、曲率の高い区間・半径の大きい区間ほど点間隔が広がる。
`bezier_two_points_matches_straight_line`（293-302行目）が通るのは制御点が2つ（直線）の
特殊ケースだからで、3点以上の曲線では等間隔性は検証されていない。`spiral` も同様に
`spiral_endpoints_have_expected_radii`（276-284行目）は半径の妥当性しか見ておらず、間隔の
均一性は未検証。**これは要求4（弧長等間隔配置）で明示的に修正する。**

`crates/drill-core/src/lib.rs`:

- `evenly_spaced_line`（145-153行目）、`evenly_spaced_arc`（155-178行目）は既に弧長等間隔
  として正しい（直線・一定半径円弧はパラメータと弧長が線形関係にあるため、パラメータ均等が
  そのまま弧長均等になる）。
- `GridConfig::snap`（117-127行目）はグリッドスナップのみ。ヤードライン・ハッシュ・他演者・
  図形へのスナップは無い。`GridConfig` は `hashes: Vec<GridLine>` を持つが、ヤードライン
  自体（`major_line_interval` 間隔での `x` 座標列）を列挙するメソッドは無い。
- `Performer { id: PerformerId, label: String, color: [u8;3] }`（226-230行目）。`PerformerId`
  は既に `pub type PerformerId = u32;`（19行目）として定義済みで、doc 15 の `Section` 拡張
  後もこの型は変わらない想定（doc 15 は `Performer` にフィールドを足すだけ）。
- `Set { name, counts, positions: Vec<Point> }`（233-238行目）に図形のパラメータを保持する
  フィールドは無い。図形を適用すると `positions` が書き換わるだけで、元のパラメータは失われる。
- `Document::positions_at`（384-394行目）はセット間の単純線形補間のみ。図形モーフィングは
  この関数の外側（本書の対象）で完結させる必要がある。

`crates/drill-core/src/editing.rs`: `centroid` / `mirror` / `flip_*` / `rotate` /
`rotate_about_centroid` / `scale` / `distribute_horizontal` / `distribute_vertical` /
`align_horizontal` / `align_vertical` は全て `&[Point] -> Vec<Point>` の純関数（1-155行目）。
軸を任意直線に取れる汎用ミラー・座標系に依存しない放射対称は無い。本書はこれらの上に
「対応表（どの演者とどの演者がペアか）」の層を足す。

`crates/drill-core/src/pathing.rs`: `optimal_assignment(from: &[Point], to: &[Point]) ->
Vec<usize>`（126-175行目）は貪欲法シード＋2-opt改善。**計算量に注意点がある**: 貪欲シードは
`O(n²)`、2-opt ループは `for _ in 0..max_passes { for i in 0..n { for k in i+1..n {...} } }`
（154-172行目、`max_passes = 4*n+8`）で、1パスが `O(n²)`。改善が無くなり次第 `break` するが、
**最悪計算量の保証は `O(n² · (4n+8)) = O(n³)`**。`n=1000` では最悪 40億回規模の比較になり得る
（実際には早期 break で収まることが多いが、保証ではない）。要求7（割り当て）でこれを
安全に使うためのラッパーを設計する。`assignment_cost` / `nearest_neighbor_interval` /
`path_length` も既存（8-115行目）で、本書はそのまま再利用する。

`crates/drill-core/src/lib.rs` に `DrillError` は存在しない（`Document::validate` は
`Result<(), String>`、303-333行目）。`SetId` も存在しない（`Set` に識別子フィールドが無い）。
これらは `DESIGN_GAPS.md` A-1・A-6 で提案されており、本書はその提案形を前提として型シグネチャに
使う（存在しない場合は本書の型は暫定的に `Result<_, String>` に読み替えて実装してよいが、
Wave 0 完了後は `DrillError` に揃える）。

`crates/drill-core/benches/core_performance.rs` が既に存在する（ベンチハーネス
`cargo bench -p drill-core --bench core_performance`）。本書のベンチはここに追記する。

## 3. 設計

### 3.0 モジュール再編成

`shapes.rs` を `shapes/mod.rs` に変え、既存の6関数はそのまま `mod.rs` に残し（シグネチャ・
テスト・ドキュメントコメントは無変更）、新規機能をサブモジュールに分割する。

```
crates/drill-core/src/shapes/
  mod.rs        既存: block/block_fit/circle/spiral/bezier/polyline（無変更）
                追加: ShapeSpec, Shape トレイト, closed_polyline, sample_by_arc_length
  arc_length.rs build_arc_length_table / t_for_arc_length（純粋な数値計算）
  fit.rs        fit_line / fit_circle / snap_to_line / snap_to_circle
  symmetry.rs   MirrorAxis / RadialSymmetry / SymmetryPairing とその適用
  assign.rs     assign_to_shape（セクション制約付き割り当て）/ optimal_assignment_bounded
  morph.rs      plan_morph / morph_points / morph_intermediate_sets
  snap.rs       SnapKind / SnapConfig / snap_point / GridConfig::yard_lines への橋渡し
```

`pub use` で `crate::shapes::{ShapeSpec, Shape, ...}` を再輸出し、既存の
`crate::shapes::circle` 等の呼び出し元（`main.rs` 等）を壊さない。

### 3.1 弧長等間隔配置（数値解法）

すべての「連続曲線」系シェイプ（既存の `bezier` / `spiral` の修正版、新規の楕円・放物線・
正弦波）が共有する基盤。曲線をパラメータ `t ∈ [0,1]` の関数として評価できれば、弧長均等な
`count` 点を求められる。

```rust
// shapes/arc_length.rs

/// Cumulative arc-length table for a parametric curve, built by evaluating
/// `eval` at `table_len` uniform steps over `t in [0,1]` and summing the
/// straight-line (chord) distance between consecutive samples.
///
/// `table[0] == 0.0`; `table[table_len - 1]` is the total approximate length.
/// Error bound: for a curve with bounded curvature, the piecewise-linear
/// chord approximation underestimates the true arc length by
/// `O(curvature * (1/table_len)^2)` per segment, so doubling `table_len`
/// roughly quarters the length error. `table_len = 128` (the default) keeps
/// relative error under ~0.1% for the smooth curves this module defines
/// (ellipse, sine, parabola, low-degree bezier); sharp corners are handled
/// by `closed_polyline` instead (exact, no approximation).
pub fn build_arc_length_table(eval: impl Fn(f32) -> Point, table_len: usize) -> Vec<f32> {
    let table_len = table_len.max(2);
    let mut table = Vec::with_capacity(table_len);
    table.push(0.0f32);
    let mut prev = eval(0.0);
    let mut acc = 0.0f32;
    for i in 1..table_len {
        let t = i as f32 / (table_len - 1) as f32;
        let p = eval(t);
        let dx = p.x - prev.x;
        let dy = p.y - prev.y;
        acc += (dx * dx + dy * dy).sqrt();
        table.push(acc);
        prev = p;
    }
    table
}

/// Find `t` such that the arc length from `0` to `t` is approximately
/// `target`, via binary search over the monotonically non-decreasing
/// `table` followed by linear interpolation inside the bracketing segment.
///
/// `O(log table.len())`. Returns `0.0` if `table` is degenerate (total
/// length `<= 0` or non-finite, e.g. all sampled points coincide or the
/// curve has non-finite control data) — see §6 for why `sample` must
/// tolerate this rather than panicking.
pub fn t_for_arc_length(table: &[f32], target: f32) -> f32 {
    let total = *table.last().unwrap_or(&0.0);
    if !(total > 0.0) || !total.is_finite() {
        return 0.0;
    }
    let target = target.clamp(0.0, total);
    let (mut lo, mut hi) = (0usize, table.len() - 1);
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if table[mid] < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let seg_len = table[hi] - table[lo];
    let local = if seg_len > 0.0 {
        (target - table[lo]) / seg_len
    } else {
        0.0
    };
    (lo as f32 + local) / (table.len() - 1) as f32
}

/// Sample `count` points evenly by arc length along the curve `eval`.
/// Reuses `out`'s allocation (matches `Document::positions_at`'s convention).
/// `table_len` defaults to 128 via `DEFAULT_ARC_TABLE_LEN`; callers doing a
/// one-shot high-precision fit (e.g. export) may pass a larger value.
pub fn sample_by_arc_length(
    eval: impl Fn(f32) -> Point,
    count: usize,
    table_len: usize,
    out: &mut Vec<Point>,
) {
    out.clear();
    match count {
        0 => {}
        1 => out.push(eval(0.5)),
        _ => {
            let table = build_arc_length_table(&eval, table_len);
            let total = *table.last().unwrap_or(&0.0);
            out.reserve(count.saturating_sub(out.capacity()));
            for i in 0..count {
                let target = total * i as f32 / (count - 1) as f32;
                let t = t_for_arc_length(&table, target);
                out.push(eval(t));
            }
        }
    }
}

pub const DEFAULT_ARC_TABLE_LEN: usize = 128;
```

計算量: テーブル構築 `O(table_len)`、`count` 点それぞれの探索が `O(log table_len)` なので
`sample_by_arc_length` 全体は `O(table_len + count · log table_len)`。`count = 1000`,
`table_len = 128` で概算 `1000 × 7 ≈ 7,000` 回の比較＋評価。1回のシェイプ適用（ドラッグ中の
ライブプレビューを含む）で `eval` 自体が三角関数を数回呼ぶ程度なら、マイクロ秒オーダーで
16.6ms 予算に対して無視できる。

**既存 `bezier` / `spiral` の修正方針**（本書はコードを書かないが、実装タスクとして明記）:
両関数の外側の分岐（`count == 0/1` の早期リターン）はそのまま残し、`_ => (0..count).map(...)`
の本体だけを `sample_by_arc_length(|t| de_casteljau(t), count, DEFAULT_ARC_TABLE_LEN, &mut out)`
（bezier）、`sample_by_arc_length(|t| spiral_point(t), count, ...)`（spiral）に置き換える。
シグネチャは変えない（`Vec<Point>` を返す既存の関数のまま、内部で `Vec` を作って返す）ので、
既存テスト `bezier_preserves_endpoints` 等はそのまま通る。新規ゴールデンテストで等間隔性を
追加検証する（§7）。

**閉じた輪郭の弧長等間隔**（星形・多角形・十字で使用、`polyline` とは異なる規約）:

```rust
// shapes/mod.rs

/// Distribute `count` points evenly by arc length around a CLOSED loop
/// through `vertices`, including the implicit closing edge from the last
/// vertex back to the first. Unlike `polyline` (which pins both the first
/// AND last sample to the first/last vertex, dividing the path into
/// `count - 1` gaps), a closed loop divides its full perimeter into `count`
/// EQUAL gaps with no duplicated seam point — the same convention `circle`
/// already uses. `count == 0` is empty; `count == 1` returns `vertices[0]`.
pub fn closed_polyline(vertices: &[Point], count: usize) -> Vec<Point> {
    if vertices.is_empty() || count == 0 {
        return Vec::new();
    }
    if vertices.len() == 1 || count == 1 {
        return vec![vertices[0]; count.max(1)][..count].to_vec();
    }
    let mut cumulative = Vec::with_capacity(vertices.len() + 1);
    cumulative.push(0.0f32);
    let mut total = 0.0f32;
    for i in 0..vertices.len() {
        let a = vertices[i];
        let b = vertices[(i + 1) % vertices.len()];
        total += ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
        cumulative.push(total);
    }
    if total <= 0.0 || !total.is_finite() {
        return vec![vertices[0]; count];
    }
    let mut out = Vec::with_capacity(count);
    let mut seg = 0usize;
    for i in 0..count {
        let target = total * i as f32 / count as f32; // NOTE: divide by `count`, not `count-1`
        while seg + 1 < vertices.len() && cumulative[seg + 1] < target {
            seg += 1;
        }
        let seg_start = cumulative[seg];
        let seg_len = cumulative[seg + 1] - seg_start;
        let local = if seg_len > 0.0 { (target - seg_start) / seg_len } else { 0.0 };
        let a = vertices[seg];
        let b = vertices[(seg + 1) % vertices.len()];
        out.push(a.lerp(b, local));
    }
    out
}
```

### 3.2 `Shape` トレイトと `ShapeSpec`

要求1: 図形を「生成物」ではなく「パラメータを保持した編集可能なオブジェクト」にする。
`dyn Shape` トレイトオブジェクトは serde 実装が煩雑になり、`drill-core` の「毎フレーム確保
ゼロ」原則とも相性が悪い（vtable 越しの呼び出し自体は問題ないが、`Box<dyn Shape>` の
シリアライズはカスタム実装が要る）ので、**タグ付き enum + 同enumへのトレイト実装**を採る。
将来プラグイン等で外部図形を追加したくなった場合の `Box<dyn Shape>` 拡張は doc 52 の相互運用
API 側の課題として §9 に送る。

```rust
// shapes/mod.rs

use crate::{DrillError, Point};
use serde::{Deserialize, Serialize};

/// A parametric formation shape, stored on a `Set` so it remains editable
/// (e.g. "just change this circle's radius") rather than being baked into
/// `positions` and forgotten. See §3.4 for how this is persisted and
/// combined with `Edit`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ShapeSpec {
    Line { start: Point, end: Point },
    Arc { center: Point, radius: f32, start_angle: f32, end_angle: f32 },
    Circle { center: Point, radius: f32 },
    Ellipse { center: Point, radius_x: f32, radius_y: f32, rotation: f32 },
    Block { top_left: Point, cols: usize, rows: usize, dx: f32, dy: f32 },
    BlockFit { rect_min: Point, rect_max: Point, cols: usize, rows: usize },
    Spiral { center: Point, start_radius: f32, end_radius: f32, turns: f32 },
    Bezier { control_points: Vec<Point> },
    Parabola { vertex: Point, curvature: f32, half_width: f32, rotation: f32 },
    SineWave { start: Point, end: Point, amplitude: f32, cycles: f32, phase: f32 },
    Star { center: Point, outer_radius: f32, inner_radius: f32, points: u32, rotation: f32 },
    Polygon { center: Point, radius: f32, sides: u32, rotation: f32 },
    Cross { center: Point, arm_length: f32, arm_width: f32 },
    FreePath { vertices: Vec<Point> },
    /// Glyph outlines already extracted and positioned (field units) by the
    /// caller. `drill-core` never parses fonts (dependency allowlist is
    /// serde/serde_json only, per `docs/design/00-conventions.md`); shaping
    /// text into contours is `drill-app`'s job (or a future `drill-render`
    /// helper), and only the resulting point loops cross into core.
    Text { contours: Vec<Vec<Point>> },
}

pub trait Shape {
    /// Populate `out` with exactly `count` points (0 for `count == 0`),
    /// reusing `out`'s existing capacity. Never panics, even on
    /// out-of-range parameters (see §6) — callers that need to reject bad
    /// parameters call `validate` first.
    fn sample(&self, count: usize, out: &mut Vec<Point>);

    /// Approximate total arc length (perimeter for closed shapes), used by
    /// `morph` correspondence and `Text` apportionment. Exact for
    /// straight-edged shapes (Line/Block/Star/Polygon/Cross/FreePath),
    /// approximated via `build_arc_length_table` otherwise.
    fn arc_length(&self) -> f32;

    /// Reject non-finite or structurally invalid parameters. Called before
    /// a `ShapeSpec` is attached to a `Set` (see §3.4) and before it is
    /// accepted from an untrusted save file.
    fn validate(&self) -> Result<(), DrillError>;
}
```

`impl Shape for ShapeSpec` は各バリアントを match し、既存関数へ委譲する。代表例のみ示す
（全バリアント同型なので `sample` の実装タスクは §8 で1バリアント=1タスクに割れる）。

```rust
impl Shape for ShapeSpec {
    fn sample(&self, count: usize, out: &mut Vec<Point>) {
        match self {
            ShapeSpec::Line { start, end } => {
                out.clear();
                out.extend(crate::evenly_spaced_line(*start, *end, count));
            }
            ShapeSpec::Circle { center, radius } => {
                out.clear();
                out.extend(super::circle(*center, radius.max(0.0), count));
            }
            ShapeSpec::Bezier { control_points } => {
                out.clear();
                out.extend(super::bezier(control_points, count)); // now arc-length internally, §3.1
            }
            ShapeSpec::Ellipse { center, radius_x, radius_y, rotation } => {
                let (rx, ry) = (radius_x.max(0.0), radius_y.max(0.0));
                let eval = |t: f32| {
                    let angle = std::f32::consts::TAU * t;
                    let (lx, ly) = (rx * angle.cos(), ry * angle.sin());
                    let (sin, cos) = rotation.sin_cos();
                    Point {
                        x: center.x + lx * cos - ly * sin,
                        y: center.y + lx * sin + ly * cos,
                    }
                };
                sample_by_arc_length(eval, count, DEFAULT_ARC_TABLE_LEN, out);
            }
            ShapeSpec::SineWave { start, end, amplitude, cycles, phase } => {
                let dx = end.x - start.x;
                let dy = end.y - start.y;
                let len = (dx * dx + dy * dy).sqrt().max(f32::EPSILON);
                let (nx, ny) = (-dy / len, dx / len); // unit perpendicular
                let eval = |t: f32| {
                    let base = start.lerp(*end, t);
                    let offset = amplitude * (phase + std::f32::consts::TAU * cycles * t).sin();
                    Point { x: base.x + nx * offset, y: base.y + ny * offset }
                };
                sample_by_arc_length(eval, count, DEFAULT_ARC_TABLE_LEN, out);
            }
            ShapeSpec::Star { center, outer_radius, inner_radius, points, rotation } => {
                let verts = star_outline(*center, outer_radius.max(0.0), inner_radius.max(0.0), *points, *rotation);
                out.clear();
                out.extend(closed_polyline(&verts, count));
            }
            ShapeSpec::Cross { center, arm_length, arm_width } => {
                let verts = cross_outline(*center, arm_length.max(0.0), arm_width.max(0.0));
                out.clear();
                out.extend(closed_polyline(&verts, count));
            }
            ShapeSpec::Text { contours } => {
                out.clear();
                out.extend(sample_text_contours(contours, count));
            }
            // Arc/Block/BlockFit/Spiral/Parabola/Polygon/FreePath: same pattern,
            // delegating to `evenly_spaced_arc`, `block`, `block_fit`,
            // `spiral`, an eval closure via `sample_by_arc_length`, an
            // outline via `closed_polyline`, and `polyline` respectively.
            _ => unreachable!("remaining variants follow the patterns above; enumerated fully in the implementation task, not elided here for space"),
        }
    }

    fn arc_length(&self) -> f32 {
        match self {
            ShapeSpec::Line { start, end } => crate::pathing::path_length(*start, *end),
            ShapeSpec::Circle { radius, .. } => std::f32::consts::TAU * radius.max(0.0),
            ShapeSpec::Star { center, outer_radius, inner_radius, points, rotation } => {
                perimeter_of(&star_outline(*center, *outer_radius, *inner_radius, *points, *rotation))
            }
            // ... one arm per variant, exact for straight edges, table-based
            // (`build_arc_length_table`) for curves.
            _ => 0.0,
        }
    }

    fn validate(&self) -> Result<(), DrillError> {
        fn finite(p: Point) -> bool { p.x.is_finite() && p.y.is_finite() }
        match self {
            ShapeSpec::Circle { center, radius } | ShapeSpec::Arc { center, radius, .. } => {
                if !finite(*center) || !radius.is_finite() {
                    return Err(DrillError::InvalidShapeParameter { shape: "circle", detail: ShapeParamIssue::NonFiniteValue });
                }
                if *radius < 0.0 {
                    return Err(DrillError::InvalidShapeParameter { shape: "circle", detail: ShapeParamIssue::NegativeRadius });
                }
                Ok(())
            }
            ShapeSpec::Bezier { control_points } | ShapeSpec::FreePath { vertices: control_points } => {
                if control_points.iter().any(|p| !finite(*p)) {
                    return Err(DrillError::InvalidShapeParameter { shape: "bezier", detail: ShapeParamIssue::NonFiniteValue });
                }
                if control_points.len() > MAX_SHAPE_VERTICES {
                    return Err(DrillError::InvalidShapeParameter { shape: "bezier", detail: ShapeParamIssue::TooManyVertices { max: MAX_SHAPE_VERTICES } });
                }
                Ok(())
            }
            ShapeSpec::Block { cols, rows, .. } | ShapeSpec::BlockFit { cols, rows, .. } => {
                let total = cols.saturating_mul(*rows);
                if total > MAX_SHAPE_PERFORMERS {
                    return Err(DrillError::InvalidShapeParameter { shape: "block", detail: ShapeParamIssue::CountExceedsLimit { max: MAX_SHAPE_PERFORMERS } });
                }
                Ok(())
            }
            // ... remaining variants follow the same two checks: finite
            // geometry, and any count-like field bounded by
            // MAX_SHAPE_PERFORMERS / MAX_SHAPE_VERTICES.
            _ => Ok(()),
        }
    }
}

/// Upper bound matching `docs/design/00-conventions.md`'s stated degraded
/// ceiling (4,000 performers / 256 sets). A `Block`/`Star`/`Polygon` etc.
/// requesting more points than this is rejected by `validate` before any
/// `Vec::with_capacity` runs.
pub const MAX_SHAPE_PERFORMERS: usize = 4_000;
/// Upper bound on user-supplied control/free-path points, independent of
/// performer count (a `FreePath` can have far more vertices than
/// performers if the designer scribbles). Chosen generously above what a
/// simplified (see §3.3) hand-drawn path should ever need.
pub const MAX_SHAPE_VERTICES: usize = 1_024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShapeParamIssue {
    NonFiniteValue,
    NegativeRadius,
    TooFewControlPoints,
    TooManyVertices { max: usize },
    CountExceedsLimit { max: usize },
}
```

`DrillError::InvalidShapeParameter { shape: &'static str, detail: ShapeParamIssue }` は
doc 42 が定義する `DrillError` enum への追加提案（本書からの提案、最終決定は doc 42）。

**不正パラメータの扱いについての要点**（要求「必ず扱うこと」への回答）:

- **半径 0**: エラーにしない。`Circle{radius:0.0}` は全点が中心に重なる縮退フォーメーションで、
  ハンドルをドラッグして半径を0まで縮める操作は正当な中間状態（ライブプレビュー中に一時的に
  通過する）。`sample` はこれを問題なく処理する（`circle`/`evenly_spaced_arc` は `radius=0`
  で全点が中心に一致するだけで NaN にはならない）。
- **半径が負**: `validate()` で `DrillError` を返す。負の半径は見た目上の意味が曖昧
  （単に符号反転した点になるだけ)なので UI 側は適用前にブロックする。`sample()` 自身は
  `radius.max(0.0)` で防御し、`validate` を経由しない古い保存ファイルが読み込まれても
  パニックはしない。
- **個数が負**: 図形の頂点数・行列数（`cols`/`rows`/`sides`/`points`）は型が
  `usize`/`u32` なので、負の値は **serde の deserialize 段階で既に拒否される**
  （JSON の `-1` を `u32` へ変換しようとすると `serde_json` がエラーを返す。これは
  `drill-core` のコードを書く前に得られる保証で、追加の実行時チェックは不要）。
  実際に対処が要るのは逆方向のリスク、すなわち**巨大な値**（`u32::MAX` 等）で
  `Vec::with_capacity(cols * rows)` が暴走・オーバーフローすることの方であり、これは
  `validate()` の `MAX_SHAPE_PERFORMERS` / `MAX_SHAPE_VERTICES` 上限チェックで塞ぐ。
  `cols.saturating_mul(rows)` を使い、乗算オーバーフローでパニックしないようにする。
- **NaN 制御点**: `validate()` の `finite()` チェックで拒否するのが一次防御。二次防御として
  `sample_by_arc_length` / `closed_polyline` は `total`（累積弧長）が非有限になった場合に
  空ではなく `vertices[0]`（もしくは `eval(0.0)`）の複製を返すフォールバックを持つ
  （3.1節のコード参照）。これにより、`validate` を経由しない経路（例: 将来のマイグレーション
  コードのバグ）から NaN 制御点が紛れ込んでも、`sample` はパニックせず有限座標を返す。

### 3.3 追加図形の生成詳細

```rust
// shapes/mod.rs — outline helpers used by ShapeSpec::sample/arc_length above.

/// 12-vertex outline of a plus/cross shape, clockwise, centered at `center`.
/// `arm_width` is the FULL width of each arm (halved internally).
fn cross_outline(center: Point, arm_length: f32, arm_width: f32) -> Vec<Point> {
    let l = arm_length.max(0.0);
    let w = (arm_width * 0.5).max(0.0);
    [
        (w, -l), (w, -w), (l, -w), (l, w), (w, w), (w, l),
        (-w, l), (-w, w), (-l, w), (-l, -w), (-w, -w), (-w, -l),
    ]
    .into_iter()
    .map(|(x, y)| Point { x: center.x + x, y: center.y + y })
    .collect()
}

/// `2 * points` vertices alternating `outer_radius`/`inner_radius`, starting
/// at the top and rotated by `rotation` radians (same convention as
/// `circle`: angle 0 is straight up, increasing clockwise in field coords).
fn star_outline(center: Point, outer_radius: f32, inner_radius: f32, points: u32, rotation: f32) -> Vec<Point> {
    let n = (points.max(2) * 2) as usize;
    (0..n)
        .map(|i| {
            let r = if i % 2 == 0 { outer_radius } else { inner_radius };
            let angle = rotation - std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
            Point { x: center.x + r * angle.cos(), y: center.y + r * angle.sin() }
        })
        .collect()
}

/// Regular `sides`-gon outline (straight edges, unlike `circle`).
fn polygon_outline(center: Point, radius: f32, sides: u32, rotation: f32) -> Vec<Point> {
    let n = sides.max(3);
    (0..n)
        .map(|i| {
            let angle = rotation - std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * i as f32 / n as f32;
            Point { x: center.x + radius * angle.cos(), y: center.y + radius * angle.sin() }
        })
        .collect()
}

fn perimeter_of(vertices: &[Point]) -> f32 {
    if vertices.len() < 2 { return 0.0; }
    (0..vertices.len())
        .map(|i| crate::pathing::path_length(vertices[i], vertices[(i + 1) % vertices.len()]))
        .sum()
}

/// Distribute `count` performers across `contours` proportional to each
/// contour's arc length (a capital "M" gets more dots than a period),
/// using the largest-remainder method so the total is EXACTLY `count`
/// despite per-contour rounding, then `closed_polyline`-samples each
/// contour with its share.
fn sample_text_contours(contours: &[Vec<Point>], count: usize) -> Vec<Point> {
    if contours.is_empty() || count == 0 {
        return Vec::new();
    }
    let lengths: Vec<f32> = contours.iter().map(|c| perimeter_of(c)).collect();
    let total_len: f32 = lengths.iter().sum();
    if total_len <= 0.0 {
        return Vec::new();
    }
    let ideal: Vec<f32> = lengths.iter().map(|&l| l / total_len * count as f32).collect();
    let mut shares: Vec<usize> = ideal.iter().map(|&v| v.floor() as usize).collect();
    let mut remainder = count.saturating_sub(shares.iter().sum());
    let mut order: Vec<usize> = (0..ideal.len()).collect();
    order.sort_by(|&a, &b| (ideal[b].fract()).total_cmp(&ideal[a].fract()));
    for &i in order.iter().take(remainder) {
        shares[i] += 1;
    }
    remainder = 0; // consumed
    let _ = remainder;
    let mut out = Vec::with_capacity(count);
    for (contour, &share) in contours.iter().zip(&shares) {
        out.extend(closed_polyline(contour, share));
    }
    out
}
```

**楕円・放物線・正弦波**は `ShapeSpec::sample` 内の `eval` クロージャで既に示した通り
（3.2節）、`sample_by_arc_length` に委譲する。`Parabola` は局所座標 `x ∈
[-half_width, half_width]`、`y = curvature · x²` を `rotation` で回転し `vertex` へ平行移動する
評価関数を使う。`Polygon` は `polygon_outline` + `closed_polyline`。

**自由描画パス（FreePath）**: UI からの生入力はマウス移動イベントの束（数百〜数千点、密に
サンプリングされている）なので、そのまま `control_points`/`vertices` として保存すると
`MAX_SHAPE_VERTICES` をすぐ超え、`polyline`/`closed_polyline` の弧長テーブル構築コストも
無駄に膨らむ。**保存前に Douglas-Peucker 法で間引く**（`drill-app` 側で実施、
`drill-core` には簡略化後の頂点列だけが渡る想定。間引きアルゴリズム自体は UI 入力処理なので
本書の範囲外だが、許容誤差 `epsilon` は `GridConfig` のステップ幅程度を既定値にする、という
指針だけ明記する）。

### 3.4 シェイプの永続化（編集可能なオブジェクトとして）

要求1の核心: 図形を適用した後も「あれは半径15ydの円だった」という情報をドキュメントに残す。
`Set` にオプショナルなフィールドを1つ追加する提案（**doc 10 の `Edit`/`Set` 定義との調整が
必要、§9 参照**）。

```rust
// lib.rs (proposed addition to `Set`, coordinated with doc 10)

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShapeAssignment {
    pub spec: shapes::ShapeSpec,
    /// The order in which shape-sampled points were handed out to
    /// performers, by index into `Document::performers` (today's index
    /// alignment) or `PerformerId` (once doc 10's stable IDs land). This
    /// is what makes re-editing possible: change `spec.radius`, re-run
    /// `sample` with the SAME `order`, and every performer's new position
    /// follows without re-deciding who goes where.
    pub order: Vec<PerformerId>,
}

pub struct Set {
    pub name: String,
    pub counts: u16,
    pub positions: Vec<Point>,
    /// `None` for freehand-edited or imported sets that were never
    /// generated from a shape. `Some` keeps the shape re-editable.
    #[serde(default)]
    pub shape: Option<ShapeAssignment>,
}
```

**再編集フロー**: ユーザーが「この円の半径を15ydから18ydに変える」を実行すると:

1. `set.shape` から `ShapeAssignment { spec, order }` を読む。
2. `spec` を新パラメータ（`radius: 18.0`）に差し替えた `new_spec` を作る。
3. `new_spec.sample(order.len(), &mut scratch)` で新しい点列を得る。
4. `order[i]` 番目の演者に `scratch[i]` を割り当てる（対応関係は3.6節「割り当て」で
   生成時に決めたものをそのまま再利用するので、再計算コストは3ではなく順序を保つだけ）。
5. `before = 変更前の positions のコピー`、`after = 変更後の positions` として単一の
   `Edit`（doc 10 提案の `Edit::MovePoints` あるいは本書提案の `Edit::ApplyShape`、下記）で
   `History` に積む。

```rust
// Proposed addition to doc 10's `Edit` enum (coordination required, §9).
// If doc 10 prefers not to add a shape-specific variant, this can be
// expressed as `Edit::Batch(vec![
//     Edit::MovePoints { set, performers: order, before, after },
// ])` plus a separate, non-undoable side-channel that updates `set.shape`
// — but that split risks the "before/after 二重管理" problem
// `DESIGN_GAPS.md` A-1 explicitly warns against, so a dedicated variant
// that carries both the position change AND the shape-spec change in one
// reversible unit is preferred:
pub enum EditAddition {
    ApplyShape {
        set: SetId,
        before_shape: Option<ShapeAssignment>,
        after_shape: ShapeAssignment,
        before_positions: Vec<Point>,
        after_positions: Vec<Point>,
    },
}
```

これで「1,000人を1つの図形へ配置する」操作は**常に1件の `Edit`** になる（要求
「Undo への載せ方」への回答）。`apply` はやはり逆操作（`before_*` を使った
`ApplyShape` 自身、あるいは専用の逆variant）を返す。

### 3.5 シェイプモーフィング

**同一の割り当て順序を使っていれば、対応関係はインデックスだけで正しい**、というのが本節の
鍵になる観察である。3.4節の `ShapeAssignment.order` を経由して生成された2つのセット
（例: セットAが円、セットBが同じ `order` で生成した楕円）は、`order[i]` が両方のセットで
同じ演者を指すので、`positions[i] ↔ positions[i]` の対応がそのまま最短距離に近い自然な
動きになる。**無関係な2セット**（片方が手編集、あるいは別の `order` で生成された）を
モーフィングする場合のみ、`pathing::optimal_assignment` で対応を作り直す。

```rust
// shapes/morph.rs

use crate::{pathing, Point};

pub struct MorphPlan {
    /// `correspondence[i]` is the index into `to` that `from[i]` morphs
    /// toward. Always a permutation of `0..to.len()` when `from.len() ==
    /// to.len()`.
    pub correspondence: Vec<usize>,
}

/// Build a morph plan. Pass `same_order = true` when both point sets were
/// produced from `ShapeAssignment`s sharing the same `order` (§3.4) — the
/// cheap, correct case. Otherwise this falls back to
/// `pathing::optimal_assignment`, which is `O(n²)` per pass with a
/// `4n+8`-pass cap (see §3.6 for why that is bounded for large `n`).
pub fn plan_morph(from: &[Point], to: &[Point], same_order: bool) -> MorphPlan {
    let correspondence = if same_order && from.len() == to.len() {
        (0..from.len()).collect()
    } else {
        super::assign::optimal_assignment_bounded(from, to, super::assign::MAX_TWO_OPT_PASSES)
    };
    MorphPlan { correspondence }
}

/// Interpolate `from` toward `to` at parameter `t in [0,1]` following `plan`.
pub fn morph_points(from: &[Point], to: &[Point], plan: &MorphPlan, t: f32) -> Vec<Point> {
    from.iter()
        .enumerate()
        .map(|(i, &p)| {
            let target = plan.correspondence.get(i).and_then(|&j| to.get(j).copied()).unwrap_or(p);
            p.lerp(target, t.clamp(0.0, 1.0))
        })
        .collect()
}

/// Generate `steps` intermediate point sets strictly between `from` (t=0,
/// exclusive) and `to` (t=1, exclusive), evenly spaced in `t`. Intended to
/// become newly-inserted `Set`s (§ "セット間の自動中割り") via
/// `Edit::Batch(vec![Edit::InsertSet { .. }; steps])` — one `Edit` for the
/// whole batch, so it undoes as a single step.
pub fn morph_intermediate_sets(from: &[Point], to: &[Point], plan: &MorphPlan, steps: usize) -> Vec<Vec<Point>> {
    (1..=steps)
        .map(|s| morph_points(from, to, plan, s as f32 / (steps + 1) as f32))
        .collect()
}
```

カウント配分（挿入した中割りセットに何カウントずつ割り振るか）は `SetCounts`
（`DESIGN_GAPS.md` A-2）の管轄であり、本書は「区間を `steps+1` 等分するのが既定」とだけ
提案し、最終的な UI（例えば「不等分にしたい」）は doc 17（カウントシート・
コンティニュイティ）に委ねる。

### 3.6 対称性ツール

既存の `editing::mirror` / `flip_vertical_axis` / `rotate` は「点の配列を変換する」層。
本節はその上に「どの演者とどの演者がペアか」という**対応表**の層を足す。対応表が要るのは、
セットの演者配列は `Document::performers` に対してインデックス整列しているため、片側を
編集したときに「反対側の**どの**演者を動かすか」を明示的に決めておかないと、奇数人数や
軸上の演者を正しく扱えないからである。

```rust
// shapes/symmetry.rs

use crate::{editing, PerformerId, Point};
use std::collections::BTreeMap;

/// A reflection axis defined by a point on the axis and a unit direction
/// vector along it (not perpendicular to it — matches "a line through
/// `point` running in direction `direction`").
#[derive(Clone, Copy, Debug)]
pub struct MirrorAxis { pub point: Point, pub direction: Point }

impl MirrorAxis {
    pub fn vertical(x: f32) -> Self { Self { point: Point { x, y: 0.0 }, direction: Point { x: 0.0, y: 1.0 } } }
    pub fn horizontal(y: f32) -> Self { Self { point: Point { x: 0.0, y }, direction: Point { x: 1.0, y: 0.0 } } }

    /// Reflect `p` across this axis.
    pub fn reflect(&self, p: Point) -> Point {
        let vx = p.x - self.point.x;
        let vy = p.y - self.point.y;
        let dot = vx * self.direction.x + vy * self.direction.y;
        let proj = Point { x: self.direction.x * dot, y: self.direction.y * dot };
        let perp = Point { x: vx - proj.x, y: vy - proj.y };
        Point { x: self.point.x + proj.x - perp.x, y: self.point.y + proj.y - perp.y }
    }

    /// Orthogonal projection of `p` onto the axis (used to re-snap
    /// on-axis performers who drifted off it during a drag).
    pub fn project(&self, p: Point) -> Point {
        let vx = p.x - self.point.x;
        let vy = p.y - self.point.y;
        let dot = vx * self.direction.x + vy * self.direction.y;
        Point { x: self.point.x + self.direction.x * dot, y: self.point.y + self.direction.y * dot }
    }
}

/// Which performer mirrors which. Built once (e.g. when the designer
/// enables symmetry mode for a set) and reused every time the master side
/// is edited.
pub struct SymmetryPairing {
    pub pairs: Vec<(PerformerId, PerformerId)>, // (master, mirrored)
    pub on_axis: Vec<PerformerId>,               // odd-count performers that stay on the axis
}

/// Apply the pairing: every `mirrored` performer's position becomes the
/// reflection of its `master`'s CURRENT position in `positions`;
/// `on_axis` performers are projected back onto the axis (tolerates small
/// drift from a drag that wasn't perfectly on-axis).
pub fn apply_mirror(axis: &MirrorAxis, pairing: &SymmetryPairing, positions: &mut BTreeMap<PerformerId, Point>) {
    for &(master, mirrored) in &pairing.pairs {
        if let Some(&p) = positions.get(&master) {
            positions.insert(mirrored, axis.reflect(p));
        }
    }
    for &id in &pairing.on_axis {
        if let Some(&p) = positions.get(&id) {
            positions.insert(id, axis.project(p));
        }
    }
}

/// N-fold rotational symmetry about `center`.
pub struct RadialSymmetry { pub center: Point, pub fold: u32 }

/// One full ring: `members[0]` is the master; `members[k]` sits at
/// `members[0]`'s position rotated by `k * 360 / fold` degrees.
pub fn apply_radial(sym: &RadialSymmetry, groups: &[Vec<PerformerId>], positions: &mut BTreeMap<PerformerId, Point>) {
    let fold = sym.fold.max(2);
    for group in groups {
        let Some(&master) = group.first() else { continue };
        let Some(&master_pos) = positions.get(&master) else { continue };
        for (k, &id) in group.iter().enumerate() {
            let angle = std::f32::consts::TAU * k as f32 / fold as f32;
            let rotated = editing::rotate(std::slice::from_ref(&master_pos), angle, sym.center)[0];
            positions.insert(id, rotated);
        }
    }
}
```

`apply_mirror`/`apply_radial` は `BTreeMap<PerformerId, Point>` を書き換えるだけの
純粋関数で、`Document` への反映（`Edit` へ変換して `History` に積む）は呼び出し側
（drill-app のドラッグ確定時ハンドラ）が行う。ドラッグ中の**ライブプレビュー**では
毎フレーム呼ぶ必要があるため、`BTreeMap` の確保コストが気になる場合は呼び出し側が
`&mut BTreeMap` を使い回す（`clear()` して再利用）ことを推奨する、と明記する。

### 3.7 フィッティング（最小二乗）

「散らばった演者集合に最も近い直線・円を当てはめる」。ドリルの整形（clinic）ツールとして、
外部の線形代数クレートを増やさず閉形式で解く。

**直線フィット**: 単純最小二乗（`y = ax+b`）は垂直に近い列で破綻するため、**全最小二乗
（直交回帰）**を使う。共分散行列の最大固有値に対応する固有ベクトルが最適直線の方向になる。

```rust
// shapes/fit.rs
use crate::{editing::centroid, Point};

pub struct LineFit { pub point: Point, pub direction: Point } // direction is a unit vector

pub fn fit_line(points: &[Point]) -> Option<LineFit> {
    if points.len() < 2 { return None; }
    let c = centroid(points);
    let (mut sxx, mut sxy, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for p in points {
        let (dx, dy) = (p.x - c.x, p.y - c.y);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    let trace = sxx + syy;
    let disc = ((sxx - syy).powi(2) + 4.0 * sxy * sxy).sqrt();
    let lambda1 = (trace + disc) * 0.5; // larger eigenvalue = direction of max variance
    if lambda1 < 1e-8 {
        return None; // all points coincide: no well-defined direction
    }
    let (dx, dy) = if sxy.abs() > 1e-12 {
        (sxy, lambda1 - sxx) // eigenvector of [[sxx,sxy],[sxy,syy]] for eigenvalue lambda1
    } else if sxx >= syy {
        (1.0, 0.0)
    } else {
        (0.0, 1.0)
    };
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-12 { return None; }
    Some(LineFit { point: c, direction: Point { x: dx / len, y: dy / len } })
}

pub fn snap_to_line(points: &[Point], fit: &LineFit) -> Vec<Point> {
    points.iter().map(|p| {
        let (dx, dy) = (p.x - fit.point.x, p.y - fit.point.y);
        let t = dx * fit.direction.x + dy * fit.direction.y;
        Point { x: fit.point.x + fit.direction.x * t, y: fit.point.y + fit.direction.y * t }
    }).collect()
}
```

**円フィット**: Kåsa の代数的最小二乗法（`x²+y²+Dx+Ey+F=0` の線形最小二乗）。3x3 の
正規方程式を Cramer の公式で解く（反復法・外部クレート不要）。

```rust
fn det3(m: [[f32; 3]; 3]) -> f32 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

fn solve3(m: [[f32; 3]; 3], b: [f32; 3]) -> Option<[f32; 3]> {
    let d = det3(m);
    if d.abs() < 1e-6 { return None; } // singular: collinear input points
    let mut result = [0.0f32; 3];
    for col in 0..3 {
        let mut mc = m;
        for row in 0..3 { mc[row][col] = b[row]; }
        result[col] = det3(mc) / d;
    }
    Some(result)
}

pub struct CircleFit { pub center: Point, pub radius: f32 }

pub fn fit_circle(points: &[Point]) -> Option<CircleFit> {
    if points.len() < 3 { return None; }
    let n = points.len() as f32;
    let (mut sx, mut sy, mut sxx, mut syy, mut sxy, mut sxz, mut syz, mut sz) =
        (0.0f32, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for p in points {
        let z = p.x * p.x + p.y * p.y;
        sx += p.x; sy += p.y; sxx += p.x * p.x; syy += p.y * p.y; sxy += p.x * p.y;
        sxz += p.x * z; syz += p.y * z; sz += z;
    }
    let m = [[sxx, sxy, sx], [sxy, syy, sy], [sx, sy, n]];
    let [d, e, f] = solve3(m, [-sxz, -syz, -sz])?;
    let center = Point { x: -d * 0.5, y: -e * 0.5 };
    let r2 = center.x * center.x + center.y * center.y - f;
    if !r2.is_finite() || r2 <= 0.0 { return None; } // degenerate/collinear input
    Some(CircleFit { center, radius: r2.sqrt() })
}

pub fn snap_to_circle(points: &[Point], fit: &CircleFit) -> Vec<Point> {
    points.iter().map(|p| {
        let (dx, dy) = (p.x - fit.center.x, p.y - fit.center.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-6 {
            Point { x: fit.center.x + fit.radius, y: fit.center.y } // coincident with center: arbitrary angle
        } else {
            Point { x: fit.center.x + dx / len * fit.radius, y: fit.center.y + dy / len * fit.radius }
        }
    }).collect()
}
```

両方とも `O(n)`（`fit_circle`/`fit_line`）＋定数（3x3 solve）。`snap_to_line`/
`snap_to_circle` は各演者の**角度・射影位置**（＝相対順序）を保ったまま最寄りの理想位置へ
移動するだけで、間隔の再等分はしない。等間隔にも直したい場合は `closed_polyline`（円）や
`editing::distribute_horizontal` 相当（直線）を後段に重ねる、という2段構成にする。

### 3.8 割り当て（`optimal_assignment` との連携、セクション制約）

`pathing::optimal_assignment` をそのまま1,000人規模へ使うと、2章で指摘した通り最悪
`O(n³)` になり得る。まず**パス数の上限を `n` に依存させないラッパー**を用意する。

```rust
// shapes/assign.rs
use crate::{pathing, PerformerId, Point};

/// Passes above this are diminishing-returns territory for formation
/// assignment (unlike TSP, we don't need a global optimum — a "good
/// enough, doesn't visibly cross" ordering is the actual requirement).
pub const MAX_TWO_OPT_PASSES: usize = 64;

/// Same guarantee as `pathing::optimal_assignment` (returns a permutation,
/// falls back to identity on length mismatch) but with the 2-opt pass
/// count capped at a CONSTANT regardless of `n`, bounding worst-case cost
/// at `O(n² · max_passes)` instead of `O(n³)`. For `n <= ~120` this is
/// close to running the uncapped version to convergence; for `n` in the
/// hundreds it trades a small amount of assignment quality for a hard
/// latency ceiling.
pub fn optimal_assignment_bounded(from: &[Point], to: &[Point], max_passes: usize) -> Vec<usize> {
    // Reimplements pathing::optimal_assignment's greedy seed + 2-opt loop
    // verbatim, replacing `4 * n + 8` with `max_passes.min(4 * n + 8)`.
    // (Not re-deriving the greedy/2-opt logic here — see pathing.rs
    // 126-175 for the loop this wraps; the only change is the pass cap.)
    pathing::optimal_assignment_with_pass_cap(from, to, max_passes) // proposed addition to pathing.rs
}
```

（`optimal_assignment_with_pass_cap` は `pathing.rs` への小さな追加として提案する
— 既存の `optimal_assignment` は `optimal_assignment_with_pass_cap(from, to, 4*n+8)`
の薄いラッパーに書き換えられ、既存テストは無変更で通る。）

**セクション制約付き割り当て**: 「セクションのまとまりを保つ」とは、シェイプ上の連続した
弧長区間に、1つのセクションの演者をまとめて配置することを意味する。

```rust
pub struct AssignmentGroup {
    pub section: crate::SectionId, // opaque key from doc 15; not interpreted here
    pub performers: Vec<PerformerId>,
}

/// Assign `groups` onto `shape_points` (already in the shape's natural
/// travel order, e.g. left-to-right along a line or clockwise around a
/// circle — i.e. `ShapeSpec::sample`'s output order).
///
/// Algorithm:
/// 1. Partition `shape_points` into contiguous runs, one per group, sized
///    to `group.performers.len()`, preserving shape order (run boundaries
///    are prefix sums of group sizes).
/// 2. Decide which group gets which run via a greedy nearest-centroid
///    match (compare each group's current centroid to each run's
///    midpoint) followed by pairwise-swap improvement — same 2-opt shape
///    as `optimal_assignment` but over GROUPS (typically <= 20 sections,
///    so `O(g²)` per pass is cheap regardless of performer count).
/// 3. Within each assigned run, call `optimal_assignment_bounded` between
///    that group's current positions and its run's points, so individual
///    performers don't cross within their own section.
///
/// Returns `(assignment, unplaced)`: `assignment` maps every placed
/// `PerformerId` to a `Point`; `unplaced` lists performers left over when
/// `sum(group sizes) > shape_points.len()` (see §6).
pub fn assign_to_shape(
    groups: &[AssignmentGroup],
    shape_points: &[Point],
    current_positions: &std::collections::BTreeMap<PerformerId, Point>,
) -> (std::collections::BTreeMap<PerformerId, Point>, Vec<PerformerId>) {
    use crate::editing::centroid;
    use std::collections::BTreeMap;

    let mut out = BTreeMap::new();
    let mut unplaced = Vec::new();

    // Step 1: contiguous runs by prefix sum, clipped to available points.
    let mut runs: Vec<&[Point]> = Vec::with_capacity(groups.len());
    let mut cursor = 0usize;
    for group in groups {
        let want = group.performers.len();
        let available = shape_points.len().saturating_sub(cursor);
        let take = want.min(available);
        runs.push(&shape_points[cursor..cursor + take]);
        cursor += take;
        if take < want {
            unplaced.extend(group.performers[take..].iter().copied());
        }
    }

    // Step 2: greedy nearest-centroid group-to-run matching, then bounded
    // 2-opt swap refinement (group count is small, so this is cheap).
    let group_centroids: Vec<Point> = groups
        .iter()
        .map(|g| centroid(&g.performers.iter().filter_map(|id| current_positions.get(id).copied()).collect::<Vec<_>>()))
        .collect();
    let run_midpoints: Vec<Point> = runs.iter().map(|r| centroid(r)).collect();
    let mut run_for_group = optimal_assignment_bounded(&group_centroids, &run_midpoints, MAX_TWO_OPT_PASSES);
    // (falls back to identity if lengths mismatch, per optimal_assignment's contract)
    if run_for_group.len() != groups.len() {
        run_for_group = (0..groups.len()).collect();
    }

    // Step 3: within-run assignment per group.
    for (gi, group) in groups.iter().enumerate() {
        let run = runs[run_for_group[gi]];
        let placed: Vec<PerformerId> = group.performers.iter().take(run.len()).copied().collect();
        let current: Vec<Point> = placed.iter().map(|id| current_positions.get(id).copied().unwrap_or_default()).collect();
        let local_assignment = optimal_assignment_bounded(&current, run, MAX_TWO_OPT_PASSES);
        for (i, &id) in placed.iter().enumerate() {
            if let Some(&target_idx) = local_assignment.get(i) {
                if let Some(&p) = run.get(target_idx) {
                    out.insert(id, p);
                }
            }
        }
    }

    (out, unplaced)
}
```

### 3.9 スナップ

`drill-core` は画面ピクセルやズーム倍率を知らない（境界の原則）。したがって「スナップ半径」は
常に**フィールド単位（yd/m）**で渡される値とし、「固定ピクセル半径をズーム倍率で割る」変換は
`drill-app` 側の責務とする。

```rust
// shapes/snap.rs

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SnapKind { Grid, YardLine, Hash, Performer(crate::PerformerId), Shape }

pub struct SnapResult { pub point: Point, pub kind: SnapKind, pub distance: f32 }

pub struct SnapConfig {
    pub grid: bool,
    pub yard_lines: bool,
    pub hashes: bool,
    pub performers: bool,
    pub shapes: bool,
    /// Field-unit radius. The caller derives this from a fixed on-screen
    /// pixel radius (e.g. 8px) divided by the current pixels-per-yard
    /// zoom factor — `drill-core` never sees pixels.
    pub radius: f32,
}

/// Snap `query` to the nearest applicable candidate within `config.radius`,
/// in PRIORITY ORDER: Performer > Shape > Hash > YardLine > Grid.
/// Rationale: performer/shape snapping expresses explicit relational
/// intent ("line up with that dot" / "stay on the curve I just drew"),
/// which should win over the always-available grid fallback even if the
/// grid point happens to be marginally closer. Returns the original
/// `query` wrapped as `SnapKind::Grid` with `distance: 0.0` if `config`
/// disables everything or nothing is within radius and grid snapping is
/// off (i.e. `Point::default()`-free — never fabricates a distance).
pub fn snap_point(
    query: Point,
    grid: &crate::GridConfig,
    yard_lines: &[f32],  // GridConfig::yard_lines(), see below
    hashes: &[f32],
    other_performers: &[(crate::PerformerId, Point)],
    shape: Option<&super::ShapeSpec>,
    config: &SnapConfig,
) -> SnapResult {
    let mut best: Option<SnapResult> = None;
    let mut consider = |candidate: Point, kind: SnapKind| {
        let d = crate::pathing::path_length(query, candidate);
        if d <= config.radius {
            let priority = |k: &SnapKind| match k {
                SnapKind::Performer(_) => 0,
                SnapKind::Shape => 1,
                SnapKind::Hash => 2,
                SnapKind::YardLine => 3,
                SnapKind::Grid => 4,
            };
            let better = match &best {
                None => true,
                Some(b) => priority(&kind) < priority(&b.kind)
                    || (priority(&kind) == priority(&b.kind) && d < b.distance),
            };
            if better {
                best = Some(SnapResult { point: candidate, kind, distance: d });
            }
        }
    };

    if config.performers {
        for &(id, p) in other_performers {
            consider(p, SnapKind::Performer(id));
        }
    }
    if config.shapes {
        if let Some(spec) = shape {
            // Snap onto the shape's outline: sample densely and take the
            // nearest sample. Adequate for a UI affordance (not a
            // geometry-exact projection); density chosen so the worst-case
            // gap between samples is well under typical snap radii.
            let mut samples = Vec::new();
            spec.sample(256, &mut samples);
            if let Some(&nearest) = samples.iter().min_by(|a, b| {
                crate::pathing::path_length(query, **a).total_cmp(&crate::pathing::path_length(query, **b))
            }) {
                consider(nearest, SnapKind::Shape);
            }
        }
    }
    if config.hashes {
        for &h in hashes {
            consider(Point { x: query.x, y: h }, SnapKind::Hash);
        }
    }
    if config.yard_lines {
        for &x in yard_lines {
            consider(Point { x, y: query.y }, SnapKind::YardLine);
        }
    }
    if config.grid {
        consider(grid.snap(query), SnapKind::Grid);
    }

    best.unwrap_or(SnapResult { point: query, kind: SnapKind::Grid, distance: 0.0 })
}
```

`GridConfig` に不足しているヤードライン列挙を追加する（`lib.rs` への小さな追加提案）:

```rust
impl GridConfig {
    /// `x` positions of every yard line, `0..=width` stepped by
    /// `major_line_interval`. Pure function of existing fields — no new
    /// state needed.
    pub fn yard_lines(&self) -> Vec<f32> {
        if self.major_line_interval <= 0.0 { return Vec::new(); }
        let count = (self.width / self.major_line_interval).floor() as usize + 1;
        (0..=count).map(|i| i as f32 * self.major_line_interval).filter(|&x| x <= self.width).collect()
    }
}
```

`hashes` は既に `GridConfig.hashes: Vec<GridLine>`（`position: f32`）に入っているので、
呼び出し側は `grid.hashes.iter().map(|h| h.position).collect()` を渡すだけでよい。

## 4. 不変条件

テストで検証可能な形で記す。

1. 任意の `ShapeSpec` で `count == 0` の `sample` は空の `out` を返す。`count == 1` は
   既存関数群（`evenly_spaced_line` 等）と同じ「妥当な1点」規約に従う。
2. `sample` は呼び出し前に `out` が持っていたヒープ確保を再利用する
   （`interpolation_reuses_output_allocation` と同型のポインタ比較テストで検証、
   `count <= out.capacity()` の場合）。
3. `closed_polyline(vertices, count).len() == count` であり、`count >= 2` のとき最初と
   最後の点は**一致しない**（`polyline` との違い）。
4. 弧長系シェイプ（Bezier/Spiral/Ellipse/Parabola/SineWave）の `sample` が返す隣接点間の
   ユークリッド距離の標準偏差は、同じ形状をパラメータ均等でサンプリングした場合より
   有意に小さい（§7 で閾値を規定するゴールデンテスト）。
5. `fit_line`/`fit_circle` は縮退データ（全点一致・共線）に対して必ず `None` を返し、
   パニックしない。
6. `assign_to_shape` が返す `(assignment, unplaced)` について、`assignment` のキー集合と
   `unplaced` の和集合は入力 `groups` の全 `PerformerId` の集合と一致し（過不足なし）、
   重複しない（全単射 + 余剰の二分割）。
7. `optimal_assignment_bounded(from, to, k)` は呼び出しごとに高々 `O(n² · k)` の比較で
   終了する（`k` は `n` に依存しない定数）。
8. `apply_mirror` は `on_axis` に列挙された演者を必ず軸上（`axis.project(p) == p` を
   満たす点）に置く。`apply_radial` はどの `group` についても、`k` 番目の演者と中心の
   距離が `master` と中心の距離に等しい（回転は距離を変えない）。
9. `validate()` が `Err` を返す `ShapeSpec` は `Set.shape` に保存されない
   （doc 10 の `Edit::apply` 側のゲートと協調、本書はチェックの中身のみ保証する）。
10. `sample` はどの `ShapeSpec`（NaN/負値/巨大値を含む）に対してもパニックしない。
    有限座標を返すか、3.2節のフォールバック規則に従って中心/原点相当へ縮退する。

## 5. 性能

図形適用はフレームループの外（離散的な編集操作）で発生するため、16.6ms のフレーム予算を
直接消費しないが、UI スレッドを目に見えて止めないことは要求される。ドラッグ中のライブ
プレビュー（ハンドルを動かすたびに再サンプルする）は実質「毎フレーム」相当になるため、
そちらは 16.6ms 予算の一部を使う。

基準規模（演者1,000人 / セット64）でのコスト概算:

| 操作 | 計算量 | 1,000人での概算 | 予算内訳 |
|---|---|---|---|
| `ShapeSpec::sample` (弧長系) | `O(table_len + n log table_len)` | 数千回の比較・三角関数評価、<0.5ms | ライブプレビュー中は16.6msの一部（他描画と共有） |
| `ShapeSpec::sample` (閉ループ/直線系) | `O(n)` | <0.1ms | 同上 |
| `fit_line` / `fit_circle` | `O(n)` | <0.1ms | 単発操作、予算外で許容 |
| `snap_to_line` / `snap_to_circle` | `O(n)` | <0.1ms | 同上 |
| `assign_to_shape`（グループ内 `optimal_assignment_bounded`） | `O(Σ kᵢ² · min(64, 4kᵢ+8))` | セクション10個×平均100人: 約4,000万回の比較 | 単発の「シェイプ適用」操作。目安10〜30ms |
| `assign_to_shape`（グループ間マッチング） | `O(g² · 64)`（`g`=セクション数、通常≤20） | 無視できる | 同上 |
| `morph_points` 1ステップ | `O(n)` | <0.1ms | 中割りセット1件あたり |
| `snap_point`（`shapes` 候補込み） | `O(other_performers + 256)`（shape候補は固定256点サンプル） | 1,000人なら約1,000回の距離計算、<0.2ms | ドラッグ中毎フレーム。予算内 |

**1,000人を1つの図形へ配置する操作のコストと Undo への載せ方**（要求への直接回答）:

- 計算コスト本体（`assign_to_shape`）は上表の通り最大約10〜30msで、これは**単発の編集操作**
  （ボタン押下やドラッグ終了時の確定）であり、フレーム予算の対象ではない。それでも
  UI スレッドを30ms止めるのは「常用操作の応答性」の観点で望ましくないため、しきい値
  `SHAPE_ASSIGN_SYNC_LIMIT`（暫定400人、§9で実測により確定）を超える場合は
  `DESIGN_GAPS.md` B-3 の `Job<T>` へ委譲し、進捗バー付きの非同期処理にする。
  400人未満は同期実行で体感即時（<5ms）。
- 計算結果を `Document` へ反映する部分（`Edit::ApplyShape` の `apply`）は、**計算済みの
  `before_positions`/`after_positions` をコピーするだけ**なので `O(n)` のメモリコピー
  （1,000人×8byte×2 ≒ 16KB）であり、Undo/Redo は常に2ms未満に収まる
  （00-conventions.md の「通常編集の Undo/Redo 2ms未満」を満たす。重い計算は`apply`時に
  1回だけ走り、undo/redoでは再計算しない設計）。
- `History` のメモリ増分: 1件の `Edit::ApplyShape` は `before_positions`+`after_positions`
  （上限4,000人×8byte×2 ≒ 64KB）＋ `ShapeAssignment` 2つ（数十〜数百バイト）。
  履歴上限200件として最大約13MB、常駐メモリ予算に対して無視できる。

## 6. 失敗モードと安全性

| 失敗モード | 対処 |
|---|---|
| 半径が負 | `validate()` で `DrillError::InvalidShapeParameter` を返す。UI は適用前にブロック。`sample()` 自身も `radius.max(0.0)` で防御。 |
| 半径が0 | エラーにしない（縮退フォーメーションとして合法）。 |
| `cols`/`rows`/`sides`/`points` が巨大 | `validate()` が `MAX_SHAPE_PERFORMERS`/`MAX_SHAPE_VERTICES` で拒否。乗算は `saturating_mul` でオーバーフローパニックを回避。 |
| 制御点・輪郭点に NaN/Inf | `validate()` の `finite()` チェックで一次拒否。バイパスされた場合は弧長テーブルの `total` が非有限になり、`t_for_arc_length`/`sample_by_arc_length`/`closed_polyline` が `0.0`または先頭点へフォールバックする二次防御。 |
| 自由描画パスの頂点数が過大（数千点のマウスサンプル） | `drill-app` 側で Douglas-Peucker 簡略化を必須の前処理とし、`validate()` が `MAX_SHAPE_VERTICES` で最終防御。 |
| `Text` の `contours` が空、または全長0 | `sample_text_contours` は空配列を返す（0人配置）。パニックしない。呼び出し側が「このテキストは表示されません」を警告表示する。 |
| `assign_to_shape` で `groups` の合計人数が `shape_points.len()` を超える | 超過分を `unplaced` に列挙して返す。呼び出し側が警告し、シェイプの `count` を増やすかグループを減らすかをユーザーに選ばせる。パニックしない。 |
| `fit_line`/`fit_circle` が縮退データ（共線・同一点）を受け取る | `None` を返す。呼び出し側（clinic パネル）はボタンを無効化するか「この選択には適用できません」を表示する。 |
| `optimal_assignment`/`optimal_assignment_bounded` の入力長不一致 | 既存の `pathing::optimal_assignment` と同じ規約で恒等写像にフォールバック（呼び出し元は常に有効な `Vec<usize>` を得る）。 |
| 巨大シェイプ適用（1,000人超）で UI スレッドをブロックしたくない | `SHAPE_ASSIGN_SYNC_LIMIT` を超える場合は `Job<T>`（doc 40）へ委譲。キャンセル可能にする。 |
| 敵性入力（他人から受け取ったプロジェクトファイルの `ShapeSpec`） | `Document::from_json` の検証パス（`validate()` 相当）を必ず通す。`Set.shape` が `Some` でも `validate()` に失敗した場合はロード時に `shape: None` へ降格し（座標 `positions` 自体は既存の検証を通れば読み込む）、致命的エラーにしない — 「図形の再編集性は失うが、ドリル本体は開ける」という `PRODUCT_QUALITY.md` の「音声・画像が欠落してもドリル本体を開ける」と同種のポリシー。 |

## 7. テスト計画

**単体テスト**（`shapes/mod.rs` 他、既存パターンに追加）

- 新規シェイプ（Ellipse/Parabola/SineWave/Star/Polygon/Cross/FreePath/Text）各々:
  `count`一致、既知パラメータでの境界点座標検証（既存の `circle_points_lie_on_radius` 等と
  同型）。
- `closed_polyline`: `count`点を返す、`count>=2`で始点≠終点、`count==1`で`vertices[0]`、
  空/単一頂点/`count==0`の境界値。
- `bezier`/`spiral` 修正後の回帰: 既存テスト（`bezier_preserves_endpoints` 等)が無変更で
  通ること。

**弧長ゴールデン数値テスト**

- 高曲率な3制御点ベジェ（例: 鋭角に折れる制御多角形）で、旧実装相当（`t`均等サンプル）と
  新実装（弧長均等）それぞれの隣接点間距離の標準偏差を計算し、新実装が**明確に小さい**
  （例: 半分未満）ことをアサートする。旧実装はテスト内でインラインの参照実装として再現する
  （本番コードには残さない）。
- `t_for_arc_length` の property test: ランダムな単調増加テーブルと `target` に対し、
  返る `t` が `[0,1]` 範囲内であること、`target=0`→`t≈0`、`target=total`→`t≈1`。

**フィッティング**

- 既知の円（ノイズ付き、`radius=20yd`程度）を`fit_circle`に通し、`center`/`radius`誤差が
  グリッド解像度以下であることを検証。
- 既知の直線（垂直に近い角度を含む）を`fit_line`に通し、通常の最小二乗が失敗するケース
  （ほぼ垂直な点列）でも有限の解を返すことを確認（全最小二乗の利点の直接証明）。
- 3点未満/共線点で`fit_circle`が`None`、2点未満/同一点で`fit_line`が`None`。

**対称性**

- `apply_mirror`: 反射後、各ペアが軸に対して正確に対称（`axis.reflect(mirrored) ==
  master`の往復）であることを検証。`on_axis`演者が軸上に留まることを検証。
- `apply_radial`: `fold=n`適用後、隣接演者間の中心からの角度差が`360/n`度、かつ中心からの
  距離が全員一致することを検証。

**割り当て**

- `assign_to_shape`: 全単射性（`assignment`のキー集合∪`unplaced` == 全入力performer、
  重複なし）のproperty test。極端ケース: グループ1つのみ、グループ数がシェイプ点数を
  超える、空グループ。
- `optimal_assignment_bounded`: `max_passes`を極端に小さくしても（例: 0=貪欲シードのみ）
  クラッシュせず有効な順列を返すこと。既存の`two_opt_never_worse_than_greedy`と同型の
  「貪欲以上」検証を`max_passes>=1`で行う。

**モーフィング**

- `plan_morph(same_order=true)`が恒等対応を返すこと。
- `morph_points`の`t=0.0`が`from`と、`t=1.0`が`to[correspondence[i]]`と一致すること。
- `morph_intermediate_sets(steps=3)`が3件の中間点列を返し、`t`が単調増加であること
  （各中間セットの対応する演者位置が`from`→`to`の直線上を単調に進む）。

**不正入力（安全性）**

- NaN/Inf を含む `ShapeSpec` を JSON 経由で `Document::from_json` に通し、パニックせず
  `Ok`（`shape: None`へ降格）または明示的な `Err` を返すことを確認。
- `cols=u32::MAX, rows=u32::MAX` のような巨大パラメータを含む JSON を読み込ませ、
  `Vec::with_capacity`のオーバーフロー/OOMではなく`validate()`のエラーで止まることを確認
  （`#[should_panic]`を使わず、正常にErrが返ることを直接assertする）。
- 半径負の`Circle`をロードし、同様に安全に拒否されることを確認。

**ストレス・ベンチ**（`crates/drill-core/benches/core_performance.rs` に追記）

- 1,000演者・弧長系シェイプ（Ellipse）の`sample`繰り返し呼び出しの時間計測、および
  1回目以降でヒープ再確保が無いことをポインタ比較で確認。
- 1,000演者・10セクション想定での`assign_to_shape`実行時間計測（§5の見積り10〜30msの
  実測による裏付け、`SHAPE_ASSIGN_SYNC_LIMIT`の確定に使う）。
- `optimal_assignment`（既存）と`optimal_assignment_bounded`（新規）の実行時間を
  `n=100,500,1000`で比較し、`bounded`版が`n`に対して線形〜準線形に留まることを確認する
  （既存版が`n`とともに急激に悪化する様子との対比）。

**ゴールデン比較**

- 代表的な図形（Circle/Star/Cross/Text の短い文字列）の`sample`出力をJSON化して
  `crates/drill-core/tests/fixtures/`相当にコミットし、将来の実装変更による意図しない
  座標ドリフトを検出する。

## 8. 実装タスク

1時間〜3時間相当の粒度に分解する。`T1`は全ての前提。`T2`/`T4`/`T9`/`T10`は`T1`後に並行可能。

| # | タスク | 依存 | 並行可否 |
|---|---|---|---|
| T1 | `shapes.rs` → `shapes/mod.rs` へ再編成。既存6関数・既存テストを無変更で移設し、`cargo test -p drill-core` が通ることを確認 | なし | — |
| T2 | `arc_length.rs`: `build_arc_length_table` / `t_for_arc_length` / `sample_by_arc_length` の実装とproperty test | T1 | T4, T9, T10と並行可 |
| T3 | 既存 `bezier` / `spiral` の内部実装を `sample_by_arc_length` 呼び出しに置換。既存テストが無変更で通ることを確認し、弧長ゴールデンテストを追加 | T2 | — |
| T4 | `closed_polyline` の実装とテスト | T1 | T2, T9, T10と並行可 |
| T5 | `ShapeSpec` enum + `Shape` トレイト + 委譲実装（既存6形状 + Line/Arc）。`MAX_SHAPE_PERFORMERS`/`MAX_SHAPE_VERTICES`/`ShapeParamIssue`を含む | T2, T4 | — |
| T6 | Ellipse / Parabola / SineWave の `sample`/`arc_length`/`validate` 追加 | T5 | T7, T8と並行可 |
| T7 | Star / Polygon / Cross の `sample`/`arc_length`/`validate` 追加（`star_outline`/`polygon_outline`/`cross_outline`含む） | T4, T5 | T6, T8と並行可 |
| T8 | FreePath（頂点数上限検証）/ Text（`sample_text_contours`、largest-remainder按分）の追加 | T4, T5 | T6, T7と並行可 |
| T9 | `fit.rs`: `fit_line` / `fit_circle` / `snap_to_line` / `snap_to_circle` | T1 | T2, T4, T10と並行可 |
| T10 | `symmetry.rs`: `MirrorAxis` / `RadialSymmetry` / `SymmetryPairing` / `apply_mirror` / `apply_radial` | T1（`editing.rs`参照のみ） | T2, T4, T9と並行可 |
| T11 | `pathing.rs`に`optimal_assignment_with_pass_cap`追加（既存`optimal_assignment`をそのラッパーに書き換え、既存テスト無変更で通す）＋`assign.rs`の`optimal_assignment_bounded` | なし（`pathing.rs`単独） | T2, T4, T9, T10と並行可 |
| T12 | `assign.rs`: `assign_to_shape`（セクション制約付き） | T5, T11 | — |
| T13 | `morph.rs`: `plan_morph` / `morph_points` / `morph_intermediate_sets` | T5, T12 | — |
| T14 | `snap.rs`: `SnapConfig` / `snap_point` / `GridConfig::yard_lines()`（`lib.rs`への小追加） | T9 | T12, T13と並行可 |
| T15 | `Set.shape: Option<ShapeAssignment>` の追加提案を doc 10 と調整し、`Edit::ApplyShape`（またはdoc10が選ぶ等価な形）の最終仕様を確定（設計調整のみ、コードなし） | T5 | いつでも並行可 |
| T16 | `DrillError::InvalidShapeParameter` の最終バリアント名・`Locale`文言を doc 42 と調整（設計調整のみ、コードなし） | T5 | いつでも並行可 |
| T17 | §7 のテスト一式（単体・ゴールデン・property・不正入力）を各機能タスク完了後に追加 | T2〜T14 各々 | 対応する実装タスク完了後すぐ着手可 |
| T18 | `core_performance.rs` へのベンチ追加（1,000人`assign_to_shape`、`optimal_assignment` vs `_bounded`比較） | T12 | T17と並行可 |

## 9. 未決事項

- **`Set.shape` フィールドと `Edit::ApplyShape` の最終形**: 本書はdoc10の`Edit`enumへの
  追加を提案しているが、doc10の担当者が既に別の統合方針（例: 全ての形状変更を
  `Edit::MovePoints`のみで表現し、`shape`メタデータは別チャネルで管理する等）を決めている
  可能性がある。T15で調整する。
- **`DrillError::InvalidShapeParameter`の最終形**: バリアント名・`ShapeParamIssue`の
  列挙・日本語/英語メッセージ文言はdoc42と合わせる。T16で調整する。
- **Text のフォント整形**: `drill-app`側で使う具体的なライブラリ（`ttf-parser` +
  `rustybuzz`、または`ab_glyph`等）は本書の範囲外。doc43（アプリ構造）またはdoc52
  （相互運用）で決める。本書が固定したのは「輪郭点列としてcoreに渡る」という境界だけ。
- **`SHAPE_ASSIGN_SYNC_LIMIT`の具体値**: §5では暫定400人としたが、実機ベンチ（T18）の
  結果で確定する。しきい値はハードウェア依存性が高いため、設定可能にするか固定値にするかも
  未決。
- **`assign_to_shape`のグループ順序の初期値**: グループ間マッチングは現在位置の重心に
  最も近いランに割り当てる貪欲法だが、「セクション定義順（doc15の`Section`一覧順）を
  優先すべきでは」というUX上の対立候補がある。doc43のUXレビュー待ち。
- **Star/Polygon の代替配置モード**: 本書は「辺周に弧長均等」のみを設計した。
  「各頂点に必ず1人ずつ置き、残りを辺に配分する」という代替要求が出た場合は追加のenum
  バリアント（例: `vertex_biased: bool`）が必要になるが、現時点でその要求は確認できていない。
- **`snap_point`のシェイプ候補サンプル密度（256点固定）**: 非常に大きい/複雑な図形
  （多数の制御点を持つFreePath等）では256点では粗すぎる可能性がある。密度をシェイプの
  `arc_length()`に応じて可変にすべきかは、実際のUI操作感を見てから決める。
