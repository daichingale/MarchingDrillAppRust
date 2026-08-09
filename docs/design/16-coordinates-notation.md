# 16. 座標系と読み上げ・記譜規約

## 1. 目的と範囲

現在 `drill-core::coordinates` は「日本語1種類・フィールド規約1種類」を前提にハードコードされている
（詳細は「2. 現状」）。マーチングの座標読み上げには団体・流派ごとに複数の慣習があり、
DrillForge が Pyware 3D の上位互換を名乗るなら、これを選択可能にしなければならない。

この文書が設計するもの:

1. フィールド規約（`FieldStandard`）— 寸法・ハッシュ位置。既存 `GridConfig` との対応。
2. 座標読み上げ規約（`CoordinateNotation`）— どの基準線を使うか、どう言い回すか。
3. `Locale` との組み合わせによる文字列生成 API。`drill-core` から日本語リテラルを排除する具体的な移行手順。
4. `GridLine` の再設計（`label: String` → `kind: HashKind` + 表示名解決）。
5. 丸め規約（`StepRounding`）と、浮動小数の境界揺れを起こさない定式化。
6. yard/meter/step の相互変換と `StepStyle`（第12番文書）との境界。
7. 読み上げ文字列 → 座標のパース（逆変換）。
8. `coordinates_csv` / `performer_sheet` / `svg.rs` / `countsheet.rs` の出力規約への追従。

**この文書が扱わないこと**:

- `DrillError` / `Locale` 型そのものの定義（第42番文書の担当。ここでは `crate::Locale` が
  `enum { Ja, En }` として存在する前提で使い、必要な `DrillError` variant の追加要求だけを述べる）。
- `Edit` コマンド代数（第10番文書）。座標入力パースの戻り値をどう `Document` に適用するかは
  第10番の管轄で、ここでは「パースすると `Point` が手に入る」ところまでを設計する。
- `StepStyle`（8-to-5 等の歩幅語彙、第12番文書）そのものの定義。境界だけを明示する（6章）。
- コンティニュイティ文（`continuity.rs` の方向名 `右前`/`静止` 等）とカウントシートの文面
  （第17番文書の担当）。ただし両モジュールが `coordinates::readable` を呼ぶ契約は本書が定める。

## 2. 現状

`crates/drill-core/src/coordinates.rs`（131行がロジック本体、以降テスト）:

- `round_quarter`（16-18行）: `f32` を最も近い0.25単位に丸める。**丸め幅0.25が関数名にハードコード**されており、
  他団体の「最寄りのハーフステップ」規約を選べない。
- `side_to_side`（46-87行）: `"サイド1"` / `"サイド2"`（65-68行）、`"外側"` / `"内側"`（78行）、
  `"ヤードラインちょうど"`（73/75行）が日本語リテラル。基準ヤードラインは
  `major_line_interval` に丸めた最寄り整数ライン固定（56行）で、規約選択の余地がない。
- `front_to_back`（94-122行）: 基準線候補を `grid.hashes` の `label: String` から直接組み立て
  （97-103行）、`"フロントサイドライン"` / `"バックサイドライン"` がハードコード（98/102行）。
  `"前"` / `"後ろ"`（119行）も同様。最寄り線の選び方（105-113行、同着はリストの先着優先）は
  規約に依らず固定。
- `readable`（125-131行）: 区切り文字 `"、"`（127行）が日本語固定。
- `coordinates_csv`（159-181行）・`performer_sheet`（136-155行）: どちらも `side_to_side` /
  `front_to_back` / `readable` を直接呼ぶだけで、`notation` や `locale` を選ぶ引数が無い。

`crates/drill-core/src/lib.rs`:

- `GridLine`（34-38行）: `label: String` を持つ。表示名を**保存データそのもの**として持っており、
  ロケール切替が保存内容と絡んでしまう設計。
- `GridConfig::default`（57-86行）: `hashes` に `"フロントハッシュ"`（75行）/ `"バックハッシュ"`
  （79行）を直書き。ハッシュ位置は `20.0` / `28.0`（フィールド高さ53.333のうち、前サイドラインから
  20yd・後サイドラインから25.333yd）。
