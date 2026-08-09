# 20. DisplayList 中間表現とレンダリング基盤

## 1. 目的と範囲

### 解決する問題

「フィールドを描く」コードが現在 2 箇所に独立して存在する。

- `crates/drill-app/src/main.rs` の egui painter 版（`draw_field` / 2Dドット描画 / `draw_stadium`）
- `crates/drill-core/src/svg.rs` の SVG 文字列組み立て版（`field_svg`）

この 2 つは座標変換式・線の本数・色・ラベル位置を**別々に**持っている。したがって

- `PRODUCT_QUALITY.md`「座標表、ドリルブック、SVG/PDF出力は同じドキュメント座標を参照する」
- `MEDIA_PIPELINE.md`「同じproject/configから同じフレーム列を生成する」

はどちらも構造的に保証できない。加えて動画書き出し（31番）が消費できる描画結果が存在しない。

本書は **DisplayList を唯一の中間表現**として挟み、egui / SVG / ラスタ(RGBA) / PDF / 将来の wgpu が
すべてこれ 1 つを消費する構造を定義する。21番(GPU)・17番(印刷)・31番(動画) はこの境界の上に乗る。

### この文書が扱うこと

1. `drill-render` クレートの位置づけと依存
2. `DrawCmd` / `DisplayList` の完全な型定義とレイヤ規約
3. データ平坦化（`points` プール + `Range<u32>`、テキストプール + スパン）
4. `clear()` 再利用とフレーム内確保ゼロの運用、容量上限
5. `Scene` 入力（Document / global_count / Camera / RenderOptions / Theme / 選択 / 警告）
6. `build()` の決定論規約
7. `Backend` トレイトとテキスト計測の抽象化
8. 座標変換の連鎖と、2D/3D を同じ DisplayList に載せる判断
9. LOD（ドット簡略化・ラベル間引き・経路間引き・グリッド間引き）
10. `svg.rs` をシリアライザへ縮小する移行手順

### この文書が扱わないこと

| 除外 | 担当 |
|---|---|
| wgpu インスタンシングの実装・シェーダ | 21 |
| スタジアム構造物・観客席・Real View の見た目 | 22 |
| カメラのキーフレーム・追従・補間 | 23（本書は `Camera` を読むだけ） |
| 衝突・歩幅の**解析**（本書は結果を受け取るだけ） | 13 |
| ルート形状の**補間**（本書は結果座標を受け取るだけ） | 11 |
| PDF の紙面レイアウト・ページ分割・mm 単位 | 17 |
| FFmpeg パイプ・エンコード | 31 |
| カウントトラック等の時間軸 UI ウィジェット | 43 |
| 座標シート HTML / ドットブック HTML（表組みであり図ではない） | 17 |

---

## 2. 現状

### 2.1 egui painter 版（`crates/drill-app/src/main.rs`、全 1,837 行）

| 行 | 内容 |
|---|---|
| 1617–1710 | `fn draw_field(painter, rect, grid)`。フィールド塗り(1618)、枠(1619)、ヤード線＋番号(1627–1648)、ステップ格子 Lines/Dots(1650–1694)、ハッシュ線＋ラベル(1696–1709)。 |
| 1625–1626 | 座標変換がクロージャ `to_x` / `to_y`。**アスペクト比を保持しない**（`rect` に引き伸ばす）。 |
| 1468–1473 | 2D ドット用の `to_screen` クロージャ。1625–1626 と**別実装**だが同じ式。 |
| 1474–1497 | 演者ドット描画。半径は選択で 6.0/8.0 の 2 値固定、ラベルは全員に無条件描画（LOD なし）。 |
| 1441–1467 | 「編集を始めましょう」カードを painter で直接描画。 |
| 292–414 | `fn draw_stadium(...)`。カメラ操作(299–310)、背景(311)、フィールド四隅ポリゴン(323–348)、ヤード線(349–368)、ハッシュ(369–385)、**毎フレーム `Vec<usize>` を確保して深度ソート**(393–394)、ドット(396–406)、HUD テキスト(407–413)。 |
| 1712–1837 | `fn draw_count_track(...)`。時間軸ウィジェット。描画と入力処理が一体（戻り値でシーク要求を返す）。 |
| 1435–1440 | `allocate_painter` → `rect.shrink(18.0)` → 2D/3D 分岐。 |

`draw_stadium` の 393 行 `let mut order: Vec<usize> = (0..len).collect();` は
00-conventions「毎フレーム走る関数は内部でヒープ確保しない」に違反している。

### 2.2 SVG 版（`crates/drill-core/src/svg.rs`、全 401 行）

| 行 | 内容 |
|---|---|
| 18–31 | `xml_escape` |
| 34–36 | `html_escape`（`xml_escape` への委譲） |
| 39–41 | `hex_color([u8;3]) -> String` |
| 44–49 | `px(f32) -> String`（`{:.2}` から末尾ゼロ除去。**量子化規約が egui 側に無い**） |
| 53–55 | `yard_number(x, width)` |
| 66–214 | `field_svg`。fit 計算(92–99)、`map` クロージャ(102)、背景(107–115)、ヤード線＋番号(118–150)、ハッシュ(153–178)、演者(181–201)、タイトル(204–211)。 |
| 218–228 | `set_svg`（`field_svg` の薄いラッパ） |
| 231–242 | `print_style`（HTML 用 CSS） |
| 247–281 | `coordinate_sheet_html` |
| 286–322 | `drill_book_html` |
| 324–401 | テスト 6 件 |

egui 版との**実質的な差異**（同じ図の 2 実装が食い違っている証拠）:

| 項目 | egui (`draw_field`) | SVG (`field_svg`) |
|---|---|---|
| アスペクト比 | 保持しない（1625–1626 で伸縮） | 保持する（95 行 `min`） |
| y 軸の向き | 上が y=0（1626） | **下が y=0**（102 行、フロントサイドラインを下端に） |
| 芝の色 | `rgb(25,71,45)`（1618） | `#2e7d32`（109 行） |
| ヤード番号 | `unit` の生値（1642） | `min(x, width-x)`（53–55, 139） |
| ステップ格子 | あり（1650–1694） | **なし** |
| ドット半径 | 6.0 / 8.0 px（1484） | 7 固定（187） |
| ラベル位置 | ドットの下（1491） | ドットの**中央**（196） |

y 軸の向きが逆であることは、座標表・印刷・画面表示が同じ座標を参照していないことを意味する。

### 2.3 カメラ（`crates/drill-core/src/camera.rs`、全 372 行）

- 26–28 `field_to_world(Point, height) -> [f32;3]`：`{x,y}` → `[x, height, y]`
- 33–49 `Camera { target, yaw, pitch, distance, fov_y_rad, near, far }`（`Serialize`/`Deserialize` 済み）
- 71–81 `position()`
- 87–113 `project(world, vw, vh) -> Option<[f32;2]>`（背面・クリップ外は `None`、原点は左上・y 下向き）
- 116–124 `project_point(point, height, vw, vh)`
- 155–185 プリセット `audience_view` / `press_box` / `overhead`

投影は既に純粋関数として完成しており、DisplayList 生成から**そのまま呼べる**。本書は camera.rs を変更しない。

### 2.4 存在しないもの

- `drill-render` クレート（`Cargo.toml` の `members` は `crates/drill-core`, `crates/drill-app` の 2 つのみ）
- `drill-export` クレート
- 中間表現・レイヤ・LOD・テキスト計測の抽象・ゴールデンテスト
- `Performer` に `symbol` フィールド（`lib.rs:225–230` は `id` / `label` / `color` のみ。記号は A-5 で追加予定）

### 2.5 参考にする既存ベンチ

`crates/drill-core/benches/core_performance.rs:9–13` は
「1,000人 × 60,000 フレームの補間 = 9.24ms」を測っている。ただしこれは `Point::lerp` のみを
`black_box` 越しに回した値で、DisplayList 生成コストの根拠には使えない。本書は 5 節で独自に見積もる。

---

## 3. 設計

### 3.1 クレートの位置づけ

```
drill-core    ドキュメントモデル・時間・座標・解析。依存 = serde / serde_json のみ。
   ▲
drill-render  DisplayList 中間表現とその生成。依存 = drill-core のみ。
              描画APIを知らない（egui / wgpu / svg / pdf / image のいずれも参照しない）。
              serde にも依存しない（ゴールデン出力は手書きのテキストエンコーダで足りる）。
   ▲
   ├── drill-export   SVG / RGBA raster / PDF / CSV。drill-render に依存。
   └── drill-app      egui backend と入力変換のみ。drill-render に依存。
```

`Cargo.toml` の `members` に `crates/drill-render` を追加する。`drill-render` の `[dependencies]` は
`drill-core = { path = "../drill-core" }` の 1 行だけ。**この 1 行だけであることを CI で検査する**（7 節）。

`drill-render` は「描画APIを知らない」を機械的に保証するため、`DrawCmd` の全フィールドを
プリミティブ（`f32` / `u8` / 本クレート定義の `Copy` 型）に限定する。

### 3.2 基礎型