- `GridConfig::soccer`（99-115行）: `"センターライン"`（109行）を直書き。マーチング規格ではなく、
  サッカーピッチ用の独自プリセット（`FieldStandard` の対象外、3章で扱う）。
- `GridConfig::indoor`（88-97行）: 90×60、ハッシュ無し。WGI 実寸とは一致しない仮値（9章で扱う）。

`crates/drill-core/src/svg.rs`: `coordinate_sheet_html`（267行）・`drill_book_html`（308行）が
`crate::coordinates::readable(p, &doc.grid)` を呼ぶ。`field_svg`（176行）は `hash.label` を
直接描画する — `GridLine` を再設計すると**この行も追従が必要**（8章）。

`crates/drill-app/src/main.rs`: 885行目でインスペクタ表示に `coordinates::readable(pos, &self.document.grid)`
を呼ぶ。1103-1124行目でユーザーがカスタムハッシュを追加するUIが `GridLine { label: ..., .. }` を
直接組み立てている — `HashKind` 化に伴い変更が要る（8章）。

日本語文字列は無いが依存関係として押さえておくもの: `crate::Document`（`Point`, `GridConfig` を保持）、
`crate::pathing::path_length`（直線距離、丸めなしの生値を返す — 本書の `measure` はこれを使わず
`GridConfig` の水平/垂直ステップ長を軸ごとに使うため path_length とは独立）。

## 3. 設計

### 3.1 全体構成 — 「測る」と「言い表す」を分離する

現状の最大の問題は、数値計算（どのヤードラインか・何歩か）と文字列生成（どの言語でどう言うか）が
1つの関数の中で分かちがたく混ざっていることである。これを2段に分ける。

```
Point + GridConfig + StepRounding
              │
              ▼
      coordinates::measure()        ← 純粋・決定論的・ロケール非依存・丸めは整数で確定
              │
      CoordinateReading（数値のみ、Stringを含まない）
              │
              ▼
   coordinates::format_lateral() / format_depth() / readable()
              │  CoordinateNotation + Locale を受け取る
              ▼
            String
```

CSV・SVG・ドリルブック・画面表示は**全てこの2段を通る**。どのモジュールも
`(point.x - line) / step` のような軸計算を独自に書いてはならない（4章の不変条件1）。

### 3.2 丸め規約 `StepRounding` と `RoundedStep`

```rust
/// Organization-specific rounding granularity for a step readout. Governs
/// every axis (lateral and depth) identically within one `CoordinateNotation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepRounding {
    Eighth,   // 1/8 step — high-precision sheets
    Quarter,  // 1/4 step — current behavior, most common (DCI-style)
    Half,     // 1/2 step
    Whole,    // nearest whole step
}

impl StepRounding {
    pub const fn denominator(self) -> i32 {
        match self {
            StepRounding::Eighth => 8,
            StepRounding::Quarter => 4,
            StepRounding::Half => 2,
            StepRounding::Whole => 1,
        }
    }
}

/// A step measurement quantized at construction time to an integer numerator
/// over `rounding.denominator()`. Two `RoundedStep` values built from
/// "the same" real-world distance are guaranteed `==` even if the upstream
/// `f32` arithmetic that produced them differs by float noise — this is the
/// fix for "サイド1 45ヤード内側2歩" vs "…内側1.999歩" flicker: rounding
/// happens exactly once, here, before any formatting or comparison, and the
/// integer numerator (not a re-rounded float) is what every consumer stores
/// and compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RoundedStep {
    numerator: i32,
    rounding: StepRounding,
}

impl RoundedStep {
    /// `value_steps` is the true (unrounded) distance in steps. Ties at
    /// exactly `x.5` of the target granularity round away from zero; a small
    /// epsilon (`1e-3` step, far below any meaningful drill precision) is
    /// folded in on the side of `value_steps`' own sign so that float error
    /// introduced by upstream subtraction/division cannot flip which integer
    /// a value rounds to depending on which direction that error happened to
    /// point.
    pub fn from_steps(value_steps: f32, rounding: StepRounding) -> Self {
        const EPS: f32 = 1e-3;
        let denom = rounding.denominator() as f32;
        let scaled = value_steps * denom;
        let biased = scaled + EPS.copysign(if scaled == 0.0 { 1.0 } else { scaled });
        Self {
            numerator: biased.round() as i32,
            rounding,
        }
    }