```rust
// crates/drill-render/src/types.rs

/// View-space 2D position, in output units (origin at the viewport's top-left, y down).
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Vec2 { pub x: f32, pub y: f32 }

/// Axis-aligned rectangle in view space. `max` is always >= `min` component-wise.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect { pub min: Vec2, pub max: Vec2 }

impl Rect {
    pub fn from_min_size(min: Vec2, size: Vec2) -> Self { /* ... */ }
    pub fn width(&self) -> f32 { self.max.x - self.min.x }
    pub fn height(&self) -> f32 { self.max.y - self.min.y }
}

/// Straight, non-premultiplied sRGB colour. Byte channels keep `DrawCmd` small
/// and make golden output exact (no float formatting of colours).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rgba { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Self { r, g, b, a: 255 } }
    pub const fn with_alpha(self, a: u8) -> Self { Self { a, ..self } }
    /// `#rrggbbaa`, always 9 bytes. Used by the golden writer and the SVG backend.
    pub fn to_hex(self, out: &mut String);
}

/// Performer dot glyph. Mirrors the `Symbol` that A-5 will add to
/// `drill_core::Performer`; until then `drill-render` owns the definition and
/// `From<drill_core::…::Symbol>` is added when core gains it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[repr(u8)]
pub enum Symbol {
    #[default] Circle = 0,
    Square = 1,
    Triangle = 2,
    Diamond = 3,
    Cross = 4,
    /// Hollow circle: guard / prop / "ghost" dot.
    Ring = 5,
}

/// Nine-way text anchor. The backend positions the ink box relative to `at`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Anchor {
    LeftTop, CenterTop, RightTop,
    LeftCenter, Center, RightCenter,
    LeftBottom, CenterBottom, RightBottom,
}

/// Which of the two logical fonts a backend must map to a concrete face.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum FontRole { Sans = 0, Mono = 1 }

/// Dash pattern, expressed in view units. `Solid` avoids allocating a pattern
/// array for the 95% case.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dash { Solid, Dashed { on: f32, off: f32 } }

/// Slice of `DisplayList::points`. Half-open, `end >= start`, both <= points.len().
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PointSpan { pub start: u32, pub end: u32 }