    pub fn as_f32(self) -> f32 {
        self.numerator as f32 / self.rounding.denominator() as f32
    }
    pub fn is_zero(self) -> bool {
        self.numerator == 0
    }
    pub fn abs(self) -> Self {
        Self { numerator: self.numerator.abs(), rounding: self.rounding }
    }
    pub fn signum(self) -> i32 {
        self.numerator.signum()
    }
}
```

`round_quarter` (coordinates.rs:16-18) is replaced by `RoundedStep::from_steps(_, StepRounding::Quarter)`
throughout; the free function is deleted so no call site can silently skip quantization.

### 3.3 `HashKind` と `GridLine` の再設計

```rust
/// The *role* of a horizontal reference line, independent of display text.
/// Canonical roles get a locale-resolved name (`hash_kind_label`); `Custom`
/// carries a user-authored literal string — the same pattern already used
/// for `Performer::label` / `Set::name`, which are user data, not app i18n,
/// so they are exempt from the "no Japanese literal in drill-core" rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HashKind {
    FrontSideline,
    BackSideline,
    FrontHash,
    BackHash,
    /// User-named reference line (e.g. a soccer center line, a third indoor
    /// guide line). `name` is stored verbatim and never translated.
    Custom { name: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridLine {
    pub position: f32,
    pub kind: HashKind,
    pub weight: f32,
}

/// Resolve a canonical `HashKind` to display text. `Custom` short-circuits to
/// its stored name regardless of `locale` (it is user content).
pub fn hash_kind_label(kind: &HashKind, locale: Locale) -> String {
    match kind {
        HashKind::Custom { name } => name.clone(),
        HashKind::FrontSideline => match locale {
            Locale::Ja => "フロントサイドライン",
            Locale::En => "front sideline",
        }
        .to_string(),
        HashKind::BackSideline => match locale {
            Locale::Ja => "バックサイドライン",
            Locale::En => "back sideline",
        }
        .to_string(),
        HashKind::FrontHash => match locale {
            Locale::Ja => "フロントハッシュ",
            Locale::En => "front hash",
        }
        .to_string(),
        HashKind::BackHash => match locale {
            Locale::Ja => "バックハッシュ",
            Locale::En => "back hash",
        }
        .to_string(),
    }
}
```

This satisfies convention 00's rule 7 ("人間可読テキストを返す関数は `Locale` を引数に取る") exactly:
`hash_kind_label` *is* the Japanese-literal-containing function, but it is Locale-branched, not
Locale-blind — the distinction DESIGN_GAPS A-6 draws. `GridLine` itself now stores zero display text
for the canonical cases, so switching the UI language changes nothing about the saved document (see
invariant 3 in 4章).

### 3.4 `FieldStandard` — dimensions and hash placement

```rust
/// A named field-marking standard. Governs field size and hash placement
/// only — step size (8-to-5 vs 6-to-5) is `StepStyle`'s concern (docs/design/12,
/// see 6章 for the boundary): the same `FieldStandard` can be marched at any
/// `StepStyle`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldStandard {
    /// NFHS high-school football: 100 x 53⅓ yd. Hash placement below is the
    /// commonly cited 53'4" (17⅓ yd) from each sideline; verify against the
    /// current NFHS rule book before shipping this as a competition-accurate
    /// preset (see 9章 未決事項).
    HighSchool,
    /// NCAA college football: hashes 60 ft (20 yd) from each sideline,
    /// symmetric.
    College,
    /// The convention already hardcoded in `GridConfig::default()` today
    /// (front hash 20 yd from front sideline, back hash 28 yd — i.e. 25⅓ yd
    /// from the back sideline). Kept as its own named variant instead of
    /// silently redefining `default()`'s numbers, since existing saved
    /// documents and golden tests depend on them.
    Dci,
    /// WGI-style indoor floor. Numbers TBD — see 9章; currently maps to the
    /// existing placeholder `GridConfig::indoor()`.
    Indoor,
    /// No named standard: the `GridConfig` was hand-edited or came from a
    /// non-marching preset (e.g. `GridConfig::soccer()`, which stays outside
    /// this enum — it targets an exhibition field, not a marching-band
    /// standards body).
    Custom,
}

impl FieldStandard {
    /// Build the `GridConfig` this standard implies. Callers that want to
    /// preserve an existing document's positions when switching standards go
    /// through `Document::replace_grid` (lib.rs:335-347), unchanged by this
    /// document.
    pub fn grid_config(self) -> GridConfig {
        match self {
            FieldStandard::HighSchool => GridConfig {
                hashes: vec![
                    GridLine { position: 17.778, kind: HashKind::FrontHash, weight: 1.0 },
                    GridLine { position: 35.556, kind: HashKind::BackHash, weight: 1.0 },
                ],
                ..GridConfig::default()
            },
            FieldStandard::College => GridConfig {
                hashes: vec![
                    GridLine { position: 20.0, kind: HashKind::FrontHash, weight: 1.0 },
                    GridLine { position: 33.333, kind: HashKind::BackHash, weight: 1.0 },
                ],
                ..GridConfig::default()
            },
            FieldStandard::Dci => GridConfig::default(),
            FieldStandard::Indoor => GridConfig::indoor(),
            FieldStandard::Custom => GridConfig::default(),
        }
    }
}
```

`GridConfig::default()` / `::indoor()` / `::soccer()` keep their current signatures (no argument);
`FieldStandard::grid_config` is additive. Both `default()` and `soccer()` are updated in place to use
`HashKind` instead of `label: String` (8章 migration step 1); their *numeric* hash positions and field
dimensions are unchanged, so no golden numeric output changes — only the `GridLine.kind` field replaces
`GridLine.label`.

### 3.5 `CoordinateNotation` — how a measurement is phrased

```rust
/// Which reference line front-to-back distance is measured against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrontBackReference {
    /// Nearest of {front sideline, every hash, back sideline} — today's
    /// behavior (coordinates.rs:97-113).
    NearestLine,
    /// Nearest *hash* only; sidelines are considered solely when the grid has
    /// no hashes at all. Common on sheets that never reference sidelines.
    NearestHash,
    /// Always this specific line, regardless of which is closer — e.g.
    /// "12 steps behind front hash" always names the front hash even when a
    /// point sits closer to the back hash.
    Fixed(HashKind),
}

/// Phrasing style for the zero-distance ("exactly on the line") case.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnLineStyle {
    /// "Side 1 45 yard line exactly" / "サイド1 45ヤードラインちょうど" —
    /// matches the current implementation's wording, byte for byte.
    Explicit,
    /// "On the Side 1 45" / "サイド1 45ちょうど" — terser sheets.
    Short,
}

/// A complete readout convention: which reference line to prefer, how to
/// phrase the on-line case, and the rounding granularity to use. `Locale`
/// (Ja/En) is orthogonal and supplied separately at format time — the same
/// `CoordinateNotation` renders in either language.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinateNotation {
    pub front_back: FrontBackReference,
    pub on_line: OnLineStyle,
    pub rounding: StepRounding,
}