/// Slice of `DisplayList::text`. Byte offsets, always on UTF-8 char boundaries.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TextSpan { pub start: u32, pub len: u32 }
```

### 3.3 `DrawCmd`

```rust
// crates/drill-render/src/cmd.rs

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DrawCmd {
    /// The turf itself. Separate from `Rect` so a GPU/raster backend can use a
    /// dedicated turf shader or texture without pattern-matching on colour.
    FieldFill { rect: Rect, fill: Rgba },

    /// Panels, cards, marquee, count-track segments, text plates.
    Rect { rect: Rect, fill: Rgba, stroke: Rgba, stroke_w: f32, corner: f32 },

    /// Yard lines, hashes, step grid, sideline.
    Line { a: Vec2, b: Vec2, width: f32, color: Rgba, dash: Dash },

    /// Shapes and guide geometry that are not a performer path.
    Polyline { points: PointSpan, width: f32, color: Rgba, dash: Dash, closed: bool },

    /// Curved guides (arc tools, curved route previews drawn as a true arc).
    /// Angles in radians, measured in view space, increasing clockwise
    /// (because view-space y points down).
    Arc { center: Vec2, radius: f32, start_rad: f32, end_rad: f32, width: f32, color: Rgba },

    /// A performer's path over a count window. Distinct from `Polyline` because
    /// backends style it differently (`taper` fades the oldest samples) and a
    /// GPU backend batches trails separately from static geometry.
    Trail { points: PointSpan, width: f32, color: Rgba, taper: bool },

    /// One performer. `depth` is 0.0 (nearest) .. 1.0 (farthest); the 2D builder
    /// always writes 0.5. Raster / SVG / PDF ignore it — commands are already in
    /// back-to-front order. The wgpu backend (doc 21) uses it as the depth value
    /// so it can draw all dots in one instanced call without sorting.
    Dot { center: Vec2, radius: f32, depth: f32, symbol: Symbol,
          fill: Rgba, stroke: Rgba, stroke_w: f32 },

    /// Selection affordance drawn *under* the dot it belongs to.
    Highlight { center: Vec2, radius: f32, width: f32, color: Rgba, style: HighlightStyle },

    /// Clinic / analysis annotation. Carries semantics, not geometry, so that
    /// C-3 ("warnings must not rely on colour alone") can be satisfied by the
    /// backend drawing a distinct glyph per kind.
    Marker { at: Vec2, kind: MarkerKind, size: f32, color: Rgba },

    /// All text. `fit` lets a backend compress a label into a known box.
    Text { at: Vec2, text: TextSpan, size_px: f32, anchor: Anchor,
           font: FontRole, color: Rgba, fit: TextFit },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum HighlightStyle { Ring, DashedRing }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum MarkerKind {
    /// Two performers closer than the clinic radius.
    Collision,
    /// Step size above the "aggressive" threshold.
    StrideAggressive,
    /// Step size above the "impossible" threshold.
    StrideImpossible,
    /// Dot outside the field boundary.
    OffField,
    /// Generic annotation pin (set notes).
    Note,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TextFit {
    /// Draw at the natural width of whatever font the backend has.
    Natural,
    /// Compress (or letter-space down) so the ink box is at most this wide.
    MaxWidth(f32),
}
```

**サイズ**: 最大バリアントは `Rect`（`Rect` 16 + `Rgba` 4 + `Rgba` 4 + `f32` 4 + `f32` 4 = 32B）と
`Dot`（8+4+4+1+4+4+4 = 29B → アライン 32B）。タグ 4B を加えて `size_of::<DrawCmd>() == 36`。
`Copy` かつ `Drop` を持たないので `Vec<DrawCmd>::clear()` は長さを 0 にするだけの O(1)。

```rust
const _: () = assert!(std::mem::size_of::<DrawCmd>() <= 40);
const _: () = assert!(std::mem::align_of::<DrawCmd>() == 4);
```

**レイヤと z 順**。ソートは行わない。ビルダは各レイヤを 1 パスずつ、**昇順に**発行するので
`cmds` は生成時点で既に z ソート済みである。レイヤごとの範囲を持つことで、
バックエンドが「選択ハイライトだけ描かない」「HUD を除いて印刷する」を O(1) で選べる。

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Layer {
    FieldFill   = 0,
    GridMinor   = 1,   // step grid
    GridMajor   = 2,   // yard lines, sideline
    Hash        = 3,
    FieldText   = 4,   // yard numbers, hash labels
    Trail       = 5,
    Highlight   = 6,   // selection rings, drawn under dots
    Dot         = 7,
    DotLabel    = 8,
    Marker      = 9,   // clinic warnings, above everything drill-related
    Overlay     = 10,  // title, HUD, marquee, onboarding card
}

impl Layer {
    pub const COUNT: usize = 11;
    pub const ALL: [Layer; Layer::COUNT] = [ /* ... */ ];
}

/// Bitmask over `Layer`, used to skip layers at render time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LayerMask(pub u16);

impl LayerMask {
    pub const ALL: LayerMask = LayerMask(0x07FF);
    /// Screen: everything.
    pub const SCREEN: LayerMask = LayerMask::ALL;
    /// Print / video: no selection rings, no HUD.
    pub const PRINT: LayerMask = /* ALL minus Highlight, minus Overlay-HUD */;
    pub fn contains(self, layer: Layer) -> bool { self.0 & (1 << layer as u16) != 0 }
}
```

### 3.4 `DisplayList`

```rust
// crates/drill-render/src/list.rs

#[derive(Debug, Default)]
pub struct DisplayList {
    /// z-sorted by construction: `cmds[layer_ranges[L]]` are exactly layer L's
    /// commands, and the ranges partition `0..cmds.len()` in ascending `Layer`.
    cmds: Vec<DrawCmd>,
    /// Flat pool for `Polyline` / `Trail` vertices.
    points: Vec<Vec2>,
    /// One flat UTF-8 buffer for every string in the frame.
    text: String,
    layer_ranges: [PointSpan; Layer::COUNT],
    viewport: Viewport,
    stats: BuildStats,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Viewport {
    /// Size in output units. The list's origin is always (0, 0); the backend
    /// translates. This makes two lists comparable regardless of window position.
    pub size: Vec2,
    /// Multiplier applied by `build` to every stroke width, dot radius and font
    /// size. 1.0 for an on-screen egui viewport (egui applies
    /// `pixels_per_point` itself); `height / 720.0` for a video frame so a 4K
    /// export has the same visual weight as a 720p one.
    pub ui_scale: f32,
    /// Output units per millimetre. 0.0 = unknown (screen). Doc 17 sets this
    /// for print so a 0.3 mm hairline is expressible.
    pub px_per_mm: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BuildStats {
    pub performers_total: u32,
    pub dots_emitted: u32,
    pub dots_culled: u32,        // outside the frustum / viewport
    pub labels_emitted: u32,
    pub labels_culled: u32,      // LOD
    pub trail_points: u32,
    pub dropped_nonfinite: u32,  // NaN / Inf reached a push_* helper
    pub clamped: u32,            // finite but beyond COORD_LIMIT
    pub truncated: bool,         // hit MAX_CMDS / MAX_POINTS / MAX_TEXT
}

impl DisplayList {
    pub fn new() -> Self { Self::default() }

    /// Reset for reuse. Keeps every allocation.
    pub fn clear(&mut self) {
        self.cmds.clear();
        self.points.clear();
        self.text.clear();
        self.layer_ranges = [PointSpan::default(); Layer::COUNT];
        self.stats = BuildStats::default();
    }

    // --- read side (backends) ---
    pub fn viewport(&self) -> &Viewport { &self.viewport }
    pub fn stats(&self) -> &BuildStats { &self.stats }
    pub fn cmds(&self) -> &[DrawCmd] { &self.cmds }
    pub fn layer(&self, layer: Layer) -> &[DrawCmd] {
        let r = self.layer_ranges[layer as usize];
        &self.cmds[r.start as usize..r.end as usize]   // ranges are an invariant
    }
    /// Never panics: an out-of-range span yields an empty slice.
    pub fn points(&self, span: PointSpan) -> &[Vec2] {
        self.points.get(span.start as usize..span.end as usize).unwrap_or(&[])
    }
    /// Never panics: an out-of-range or non-boundary span yields "".
    pub fn text(&self, span: TextSpan) -> &str {
        let (s, e) = (span.start as usize, span.start as usize + span.len as usize);
        self.text.get(s..e).unwrap_or("")
    }

    /// Release slack after a document change. Never called during playback.
    pub fn shrink_to_budget(&mut self);

    /// Stable text serialization for golden tests (7 節).
    pub fn write_golden(&self, out: &mut String);
}
```

**平坦化を選ぶ理由**

1. **確保回数**。素朴な `DrawCmd::Trail { points: Vec<Vec2> }` は 1,000 人分で毎フレーム
   1,000 回の `malloc` + 1,000 回の `free` になる。00-conventions「再生中のフレーム内ヒープ確保ゼロ」に
   正面から違反する。プール方式なら**フレームあたり 0 回**（定常状態）。
2. **`DrawCmd` が `Copy` でいられる**。`Vec`/`String` を内包すると `Drop` が付き、`cmds.clear()` が
   O(n) のデストラクタ走査になり、`Vec<DrawCmd>` の memcpy 拡張もできなくなる。
3. **バックエンド側の再走査が容易**。SVG は同じ `Trail` を 2 回走査する（`points` 属性の生成と
   bbox 計算）。ラスタはスキャンライン単位で何度も参照する。GPU は `points` をそのまま
   頂点バッファへ 1 回の `write_buffer` でアップロードできる。所有権が分散していると全部できない。
4. **境界検査が 1 箇所に集まる**。`PointSpan` の妥当性は不変条件 I3（4 節）1 つで表現でき、
   バックエンドは `points()` / `text()` のヘルパを使う限り添字パニックしない。

**`strings: Vec<String>` ではなく `text: String` + `TextSpan` を採る理由**（`DESIGN_GAPS.md` B-1 のスケッチを精緻化）。
`Vec<String>` は「文字列 1 本 = 1 確保」であり、300 ラベルなら毎フレーム 300 回の確保になる。
理由 1 がそのまま当てはまるので、単一バッファ + スパンに置き換える。
1,000 ラベル（平均 4 バイト）でもフレームあたりのコピーは 4 KB、実測で 1µs 未満である。

### 3.5 `Scene` 入力

```rust
// crates/drill-render/src/scene.rs
use drill_core::{Document, GridConfig, Point, camera::Camera};

pub struct Scene<'a> {
    pub doc: &'a Document,
    /// The already-interpolated frame. See `Frame` below for why it is not
    /// recomputed here.
    pub frame: Frame<'a>,
    pub view: ViewKind,
    pub viewport: Viewport,
    pub options: &'a RenderOptions,
    pub theme: &'a Theme,
    /// Ascending, deduplicated performer indices. Empty = nothing selected.
    pub selection: &'a [u32],
    /// Pre-computed clinic results (doc 13). `build` never analyses.
    pub markers: &'a [WarningMarker],
}

/// One instant of the drill, in document terms.
#[derive(Clone, Copy)]
pub struct Frame<'a> {
    pub global_count: f32,
    pub set_index: usize,
    pub local_count: f32,
    /// Index-aligned with `doc.performers`. Length must equal `performers.len()`;
    /// a mismatch makes `build` emit `min(len)` dots and set `stats.truncated`.
    pub positions: &'a [Point],
}

impl<'a> Frame<'a> {
    /// Fill `scratch.positions` from the document and return a borrowing `Frame`.
    /// The app already keeps `frame_positions` for hit-testing, and the video
    /// exporter drives counts from rational frame time, so both callers own the
    /// interpolation; `build` must not do it twice.
    pub fn resolve(doc: &'a Document, global_count: f32, out: &'a mut Vec<Point>) -> Frame<'a>;
}

#[derive(Clone, Copy)]
pub enum ViewKind {
    Field2D { fit: FitMode },
    Stadium3D { camera: Camera },
}

/// How the field rectangle maps into the viewport.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FitMode {
    /// Preserve aspect ratio, centre, leave `margin_px` for edge labels.
    /// This is what `svg.rs:92–99` does and what `draw_field` should do.
    Contain,
    /// Stretch to fill. Kept only so the current app behaviour can be
    /// reproduced during migration; not offered in the UI.
    Stretch,
}
```

```rust
#[derive(Clone, Debug)]
pub struct RenderOptions {
    pub show_field: bool,
    pub grid: GridDensity,
    pub show_hashes: bool,
    pub show_yard_numbers: bool,
    pub labels: LabelMode,
    pub trails: TrailMode,
    /// Counts of path drawn behind / ahead of `global_count`.
    pub trail_counts_back: f32,
    pub trail_counts_forward: f32,
    pub show_selection: bool,
    pub show_markers: bool,
    pub overlay: OverlayMode,
    /// Dot radius in view units at `ui_scale == 1.0`, before LOD.
    pub dot_radius: f32,
    pub label_size: f32,
    pub margin: f32,
    pub lod: LodPolicy,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GridDensity { None, Major, Step, Full }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LabelMode { None, Selected, All }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrailMode { None, Selected, All }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverlayMode { None, TitleOnly, Hud }

impl RenderOptions {
    pub fn screen() -> Self;      // Full grid, LabelMode::All + LOD, Hud
    pub fn print() -> Self;       // Major grid, LabelMode::All, TitleOnly, no trails
    pub fn video() -> Self;       // Step grid, LabelMode::Selected, TitleOnly
    pub fn thumbnail() -> Self;   // Major grid, no labels, no trails, None overlay
}
```

```rust
/// Every colour the builder can emit. No colour literal appears anywhere else
/// in `drill-render`, so a theme swap is total and testable.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub background: Rgba,
    pub turf: Rgba,
    pub sideline: Rgba,
    pub line_major: Rgba,
    pub line_minor: Rgba,
    pub step_grid: Rgba,
    pub hash: Rgba,
    pub field_text: Rgba,
    pub dot_fill_fallback: Rgba,
    pub dot_stroke: Rgba,
    pub dot_label: Rgba,
    pub trail: Rgba,
    pub selection: Rgba,
    pub marker_collision: Rgba,
    pub marker_stride: Rgba,
    pub overlay_text: Rgba,
    pub overlay_panel: Rgba,
}

impl Theme {
    /// Matches the current dark app look (`main.rs:1618` turf, etc.).
    pub const SCREEN_DARK: Theme;
    /// White paper, black ink, hairline grid. Doc 17 uses this.
    pub const PRINT_LIGHT: Theme;
}
```

```rust
/// A clinic finding, already resolved to performer indices by doc 13.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WarningMarker {
    pub performer: u32,
    /// Optional partner (collision). `u32::MAX` = none.
    pub partner: u32,
    pub kind: MarkerKind,
}
```

### 3.6 `build`

```rust
// crates/drill-render/src/build.rs

/// Scratch reused across frames. Holds every allocation `build` needs.
#[derive(Debug, Default)]
pub struct BuildScratch {
    /// 3D: back-to-front draw order (performer indices).
    order: Vec<u32>,
    /// 3D: squared distance from the eye, index-aligned with `order`.
    depth_sq: Vec<f32>,
    /// Projected view-space position per performer; f32::NAN = culled.
    projected: Vec<Vec2>,
    /// LOD label de-clutter grid: cell -> 1 if taken.
    occupancy: Vec<u8>,
    /// Trail sampling buffer.
    samples: Vec<Point>,
}

impl BuildScratch {
    pub fn new() -> Self { Self::default() }
    /// Pre-size for `performers` performers so the first frame allocates once.
    pub fn reserve(&mut self, performers: usize);
}

/// Deterministic, allocation-free (after warm-up) construction of one frame.
///
/// `out` is cleared first. Panics never; malformed input is dropped and counted
/// in `out.stats()`.
pub fn build(scene: &Scene<'_>, scratch: &mut BuildScratch, out: &mut DisplayList);
```

内部は `ViewKind` で 2 本に分岐する。

```rust
fn build_field2d(scene: &Scene<'_>, fit: FitMode, s: &mut BuildScratch, out: &mut DisplayList);
fn build_stadium3d(scene: &Scene<'_>, camera: &Camera, s: &mut BuildScratch, out: &mut DisplayList);
```

どちらも**同じレイヤ順・同じヘルパ**でコマンドを積むので、出力型は完全に同一である（3.9 節で判断理由）。

ビルダの発行順（= レイヤ順）:

| # | パス | レイヤ | 2D | 3D |
|---|---|---|---|---|
| 1 | `emit_field_fill` | FieldFill | 矩形 1 | 四隅を投影した `Polyline{closed:true}` 1 |
| 2 | `emit_step_grid` | GridMinor | 間引き後の線 | 同左（投影） |
| 3 | `emit_yard_lines` | GridMajor | 縦線 + サイドライン | 同左 |
| 4 | `emit_hashes` | Hash | 横線 | 同左 |
| 5 | `emit_field_text` | FieldText | ヤード番号・ハッシュ名 | 3D では既定オフ |
| 6 | `emit_trails` | Trail | 選択/全員の経路 | 同左 |
| 7 | `emit_highlights` | Highlight | 選択リング | 同左 |
| 8 | `emit_dots` | Dot | 索引昇順 | **深度降順**（遠→近） |
| 9 | `emit_labels` | DotLabel | LOD 適用 | 同左 |
| 10 | `emit_markers` | Marker | `scene.markers` を索引昇順に | 同左 |
| 11 | `emit_overlay` | Overlay | タイトル / HUD | 同左 |

各パスの終わりに `out.close_layer(Layer::X)` を呼び、`layer_ranges` を確定させる。

### 3.7 座標変換

連鎖は 4 段で、DisplayList に入る時点で**すべて適用済み**である。

```
フィールド座標 (Point{x,y}, yd or m, y=0 がフロントサイドライン)
      │  ① field_to_world:  [x, height, y]              (camera.rs:26)
      ▼
ワールド座標 (右手系, +Y が上)
      │  ② view = lookAt(eye, target)                    (camera.rs:127)
      ▼
ビュー座標
      │  ③ proj = perspective(fov, aspect) → NDC → 画素   (camera.rs:142, 110–112)
      ▼
スクリーン座標 (原点=ビューポート左上, y 下向き)
      │  ④ ui_scale をストローク幅・フォント・半径に適用
      ▼
DrawCmd の Vec2
```

2D はこの連鎖の**アフィンな特殊形**として、専用の軽量写像で行う（透視除算を通さない）。

```rust
// crates/drill-render/src/layout.rs

/// Affine field -> view mapping for the 2D view. Computed once per build.
#[derive(Clone, Copy, Debug)]
pub struct FieldMap {
    ox: f32, oy: f32,   // view-space offset of field (0, height)
    sx: f32, sy: f32,   // view units per field unit; sy is negative-free (see map)
}

impl FieldMap {
    /// `Contain` preserves aspect and centres, leaving `margin` on every side.
    pub fn fit(grid: &GridConfig, viewport: &Viewport, fit: FitMode, margin: f32) -> FieldMap;

    /// Field -> view. Front sideline (`y == 0`) maps to the **bottom** of the
    /// field box, matching `svg.rs:102` and every printed drill chart. The
    /// current egui path (`main.rs:1626`) has this inverted and is corrected by
    /// this migration.
    #[inline]
    pub fn map(&self, p: Point) -> Vec2 {
        Vec2 { x: self.ox + p.x * self.sx, y: self.oy - p.y * self.sy }
    }

    /// Inverse, for the app's hit-testing and drag handling.
    #[inline]
    pub fn unmap(&self, v: Vec2) -> Point;

    /// View units per field unit along x. Used by LOD to decide grid decimation.
    pub fn scale_x(&self) -> f32 { self.sx }
}
```

`map` が 1 つの `#[inline]` 関数に閉じていることが、2 実装の食い違い（2.2 節の表）が
二度と起きないことの機械的な保証である。`unmap` を同じ場所に置くことで、
**入力（ドラッグ）と出力（描画）が同じ写像を使う**ことも保証される。

3D は `camera.rs` の `project` をそのまま呼ぶ。`drill-render` は投影行列を再実装しない。

```rust
#[inline]
fn project_dot(cam: &Camera, p: Point, height_m: f32, vp: &Viewport) -> Option<(Vec2, f32)> {
    let world = drill_core::camera::field_to_world(p, height_m);
    let screen = cam.project(world, vp.size.x, vp.size.y)?;
    let d = eye_distance(cam, world);
    Some((Vec2 { x: screen[0], y: screen[1] }, d))
}
```

`project` が `None`（背面・クリップ外）を返した演者は `stats.dots_culled` を増やして
コマンドを出さない。ラベル・ハイライト・マーカーも同じ判定を共有する
（`scratch.projected[i]` に `NAN` を書いておき、後続パスは `is_nan` を見るだけ）。

### 3.8 決定論規約

同じ `(Document, RenderOptions, Theme, Viewport, global_count, Camera, selection, markers)` からは
**バイト単位で同じ `DisplayList`** が出る。同一ビルド・同一ターゲットの範囲でこれを保証する
（`sin`/`cos` の実装差があるため、異なる libm 間では 3.8.6 の量子化後の一致のみを保証する）。

1. **走査順の固定**。演者は常に `doc.performers` の Vec 索引昇順。
   `HashMap` / `HashSet` を走査しない（`BTreeMap` / ソート済みスライスのみ）。
   `selection` は昇順スライスとして受け取る（`BTreeSet<usize>` を持つ現行 `main.rs:84` は
   呼び出し側で `Vec<u32>` へ写す）。
2. **反復加算の禁止**。グリッド線は `x += interval`（現行 `main.rs:1647` / `svg.rs:149`）ではなく
   `let x = i as f32 * interval;` で求める。反復加算は丸め誤差が蓄積し、
   `while x <= gw + 1e-3` の判定で**線の本数が 1 本ずれ得る**。本数は
   `let n = (grid.width / interval).floor() as u32 + 1;` で先に決める。
3. **演算順序の一元化**。座標変換は `FieldMap::map` / `project_dot` の 2 関数だけが行う。
   式をインラインで書き直さない（同じ数式でも括弧の位置で結果が変わる）。
4. **FMA 禁止**。`f32::mul_add` を使わない。有効/無効で結果が変わり、
   ターゲット（x86-64-v2 と v3 など）で差が出る。
5. **f64 との混在禁止**。`drill-render` は `f64` を使わない。カウントは `f32`、
   時間は呼び出し側（31番）で有理数から `f32` に落として渡す。
6. **量子化された比較**。ゴールデンは座標を `(v * 64.0).round() / 64.0`（1/64 単位）で
   丸めて出力する。これは 1080p で 0.016 px 未満の粒度であり、視覚的に無意味な差では落ちない。
7. **ソートの安定性**。3D の深度ソートは `sort_unstable_by` を使わない。
   `order.sort_by(|&a, &b| depth[b].total_cmp(&depth[a]).then(a.cmp(&b)))`
   と第 2 キーに索引を入れ、同深度でも順序を一意にする。
   （現行 `main.rs:394` は第 2 キーが無く、同深度の描画順が不定。）
8. **時間非依存**。`build` は `Instant::now()` / 乱数 / スレッド ID / 環境変数を読まない。
   「マーチングアント」のようなアニメーションは `HighlightStyle::DashedRing` として
   意味だけを渡し、位相はバックエンドが持つ。

### 3.9 2D と 3D は同じ DisplayList を使う（判断）

**結論: 同じ `DisplayList` 型・同じ `DrawCmd` を使い、ビルド関数だけを分ける。**

根拠:

1. `DrawCmd` は既にスクリーン空間である。透視除算が済んでいるので、
   「投影がアフィンか透視か」は表現に一切現れない。分ける理由が型に無い。
2. 3D が追加で要求するのは (a) 深度順の描画、(b) 視錐台外の除去、(c) 距離による半径変化の 3 つだけで、
   すべて**コマンドの順序と半径の値**に吸収される。新しいバリアントも新しいフィールドも要らない。
3. 分けると、SVG / ラスタ / PDF の 3 バックエンドがそれぞれ 2 種類の入力を扱うことになり、
   実装数が 4 から 8 に倍増する。31番の「3D Real View のオフラインレンダリング」（P2）が
   バックエンドの書き直しになる。
4. ゴールデンテストの仕組み（`write_golden`）が 1 つで済む。

**唯一の妥協点**は 21番（wgpu）である。GPU は深度バッファでソート不要に描きたい。
そのために `DrawCmd::Dot` に `depth: f32` を 1 フィールドだけ持たせる。
2D ビルダは常に `0.5` を書き、CPU バックエンド 3 種は無視する。
`Dot` は 32 バイト境界に収まったままなので、コストは 0 である。
この 1 フィールドを**今**入れておくことが、21番が後から `DisplayList` の形を壊さないための保険になる。

線・トレイルには `depth` を持たせない。3D では静的ジオメトリを全ドットより下のレイヤで
描く近似で足りる（フィールド線が演者の手前に来ることは実用上ない）。9 節に未決として残す。

### 3.10 `Backend` トレイト

```rust
// crates/drill-render/src/backend.rs

/// Read-only view of the pools, handed to every `draw` call.
#[derive(Clone, Copy)]
pub struct DrawCtx<'a> {
    list: &'a DisplayList,
}

impl<'a> DrawCtx<'a> {
    pub fn viewport(&self) -> &Viewport;
    /// Empty slice if the span is out of range. Never panics.
    pub fn points(&self, span: PointSpan) -> &'a [Vec2];
    /// "" if the span is out of range or not on a char boundary. Never panics.
    pub fn text(&self, span: TextSpan) -> &'a str;
    /// Deterministic metric model — see 3.11.
    pub fn metrics(&self, font: FontRole) -> &'static FontMetrics;
}

/// Optional features. A backend that answers `false` still must not fail;
/// `render` applies the documented degradation before calling `draw`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Capability {
    /// `Dash::Dashed`. Degradation: drawn solid.
    Dashes,
    /// `Rect::corner > 0`. Degradation: square corners.
    RoundedRects,
    /// `Trail::taper`. Degradation: uniform alpha.
    TrailTaper,
    /// `TextFit::MaxWidth`. Degradation: natural width.
    TextFitting,
    /// `Arc`. Degradation: `render` tessellates into a `Polyline`
    /// (deterministic: `max(8, ceil(sweep_rad * radius / 2.0))` segments).
    Arcs,
}

pub trait Backend {
    type Error;

    fn begin(&mut self, viewport: &Viewport) -> Result<(), Self::Error>;
    fn draw(&mut self, cmd: &DrawCmd, ctx: &DrawCtx<'_>) -> Result<(), Self::Error>;
    fn end(&mut self) -> Result<(), Self::Error>;

    /// Everything is supported unless a backend says otherwise.
    fn supports(&self, _cap: Capability) -> bool { true }

    /// Batching hook. Default forwards to `draw` in order. A wgpu backend
    /// overrides this for `Layer::Dot` to issue one instanced draw call.
    fn draw_layer(
        &mut self,
        _layer: Layer,
        cmds: &[DrawCmd],
        ctx: &DrawCtx<'_>,
    ) -> Result<(), Self::Error> {
        for cmd in cmds {
            self.draw(cmd, ctx)?;
        }
        Ok(())
    }
}

/// Drive a backend over a list. This is the only entry point backends need.
pub fn render<B: Backend>(
    list: &DisplayList,
    mask: LayerMask,
    backend: &mut B,
) -> Result<(), B::Error>;
```

`render` の責務:

1. `begin(viewport)`
2. `Layer::ALL` を昇順に走査し、`mask.contains(layer)` のレイヤだけ `draw_layer` を呼ぶ
3. `supports` が `false` を返す機能を、上表の**規定された劣化**へ落としてから渡す
   （`Arc` のテセレーションは `render` 側で行い、テセレーション結果は
   `DrawCtx` に載せた一時プールではなく **`render` のローカル `&mut Vec<Vec2>`** を使う
   — `DisplayList` は不変参照のままである）
4. `end()`

**4 バックエンドが満たすべき最小実装**

| バックエンド | クレート | `begin` | `end` | 非対応 Capability |
|---|---|---|---|---|
| egui | drill-app | `Painter` と原点 `Pos2` を保持 | なし | `TrailTaper`（`Shape::line` に頂点色が無い版では均一 α） |
| SVG | drill-export | `<svg viewBox>` を書く | `</svg>` | なし（`stroke-dasharray` / `textLength` があるため） |
| raster RGBA | drill-export | RGBA バッファを背景色でクリア | なし | `TextFitting`（フォールバックは自然幅） |
| PDF | drill-export（17番） | ページを開き CTM を設定 | ページを閉じる | `RoundedRects`（角丸は 4 ベジェで自前実装するので実質対応可） |

### 3.11 テキスト計測の抽象化

**問題**: `build` はラベル間引き（LOD）とテキスト背景プレートの寸法決定で幅を知りたい。
しかし egui は `Fonts::layout`、SVG はブラウザのフォント、ラスタは自前ラスタライザ、
PDF は Base-14 メトリクスと、**4 つとも別のフォント実装**を持つ。
`build` がバックエンドへ計測を問い合わせると、
(a) `build` がバックエンド依存になり「同じ DisplayList が出る」が崩れ、
(b) 決定論（3.8）が外部フォントに人質を取られ、
(c) ゴールデンテストがフォント更新で落ちるようになる。

**解決**: 3 層に分ける。

**層1 — `build` は原則として計測しない。**
`DrawCmd::Text` はアンカー 9 通りだけを持ち、アンカー基準の実配置はバックエンドの仕事とする。
現在必要なテキスト（ヤード番号・ハッシュ名・ドットラベル・タイトル・HUD）は全部これで足りる。

**層2 — どうしても幅が要る判断には、`drill-render` 内蔵の決定論的メトリクスモデルを使う。**
実フォントではなく**定数表**である。

```rust
// crates/drill-render/src/metrics.rs

/// A deterministic, backend-independent approximation of a font's metrics.
/// Not a font: it never rasterizes and never reads a file. Values are in
/// 1/1000 em, matching the PDF/AFM convention.
#[derive(Clone, Copy, Debug)]
pub struct FontMetrics {
    pub ascent_per_mille: i16,
    pub descent_per_mille: i16,
    /// Advance width for ASCII 0x20..=0x7E (95 entries).
    pub ascii: [u16; 95],
    /// Advance for anything else (CJK, symbols). Wide by construction.
    pub fallback: u16,
}

impl FontMetrics {
    /// Helvetica/Arial-class metrics. Within ~5% of Segoe UI and Noto Sans for
    /// the digit/latin subset that drill labels use.
    pub const SANS: FontMetrics = /* const table */;
    /// Fixed 600/1000 em advance. Exact for any monospace face.
    pub const MONO: FontMetrics = /* const table */;

    /// Deterministic, allocation-free, O(len). Never panics.
    pub fn measure(&self, s: &str, size_px: f32) -> f32 {
        let mut mille: u32 = 0;
        for ch in s.chars() {
            let a = match u32::from(ch) {
                c @ 0x20..=0x7E => self.ascii[(c - 0x20) as usize],
                _ => self.fallback,
            };
            mille = mille.saturating_add(u32::from(a));
        }
        mille as f32 * size_px * 0.001
    }

    pub fn line_height(&self, size_px: f32) -> f32;
}
```

この値は**誤差 ±10% を許容する判断にだけ**使う。具体的には
(a) ラベル de-clutter のセル幅、(b) テキストプレートのパディング、
(c) `TextFit::MaxWidth` の閾値。グリフの実描画位置には一切関与しない。

**層3 — バックエンドとの契約。**
バックエンドは、`FontRole` に対して「`FontMetrics::measure` の 1.15 倍の箱に
インクボックスが収まる」フォントを選ばなければならない。満たせない場合は
`TextFit::MaxWidth(w)` を尊重して圧縮する（`Capability::TextFitting`）。
SVG は `textLength="w" lengthAdjust="spacingAndGlyphs"`、
PDF は `Tz`（水平スケール）、egui は `size_px` を縮小、ラスタは字送りを詰める。
この契約はテスト可能である（7 節「メトリクス契約テスト」）。

**結果**: `build` はフォントを一切知らないまま決定論的であり続け、
ゴールデンは実フォントの更新に影響されない。

### 3.12 LOD

```rust
#[derive(Clone, Copy, Debug)]
pub struct LodPolicy {
    /// Minimum view-space spacing between two labels. 0 = never cull.
    pub label_min_spacing: f32,
    /// Hard cap on labels per frame.
    pub label_budget: u32,
    /// Below this dot radius every symbol degrades to `Circle`.
    pub symbol_min_radius: f32,
    /// Below this dot radius the outline stroke is dropped.
    pub stroke_min_radius: f32,
    /// Maximum sample count per trail.
    pub trail_max_points: u16,
    /// Minimum view-space spacing between two step-grid lines.
    pub grid_min_spacing: f32,
}

impl Default for LodPolicy {
    fn default() -> Self {
        Self {
            label_min_spacing: 14.0,
            label_budget: 400,
            symbol_min_radius: 2.5,
            stroke_min_radius: 1.5,
            trail_max_points: 16,
            grid_min_spacing: 4.0,
        }
    }
}
```

**ドット**: 半径 `r = options.dot_radius * ui_scale * distance_factor` を計算し、
`r < symbol_min_radius` なら `Symbol::Circle` へ落とす（三角形や十字は頂点が 1px を切ると
形として読めず、SVG では無駄なパス、ラスタでは無駄なスキャンになる）。
`r < stroke_min_radius` なら `stroke_w = 0.0`。`r` は `clamp(0.5, 64.0)`。

**ラベル**: ビューポートを `label_min_spacing` 角の一様格子に分割し、
`scratch.occupancy: Vec<u8>` に先着マークを立てる。O(n)、確保はウォームアップ 1 回のみ。
処理順は **(1) 選択中の演者を索引昇順 → (2) 残りを索引昇順**。
これで「選択したのにラベルが消える」が起きず、かつ決定論的（先着が索引順で一意）。
`label_budget` に達したら以降は `labels_culled` を数えて打ち切る。
1,000 人・1200×700 のビューポートでは格子セルが 85×50 = 4,250 個あるので、
実際には密集部だけが間引かれる。

**経路トレイル**: サンプル数は
`n = clamp(ceil(counts * 2.0) as u16, 2, trail_max_points)`。
11番の `RouteShape::Straight` は曲率ゼロなので**常に 2 点**に落とす
（1,000 人全員のトレイルで 2,000 点 = 16 KB）。曲線・折れ線のみ最大 16 点。
さらに `TrailMode::All` かつ演者数 > 500 のときは `trail_max_points` を 8 に半減する。

**ステップ格子**: 画面間隔 `dx_view = (horizontal_units / horizontal_steps) * map.scale_x()` が
`grid_min_spacing` 未満なら、**`grid_min_spacing` 以上になる最小の `2^k` 倍**に間引く。
`k = ceil(log2(grid_min_spacing / dx_view))` を整数演算で求めるので決定論的。

**上限（敵性入力対策も兼ねる）**

```rust
pub const MAX_CMDS: usize = 1 << 20;    // 1,048,576  (≈ 37 MB)
pub const MAX_POINTS: usize = 1 << 22;  // 4,194,304  (≈ 33 MB)
pub const MAX_TEXT: usize = 1 << 20;    // 1 MiB
pub const MAX_TEXT_LEN: usize = 64;     // bytes per single string
pub const COORD_LIMIT: f32 = 1.0e5;
```

到達したら `stats.truncated = true` にして**それ以降の発行を静かに止める**。
パニックも OOM も起こさない。

---

## 4. 不変条件

すべてテストで検証できる形にしてある（対応するテスト名は 7 節）。

| # | 不変条件 |
|---|---|
| I1 | `build` の直後、`cmds` はレイヤ昇順に並んでいる。`layer_ranges` は `0..cmds.len()` を隙間なく重複なく分割する。 |
| I2 | すべての `Vec2` 成分は有限で、`|v| <= COORD_LIMIT`。すべての `radius` / `width` / `size_px` は有限で `0.0 <= v <= 1.0e4`。 |
| I3 | すべての `PointSpan` は `start <= end <= points.len()`。すべての `TextSpan` は `start + len <= text.len()` かつ両端が UTF-8 文字境界。 |
| I4 | `build(scene, s, &mut a)` と `build(scene, s, &mut b)` の `write_golden` 出力は一致する（同一プロセス内でバイト一致）。 |
| I5 | 同じ `Scene` を 2 回目以降 `build` したとき、`cmds` / `points` / `text` の `capacity()` と `as_ptr()` が変化しない（フレーム内確保ゼロ）。`BuildScratch` も同じ。 |
| I6 | `build` はパニックしない。`Document` に NaN / Inf / 1e30 の座標が含まれていても、演者数と `positions` 長が不一致でも、`sets` が空でも返る。 |
| I7 | `DisplayList` は `Scene` の寿命を持たない（`'static`）。ワーカースレッドへ `Send` できる。 |
| I8 | `render` はどの `Backend` に対しても、`begin` を 1 回、`end` を 1 回だけ呼ぶ。`draw_layer` はレイヤ昇順に高々 1 回ずつ。 |
| I9 | 同じ `DisplayList` を 2 つのバックエンドへ流したとき、両者が読む `Dot::center` は同一値である（座標の唯一性）。 |
| I10 | `RenderOptions` のどのフラグを落としても、残るコマンド列は「落とさなかった場合の部分列」になる（表示 ON/OFF が幾何を変えない）。 |
| I11 | `drill-render` の依存は `drill-core` ただ 1 つ。 |
| I12 | `Theme` を差し替えると、出力中のすべての `Rgba` が新テーマの構成色のいずれかになる（色リテラルの散在が無いことの検査）。 |
| I13 | `DisplayList` の常駐バイト数は `shrink_to_budget` 呼び出し後、基準規模で 4 MB を超えない。 |

---

## 5. 性能

### 5.1 コマンド数の見積り（演者 1,000 人・1 フレーム・1200×700 ビューポート）

| 要素 | 典型（画面既定） | 最悪（全表示 ON） |
|---|---|---|
| `FieldFill` | 1 | 1 |
| ヤード線（100yd / 5yd + 両サイドライン） | 21 | 21 |
| ヤード番号テキスト | 21 | 21 |
| ステップ格子（8/5yd → x 160, y 85、間引き後） | 245 | 245 |
| ハッシュ線 + ラベル | 4 | 4 |
| トレイル（`Straight` 2 点） | 0（既定 `Selected`・50 人） 50 | 1,000 |
| 選択ハイライト | 50 | 1,000 |
| ドット | 1,000 | 1,000 |
| ラベル（LOD 後） | 400 | 400 |
| 警告マーカー | 32 | 256 |
| オーバーレイ | 6 | 12 |
| **合計コマンド** | **1,830** | **3,960** |

### 5.2 バイト数

| プール | 典型 | 最悪 |
|---|---|---|
| `cmds` | 1,830 × 36 B = **65.9 KB** | 3,960 × 36 B = **142.6 KB** |
| `points` | 50 トレイル × 2 = 100 点 × 8 B = 0.8 KB | 1,000 × 16 点 = 16,000 点 × 8 B = **128 KB** |
| `text` | 400 ラベル × 4 B + 固定文言 ≈ 2 KB | 1,000 × 8 B ≈ 8 KB |
| `BuildScratch` | 1,000×(4+4+8) + occupancy 4,250 ≈ **20 KB** | 同 |
| **合計** | **約 89 KB** | **約 299 KB** |

L2 に収まる規模であり、バックエンドの走査は完全にストリーミングになる。
上限規模（4,000 人）でも `cmds` 最悪 15,900 × 36 B = 572 KB、`points` 512 KB。
`Vec` の倍々成長を考慮した常駐上限を **`DisplayList` 1 個あたり 4 MB** と定め、
`shrink_to_budget()` を「ドキュメント切替時・アイドル時のみ」呼ぶ（フレーム内では呼ばない）。
アプリは画面用 1 個 + サムネイル用 1 個の計 2 個、書き出しワーカーは 1 個を持つ。

### 5.3 16.6ms 予算の取り分

| 段階 | 予算 | 目標（実測目安） |
|---|---|---|
| `drill_render::build`（2D・1,000人） | **1.5 ms（9%）** | 0.5 ms |
| `drill_render::build`（3D・1,000人、深度ソート込み） | **2.0 ms（12%）** | 0.9 ms |
| egui バックエンド（`DisplayList` → `egui::Shape`） | **2.0 ms（12%）** | 1.2 ms |
| 残り（egui テセレーション・入力・パネル・GPU 提出） | 12.6 ms | — |

`build` 1.5 ms の根拠: コマンド 1,830 本、1 本あたりの仕事は
「2〜8 回の乗加算 + `Vec::push`（36 B の memcpy）+ 分岐 2〜3」でおよそ 40〜120 サイクル。
1,830 × 120 = 22 万サイクル ≈ 0.07 ms。ラベル LOD の格子走査が O(n) で 1,000 回、
3D は追加で `sort_by` が O(n log n) = 約 1 万比較。
理論値の合計は 0.15 ms 程度で、実装のオーバーヘッドを 3〜5 倍見込んでも 0.5〜0.8 ms に収まる。
1.5 ms は**回帰検出用の上限**として置く（超えたら CI 失敗）。

egui バックエンド 2.0 ms の根拠: `egui::Shape` は 1 個あたり 40〜100 B の
`Vec<Shape>` push とパスの構築で、コマンド数と同オーダー。
現行実装は `painter.circle_filled` を 1,000 回呼んでおり、置き換えても呼び出し回数は変わらない。
副次的に、現行 `draw_stadium:393` の毎フレーム `Vec` 確保（1,000 × 8 B = 8 KB の
malloc/free）が `BuildScratch` へ移り消える。

### 5.4 ラスタ／動画側（31番へ渡す前提値）

1920×1080 RGBA = 8.29 MB/frame。ソフトウェアラスタの主コストは
フィールド塗り 2.07 M px と 1,000 ドット（半径 6px → 約 113 px/dot = 11.3 万 px）で、
テキストを除けば **1 フレーム 8〜12 ms（単スレッド）** が目標。
これは 31番の見積り根拠として提供するもので、本書の受け入れ基準には含めない。
`DisplayList` の生成コストはフレームあたり 0.5 ms なので、書き出し全体の 5% 未満に収まる。

---

## 6. 失敗モードと安全性

| # | 壊れ方 | 対処 |
|---|---|---|
| F1 | `Document` の座標が NaN / ±Inf（インポート・破損ファイル・0 除算の産物） | `push_*` ヘルパが全座標を検査。非有限なら**そのコマンドを発行せず** `stats.dropped_nonfinite += 1`。0.0 に丸めない（原点にドットが湧いて誤った図になるため）。 |
| F2 | 巨大座標（ズームアウト、`grid.width = 1e20`、悪意ある入力） | 有限であれば `clamp(-COORD_LIMIT, COORD_LIMIT)` して `stats.clamped += 1`。SVG の座標オーバーフローとラスタの整数オーバーフローを両方防ぐ。 |
| F3 | `grid.width` / `grid.height` が 0 または負 | `FieldMap::fit` が `max(f32::EPSILON)` を取り、`sx`/`sy` が非有限になる経路を塞ぐ。`fit` の戻り値自体も F1/F2 の検査を通す。 |
| F4 | `frame.positions.len() != doc.performers.len()` | `min(len)` までしか発行せず `stats.truncated = true`。添字パニックしない。 |
| F5 | 演者 4,000 人 × 全ラベル × 全トレイルで確保が爆発 | `MAX_CMDS` / `MAX_POINTS` / `MAX_TEXT` に達したら発行停止 + `truncated`。上限は 3.12 節の定数で固定。 |
| F6 | 極端に長いラベル（インポート由来の 1 MB 文字列） | `MAX_TEXT_LEN = 64` バイトで**文字境界に沿って**切り詰め（`s.char_indices()` で境界を取る。バイト単位で切らない）。 |
| F7 | ラベルに制御文字・双方向制御文字（U+202E 等）が混入 | `build` で `char::is_control()` と U+2066..U+2069 / U+202A..U+202E を除去。エスケープはバックエンドの責務だが、**制御文字の除去は中間表現の責務**（4 バックエンド全部で同じ処理を書かせない）。 |
| F8 | バックエンドが不正な `PointSpan` / `TextSpan` を受け取る | `DrawCtx::points` / `DrawCtx::text` が `get()` ベースで空を返す。バックエンドが生の `&Vec` を触れないよう、プールは非公開フィールドにしてアクセサ経由のみ。 |
| F9 | `camera.project` が全点に対して `None`（カメラがフィールドの裏） | `dots_emitted == 0` で正常終了。空の `DisplayList` は有効。アプリは `stats.dots_emitted == 0 && performers_total > 0` を見て「カメラをリセット」を案内できる。 |
| F10 | `Arc` 非対応バックエンドで sweep が巨大（`start_rad = 0, end_rad = 1e6`） | `render` のテセレーションは分割数を `clamp(8, 1024)` に固定。 |
| F11 | 2 時間再生でメモリが増え続ける | `clear()` が容量を保持し、上限は `Vec` の倍々成長で頭打ち（I13）。100,000 回 build して `capacity` 不変をテストする。 |
| F12 | ワーカースレッドで書き出し中に `Document` が編集される | `DisplayList` が `'static`（I7）なので、31番はドキュメントのスナップショットを取り、そこから `Scene` を作る。`DisplayList` 自体は借用を持たない。 |
| F13 | テーマの色に α=0 が入り「描いたのに見えない」 | 検査しない（正当なユースケース）。ただし `stats` には計上されるので、デバッグ時に「コマンドは出ている」が分かる。 |

`build` は `unwrap` / `expect` / 添字 `[]` / `panic!` / `unreachable!` を含まない。
Clippy lint `indexing_slicing` / `unwrap_used` / `expect_used` / `panic` を
`drill-render` に `#![deny]` で掛ける（`layer()` の内部添字だけは
I1 を保証する `debug_assert` + `get().unwrap_or(&[])` で書く）。

---

## 7. テスト計画

### 7.1 単体

| テスト | 内容 |
|---|---|
| `field_map_contain_preserves_aspect` | `FitMode::Contain` で `sx == sy`、フィールドが中央、四辺のマージンが `margin` 以上。 |
| `field_map_front_sideline_is_at_bottom` | `map(Point{x:0,y:0}).y > map(Point{x:0,y:h}).y`。2.2 節の y 反転バグの回帰防止。 |
| `field_map_round_trip` | `unmap(map(p)) ≈ p`（誤差 1e-4）を 10,000 点で。 |
| `sanitize_drops_nonfinite` | NaN / +Inf / -Inf を含む Document で `dropped_nonfinite == 3`、コマンド数が期待どおり減る。 |
| `sanitize_clamps_huge` | 1e30 → `COORD_LIMIT`、`clamped` が増える。 |
| `grid_line_count_is_exact` | `width=100, interval=5` で線 21 本。`width=99.999` / `100.001` でも 21 / 21 本（反復加算バグの回帰防止）。 |
| `font_metrics_measure_is_monotonic` | 文字を足すと幅が単調増加、空文字は 0、CJK は `fallback` 幅。 |
| `text_truncation_is_char_boundary` | マルチバイト文字が 64 バイト境界を跨ぐケースで `from_utf8` が通る。 |
| `control_chars_are_stripped` | U+202E / U+0007 を含むラベルが除去される。 |
| `cmd_size_is_bounded` | `size_of::<DrawCmd>() <= 40`（`const _` としても入れる）。 |
| `layer_ranges_partition_cmds` | I1。 |
| `lod_symbol_degrades_below_threshold` | 半径 2.4 で `Symbol::Circle`、2.6 で元の記号。 |
| `lod_label_prefers_selection` | 密集配置で選択中の演者のラベルが必ず残る。 |
| `layer_mask_print_drops_highlight_and_hud` | `LayerMask::PRINT` で `Highlight` / HUD が 0 本。 |

### 7.2 ゴールデン

`DisplayList::write_golden` は**安定テキスト**を出す。追加依存なし、差分が人間に読める。

```
# drill-render golden v1
viewport 1200.000 700.000 scale 1.000 px_per_mm 0.000
layer FieldFill 0..1
fieldfill rect 34.000 24.000 1166.000 645.328 fill #2e7d32ff
layer GridMajor 1..22
line 34.000 24.000 34.000 645.328 w 2.000 #ffffffd9 solid
...
layer Dot 271..1271
dot 168.500 400.750 d 0.500 r 6.000 circle fill #f5c542ff stroke #000000ff sw 1.000
...
layer DotLabel 1271..1671
text 168.500 410.750 anchor center-top size 9.000 mono #ffffffff fit natural "A1"
# stats performers=1000 dots=1000 culled=0 labels=400 dropped=0 clamped=0 truncated=false
```

規約: 座標は 1/64 に量子化して `{:.3}`、色は `#rrggbbaa`、文字列は
`"` と `\` と制御文字をエスケープして引用、各コマンド 1 行、レイヤ境界に `layer` 行、末尾に stats。

`crates/drill-render/tests/golden/` に `.expected` をコミットする。

| ゴールデン | 内容 |
|---|---|
| `field2d_demo_8x10_1200x700.expected` | 既定オプション、`Document::demo(8, 10)` |
| `field2d_print_a4.expected` | `RenderOptions::print()` + `Theme::PRINT_LIGHT` |
| `field2d_no_grid_no_labels.expected` | 全オプション OFF の最小形 |
| `field2d_selection_and_trails.expected` | 選択 10 人 + トレイル + 警告マーカー 3 件 |
| `stadium3d_press_box_1200x700.expected` | `Camera::press_box`、深度順の検証込み |
| `field2d_stress_1000.expected` | 1,000 人。LOD が効いていることの回帰 |

環境変数 `DRILL_GOLDEN_UPDATE=1` で `.expected` を再生成する（差分レビュー必須）。

### 7.3 property

| テスト | 内容 |
|---|---|
| `build_is_deterministic` | ランダムな Document 200 個 × 2 回 build して golden 文字列が一致（I4）。 |
| `all_coords_finite_and_bounded` | ランダム Document（NaN/Inf/1e30 を意図的に混入）で I2 を全コマンドに対して検査。 |
| `all_spans_in_range` | I3。 |
| `layers_are_ascending` | I1。 |
| `option_off_is_subsequence` | I10。各フラグを 1 つずつ落とし、残るコマンド列が全 ON の部分列であることを確認。 |
| `theme_colours_are_from_theme` | I12。出力の `Rgba` 集合 ⊆ テーマの構成色 ∪ 演者色。 |
| `build_never_panics` | 完全ランダムなバイト列から `serde_json` で復元できた Document 全部に対して build（51番のファザと共有）。 |

### 7.4 ストレス

| テスト | 内容 |
|---|---|
| `zero_allocation_after_warmup` | 3 回 build して以降 1,000 回、`cmds`/`points`/`text`/scratch 各 `Vec` の `as_ptr()` と `capacity()` が不変（`lib.rs:485` の `interpolation_reuses_output_allocation` と同じ手法）。 |
| `two_hour_playback_memory` | 60fps × 7,200 秒 = 432,000 回の build を 1/100 に間引いて 4,320 回実行し、`capacity` の単調増加が無いことを検査。 |
| `four_thousand_performers_truncates` | 4,000 人 × 全ラベル × 全トレイルで `truncated == true`、パニックなし、実行時間 < 20 ms。 |
| `nan_document_survives` | 全座標 NaN の 1,000 人ドキュメントで `dots_emitted == 0`、`dropped_nonfinite == 1000`。 |

### 7.5 バックエンド

| テスト | 内容 |
|---|---|
| `svg_backend_is_well_formed` | `<svg` 開始・`</svg>` 終了・`<circle>` 数 == `dots_emitted`（既存 `svg.rs:338` のテスト意図を継承）。 |
| `svg_backend_escapes_text` | 既存 `svg.rs:386–400` を移植。`A & B <"'>` が全部エスケープされる。 |
| `backend_receives_begin_end_once` | 記録用モックバックエンドで I8。 |
| `capability_degradation_is_specified` | `supports` が全部 `false` のモックで、破線→実線・角丸→直角・Arc→Polyline に落ちることを確認。 |
| `metrics_contract_egui` | egui の `Fonts::layout` で実測した幅が `FontMetrics::measure * 1.15` 以内（3.11 層3 の契約）。ASCII 95 文字 + 代表ラベル 50 個で検査。 |
| `metrics_contract_svg_pdf` | PDF Base-14 の AFM 幅と `FontMetrics::SANS` の差が 5% 以内。 |
| `cross_backend_dot_positions_match` | 同じ `DisplayList` を SVG バックエンドとダンプ用モックへ流し、`cx`/`cy` 属性が golden のドット座標と一致（I9・`PRODUCT_QUALITY.md`「同じドキュメント座標を参照する」の直接検証）。 |

### 7.6 ベンチ（`crates/drill-render/benches/render_performance.rs`、`harness = false`）

| ベンチ | 上限（CI で assert） |
|---|---|
| `build_field2d_1000` | 1.5 ms |
| `build_field2d_4000` | 6.0 ms |
| `build_stadium3d_1000` | 2.0 ms |
| `svg_backend_1000` | 8.0 ms |
| `golden_write_1000` | 5.0 ms |

egui バックエンドのベンチは `drill-app` 側（eframe 依存のため）。

### 7.7 CI 検査

- `cargo tree -p drill-render --depth 1` の出力が `drill-core` 1 行のみ（I11）。
- `drill-render` のソースに `egui` / `wgpu` / `svg` / `image` / `printpdf` の文字列が出現しない。
- `drill-render` のソースに日本語文字列リテラルが出現しない（00-conventions）。

---

## 8. 実装タスク

各 1〜3 時間。`→` は依存。

### フェーズ 1: クレートと型（直列）

| ID | 内容 | 時間 | 依存 |
|---|---|---|---|
| T1 | `crates/drill-render` 新設。`Cargo.toml` の members 追加。`types.rs`（`Vec2`/`Rect`/`Rgba`/`Symbol`/`Anchor`/`FontRole`/`Dash`/`PointSpan`/`TextSpan`）+ `cmd.rs`（`DrawCmd`/`Layer`/`LayerMask`）+ `size_of` の `const _` アサート。 | 2h | — |
| T2 | `list.rs`: `DisplayList` / `Viewport` / `BuildStats` / `clear` / アクセサ / `close_layer` / 上限定数 / `shrink_to_budget`。 | 2h | T1 |
| T3 | `layout.rs`: `FieldMap::fit` / `map` / `unmap` / `scale_x`、`sanitize`（`fin` / `clamp_coord`）、`push_*` ヘルパ群。 | 2h | T1 |

### フェーズ 2: ビルダ（T3 後・一部並行）

| ID | 内容 | 時間 | 依存 |
|---|---|---|---|
| T4 | `scene.rs`: `Scene` / `Frame` / `ViewKind` / `FitMode` / `RenderOptions` + 4 プリセット / `GridDensity` 等 / `Theme` 2 定数 / `WarningMarker`。 | 2h | T1 |
| T5 | `build_field2d` 前半: `emit_field_fill` / `emit_yard_lines` / `emit_hashes` / `emit_field_text`。線本数の非反復計算。 | 3h | T3, T4 |
| T6 | `build_field2d` 後半: `emit_dots` / `emit_highlights` / `emit_labels` / `emit_markers` / `emit_overlay`。 | 3h | T5 |
| T7 | `emit_step_grid` + グリッド間引き LOD。 | 2h | T5 |
| T8 | `metrics.rs`: `FontMetrics` 定数表（SANS / MONO）+ `measure` + `line_height` + 単体テスト。**T1 の直後から並行可能**。 | 2h | T1 |
| T9 | ラベル LOD（occupancy 格子・選択優先・budget）+ ドット記号 LOD。 | 3h | T6, T8 |
| T10 | `emit_trails` + トレイル LOD。11番の `RouteShape` が未実装の間は直線 2 点のみ。 | 2h | T6 |
| T11 | `build_stadium3d`（`camera::project` 利用・深度ソート・視錐台外除去・距離半径）。 | 3h | T6 |

### フェーズ 3: バックエンド境界（T2 後・フェーズ 2 と並行可）

| ID | 内容 | 時間 | 依存 |
|---|---|---|---|
| T12 | `backend.rs`: `Backend` / `DrawCtx` / `Capability` / `render` / Arc テセレーション / 劣化処理。 | 3h | T2 |
| T13 | `golden.rs`: `write_golden` + エスケープ + 量子化 + `DRILL_GOLDEN_UPDATE` 対応のテストハーネス。 | 2h | T2 |
| T14 | ゴールデン 6 本の生成とレビュー。 | 2h | T6, T11, T13 |

### フェーズ 4: 移設（10 節の手順）

| ID | 内容 | 時間 | 依存 |
|---|---|---|---|
| T15 | `crates/drill-export` 新設。`drill-core/src/svg.rs` を `drill-export/src/html.rs`（表組み系）と `drill-export/src/svg.rs`（図系）へ分割移動。`drill-core/src/lib.rs:15` の `pub mod svg;` を削除。 | 2h | — |
| T16 | `SvgBackend` 実装（`drill-export`）。`field_svg` / `set_svg` を `build + SvgBackend` の薄いラッパへ差し替え、既存テスト 6 件を通す。 | 3h | T12, T15 |
| T17 | `EguiBackend` 実装（`drill-app/src/view/display_list.rs`）。 | 3h | T12 |
| T18 | `main.rs` 差し替え その1: `draw_field`(1617–1710) と 2D ドット/ラベル(1468–1497) を `build_field2d + EguiBackend` へ。`to_screen`(1468) を `FieldMap::map`、ヒットテストを `FieldMap::unmap` へ。 | 3h | T17, T9 |
| T19 | `main.rs` 差し替え その2: `draw_stadium`(292–414) を `build_stadium3d + EguiBackend` へ。カメラ入力処理(299–310)だけを残す。393 行の毎フレーム `Vec` 確保を除去。 | 3h | T18, T11 |
| T20 | `RasterBackend` スケルトン（RGBA 出力・塗り・線・円・記号のみ。テキストは 31番へ）。31番へ渡す境界の実体化。 | 3h | T12 |

### フェーズ 5: 検証

| ID | 内容 | 時間 | 依存 |
|---|---|---|---|
| T21 | property テスト 7 件。 | 3h | T14 |
| T22 | ストレス 4 件（確保ゼロ・2時間・4,000人・NaN）。 | 2h | T14 |
| T23 | ベンチ 5 本 + CI 上限 assert。 | 2h | T14 |
| T24 | メトリクス契約テスト（egui / PDF AFM）。 | 2h | T8, T17 |
| T25 | CI 依存検査（`cargo tree` / 禁止文字列 / 日本語リテラル）。 | 1h | T1 |

### 並行性

```
T1 ─┬─ T2 ─┬─ T12 ─┬─ T16(←T15) ─┐
    │      │       ├─ T17 ─ T18 ─ T19
    │      └─ T13 ─┘              │
    ├─ T3 ─ T5 ─┬─ T6 ─┬─ T9 ─────┤
    │           │      ├─ T10     │
    │           │      └─ T11 ────┤
    │           └─ T7              │
    ├─ T4 ──────┘                  │
    ├─ T8 ─────────────── T24      │
    └─ T25                         │
T15 ──────────────────── T16       │
                    T14 ─ T21/T22/T23
                                T20
```

3 人（3 エージェント）で回す場合の分割:
**A** = T1→T2→T12→T13→T20、**B** = T3→T5→T6→T7→T9→T10→T11、**C** = T4→T8→T15→T16→T17→T18→T19。
合流点は T14（ゴールデン）と T18/T19（アプリ差し替え）。

---

## 9. 未決事項

| # | 内容 | 決めるために必要なもの |
|---|---|---|
| U1 | `Symbol` の所有者。A-5 が `drill_core::Performer.symbol` を入れるまで `drill-render` が定義を持つ。二重定義期間をどう扱うか（`From` 実装で吸収するか、A-5 完了まで `drill-render` 側を `pub(crate)` にするか）。 | A-5（15番）の型確定 |
| U2 | 3D における線・トレイルの深度。現案は「静的ジオメトリは全ドットより下のレイヤ」という近似。空中を通る曲線ルートや、演者の手前を横切るプロップが出てきたときに破綻する。 | 22番（スタジアム）と 11番（ルート）の要求 |
| U3 | `Trail::taper` の色補間をバックエンド任せにしているため、egui（頂点色なし）と SVG（`linearGradient`）で見え方が異なる。`Trail` を N 本の `Line` に分解して α を段階的に変える案もあるが、コマンド数が 8 倍になる。 | 実装後の見た目比較 |
| U4 | 印刷（17番）の mm 単位と本書の view 単位の橋渡し。`Viewport::px_per_mm` 1 つで足りるか、それとも `Rect` に mm 版が要るか。 | 17番のページレイアウト設計 |
| U5 | wgpu バックエンド（21番）が `Layer::Dot` の連続範囲だけを見てインスタンシングできるか。現案では `layer(Layer::Dot)` が連続スライスなので 1 回の `draw_layer` で足りるはずだが、テクスチャアトラス（記号 6 種）の扱いは 21番の判断。 | 21番の設計 |
| U6 | `ui_scale` と egui の `pixels_per_point` の二重適用防止。現案は「egui バックエンドでは `ui_scale = 1.0` を必ず使う」という規約だが、規約より型で縛るべきか（`Viewport::for_egui()` / `Viewport::for_raster()` のコンストラクタのみ公開する等）。 | 43番の DPI 検証結果 |
| U7 | `FontMetrics::SANS` の実値。Helvetica AFM をそのまま使うか、Noto Sans JP / Segoe UI の実測平均を取るか。契約が 1.15 倍なので実害は小さいが、`TextFit::MaxWidth` の精度に効く。 | 42番（i18n）のフォント選定 |
| U8 | ゴールデンをバイナリ形式でも持つか。現案はテキストのみ（差分が読める・追加依存なし）。1,000 人のゴールデンが約 130 KB / 本 × 6 本 = 780 KB になるので、リポジトリ肥大が問題になれば `field2d_stress_1000` だけハッシュ比較へ落とす。 | リポジトリ運用の判断 |
| U9 | `draw_count_track`(main.rs:1712–1837) を将来 DisplayList 化するか。本書は「時間軸 UI ウィジェットであり、入力処理と一体なので 43番の担当」として除外した。ただし動画に「カウントトラック焼き込み」を入れたくなった時点で再考が要る。 | 31番の P1 要求 |