impl CoordinateNotation {
    /// Exactly today's behavior. Existing tests in coordinates.rs pin this.
    pub const fn standard() -> Self {
        Self { front_back: FrontBackReference::NearestLine, on_line: OnLineStyle::Explicit, rounding: StepRounding::Quarter }
    }
    /// "12 steps behind front hash" style, common on DCI-influenced drill
    /// books that always anchor on the front hash.
    pub const fn front_hash_anchored() -> Self {
        Self { front_back: FrontBackReference::Fixed(HashKind::FrontHash), on_line: OnLineStyle::Short, rounding: StepRounding::Quarter }
    }
    /// Terse hash-only phrasing with half-step rounding, seen on some indoor
    /// (WGI) sheets.
    pub const fn indoor_terse() -> Self {
        Self { front_back: FrontBackReference::NearestHash, on_line: OnLineStyle::Short, rounding: StepRounding::Half }
    }
}
```

`FieldStandard` and `CoordinateNotation` are independent axes on purpose: a College-marked field can be
read out in `front_hash_anchored` style and vice versa. The app UI is free to offer a combined preset
picker (e.g. "DCI style" = `FieldStandard::Dci` + `CoordinateNotation::front_hash_anchored()`), but that
pairing lives in `drill-app`, not `drill-core`.

### 3.6 `CoordinateReading` — the locale-oblivious measurement

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side { Side1, Side2, OnCenter }

/// The complete numeric measurement of a `Point` against a `GridConfig`,
/// under one `CoordinateNotation`. Contains no `String`; every formatter in
/// this module (and every other module — 8章) derives its text from one of
/// these, never from `Point`/`GridConfig` directly.
#[derive(Clone, Debug, PartialEq)]
pub struct CoordinateReading {
    pub side: Side,
    pub yard_line: u16,
    pub lateral_steps: RoundedStep,   // sign: + outside, - inside; 0 => on the line
    pub reference: HashKind,
    pub depth_steps: RoundedStep,     // sign: + behind (larger y), - in front; 0 => on the line
}

/// Measure `point` against `grid` under `notation`. Pure, deterministic,
/// alloc-free (no `String`s built here — `HashKind::Custom`'s `name` is
/// cloned only if formatted later, not here... actually `reference` clones
/// the matched `GridLine`'s `kind`, which for `Custom` does allocate; callers
/// building thousands of readings per frame should avoid `Custom` hashes in
/// hot paths, or cache the reading — see 5章).
pub fn measure(point: Point, grid: &GridConfig, notation: CoordinateNotation) -> CoordinateReading {
    // 1. Lateral axis: nearest yard line snapped to major_line_interval
    //    (unchanged from coordinates.rs:56), converted to Side + yard number
    //    + signed RoundedStep exactly as today (coordinates.rs:60-68), but
    //    quantized via RoundedStep::from_steps(_, notation.rounding) instead
    //    of round_quarter.
    // 2. Depth axis: candidate reference lines are selected per
    //    notation.front_back:
    //      NearestLine  -> {front sideline, grid.hashes.., back sideline}, nearest wins (ties: earlier entry)
    //      NearestHash  -> grid.hashes if non-empty, else same as NearestLine
    //      Fixed(kind)  -> the single GridLine whose kind == `kind`, or (if
    //                      absent) fall back to NearestLine so a document
    //                      missing a hash never panics or produces no reading
    //    signed RoundedStep as today (coordinates.rs:115-121), quantized via
    //    notation.rounding.
    unimplemented!("see algorithmic description above; ports coordinates.rs:46-122 verbatim except for quantization and reference selection")
}

/// Format the lateral half, e.g. "Side 1 45 yard line, 2 steps outside" /
/// "サイド1 45ヤードラインの外側に2歩".
pub fn format_lateral(reading: &CoordinateReading, notation: CoordinateNotation, locale: Locale) -> String { .. }

/// Format the depth half, e.g. "2 steps behind front hash" /
/// "フロントハッシュの2歩後ろ".
pub fn format_depth(reading: &CoordinateReading, notation: CoordinateNotation, locale: Locale) -> String { .. }

/// Full readable coordinate: both halves joined by a locale-appropriate
/// separator (En: ", "; Ja: "、" — unchanged from today, just no longer
/// hardcoded at the call site: it is chosen inside this function from
/// `locale`).
pub fn readable(point: Point, grid: &GridConfig, notation: CoordinateNotation, locale: Locale) -> String {
    let reading = measure(point, grid, notation);
    let sep = match locale { Locale::Ja => "、", Locale::En => ", " };
    format!("{}{}{}", format_lateral(&reading, notation, locale), sep, format_depth(&reading, notation, locale))
}
```

`side_to_side` and `front_to_back` as *public, standalone* functions are removed — the concern they
represented (get half a reading without building the other half) is served by calling `measure` once and
then only the one `format_*` function needed, which is strictly cheaper and removes the risk of the two
halves being computed via diverging code paths.

## 4. 不変条件

1. **単一計算経路**: `Point` と `GridConfig` から歩数を導く算術（`(pos - line) / step`）は
   `coordinates::measure` の中にしか存在しない。`svg.rs`・`countsheet.rs`・`drill-app` のどこにも
   同じ計算を再実装しない。テストで検証可能な形は7章のgrep回帰テスト。
2. **丸めは一度だけ**: `RoundedStep` は `measure` の内部で一度構築されたら、以降の全ての
   フォーマッタ・比較・CSV出力はその整数 `numerator` を再利用する。`f32` に戻す
   (`as_f32`) のは最終的な数値列（CSVのx,y生値など、歩数ではない列）を出すときだけ。
   これにより「1.999歩」表記は構造的に発生しない — 丸めた**後**の整数だけが流通する。
3. **保存内容とロケールの分離**: `Document`（`GridConfig` を含む）は `HashKind::Custom` の
   `name` 以外、一切の表示文字列を持たない。`Locale` と `CoordinateNotation` は関数引数としてのみ
   存在し、`Document` のどのフィールドにも保存されない。日本語UIで開いて英語UIで保存し直しても
   バイト単位で同じ `GridConfig`/`Point` が書き出される。
4. **`FieldStandard` は幾何のみ、`CoordinateNotation` は言い回しのみ**: 両者は独立した型であり、
   互いのバリアントを参照しない（`Fixed(HashKind)` が `HashKind` を参照するのは許容 — `HashKind` は
   幾何側の語彙であり、`GridLine.kind` と同じ型を再利用しているだけ）。
5. **`measure` は `grid.hashes` が空でもパニックしない**: `FrontBackReference::Fixed` が指す
   `HashKind` がグリッドに存在しない場合、`NearestLine` にフォールバックする（3.6節）。
6. **既定値の後方互換**: `CoordinateNotation::standard()` は既存 `coordinates.rs` の
   全既存テスト（193-321行）をそのまま通す。回帰なしに規約選択機能を追加する。

## 5. 性能

基準規模（演者1,000人・セット64）での想定利用箇所:

- **画面表示（インスペクタ、main.rs:885相当）**: 選択中の演者1人・1セットのみ
  `readable` を呼ぶ。1回あたり `measure` + 2回の `format_*` で `String` 2〜3個の確保。
  60fpsで**選択中の1人だけ**呼ぶ前提なら16.6ms予算に対して無視できる（他の描画コストに対し
  1マイクロ秒未満）。**演者ごとに毎フレーム呼んではいけない** — DisplayList（第20番文書）の
  座標描画は生の `Point` を使い、この文字列APIを経由しない。
- **CSV/HTML一括出力（`coordinates_csv`, `performer_sheet`, `coordinate_sheet_html`,
  `drill_book_html`）**: 1,000人×64セット=64,000回の `measure` 呼び出し。各回は浮動小数演算
  数十回＋整数化1回、`HashKind` のクローン（`Custom` 以外はコピー相当）。実測見積り: 64,000回×
  約200ns = 13ms。書き出しはB-3（Job基盤、非同期）の対象であり、UIスレッドを塞がない
  （00-conventions.mdの性能予算はUIフレームに対するものであり、バックグラウンドJobには直接適用
  されないが、進捗表示のため1バッチ=数百件単位でチャンクすることを推奨）。
- **`HashKind::Custom` のクローンコスト**: `measure` が `reference: HashKind` をコピーする際、
  `Custom { name }` は `String` クローンが発生する。カスタムハッシュを使う document で
  64,000回のCSV出力を行うと文字列アロケーションが積み重なる。対策として `format_depth` 側だけが
  実際に文字列を必要とするので、`CoordinateReading.reference` を `HashKind` の**参照**
  (`&'a HashKind`) に変えるライフタイム付き版 `measure_ref` を高頻度パス向けに用意してもよいが、
  P0では素直な所有版で十分（13ms全体のうち文字列クローンが占める割合はカスタムハッシュ2本程度なら
  数%未満）。

## 6. 失敗モードと安全性

- **信頼できない入力（インポートしたv1ドキュメント）**: `GridLine.label` → `HashKind` への
  移行（8章）で、既知の文字列（"フロントハッシュ" 等）以外は全て `HashKind::Custom { name }`
  にフォールバックする。未知の文字列で panic しない。長さ上限（例: 256バイト）を超える `label`
  は切り詰めて `Custom` 化する（信頼できない入力からの無制限文字列確保を防ぐ、00-conventions.md
  「サイズ上限」要件）。
- **`Fixed(HashKind)` が存在しない参照を指す**: パニックせず `NearestLine` にフォールバック
  （4章不変条件5）。ログや警告は `drill-app` 側の責務（本書は返り値のみ規定）。
- **NaN/Inf 座標**: `measure` は `Point.x`/`Point.y` が非有限値でも panic しないこと
  （`RoundedStep::from_steps` の `.round() as i32` は非有限入力に対し実装依存の飽和値になるが
  UBは起こさない）。ただし表示上意味のない値になるため、`measure` の前段（`Document::validate` /
  将来の `DrillError::InvalidGrid` 系）で非有限座標を拒否するのが正しい防御線であり、本書の関数は
  「壊れない」ことのみを保証する。
- **パース（7章）の失敗**: 不正・曖昧な入力は必ず `Err(DrillError::CoordinateParse(_))` を返す。
  `unwrap`/`expect`/添字パニックを使わない。

## 7. テスト計画

- **回帰**: 既存 `coordinates.rs` テスト（193-321行）を `CoordinateNotation::standard()` +
  `Locale::Ja` で完全再現する形に移植し、1文字も変えずに通す。
- **`RoundedStep` 境界値**: `1.999999f32` と `2.000001f32` を `StepRounding::Quarter` で
  丸めた結果が共に `numerator == 8`（=2.0歩）で一致することを property test で検証
  （`proptest` は使わず、手動で境界値近傍を±1e-4刻みで列挙するテーブル駆動テストで足りる —
  drill-core の依存は serde のみという制約を守る）。
- **`Locale` 切替が `Document` を変えない**: `Document::demo` を `to_json()` → `Locale::En` で
  `readable` を何度呼んでも `to_json()` の出力が変わらないことを assert。
- **`HashKind::Fixed` フォールバック**: `grid.hashes` が空の `GridConfig` に対し
  `CoordinateNotation::front_hash_anchored()` で `measure` を呼び、`NearestLine` 相当の結果
  （フロント/バックサイドライン基準）になることを確認。
- **ゴールデン比較**: 同一 `Document` に対し `coordinates_csv` の `side_to_side`/`front_to_back`
  列と `drill_book_html` に埋め込まれた文字列が同一の `measure` 結果から生成されていること —
  同じ `(performer, set)` の組について両出力の対応するテキストが完全一致することを assert
  （8章の「単一経路」不変条件の直接検証）。
- **grepベースの回帰ガード**: CIスクリプト（`xtask` かテストの一部）で
  `crates/drill-core/src` と `crates/drill-app/src` を対象に、`coordinates.rs` 以外のファイルで
  `.horizontal_units` や `.vertical_units` を `/` や `-` と組み合わせて使っている箇所が無いかを
  grep する軽量チェックを追加し、不変条件1の再発を機械的に防ぐ。
- **パースの往復**: `readable(point, grid, notation, locale)` の出力を
  `parse_coordinate(_, grid, notation, locale)` に通し、`RoundedStep` 精度の範囲で元の `point`
  に戻ることを標準規約・両ロケールで検証（7章）。

## 8. 実装タスク

依存関係: 1→2→(3,4)→5、6・7は3完了後に並行可能。1タスク=1〜3時間相当。

1. **`HashKind` 導入と `GridLine` 移行**（lib.rs:34-38, 57-86, 88-97, 99-115）:
   `label: String` を `kind: HashKind` に置換。`default()`/`indoor()`/`soccer()` の3箇所を
   `HashKind::FrontHash`/`BackHash`/`Custom{name:"センターライン"}` に更新
   （`soccer()` はマーチング規格外なので `Custom` のまま — 3.4節）。
   `main.rs:1103-1124` のカスタムハッシュ追加UIを `HashKind::Custom` 構築に更新。
   `svg.rs:176` の `hash.label` 参照を `hash_kind_label(&hash.kind, locale)` 呼び出しに変更
   （`field_svg`/`set_svg` のシグネチャに `locale: Locale` 追加が必要 — 破壊的変更、呼び出し元
   main.rs 側の書き出しパネルで固定ロケールを渡す）。
2. **`RoundedStep`/`StepRounding` 実装**（coordinates.rs 新規）: `round_quarter` を削除し置換。
3. **`FieldStandard` 実装**（coordinates.rs か lib.rs 新設 `field_standard.rs`、どちらに置くかは
   9章の未決事項）。
4. **`CoordinateNotation`/`CoordinateReading`/`measure`/`format_lateral`/`format_depth`/`readable`
   実装**（coordinates.rs 全面書き換え）: 3.6節のアルゴリズム記述を実装に落とす。
5. **呼び出し元の追従**: `coordinates_csv`/`performer_sheet` に `notation`/`locale` 引数を追加し、
   ヘッダーを `performer,label,set,counts,x,y,side,yard_line,lateral_steps,reference,depth_steps,
   side_to_side,front_to_back` に拡張（数値列を追加、既存テキスト列は維持）。
   `coordinate_sheet_html`/`drill_book_html`（svg.rs:247-322）と main.rs:885 に
   `notation`/`locale` 引数を配線（アプリ側のデフォルトは `CoordinateNotation::standard()` +
   実行時UI言語設定）。
6. **v1→v2マイグレーションへの接続**（第A-7/schema章と協調): `GridLine.label` (v1) →
   `HashKind`(v2) の写像を `migrate_v1_to_v2` に追加。既知文字列4種（フロント/バックハッシュ、
   フロント/バックサイドライン、センターライン）をマップ、それ以外は `Custom{name}`。
7. **逆変換パーサ実装**（7章のアルゴリズム記述を実装、`DrillError::CoordinateParse` variant
   追加は第42番文書側との協調が必要 — 本書はvariant名と意味だけを要求として渡す）。

## 9. 未決事項

- `FieldStandard::HighSchool`/`College` の正確な寸法値は本書の執筆にあたり一般に流布している
  概算値を用いた。競技用の正式な数値として販売前に一次資料（NFHS/NCAA規則書）で裏取りが必要。
- `FieldStandard::Indoor` の実寸（現状 `GridConfig::indoor()` の 90×60、単位未確認）は WGI の
  公式フロア規格と照合されていない。第22番（3Dスタジアム）・第16番のどちらが実測値を持つべきかも
  未確定。
- `FieldStandard` の実装先クレート内配置（`coordinates.rs` に同居させるか、
  `field_standard.rs` を新設するか）は未確定。モジュール分割の粒度は実装時に判断してよい。
- `parse_coordinate` の文法（7章）はキーワード集合を英語・日本語それぞれで確定していない。
  特に日本語の助詞ゆらぎ（「の外側に」「外側」「外」等の表記ゆれをどこまで許容するか）は
  ユーザーテストで決めるべきで、本書は骨格アルゴリズムのみを規定する。
- `HashKind::Custom` を持つ `GridLine` が複数・同名で存在する場合の一意性（`Fixed(HashKind)`
  で `Custom{name}` を指定したときのマッチ規則）は未規定。現状案は名前の完全一致だが、
  UIでの選択体験（ドロップダウンでどう見せるか）と合わせて決める。
- CSVスキーマ拡張（8章タスク5）は既存の `csv_has_header_and_one_row_per_pair` テストを壊す
  破壊的変更である。第41番（永続化）または第52番（相互運用）文書とスキーマバージョニングの
  方針をすり合わせる必要がある。
