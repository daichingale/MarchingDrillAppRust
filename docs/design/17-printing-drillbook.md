# 17. カウントシート・コンティニュイティ・ドリルブック・印刷

## 1. 目的と範囲

### 解決する問題

現状の帳票出力は `crates/drill-core/src/svg.rs` が HTML 文字列を組み立て、利用者がブラウザの
「印刷してPDFに保存」機能を使う方式である。これには構造的な限界がある。

- ページサイズ・余白・改ページ位置をアプリが制御できない（ブラウザ・OS・プリンタドライバ依存）。
- ヘッダ/フッタ・ページ番号を差し込めない。
- 表が2ページにまたがるとき、ヘッダ行の再掲や「1行を割らない」制御ができない。
- 日本語フォントのサブセット化・埋め込みを制御できない（ブラウザの印刷PDFはシステムフォント参照や
  フルフォント埋め込みになりがちで、配布先の環境に依存する）。
- 出力がブラウザ・OSのバージョンに依存し、`PRODUCT_QUALITY.md`「座標表、ドリルブック、SVG/PDF出力は
  同じドキュメント座標を参照する」の**バイト単位の再現性**を証明できない。

紙の成果物（ドリルブック・カウントシート・セット別チャート）はマーチング指導の現場で最も
日常的に使われる成果物であり、ここの品質が「販売可能な水準」の評価に直結する
（`PRODUCT_QUALITY.md` P1「PDFチャート、演者別ドリルブック、印刷プレビュー」）。

### 本書が設計するもの

1. 帳票の種類ごとの仕様（演者別ドリルブック・セット別チャート・カウントシート・
   プロダクションシート・セクション別サマリ・難易度レポート）。
2. `crates/drill-export` に置く軽量ページレイアウトエンジン（ページサイズ・余白・
   ヘッダ/フッタ・ページ番号・改ページ規則・行あふれ処理）。既存 HTML 出力との関係。
3. ネイティブ PDF 生成クレートの選定（実地調査つき）、日本語フォント（`assets/NotoSansJP.ttf`）の
   埋め込み・CID フォント・サブセット化。
4. フィールド図描画と [20-display-list.md](20-display-list.md) の `DisplayList` との境界。
5. リハーサル記号・小節番号・音楽対応表の拡張。
6. 1,000人規模の一括ドリルブック出力のジョブ化・進捗・キャンセルと、時間/メモリ/ページ数の見積り。
7. 印刷プレビューUIの要件（[43-app-architecture.md](43-app-architecture.md) へ渡す仕様）。
8. 決定論的な出力とゴールデンテストの方法。

### 本書が扱わないこと

| 除外 | 担当 |
|---|---|
| フィールド図の描画コマンド生成そのもの（`DrawCmd` / `Layer` / `build`） | 20 |
| 座標読み上げの語彙・規約（`CoordinateNotation` / `FieldStandard`） | 16 |
| `Locale` / `DrillError` の型定義そのもの | 42 |
| 歩幅難易度スコアの算出アルゴリズム（`DrillDifficulty`） | 12（本書は結果を消費するだけ） |
| 衝突・間隔解析アルゴリズム（`TransitionScan` / `SpacingReport`） | 13（本書は結果を消費するだけ） |
| `Job<T>` の実体そのもの | 40（未執筆。本書は `DESIGN_GAPS.md` B-3 のスケッチを暫定契約として使う） |
| 動画書き出し（フレーム列のラスタライズ） | 31 |
| 印刷プレビューUIのウィジェット実装（ボタン配置・入力処理） | 43（本書は要件のみを渡す） |

## 2. 現状

### 2.1 帳票生成コード

`crates/drill-core/src/svg.rs`（全401行、`pub mod svg;` は `lib.rs:15`）:

| 行 | 内容 |
|---|---|
| 66–214 | `field_svg(doc, set_index, width_px, height_px)`。フィールド・グリッド・ハッシュ・演者ドットを独自にSVG文字列として組み立てる。**`DisplayList` を経由しない独立実装**（20番の課題そのもの）。 |
| 218–228 | `set_svg` — `field_svg` の薄いラッパ。 |
| 231–242 | `print_style()` — インライン `<style>` によるブラウザ印刷用CSS（`page-break-before:always` など）。 |
| 247–281 | `coordinate_sheet_html(doc)` — 全演者×全セットの座標を1枚のHTML表にする。ページ制御なし。 |
| 286–322 | `drill_book_html(doc)` — 演者ごとに `<div class="section page">` で区切り、CSSの `page-break-before` に改ページを委ねる。**「1行を割らない」「ヘッダ行を次ページで再掲」制御は無い**（テーブルが長ければブラウザ任せで途中から切れる）。 |

`crates/drill-core/src/countsheet.rs`（既読、全183行）: `rehearsal_mark`（18–30行、二重基数26の記号列）、`SetTiming`（33–51行）、`count_sheet`（57–78行）、`count_sheet_text`（90–127行、プレーンテキスト表）。**音楽キュー・リハーサル記号と歌詞/演出の対応表（プロダクションシート）に相当するものは無い**。

`crates/drill-core/src/continuity.rs`（既読、全300行）: `ContinuitySegment`（26–47行）、`performer_continuity`（162–169行）、`continuity_text`（174–184行）。演者1人・1遷移ごとの日本語の説明文を返すのみで、**帳票のページに配置する責務は持たない**（本来持つべきではない — 本書がその配置を行う）。

`crates/drill-core/src/coordinates.rs`（既読、全322行）: `readable` / `performer_sheet` / `coordinates_csv`。同上、数値と文字列を返すのみ。

### 2.2 存在しないもの

- `crates/drill-export` クレート（`Cargo.toml` の `members` に無い。20番文書 T15/T16 が新設を予定しているが未着手）。
- ページレイアウト概念（`PageSize` / `Margins` / ヘッダ・フッタ・ページ番号・改ページ規則）はリポジトリのどこにも無い。
- PDF 生成コードは皆無（`printpdf` / `pdf-writer` / `lopdf` / `krilla` いずれも `Cargo.toml` に無い）。
- 音楽キュー・演出注記（プロダクションシートの元データ）を保持する `Document` フィールドは無い
  （`Set` は `lib.rs:232-238` の `{ name, counts, positions }` のみ）。
- セクション（`Section` / `Performer.section`）は未実装（15番文書が設計中、`Performer` はまだ
  `{ id, label, color }` の3フィールドのみ — `lib.rs:225-230`）。「セクション別サマリ」は
  15番の型が着地するまで単一セクション（"Ensemble"）で代替する。
- 難易度スコア（`DrillDifficulty`）・衝突走査（`TransitionScan`）は12番・13番が設計済みだが
  実装コードは無い（`crates/drill-core/src/difficulty.rs` / `clinic.rs` は存在しない）。
- `Job<T>` 基盤（40番）は未執筆・未実装。`crates/drill-app/src/main.rs` の書き出しは全て同期実行
  （`DESIGN_GAPS.md` B-3）。

### 2.3 参照する既存の型（20番文書で確定済み）

`drill-render`（20番）が提供する以下は、本書がそのまま消費する既定契約である。

- `DisplayList` / `DrawCmd` / `Layer` / `LayerMask::PRINT`（20章 3.3, 3.4）
- `Backend` トレイトと `render(list, mask, backend)`（20章 3.10）
- `Scene` / `RenderOptions::print()` / `Theme::PRINT_LIGHT`（20章 3.5）
- `Viewport { size, ui_scale, px_per_mm }` — `px_per_mm` は「20番文書がフィールドを用意し、
  17番が値を決める」契約になっている（20章 3.4 doc comment、20章 9節 U4）。**本書が U4 に回答する**
  （3.5節）。
- `drill_render::metrics::FontMetrics`（20章 3.11）— テキスト幅の決定論的近似。本書はページ
  レイアウトの行高さ計算にもこれを再利用する（3.3節）。

---

## 3. 設計

### 3.1 帳票の種類と仕様

`crates/drill-export/src/report.rs` に置く `ReportKind` が全帳票の入口になる。

```rust
// crates/drill-export/src/report.rs

use drill_core::{Document, Locale, PerformerId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportKind {
    /// 演者1人 × 全セットの座標・カウント・コンティニュイティ。既定は表形式（3.1.1節）。
    PerformerDrillBook,
    /// セット1つ × 全演者のフィールド図。DisplayList をそのまま1ページに配置する（3.1.2節）。
    SetChart,
    /// 全セットのタイミング一覧（既存 `countsheet::count_sheet` の帳票化、3.1.3節）。
    CountSheet,
    /// 音楽キュー・リハーサル記号・視覚イベントの対応表（3.1.4節）。
    ProductionSheet,
    /// セクション単位の人数・カウントシート・ドリルブックの束（3.1.5節）。
    SectionSummary,
    /// 12番 `DrillDifficulty` と13番 `TransitionScan` を統合した難易度・クリニックレポート（3.1.6節）。
    DifficultyReport,
}
```

各帳票は「本文（`Vec<Block>`、3.3節）を組み立てる関数」として実装し、ページ配置・PDF/HTML化は
共通のレイアウトエンジンに委ねる。**帳票ごとに独自のページ送りロジックを書かない**（4節不変条件1）。

#### 3.1.1 演者別ドリルブック（`PerformerDrillBook`）

既定は**表形式**（現行 `drill_book_html` の後継）であり、フィールド図は含めない。理由は5.1節で
定量的に示す — 1,000人 × 64セットの全員にフィールド図付きページを配ると 64,000 ページになり、
実務的な配布物として成立しない。フィールド図が要る用途は「セット別チャート」（全員が同じ図を共有する）
が担う。

```rust
// crates/drill-export/src/report.rs（続き）

/// `PerformerDrillBook` の見た目のバリエーション。既定は `Compact`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrillBookStyle {
    /// 表のみ。セット名・カウント・小節:拍・座標・コンティニュイティを1行/セットで並べる。
    Compact,
    /// `Compact` に加え、各セットのフィールド図（その演者をハイライト）を追加する。
    /// ページ数が `Compact` の何倍にもなるため、一括出力では既定で無効（3.9節・9節）。
    Illustrated,
}

pub struct PerformerDrillBookParams {
    pub style: DrillBookStyle,
    pub beats_per_measure: u16,
    pub locale: Locale,
}

/// 1人分のドリルブック本文を組み立てる。`performer_index` が範囲外なら空の `Vec` を返す
/// （既存 `continuity_text` / `performer_sheet` の「範囲外は空文字列」規約を踏襲、4節）。
pub fn build_performer_drill_book(
    doc: &Document,
    performer_index: usize,
    params: &PerformerDrillBookParams,
) -> Vec<Block>;
```

`Compact` の内部構成（`Block`、3.3節の型）:

```
Heading  "T1 — Alice Smith"                      (演者ラベル + 表示名)
Table [
  header: ["Mark", "Set", "Count", "Meas:Beat", "Time", "Coordinate", "Continuity"]
  rows:   countsheet::count_sheet() の行 × coordinates::readable() × continuity::performer_continuity()
]
```

3つの既存関数（`count_sheet` / `readable` / `performer_continuity`）を1回ずつ呼んで zip するだけで、
新しい座標計算を一切増やさない（16番文書 4章不変条件1「単一計算経路」を継承する）。

`Illustrated` は `Compact` の各行の直後に `Block::FieldDiagram`（3.3節）を差し込む。3.5節で述べる
「共有 DisplayList + 軽量ハイライト」方式でのみ実用的なコストに収まる。

#### 3.1.2 セット別チャート（`SetChart`）

```rust
pub struct SetChartParams {
    pub locale: Locale,
    /// 既定 `RenderOptions::print()`（20章 3.5）。
    pub render_options: drill_render::RenderOptions,
}

/// 1セット分。ページ数 = セット数（演者数に依存しない — 3.1.1節との対比）。
pub fn build_set_chart(
    doc: &Document,
    set_index: usize,
    params: &SetChartParams,
) -> Vec<Block>;
```

構成:

```
Heading  "Set 3 — Diamond (rehearsal mark C)"
FieldDiagram (フル: 全演者ドット・ラベル・ヤード線・ハッシュ)
Table (任意): セット注記・カウント・到達タイミング
```

#### 3.1.3 カウントシート（`CountSheet`）

既存 `countsheet::count_sheet_text`（プレーンテキスト、90–127行）をページ化するだけの薄い皮。

```rust
pub fn build_count_sheet(doc: &Document, beats_per_measure: u16, locale: Locale) -> Vec<Block> {
    // Heading + 1 Table（ヘッダ行 + count_sheet() の行）。既存の列に加え、
    // 3.7節で拡張する rehearsal_mark 由来の小節番号レンジ列を追加する。
    unimplemented!()
}
```

#### 3.1.4 プロダクションシート（`ProductionSheet`）— 音楽と視覚の対応表

現状 `Document` は音楽キュー（歌詞・演出指示・効果のタイミング）を一切保持しない。本書はこれを
`countsheet.rs` への追加として提案する（3.7節）。

```rust
pub struct ProductionSheetParams { pub beats_per_measure: u16, pub locale: Locale }

pub fn build_production_sheet(
    doc: &Document,
    cues: &[drill_core::countsheet::ProductionCue],
    params: &ProductionSheetParams,
) -> Vec<Block>;
```

列: `Mark | Set | Meas:Beat | Time | Cue（歌詞・演出）| Visual（フォーメーション名）`。

#### 3.1.5 セクション別サマリ（`SectionSummary`）

15番文書の `Section` / `Performer.section` に依存する。**15番の型が未着地の間は暫定で
「単一セクション "Ensemble"」に全演者を割り当てて代替する**（15番の v1→v2 移行既定値と同じ扱い、
9節）。

```rust
/// 15番 `Section` 着地後のシグネチャ。未着地の間は `sections: &[]` を渡すと
/// 内部で "Ensemble" 1件に fallback する。
pub fn build_section_summary(
    doc: &Document,
    sections: &[drill_core::Section],   // 15番着地前は空スライスでよい
    locale: Locale,
) -> Vec<Block>;
```

構成: セクションごとに `Heading`（人数入り）+ `Table`（メンバー一覧: ラベル・ドリルナンバー）。
「セクションだけのカウントシート」（例: Trumpetのみ）は `PerformerDrillBook` を
`Selection::by_section`（15章 3.7）で絞り込んだ演者集合に対してループするだけで実現できるため、
本書は専用の計算を増やさない。

#### 3.1.6 難易度レポート（`DifficultyReport`）

12番 `ShowDifficulty` / `hardest_performers` / `continuous_movement_runs` と13番
`TransitionScan` / `CollisionEvent` / `Severity` を1本のレポートに統合する。**難易度・衝突の
計算そのものは行わない**（本書は結果を受け取って表にするだけ）。

```rust
pub struct DifficultyReportInput<'a> {
    pub show: &'a drill_core::difficulty::ShowDifficulty,
    pub runs: &'a [drill_core::difficulty::MovementRun],
    pub scans: &'a [drill_core::clinic::TransitionScan],   // セットごと、疎でよい（無い箇所はスキップ）
    pub top_n: usize,                                       // 既定 10
}

pub fn build_difficulty_report(
    doc: &Document,
    input: &DifficultyReportInput<'_>,
    locale: Locale,
) -> Vec<Block>;
```

構成: `Heading "Overall: 62/100"` + `Table`（最も過酷な演者トップN、12章 `hardest_performers`）+
`Table`（休みなく動き続ける区間、`continuous_movement_runs`）+ `Table`（セットごとの衝突件数、
`TransitionScan::contact_count`/`danger_count` を `Severity` 別に集計）。数値をそのまま転記するだけで、
新しい閾値判定は行わない（4節不変条件2）。

### 3.2 `crates/drill-export` の構成と依存

```toml
# crates/drill-export/Cargo.toml
[dependencies]
drill-core   = { path = "../drill-core" }
drill-render = { path = "../drill-render" }
krilla       = "0.8"     # 3.4節で選定
serde        = { workspace = true }   # ReportKind 等のパラメータをUI側で永続化する場合に使用
```

00-conventions.md のクレート境界表は「`drill-export`: SVG/PNG/PDF/動画/CSV。`drill-render` に依存」
とだけ書いており、20番文書 I11（「`drill-render` の依存は `drill-core` ただ1つ」）のような**単一依存の
機械検査は `drill-render` 専用**である。`drill-export` は本書の帳票データ（`Document` の
`countsheet`/`continuity`/`coordinates`/`difficulty`/`clinic` 出力）を直接扱う必要があるため、
`drill-core` と `drill-render` の**両方**に依存する。依存方向は両方とも「上から下」（00-conventions
の層表で `drill-export` は両方より下）であり、循環しない。

```
crates/drill-export/src/
  lib.rs
  page.rs      // PageSize / Margins / PageTemplate / Block / PageLayout（3.3節）
  fonts.rs     // FontAsset / サブセット化 / フォールバック鎖（3.6節）
  pdf.rs       // PdfBackend（drill_render::backend::Backend実装）/ PageCanvas（3.5節）
  html.rs      // 20章 T15 で svg.rs から移設される表組みビルダ（coordinate_sheet_html 等）の後継
  svg.rs       // 20章 T16 で移設される SvgBackend（field_svg の後継、本書は変更しない）
  report.rs    // 3.1節の ReportKind とビルダ群
  batch.rs     // 一括出力ジョブ（3.9節）
```

`html.rs` と `svg.rs` は20番文書が定めた移設先であり、本書はその中身（`SvgBackend` の実装）を
再定義しない。本書が新たに `html.rs` に足すのは、`report.rs` の `Block` 列を HTML の `<table>` に
落とす**シリアライザ**（3.3節末尾）だけである。

### 3.3 ページレイアウトエンジン

#### 3.3.1 ページ定義

```rust
// crates/drill-export/src/page.rs

/// mm 単位。`Custom` は将来の特殊媒体（横断幕など）向け。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PageSize {
    A4,
    Letter,
    Tabloid,
    Custom { width_mm: f32, height_mm: f32 },
}

impl PageSize {
    pub fn dims_mm(self) -> (f32, f32) {
        match self {
            PageSize::A4 => (210.0, 297.0),
            PageSize::Letter => (215.9, 279.4),
            PageSize::Tabloid => (279.4, 431.8),
            PageSize::Custom { width_mm, height_mm } => (width_mm.max(1.0), height_mm.max(1.0)),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Orientation { Portrait, Landscape }

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Margins { pub top_mm: f32, pub right_mm: f32, pub bottom_mm: f32, pub left_mm: f32 }

impl Margins {
    pub const fn uniform(mm: f32) -> Self { Self { top_mm: mm, right_mm: mm, bottom_mm: mm, left_mm: mm } }
}

/// ヘッダ/フッタの中身は `Block` ではなく専用の軽量トークン列にする。毎ページ呼ばれるため
/// `Document` 全体を渡さず、必要な値だけを渡す（4節不変条件3、性能）。
#[derive(Clone, Debug)]
pub struct HeaderFooterSpec {
    pub height_mm: f32,
    pub left: Vec<HeaderToken>,
    pub center: Vec<HeaderToken>,
    pub right: Vec<HeaderToken>,
}

#[derive(Clone, Debug)]
pub enum HeaderToken {
    Text(String),          // 文書タイトル・演者名など、呼び出し時に確定する文字列
    PageNumber,            // "3"
    PageCount,             // "12"（3.3.3節：2パスレイアウトで解決）
    GeneratedDate,         // 決定論のため呼び出し側が渡すタイムスタンプを表示（3.10節）
}

#[derive(Clone, Debug)]
pub struct PageTemplate {
    pub size: PageSize,
    pub orientation: Orientation,
    pub margins: Margins,
    pub header: Option<HeaderFooterSpec>,
    pub footer: Option<HeaderFooterSpec>,
}

impl PageTemplate {
    /// 版面（本文が使える矩形）の高さ。ヘッダ・フッタの高さを引く。
    pub fn content_height_mm(&self) -> f32 {
        let (_, h) = self.oriented_dims_mm();
        let header = self.header.as_ref().map_or(0.0, |h| h.height_mm);
        let footer = self.footer.as_ref().map_or(0.0, |f| f.height_mm);
        (h - self.margins.top_mm - self.margins.bottom_mm - header - footer).max(0.0)
    }
    pub fn content_width_mm(&self) -> f32 {
        let (w, _) = self.oriented_dims_mm();
        (w - self.margins.left_mm - self.margins.right_mm).max(0.0)
    }
    fn oriented_dims_mm(&self) -> (f32, f32) {
        let (w, h) = self.size.dims_mm();
        match self.orientation { Orientation::Portrait => (w, h), Orientation::Landscape => (h, w) }
    }
}
```

#### 3.3.2 `Block` — 本文の中間表現

`Block` は帳票の**内容**を表し、ページの何ページ目に乗るかを知らない（`PageLayout` の仕事、
3.3.3節）。HTML/PDF どちらのシリアライザも同じ `Block` 列を読む — これは20番文書が図に対して
`DisplayList` を単一の中間表現にした判断と対になる、表組み・見出し側の中間表現である。

```rust
#[derive(Clone, Debug)]
pub enum Block {
    Heading { text: String, level: u8 },           // level: 1=章題, 2=小見出し
    Paragraph { text: String },
    Table(TableBlock),
    /// 20章の `DisplayList` をそのまま1ブロックとして埋め込む。`aspect` はフィールドの
    /// 縦横比（`grid.height / grid.width`）— 実際の `DisplayList` はレイアウト確定後、
    /// 確保した矩形の寸法で `drill_render::build` を呼んで得る（3.5節、遅延評価）。
    FieldDiagram { scene_key: SceneKey, aspect: f32 },
    Spacer { height_mm: f32 },
    /// 明示的な改ページ。
    PageBreak,
    /// 内部を1つの塊としてページをまたがせない（3.3.3節の改ページ規則）。
    KeepTogether(Vec<Block>),
}

/// `FieldDiagram` が実際にどの `Scene` を描くべきかを指す軽量な鍵。`Block` 自体には
/// `Document` への参照を持たせない（`Block` は `'static` にして、バッチ処理でスレッド間を
/// 移動できるようにするため — 20章 I7 の `DisplayList` が `'static` であることと同じ理由）。
#[derive(Clone, Copy, Debug)]
pub struct SceneKey { pub set_index: usize, pub highlight_performer: Option<u32> }

#[derive(Clone, Debug)]
pub struct TableBlock {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// 既定 9.0pt 相当。行あふれ計算に使う（3.3.3節）。
    pub font_size_px: f32,
}
```

#### 3.3.3 `PageLayout` — 配置とページ送り

```rust
// crates/drill-export/src/page.rs（続き）

use drill_render::metrics::FontMetrics;

#[derive(Clone, Debug)]
pub struct PageContent {
    pub page_index: u32,
    /// 版面原点からのオフセット付きで確定した描画要素。
    pub placed: Vec<PlacedBlock>,
}

#[derive(Clone, Debug)]
pub struct PlacedBlock {
    pub y_mm: f32,
    pub height_mm: f32,
    /// テーブルは複数ページに割れるため、元の `TableBlock` の行範囲を持つ。
    pub content: PlacedContent,
}

#[derive(Clone, Debug)]
pub enum PlacedContent {
    Heading { text: String, level: u8 },
    Paragraph { text: String },
    TableSlice { header: Vec<String>, rows: std::ops::Range<usize>, source: std::rc::Rc<TableBlock> },
    FieldDiagram { scene_key: SceneKey, rect_mm: (f32, f32) }, // (width_mm, height_mm)
    Spacer,
}

pub struct PageLayout<'a> {
    template: &'a PageTemplate,
    cursor_mm: f32,
    pages: Vec<PageContent>,
    stats: LayoutStats,
}

#[derive(Clone, Copy, Default, Debug)]
pub struct LayoutStats {
    pub pages: u32,
    pub forced_overflows: u32,   // KeepTogether がページ丸ごとより大きく、はみ出しを許容した回数
}

impl<'a> PageLayout<'a> {
    pub fn new(template: &'a PageTemplate) -> Self {
        let mut s = Self { template, cursor_mm: 0.0, pages: Vec::new(), stats: LayoutStats::default() };
        s.pages.push(PageContent { page_index: 0, placed: Vec::new() });
        s
    }

    /// `Block` 列を順に配置する。1回の呼び出しでレイアウト全体を行う（差分レイアウトはしない —
    /// 帳票生成はホットパスではないため単純さを優先する、5節）。
    pub fn place_all(&mut self, blocks: &[Block]) {
        for block in blocks {
            self.place_one(block);
        }
    }

    fn remaining_mm(&self) -> f32 {
        (self.template.content_height_mm() - self.cursor_mm).max(0.0)
    }

    fn new_page(&mut self) {
        let index = self.pages.len() as u32;
        self.pages.push(PageContent { page_index: index, placed: Vec::new() });
        self.cursor_mm = 0.0;
        self.stats.pages = self.pages.len() as u32;
    }

    fn place_one(&mut self, block: &Block) {
        match block {
            Block::PageBreak => self.new_page(),
            Block::Spacer { height_mm } => self.push(PlacedContent::Spacer, *height_mm),
            Block::Heading { text, level } => {
                let h = heading_height_mm(*level);
                // 見出しの直後は最低でも1データ行が入る余地を残す（孤立見出し防止）。
                if self.remaining_mm() < h + MIN_FOLLOW_MM {
                    self.new_page();
                }
                self.push(PlacedContent::Heading { text: text.clone(), level: *level }, h);
            }
            Block::Paragraph { text } => {
                let h = paragraph_height_mm(text);
                if self.remaining_mm() < h { self.new_page(); }
                self.push(PlacedContent::Paragraph { text: text.clone() }, h);
            }
            Block::Table(table) => self.place_table(table),
            Block::FieldDiagram { scene_key, aspect } => self.place_diagram(*scene_key, *aspect),
            Block::KeepTogether(inner) => self.place_keep_together(inner),
        }
    }

    /// テーブルは行単位であふれる。ヘッダ行は割った先の各ページで再掲する。
    fn place_table(&mut self, table: &TableBlock) {
        let row_h = row_height_mm(table.font_size_px);
        let header_h = row_h * 1.15; // ヘッダは本文よりわずかに大きい行高を仮定
        let shared = std::rc::Rc::new(table.clone());
        let mut start = 0usize;
        while start < table.rows.len() || start == 0 {
            if self.remaining_mm() < header_h + row_h {
                self.new_page();
            }
            let capacity_rows = ((self.remaining_mm() - header_h) / row_h).floor().max(0.0) as usize;
            let end = (start + capacity_rows.max(1)).min(table.rows.len());
            let h = header_h + (end - start) as f32 * row_h;
            self.push(
                PlacedContent::TableSlice { header: table.header.clone(), rows: start..end, source: shared.clone() },
                h,
            );
            start = end;
            if start >= table.rows.len() { break; }
            self.new_page();
        }
    }

    /// フィールド図は幅いっぱい・アスペクト比維持で高さを決める。版面の残りに入らなければ
    /// 新しいページ全体を使う。それでも入らない場合（極端に細長いカスタム用紙）は、
    /// 上限までクランプしてはみ出しを記録する（パニックしない、6節F1）。
    fn place_diagram(&mut self, scene_key: SceneKey, aspect: f32) {
        let width = self.template.content_width_mm();
        let mut height = width * aspect;
        let full_page = self.template.content_height_mm();
        if height > self.remaining_mm() {
            if height <= full_page {
                self.new_page();
            } else {
                height = full_page; // クランプ
                self.stats.forced_overflows += 1;
                self.new_page();
            }
        }
        self.push(PlacedContent::FieldDiagram { scene_key, rect_mm: (width, height) }, height);
    }

    /// 塊がページ全体より大きい場合を除き、途中で割らない。
    fn place_keep_together(&mut self, inner: &[Block]) {
        let h = measure_all_mm(inner, self.template);
        if h > self.remaining_mm() && h <= self.template.content_height_mm() {
            self.new_page();
        }
        // h > content_height_mm() の場合は現在ページに置いてはみ出しを許す（6節F1）。
        for b in inner { self.place_one(b); }
    }

    fn push(&mut self, content: PlacedContent, height_mm: f32) {
        let y = self.cursor_mm;
        self.pages.last_mut().expect("at least one page").placed.push(PlacedBlock { y_mm: y, height_mm, content });
        self.cursor_mm += height_mm;
    }

    pub fn finish(self) -> (Vec<PageContent>, LayoutStats) { (self.pages, self.stats) }
}

const MIN_FOLLOW_MM: f32 = 6.0;

/// 行高さの決定論的近似。実フォントを問い合わせない（20章 3.11 と同じ理由 — バックエンド非依存・
/// ゴールデン安定）。`FontMetrics::line_height` を `px_per_mm`（3.5節）で mm に変換する。
fn row_height_mm(font_size_px: f32) -> f32 {
    FontMetrics::SANS.line_height(font_size_px) / POINTS_PER_MM + ROW_PADDING_MM
}
const ROW_PADDING_MM: f32 = 1.2;
const POINTS_PER_MM: f32 = 72.0 / 25.4; // 3.5節で定義する定数と同一。ここでは参照のみ。

fn heading_height_mm(level: u8) -> f32 { if level <= 1 { 10.0 } else { 7.0 } }
fn paragraph_height_mm(text: &str) -> f32 {
    // 折り返し行数を `FontMetrics` で概算（誤差はページ送り判定にのみ影響し、10%程度は許容 — 20章 3.11）。
    let _ = text;
    5.0 // 概算値。実装タスクで `FontMetrics::measure` を使った折返し行数計算に差し替える。
}
fn measure_all_mm(blocks: &[Block], template: &PageTemplate) -> f32 {
    // `PageLayout` を使わない静的な採寸パス。`place_*` と同じ関数を呼ぶことで二重実装を避ける
    // （テストで両者の一致を検証する、7節）。
    let mut probe = PageLayout::new(template);
    probe.place_all(blocks);
    probe.pages.iter().map(|p| p.placed.iter().map(|b| b.height_mm).sum::<f32>()).sum()
}
```

**ページ番号・総ページ数（`HeaderToken::PageCount`）の解決**: `PageLayout::place_all` は総ページ数を
確定させて初めて `PageCount` を書けるため、レイアウトは2パスで行う。1パス目でページ数を確定し、
2パス目（同じ入力・同じ関数なので決定論的に同じ結果になる — 4節不変条件4）でヘッダ/フッタの
`HeaderToken::PageCount` を実値に置換して最終 `Vec<PageContent>` を得る。1パス目の結果を破棄して
2パス目を素朴に再実行するのは、帳票生成が16.6ms予算の対象外（5節）であるための単純化であり、
1,000人規模でも2倍のレイアウトコストは無視できる。

**既存 HTML 出力との関係（判断）**: HTML はプロダクション実行のたびに `Block` 列を経由する薄い
シリアライザとして**残す**。廃止しない理由:

1. 依存ゼロで、PDF 生成が使えない環境（アセット未検証・krilla のビルド失敗時）でも動く
   フォールバックになる。
2. メール添付やクイックコピペなど、正式な印刷を伴わない用途では今のままで十分。

ただし今の `drill_book_html`/`coordinate_sheet_html` のような**独自のページ制御ロジックは持たせない**。
`html.rs` は `PageLayout` の出力（`Vec<PageContent>`）を `<div class="page">` 単位でそのまま
書き出すだけにする（改ページ判断はブラウザではなく本書のレイアウトエンジンが行う。ブラウザ印刷の
`page-break-before` CSS は「本エンジンが決めた改ページ位置を尊重させる」ためだけに使う）。これにより
HTML と PDF が**同じ位置で改ページする**（4節不変条件1の帰結）。

### 3.4 PDF生成: クレート選定

候補3つを実地調査した（2026年8月時点、`crates.io` / `docs.rs` / GitHub リポジトリを確認）。

| クレート | 抽象度 | フォントサブセット | CJK/CIDフォント | ライセンス | 備考 |
|---|---|---|---|---|---|
| **krilla** (0.8系) | 高レベル（fill/stroke/gradient/glyph/imageのプリミティブ） | `subsetter` クレートによる CFF/TTF 両対応のサブセット化 | CID-keyed フォント書き込みロジックを **Typst から移植**（Typst は CJK 組版で実運用されている） | **MIT OR Apache-2.0** | `pdf-writer`（同じ作者・同ライセンス）の上に構築。PDF/A-1〜4・PDF/UA-1 のエクスポートモードを検証済みと明記。90+ スナップショットテスト・6種のPDFビューアに対する210+の視覚回帰テストがあると主張。 |
| printpdf (0.12系) | 中レベル（要素を1つずつ追加するAPI） | 「フォントの自動サブセット化」を謳うが CJK/CID の扱いはドキュメント上明記なし | 不明瞭（`lopdf` の上に構築、作成専用） | MIT | ダウンロード数は3者中最多だが、抽象度が低く「表を配置する」レベルの機能を自前で組む必要がある。 |
| lopdf | 低レベル（PDFオブジェクトを直接組み立てる） | 無し（自前実装が必要） | 無し（CMap/CIDフォント辞書を手書きする必要がある） | MIT | `printpdf` の基盤。PDF仕様の詳細知識が要求され、CIDフォント埋め込みをゼロから実装するコストが高い。 |

**選定: krilla。**

根拠:

1. **CJK対応の系譜**が最も確からしい。CID フォント書き込みと PDF メタデータ出力のロジックを
   Typst（多言語・CJK組版を実運用しているタイプセッティングエンジン）から移植したと明記されており、
   ゼロから CMap/CID を書く lopdf 経由の2案より検証済みの実装に乗れる可能性が高い。
2. **サブセット化が一次機能**として提供され（`subsetter` クレート、CFF/TTFどちらの
   フレーバーにも対応）、9.6MBの `assets/NotoSansJP.ttf`（3.6節）を毎回まるごと埋め込む事態を
   避けられる。
3. **ライセンスが MIT OR Apache-2.0** で、`PRODUCT_QUALITY.md`/`ARCHITECTURE.md` が前提とする
   商用販売（`53-product.md` 想定のクローズドソース配布）と衝突しない。コピーレフト（GPL系）条項が
   無い。
4. **高レベルAPI**（fill/stroke/glyph/image のプリミティブ）により、本書が定義する `PageCanvas`
   抽象（3.5節）を薄く実装できる。lopdf のように PDF オブジェクトグラフを自前で管理する必要がない。
5. PDF/A・PDF/UA のエクスポートモードが検証済みと謳われており、将来アーカイブ用途
   （長期保存が必要な公演記録としてのPDF/A出力）を求められても同じクレート内で対応できる余地がある
   （今回はP0スコープ外だが依存追加なしで拡張可能というのは評価に値する）。

**留保（9節で「未決事項」として明記）**: 上記の CJK/CID 対応は krilla 自身のドキュメントに
「CJK」という単語で明記されているわけではなく、「Typst由来のCID実装」という間接的な根拠に基づく
判断である。`assets/NotoSansJP.ttf` を実際に埋め込み、Acrobat・ブラウザ内蔵PDFビューア・
プリンタドライバの複数環境で日本語グリフが正しく表示されるかを**実装着手前のスパイクタスク**で
検証することを実装タスク（8節 T3）の最初に置く。

`subsetter`（krilla の内部依存）は SID-keyed（CFF）フォントを CID-keyed に変換し、GID→CID の
恒等写像を作る、と明記されている。`NotoSansJP.ttf` は TTF（glyf）フレーバーであり CFF ではないが、
krilla は「CFF-flavored と TTF-flavored の両方でサブセット化に対応する」としているため、
`.ttf` のままサブセット埋め込みが可能という前提で設計する（この前提もスパイクタスクで検証する）。

### 3.5 `PdfBackend` と `PageCanvas`（`DisplayList` との境界）

**境界の原則**: フィールド図の**幾何**（線1本1本の位置・ドットの座標・色）は `drill-render` の
`build()` が決め、`drill-export` はそれを**紙面上のどこに置くか**（3.3節のページレイアウト）と
**どう描画命令に翻訳するか**（本節）だけを決める。20番文書 I9（「同じ `DisplayList` を2つの
バックエンドへ流したとき、両者が読む `Dot::center` は同一値」）が、SVG・PDF・画面表示の間で
図が一致することの直接の根拠であり続ける。**本書は `DrawCmd` の新しいバリアントを追加しない。**

krilla の正確な描画API（メソッド名）を本書執筆時点で確定させると、krilla の将来のマイナー
バージョンアップでの破壊的変更に本設計文書が縛られる。そこで `drill-export` 内に薄い自前の
抽象 `PageCanvas` を挟み、`PdfBackend` はこれだけに依存する形にする。krilla への実際の橋渡しは
1個の `impl PageCanvas for KrillaCanvas` に閉じ込める（実装タスクで確定、8節）。

```rust
// crates/drill-export/src/pdf.rs

use drill_render::{DrawCmd, Layer, Rgba, Vec2, backend::{Backend, DrawCtx, Capability}};

/// `drill-export` が要求する最小限の2Dベクタ描画面。`KrillaCanvas` がこれを krilla の
/// 実APIへ翻訳する。単位は常にPDFポイント（1/72インチ）— 3.5.1節で mm と対応づける。
pub trait PageCanvas {
    fn fill_path(&mut self, path: &Path, fill: Rgba);
    fn stroke_path(&mut self, path: &Path, stroke: Rgba, width_pt: f32, dash: Option<(f32, f32)>);
    /// グリフ列は `FontId` が指すサブセット化済みフォントの中から選ぶ（3.6節）。
    fn fill_text(&mut self, at: Vec2, text: &str, font: FontId, size_pt: f32, color: Rgba);
    /// テキストを指定幅へ収める水平スケール（PDFの `Tz` 演算子相当）。20章 3.11 のテキスト契約。
    fn fill_text_fit(&mut self, at: Vec2, text: &str, font: FontId, size_pt: f32, max_width_pt: f32, color: Rgba);
    /// 直前に描いた内容を再利用可能な部品として登録する（PDFの Form XObject 相当）。
    /// セット別チャートの共有フィールド図を、同一ドキュメント内の複数箇所や
    /// 複数出力ファイルへ低コストで複製するために使う（3.9節）。
    fn begin_reusable(&mut self) -> ReusableHandle;
    fn end_reusable(&mut self, handle: ReusableHandle);
    fn place_reusable(&mut self, handle: ReusableHandle, at: Vec2);
    fn new_page(&mut self, width_pt: f32, height_pt: f32);
}

#[derive(Clone, Copy, Debug)]
pub struct Path { pub points: Vec<Vec2>, pub closed: bool } // Vec は経路生成時のみ確保（3.5.2節）

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontId(pub u16);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReusableHandle(pub u32);

/// `drill_render::backend::Backend` の実装。`C: PageCanvas` を差し替えることで
/// 同じ変換ロジックを krilla 実装とテスト用モックの両方で使える。
pub struct PdfBackend<'c, C: PageCanvas> {
    canvas: &'c mut C,
    sans: FontId,
    mono: FontId,
    origin_pt: Vec2,   // ページ内でこの DisplayList が占める矩形の左上（3.5.1節）
}

impl<'c, C: PageCanvas> Backend for PdfBackend<'c, C> {
    type Error = std::convert::Infallible; // PageCanvas 側が確保失敗時にパニックしない設計にする（6節）

    fn begin(&mut self, _viewport: &drill_render::Viewport) -> Result<(), Self::Error> { Ok(()) }
    fn end(&mut self) -> Result<(), Self::Error> { Ok(()) }

    fn draw(&mut self, cmd: &DrawCmd, ctx: &DrawCtx<'_>) -> Result<(), Self::Error> {
        match *cmd {
            DrawCmd::FieldFill { rect, fill } => self.canvas.fill_path(&self.rect_path(rect), fill),
            DrawCmd::Rect { rect, fill, stroke, stroke_w, corner } => self.draw_rect(rect, fill, stroke, stroke_w, corner),
            DrawCmd::Line { a, b, width, color, dash } => self.canvas.stroke_path(
                &Path { points: vec![self.map(a), self.map(b)], closed: false },
                color, width, dash_pt(dash),
            ),
            DrawCmd::Dot { center, radius, symbol, fill, stroke, stroke_w, .. } => self.draw_dot(center, radius, symbol, fill, stroke, stroke_w),
            DrawCmd::Text { at, text, size_px, anchor, font, color, fit } => self.draw_text(at, ctx.text(text), size_px, anchor, font, color, fit),
            // Polyline / Trail / Arc / Highlight / Marker は同じ変換方針（点列→Path、記号→図形）で実装する。
            _ => {}
        }
        Ok(())
    }

    fn supports(&self, cap: Capability) -> bool {
        match cap {
            Capability::RoundedRects => true,  // 自前で4ベジェを構成する（20章 3.10 の表の判断を踏襲）
            Capability::Arcs => true,          // krilla のベジェ曲線APIで直接描ける想定
            _ => true,
        }
    }
}
```

#### 3.5.1 `px_per_mm` の決定（20章 U4 への回答）

**印刷用の `Viewport` は常に PDF ポイント（1/72インチ）を「view単位」として使う。**

```rust
pub const POINTS_PER_MM: f32 = 72.0 / 25.4; // ≈ 2.83465

pub fn print_viewport(width_mm: f32, height_mm: f32) -> drill_render::Viewport {
    drill_render::Viewport {
        size: Vec2 { x: width_mm * POINTS_PER_MM, y: height_mm * POINTS_PER_MM },
        ui_scale: 1.0,          // 印刷は「1 view単位 = 1pt」を基準の太さとする
        px_per_mm: POINTS_PER_MM,
    }
}
```

理由: krilla（PDF全般）はポイントをネイティブ単位として扱う。`ui_scale = 1.0` を印刷の基準に
固定することで、20章 3.12 U6 の「`ui_scale` の二重適用防止」が印刷経路にもそのまま適用できる
（`Viewport::for_print(width_mm, height_mm)` のような専用コンストラクタを20章の `Viewport` に
追加することを実装タスクで提案する — 8節）。0.3mm の罫線は `0.3 * POINTS_PER_MM ≈ 0.85pt` の
`stroke_w` として表現できる。

`FieldDiagram` ブロックが確保した矩形 `(width_mm, height_mm)`（3.3.3節）に対し、
`drill_render::build` を `print_viewport(width_mm, height_mm)` で呼び、返った `DisplayList` を
`render(&list, LayerMask::PRINT, &mut PdfBackend { origin_pt: ページ内オフセット, .. })` に渡す。
`origin_pt` は `PdfBackend::map` がすべての座標に加算する平行移動であり、`DisplayList` 自体は
常に原点 (0,0) 基準のまま（20章 `Viewport` doc comment の規約どおり）。

#### 3.5.2 印刷向け `LodPolicy`

20章 3.12 の既定 `LodPolicy`（`label_budget: 400`）は 1200×700px 相当の**画面**を基準にした値である。
印刷は同じ視野角に対して解像度がはるかに高い（A4横 297mm ≈ 842pt に対し、実務的な可読フォントサイズは
画面の実効ppiよりずっと小さく詰め込める）。本書は印刷向けの既定値を追加する。

```rust
impl LodPolicy {
    /// セット別チャート（3.1.2節）向け。1,000人規模でも大半のラベルが残る。
    pub fn print() -> Self {
        Self {
            label_min_spacing: 6.0,   // pt。screen既定14.0の半分未満（印刷は密でも読める）
            label_budget: 2_000,      // screen既定400の5倍
            symbol_min_radius: 1.2,
            stroke_min_radius: 0.6,
            trail_max_points: 16,
            grid_min_spacing: 2.0,
        }
    }
}
```

この定数は20章 `LodPolicy` に対する**追加のコンストラクタ**であり、`DrawCmd`/`Layer` など
型そのものには触れない。20章の実装タスクへの追加1件として8節に計上する。

### 3.6 日本語フォント埋め込み・サブセット化・混植

`assets/NotoSansJP.ttf` は 9,589,900 バイト（9.6MB、実測 — `ls -la assets/` で確認）。1,000人規模の
一括出力でページごと・演者ごとにこれをまるごと埋め込むことは論外（5節の見積りが破綻する）。

```rust
// crates/drill-export/src/fonts.rs

/// 埋め込み前のフォント資産。`bytes` は起動時に一度だけ読み込み、以降は使い回す。
pub struct FontAsset {
    pub bytes: std::sync::Arc<[u8]>,
    pub role: drill_render::FontRole,   // Sans | Mono（20章 3.2）
}

/// 複数フォントのフォールバック鎖。現状は NotoSansJP 1本で足りる
/// （Noto Sans JP は日本語・ラテン文字の両方のグリフを含む — 42番文書 3.11 で確認済み）。
/// 将来 Noto Sans JP がカバーしない文字集合（ハングル・キリル文字等）の言語を追加する場合に
/// 備え、鎖として設計しておく。
pub struct FontFallbackChain(Vec<FontAsset>);

impl FontFallbackChain {
    pub fn single(asset: FontAsset) -> Self { Self(vec![asset]) }
    /// `text` に必要な全グリフを最初にカバーできるフォントを返す。無ければ最後の要素
    /// （既定フォント）にフォールバックし、豆腐（.notdef）表示は許容する（6節、敵性入力対策）。
    pub fn resolve_for(&self, text: &str) -> &FontAsset { self.0.last().expect("non-empty chain") }
}

/// 1回の帳票生成（PDFファイル1つ）で実際に使われた文字だけを集める。
/// `report.rs` が組み立てた全 `Block` を1回走査するだけの O(総文字数)。
#[derive(Default)]
pub struct GlyphUsage(std::collections::BTreeSet<char>);

impl GlyphUsage {
    pub fn scan_blocks(&mut self, blocks: &[Block]) { /* Heading/Paragraph/Table の文字列を走査 */ }
    pub fn scan_str(&mut self, s: &str) { self.0.extend(s.chars()); }
    pub fn chars(&self) -> impl Iterator<Item = char> + '_ { self.0.iter().copied() }
}

/// `asset` を `usage` の文字だけにサブセット化し、PDF へ1回だけ埋め込む。
/// 戻り値の `FontId` はその PDF 文書内で有効な参照。
pub fn embed_subset(canvas: &mut impl PageCanvas, asset: &FontAsset, usage: &GlyphUsage) -> FontId;
```

**混植（日英混在）の扱い**: `NotoSansJP.ttf` 単体がラテン文字・日本語（ひらがな・カタカナ・
常用漢字含む）の両方を持つため、通常の帳票（演者ラベル・セット名・数値・日本語見出し）は
**フォールバック無しの単一フォント**で組める。`FontRole::Mono`（20章のカウント数字の等幅表示等）は
専用の等幅フォントを資産として持たないため、`NotoSansJP` を等幅**近似**として使い、
`fill_text_fit`（3.5節）の `Tz` 水平スケールで字送りを揃える。これは20章 3.11「層3」の契約
（`FontMetrics::measure` の1.15倍以内にインクボックスを収める）をPDF側で満たす具体的な実装である。

**サブセット化の効果（5節で数値化）**: 演者ラベルはほとんどASCII（"T1" 等）、日本語が現れるのは
セット名・タイトル・見出し語彙程度なので、実際に使われるユニーク文字数は帳票1件あたり
数十〜数百程度に収まる。TTFの平均的なグリフ輪郭サイズ（数百〜数千バイト）から、
サブセットは概ね**数百KB以内**に収まる（5.2節で具体的な見積りを行う）。

### 3.7 リハーサル記号・小節番号・音楽対応の拡張

既存 `countsheet::rehearsal_mark`（18–30行、二重基数26）はそのまま再利用する（変更不要）。
本書が `countsheet.rs` に追加するのは以下2点。

```rust
// crates/drill-core/src/countsheet.rs（本書が追加提案する部分）

/// `SetTiming` に対応する小節レンジ。「小節12〜14」のような表示に使う
/// （プロダクションシート・カウントシートの新列、3.1.3/3.1.4節）。
pub fn measure_range(doc: &Document, timing: &SetTiming, beats_per_measure: u16) -> (u32, u32) {
    let end_count = timing.start_count as f32 + f32::from(timing.counts);
    let (start_measure, _) = doc.tempo.measure_beat(timing.start_count as f32, beats_per_measure);
    let (end_measure, end_beat) = doc.tempo.measure_beat(end_count, beats_per_measure);
    // 次の小節の1拍目ちょうどで終わる場合は「その前の小節まで」を含める（境界の直感的表示）。
    let end_measure = if end_beat <= 1.0 + 1e-3 && end_measure > start_measure { end_measure - 1 } else { end_measure };
    (start_measure, end_measure.max(start_measure))
}

/// ユーザーが特定のグローバルカウントに付けた注記（歌詞・演出キュー）。自由記述であり
/// 翻訳しない（42番文書 3.7節「自由記述は翻訳しない」分類に従う）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProductionCue {
    pub at_count: f32,
    pub label: String,
}

/// `count_sheet` の各行に、その区間へ落ちるキューを結び付ける。O(sets × cues) — 63×数十件規模
/// なら無視できる。cues が多い場合（数千件）はソート済み前提の二分探索へ変更可能（9節）。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProductionRow { pub timing: SetTiming, pub cues: Vec<ProductionCue> }

pub fn production_sheet(doc: &Document, beats_per_measure: u16, cues: &[ProductionCue]) -> Vec<ProductionRow> {
    count_sheet(doc, beats_per_measure)
        .into_iter()
        .map(|timing| {
            let end = timing.start_count as f32 + f32::from(timing.counts);
            let mut my_cues: Vec<ProductionCue> = cues
                .iter()
                .filter(|c| c.at_count >= timing.start_count as f32 && c.at_count < end)
                .cloned()
                .collect();
            my_cues.sort_by(|a, b| a.at_count.total_cmp(&b.at_count));
            ProductionRow { timing, cues: my_cues }
        })
        .collect()
}
```

**永続化の要否**: `ProductionCue` は「演者が打った自由記述」と同種の作品データであり、UIを閉じても
消えてほしくない。本書はこれを `Document` に追加することを提案するが、`Document` のスキーマ変更・
`Edit` enum への配線は10番文書の管轄であるため、**本書はここでは変更を提案するに留め、実装は
10番と協調する**（DESIGN_GAPSの `Edit` enum に本書が要求するバリアントを8節に明記する）。

```rust
// crates/drill-core/src/lib.rs, struct Document に追加提案（10番文書との協調が必要）
pub struct Document {
    // ...既存フィールド...
    #[serde(default)]
    pub production_cues: Vec<countsheet::ProductionCue>,
}

// Edit enum への追加提案（10番文書が Edit enum 本体を確定させた後に配線する）
// Edit::SetProductionCues { before: Vec<ProductionCue>, after: Vec<ProductionCue> }
```

### 3.8 難易度・クリニックレポートの統合

3.1.6節の `build_difficulty_report` が消費する型はすべて12番・13番が確定済みである
（`ShowDifficulty` / `MovementRun` / `TransitionScan` / `Severity`）。本書が追加するのは
「その結果を `Block` 列に変換する」変換関数だけであり、新しい分析ロジックは持たない
（4節不変条件2）。`Severity::glyph()`（13章3.3節、`✕`/`⚠`/`△`）をそのまま表のセルに使うことで、
`PRODUCT_QUALITY.md`「警告は色だけに依存せず文字と形でも示す」の要件を紙面上でも満たす
（色が使えないモノクロ印刷でも記号で重大度が伝わる）。

### 3.9 一括出力とジョブ化

40番文書（非同期ジョブ基盤）は未執筆のため、`DESIGN_GAPS.md` B-3 のスケッチを暫定契約として使う
（13番文書が `ClinicCache::refresh_nearest` で同様の割り切りをしている前例に倣う）。

```rust
// crates/drill-export/src/batch.rs

use std::sync::atomic::{AtomicU32, AtomicBool};
use std::path::{Path, PathBuf};

/// 一括生成の出力先。`SingleFile` は `DrillBookStyle::Illustrated` と併用しない
/// （5.1節の見積りにより、単一ファイルに収めると 64,000 ページになり実務上破綻するため）。
pub enum BatchOutput {
    SingleFile(PathBuf),
    PerPerformerDir { dir: PathBuf },
}

pub struct BatchDrillBookRequest {
    pub style: DrillBookStyle,
    pub template: PageTemplate,
    /// 空 = 全演者。`Selection`（15章）と組み合わせて「このセクションだけ」も表現できる。
    pub performers: Vec<PerformerId>,
    pub beats_per_measure: u16,
    pub locale: Locale,
    pub output: BatchOutput,
}

#[derive(Clone, Debug, Default)]
pub struct BatchReport {
    pub files_written: Vec<PathBuf>,
    pub total_pages: u32,
    pub elapsed_ms: u64,
}

/// `progress` は 0..=10_000（DESIGN_GAPS B-3 の `Job<T>` 契約）。`cancel` はキャンセル要求。
/// 呼び出し側（`drill-app`）はワーカースレッドでこれを回し、`progress`/`cancel` を
/// `Arc` 越しに共有する。本関数自体はスレッドを起動しない（純粋な計算関数のまま保つ —
/// スレッド管理は40番文書の責務）。
///
/// キャンセル確認の頻度: 演者1人分（`PerformerDrillBook`）またはセット1つ分
/// （`SetChart`）ごと。5節の見積りで1単位あたり数十ミリ秒〜のオーダーなので、
/// キャンセルの反応が数百ミリ秒を超えて遅れることはない。
pub fn run_batch_drill_books(
    doc: &Document,
    request: &BatchDrillBookRequest,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<BatchReport, drill_core::DrillError>;
```

**共有フィールド図の使い回し（`Illustrated` を実用的なコストに収める鍵）**: `Illustrated` 演者別
ドリルブックは「全演者が同じセットの図を見る」という3.1.2節と同じ性質を持つ。したがって
本書は次の2段構成を取る。

1. `SetChart` 相当の共有ベースを**セット数だけ**（例: 64回）ビルドし、`PageCanvas::begin_reusable`/
   `end_reusable`（3.5節）で1つの再利用可能な描画部品として登録する。
2. 演者ごとのページでは、その部品を `place_reusable` で複製し、その演者の位置に対応する
   1個の軽量なハイライト（リング）だけを**その場で描き足す**。ハイライト位置は
   `drill_render::layout::FieldMap::fit`/`map`（20章3.7節、いずれも公開・O(1)の純粋関数）を
   `SetChart` ビルド時と全く同じ引数（`grid` / `viewport` / `FitMode::Contain` / `margin`）で
   呼び直すだけで求まる — **DisplayList を演者ごとに再構築しない**。

```rust
/// 3.5節の FieldMap を使った軽量ハイライト計算。O(1)。
fn highlight_position(doc: &Document, set_index: usize, performer_index: usize, viewport: &drill_render::Viewport) -> Option<Vec2> {
    let set = doc.sets.get(set_index)?;
    let p = *set.positions.get(performer_index)?;
    let map = drill_render::layout::FieldMap::fit(&doc.grid, viewport, drill_render::FitMode::Contain, PRINT_MARGIN_PT);
    Some(map.map(p))
}
```

これにより `Illustrated` の総コストは「セット数回のフルビルド」+「演者数×セット数回の
O(1)ハイライト計算 + 軽量な描画命令の複製」に分解され、5.1節の見積りが成立する。
**`PerPerformerDir` 出力でのみ `Illustrated` を許可する**理由もここにある —
再利用可能部品（Form XObject 相当）は1つのPDF文書内でのみ共有できるため、`SingleFile` に
1,000人分を詰め込むと共有の恩恵が消え、なおかつ 64,000 ページという非現実的な成果物になる。

### 3.10 決定論とゴールデンテスト方式

00-conventions.md 不変条件5「同じ `(Document, config, count)` からは常に同じ出力」をPDFにも
適用する。ただし PDF フォーマット特有の2つの障害がある。

1. **生成時刻**: PDFの `/CreationDate` は通常システム時刻から書かれる。
2. **文書ID**: PDF仕様の `/ID` はしばしばランダム値または時刻由来のハッシュで埋められる。

対策:

```rust
// crates/drill-export/src/pdf.rs（続き）

/// システムクロックへ依存しない、呼び出し側が明示的に渡す日時。`drill-export` は
/// `std::time::SystemTime` を読まない（00-conventions のクレート境界を厳密には破らないが、
/// 決定論のためにあえて呼び出し側へ委譲する — `drill-app` 側が実際の時刻を1回読んで渡す）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp { pub year: u16, pub month: u8, pub day: u8, pub hour: u8, pub minute: u8, pub second: u8 }

pub struct PdfMeta {
    pub title: String,
    pub creation_date: Option<Timestamp>,   // None ならゴールデンテスト向けに省略する
    /// `/ID` に使う64bitの種。`Some` なら文書内容のハッシュなど呼び出し側が決めた
    /// 決定論的な値を使う。`None` の場合の既定動作は9節の未決事項。
    pub id_seed: Option<u64>,
}
```

**ゴールデン比較の方式**: PDFバイト列全体の一致は `id_seed`/`creation_date` を固定すれば理論上
可能だが、krilla 内部の圧縮（Flate）や zlib実装バージョン差でバイト列が揺れるリスクがある
（9節で検証タスクを置く）。本書は2段構えのテストを定義する。

1. **内容determinism（強い保証、常に成立させる）**: `report.rs` の `Block` 生成と
   `PageLayout::place_all` の出力（`Vec<PageContent>`）を対象にした**テキストゴールデン**
   （20章 `write_golden` と同じ手法 — 座標・行を安定フォーマットの文字列に落として `.expected` と
   比較）。PDFエンコード層より手前で決定論を保証するので、krilla のバージョン差に影響されない。
2. **バイトdeterminism（弱い保証、可能なら追加）**: `Timestamp`/`id_seed` を固定した2回の
   `run_batch_drill_books` 呼び出しの出力ファイルが**バイト一致**することを1本のテストで確認する。
   krilla のマイナーバージョンアップでこれが崩れた場合はテストが検出し、
   「タイムスタンプのみ差分」まで許容範囲を緩めた比較（`/CreationDate` と `/ID` の値域だけを
   正規化してから比較）に切り替える運用とする。

### 3.11 印刷プレビューUIの要件（43番文書への申し送り）

本書はUIウィジェットの実装は行わないが、43番文書が満たすべき要件を以下に確定させる。

1. **プレビューは3つ目のキャンバス実装として提供する**: `PageCanvas` を egui 上に直接描画する
   `EguiPageCanvas`（20章の `EguiBackend` と対になる存在）を用意し、PDFエンコードを経由せずに
   ページを即座に表示する。PDFファイルを一度書き出してから再読込して表示する設計にしない
   （往復コストと外部ビューア依存を避ける）。
2. **ページ送り**: 前/次/ジャンプ・サムネイル一覧。単純なページ切替は再レイアウトを伴わないため
   16.6ms予算内（表示中の `Vec<PageContent>` から該当ページを引くだけ）。
3. **再レイアウトを要する操作**（用紙サイズ・余白・帳票種別・対象演者の変更）は`Job<T>`
   （40番、暫定は`DESIGN_GAPS.md`B-3）を介して非同期に行い、UIスレッドをブロックしない。
   処理中はスピナーと「前回のプレビューを表示したまま更新中」を示す（13章のデバウンス表示
   `⟳ 更新中` と同じ語彙を使う）。
4. **一括出力前の見積り表示**: 5節の計算式をUIに埋め込み、「約2,000ページ・約9MB・推定15秒」の
   ように**開始前に**見積りを出す（`PRODUCT_QUALITY.md`「安全性」節の踏襲 — 意図しない巨大出力に
   利用者が事前に気づける）。
5. **進捗とキャンセル**: `run_batch_drill_books` の `progress`/`cancel` をプログレスバーと
   キャンセルボタンに直結する。キャンセル後は**部分的に書かれたファイルを削除する**
   （00-conventions「データ喪失防止」— ただし削除対象は今回の実行で新規作成したファイルのみ、
   既存ファイルの上書きは3.11節末尾のとおり原子的に行う）。
6. **書き込みの原子性**: 各PDFファイルは一時ファイルへ書いてから `rename` する
   （00-conventions「上書きは原子的置換」）。`PerPerformerDir` 出力で一部の演者だけ失敗しても、
   既に成功した他の演者のファイルは有効なまま残る（部分成功を許容し、`BatchReport` に
   失敗一覧を含める設計を8節の実装タスクに含める）。

## 4. 不変条件

| # | 不変条件 |
|---|---|
| I1 | 全ての帳票種別は `Vec<Block>` を組み立てたのち `PageLayout::place_all` のみを経由してページ化される。帳票ごとに独自の改ページ判定コードを持たない。 |
| I2 | `Block::FieldDiagram` が最終的に描く図形の座標は、`drill_render::build` が返す `DisplayList` の座標と完全一致する（20章 I9 を印刷経路に拡張したもの）。`drill-export` はフィールド上の点の位置を独自に再計算しない。 |
| I3 | 同一の `(Document, ReportKind, PageTemplate, Locale, creation_date, id_seed)` からは、`PageLayout` の出力（`Vec<PageContent>`）がバイト単位で一致する。PDFバイト列の一致は3.10節の2段構えの保証にとどめる。 |
| I4 | `Table` ブロックは行の途中でページを跨がない。ヘッダ行は割った先の全ページで再掲する（3.3.3節）。 |
| I5 | `KeepTogether` ブロックは、内容の高さが1ページの版面高さ以下である限り、ページを跨がない。 |
| I6 | フォントのサブセット埋め込みは1回の帳票生成（PDF1ファイル）につき、実際に使用された文字のみを含む。全文字を毎回埋め込まない。 |
| I7 | `Illustrated` ドリルブックの `SingleFile` 出力は許可しない（3.9節、ページ数が実務上破綻するため）。`BatchDrillBookRequest` の構築時点でこの組合せを拒否する。 |
| I8 | 一括出力は演者/セット単位でキャンセルを確認し、キャンセル時に未完了の出力ファイルを残さない。 |

## 5. 性能

基準規模: 演者1,000人・セット64（`PRODUCT_QUALITY.md`/00-conventions.md 共通）。帳票生成は
UIの毎フレーム処理ではないため16.6ms予算の対象外だが、「ジョブとして許容できる時間」を見積もる。

### 5.1 ページ数・時間・ファイルサイズの見積り

**演者別ドリルブック（`Compact`、既定、`SingleFile`）**

- 版面高さ: A4縦・上下余白20mm・ヘッダ/フッタ計10mm ⇒ 約257mm。
- 行高さ: 9pt本文、`FontMetrics::SANS.line_height(9pt)` ≈ 3.2mm相当 + パディング1.2mm ≈ 5.0mm/行。
- 見出し + ヘッダ行を差し引いた本文域 ≈ 236mm ⇒ 1ページ約47行。
- 64セット/人 ⇒ 1人あたり2ページ。1,000人 ⇒ **約2,000ページ**。
- 生成コスト: `count_sheet`/`readable`/`performer_continuity` の呼び出しは16番文書5節が既に
  64,000回の `measure` 呼び出しを13msと見積もっている。本書のテーブル構築はこれと同オーダー
  （数十ms）。PDFエンコード（krilla、ラスタライズなし・純粋なテキスト配置命令）は
  1ページあたり数十〜数百のテキスト描画命令で、2,000ページ全体で**数秒程度**と見積もる
  （krilla自体のベンチマークが無いため、実測は8節の実装タスクでベンチを追加して確認する）。
  フォントサブセット化は使用文字数（数百字程度）に対し1回のみで数十ms。
  **合計: 生成時間はおおむね5〜15秒のオーダー**。
- ファイルサイズ: 1ページの圧縮後テキストコンテンツを3〜5KB程度と見積もると2,000ページで
  約6〜10MB、共有フォントサブセット（数百KB、3.6節）を加えて**総計およそ8〜12MB**。

**セット別チャート（全セット、`SetChart`）**

- ページ数 = セット数 = **64ページ**（演者数に依存しない）。
- 1ページのDisplayList構築コスト: 20章5.3節の見積り（1,000人・build 約0.5〜1.5ms）をそのまま
  適用 ⇒ 64ページで**約100ms未満**。
- PDFエンコード: 1ページ約3,000描画命令（ドット1,000×概算3命令+グリッド線約270本）。
  20章5.3節のSVGバックエンドのベンチ上限（1,000人で8ms）を参考値とすると、PDFも同オーダーと
  見積もり、64ページで**1秒未満**。
- ファイルサイズ: 1ページ圧縮後40〜60KB程度 ⇒ 64ページで約3MB、共有フォント込みで
  **総計およそ4MB**。

**演者別イラスト付きドリルブック（`Illustrated`、`PerPerformerDir`、任意・P2扱い）**

- 出力は1,000個の独立ファイル、各64ページ ⇒ 総ページ数 **64,000ページ相当**（ファイルは分割）。
- 3.9節の共有部品方式により、セットごとのベースは64回だけフルビルドする（上記セット別チャートと
  同じコスト、100ms + 1秒未満）。
- 演者ごとの複製は「ハイライト位置計算（O(1)）+ 既存部品の複製 + 1個の軽量描画命令追加」であり、
  1回あたり実装依存だが概算5〜10ms（PDF文書ごとの独立性のため、部品の複製そのものはファイルごとに
  再エンコードが必要 — 3.9節）。1,000人×64セット = 64,000回 ⇒ **総計7〜11分程度**。
- 各ファイルは独立して64ページ・フォントサブセット込みで**数百KB〜1MB程度**（1人分のテキスト量+
  共有図形の複製コストのみで、フォントサブセットは各ファイルで再計算不要 — 使用文字集合は
  ほぼ全ファイル共通なので、サブセット結果のバイト列をキャッシュして使い回せる、8節）。
- **この規模は同期実行に適さない。3.9節の `Job<T>` 契約に必ず乗せ、進捗・キャンセルを提供する
  ことを必須要件とする**（4節不変条件7の理由）。

**メモリ**: 最も重いのは `Compact`/`SetChart` の `SingleFile` 生成で、krilla が最終 `finish()` まで
文書全体をメモリ上に保持する前提でも出力想定サイズ（8〜12MB）の数倍、**目安200MB未満**に収まる
（krillaが実際にどこまでストリーミング書き出しをサポートするかは9節の未決事項）。
`PerPerformerDir` はファイルごとに書き出して破棄するため、ピークメモリは
「1ファイル分（数百KB〜1MB）+ セットごとの共有部品キャッシュ（64個 × 数百KB ≈ 十数MB）」程度で
頭打ちになり、**1,000ファイルに対して線形に増加しない**（3.9節の設計がこれを保証する）。

### 5.2 フォントサブセットのサイズ

`assets/NotoSansJP.ttf`（9.6MB、実測）に対し、1帳票あたりの実使用文字数を保守的に
「ASCII全体 + 日本語ユニーク文字数百字」と見積もると、TTFグリフの平均輪郭サイズ
（複雑な漢字で数KB、単純な仮名・ラテン文字で数百バイト）から、サブセットは
**概ね100〜500KB程度**に収まると見積もる（9.6MBに対して2〜5%程度）。これがサブセット化を
必須とする直接の根拠であり、フル埋め込みでは1ページ生成ごとに9.6MBが乗ることになり
5.1節の見積りが1〜2桁悪化する。

## 6. 失敗モードと安全性

| # | 壊れ方 | 対処 |
|---|---|---|
| F1 | `Block::FieldDiagram`/`KeepTogether` の必要高さが1ページの版面高さを超える（極端な余白設定・カスタム用紙） | `PageLayout` はクランプして1ページに強制収容し、`LayoutStats::forced_overflows` を加算する。パニックしない（3.3.3節）。UIは`forced_overflows > 0`のとき警告を表示する。 |
| F2 | `Table` の1行がフォントサイズに対して極端に長い文字列を含む（インポート由来の壊れたラベル） | `fill_text_fit`（3.5節）でセル幅に収める。行高さの見積り自体は折返しを考慮しないため、極端な場合は表示がはみ出し得るが、レイアウトの成否には影響しない（テキストの視覚的トリミングはPDF内で完結する）。 |
| F3 | `NotoSansJP.ttf` の読み込み・サブセット化が失敗する（アセット破損・krillaのバグ） | `embed_subset` は `Result` を返し、失敗時は帳票生成全体を中断して `DrillError` を返す（HTML出力へのフォールバックを提示するのはUI側の責務、3.11節）。 |
| F4 | 一括出力中にディスクフルになる | 各ファイルは一時ファイル→`rename`（00-conventions）。書き込み失敗はその演者/セットの失敗として`BatchReport`に記録し、**既に成功した他ファイルは残す**（部分成功、3.11節）。全体を中断するかは`cancel`と同じ経路で判定する。 |
| F5 | `ProductionCue.at_count` が全セットの範囲外（負・グローバルカウント超過） | `production_sheet` はどの行にも一致せず黙って無視する（フィルタ条件に当たらないだけ）。パニックしない。 |
| F6 | `DrillBookLayout::Illustrated` が `BatchOutput::SingleFile` と共に指定される | `run_batch_drill_books` は開始前に`DrillError`（新規バリアント、42番文書と協調）を返して拒否する（4節不変条件7）。 |
| F7 | `Document` の座標にNaN/Infが含まれる（信頼できない入力） | `FieldDiagram` の実描画は20章の`build`に委譲しており、20章F1/F2（非有限は非表示、巨大値はクランプ）がそのまま適用される。本書は追加の防壁を持たない代わりに、20章の保証に依存する。 |
| F8 | 演者数・セット数が上限規模（4,000人・256セット）を超える | 5節の見積りが線形に拡大するのみでアルゴリズム上の破綻はない（`PageLayout`・`GlyphUsage`いずれもO(n)）。ページ数が数万を超える`SingleFile`要求はF6と同様の事前拒否をUIに実装することを43番文書へ申し送る。 |

## 7. テスト計画

**単体テスト（`crates/drill-export/src/page.rs`）**

- `page_template_content_height_subtracts_margins_and_header_footer`
- `table_block_splits_across_pages_without_breaking_a_row`（I4）
- `table_header_repeats_on_continuation_page`
- `field_diagram_preserves_aspect_ratio_within_available_width`
- `keep_together_does_not_split_when_it_fits_on_a_fresh_page`（I5）
- `keep_together_overflows_gracefully_when_larger_than_one_page`（F1）
- `heading_avoids_orphaning_at_page_bottom`

**単体テスト（`crates/drill-export/src/fonts.rs`）**

- `glyph_usage_scan_collects_unique_chars_only`
- `fallback_chain_falls_back_to_last_asset_for_unknown_glyphs`

**単体テスト（`crates/drill-core/src/countsheet.rs` 拡張分）**

- `measure_range_reports_inclusive_measure_span`
- `production_sheet_assigns_cues_to_the_right_set`
- `production_sheet_ignores_out_of_range_cues`（F5）

**バックエンドテスト（`crates/drill-export/src/pdf.rs`）**

- `pdf_backend_dot_positions_match_svg_backend`（20章 I9 の印刷経路への拡張。同一 `DisplayList` を
  `SvgBackend` と `PdfBackend`（モック `PageCanvas`）へ流し、対応する円/グリフの中心座標が一致する
  ことを確認）。
- `pdf_backend_receives_begin_end_once`（20章 I8 と同じ手法）。

**ゴールデンテスト**

- `Document::demo(8, 10)` に対する `PerformerDrillBook`（`Compact`）・`SetChart`・`CountSheet`・
  `ProductionSheet` それぞれの `Vec<PageContent>` を安定テキスト形式で `.expected` にコミットする
  （20章 `write_golden` と同じ手法、3.10節「内容determinism」）。
- 固定 `Timestamp`/`id_seed` での2回連続生成によるPDFファイルのバイト一致テスト（3.10節「弱い保証」）。

**プロパティテスト**

- 任意の `Vec<Block>`（ランダム生成、行数・見出し数を変動）に対し、`PageLayout::place_all` が
  パニックしないこと、かつ全ての `PlacedBlock` の `y_mm + height_mm <= content_height_mm()` が
  成り立つこと。
- 任意の `PageTemplate`（余白・用紙サイズを極端な値まで変動）に対しても `content_height_mm()` /
  `content_width_mm()` が非負であること（F1のガードの網羅性）。

**ストレス**

- `Document::demo(1000, 1)` 相当（1,000人 × 64セット相当に拡張したフィクスチャ）で
  `PerformerDrillBook(Compact, SingleFile)` を実行し、5.1節の見積り（時間・ページ数・ファイル
  サイズ）を実測して上限をベンチにする。
- 同フィクスチャで `Illustrated + PerPerformerDir` を**演者100人分だけ**（全数は実行時間が長いため
  CIでは間引く）実行し、1人あたりの時間が5.1節の見積り（7〜10ms/人相当）を大きく超えないことを
  確認する。

**CI検査**

- `crates/drill-export` のソースに `std::time::SystemTime::now()` の呼び出しが無いことを検査する
  grep（3.10節の決定論方針の機械的な再発防止）。

## 8. 実装タスク

1タスク=1〜3時間相当。依存は`→`で示す。

### フェーズ0: 前提の検証（直列・最優先）

| ID | 内容 | 依存 |
|---|---|---|
| T0 | **スパイク**: krillaに`assets/NotoSansJP.ttf`を実際に埋め込み、複数のPDFビューア（Acrobat・ブラウザ内蔵ビューア・OS標準ビューア）で日本語グリフが表示されることを確認する。TTFフレーバーのサブセット化が実際に機能するかも確認する。失敗した場合は3.4節の選定を再検討する（9節）。 | — |

### フェーズ1: クレートと型

| ID | 内容 | 依存 |
|---|---|---|
| T1 | `crates/drill-export` 新設。`Cargo.toml` の `members` 追加、`drill-core`/`drill-render`/`krilla` 依存。20章 T15/T16（`html.rs`/`svg.rs` 移設）を前提とする（無ければ本タスクの一部としてスタブを先に置く）。 | T0 |
| T2 | `page.rs`: `PageSize`/`Margins`/`PageTemplate`/`Block`/`TableBlock`/`SceneKey` の型定義。 | T1 |
| T3 | `page.rs`: `PageLayout`（`place_all`/`place_table`/`place_diagram`/`place_keep_together`/2パスのページ番号解決）。 | T2 |
| T4 | `fonts.rs`: `FontAsset`/`FontFallbackChain`/`GlyphUsage`。 | T1 |

### フェーズ2: PDFバックエンド

| ID | 内容 | 依存 |
|---|---|---|
| T5 | `pdf.rs`: `PageCanvas` トレイト定義、`PdfBackend`（`drill_render::backend::Backend`実装）。 | T2, T4 |
| T6 | `pdf.rs`: `KrillaCanvas`（`PageCanvas`のkrilla実装）、`embed_subset`の実装。 | T0, T5 |
| T7 | `20-display-list.md` への追補: `Viewport::for_print(width_mm, height_mm)` コンストラクタ、`LodPolicy::print()` の追加（20章の型への小規模な追加、20番担当と協調）。 | — |
| T8 | `PdfMeta`/`Timestamp`/決定論のための`id_seed`配線。 | T6 |

### フェーズ3: 帳票ビルダ（T3, T5 後・並行可）

| ID | 内容 | 依存 |
|---|---|---|
| T9 | `report.rs`: `build_performer_drill_book`（`Compact`）。 | T3 |
| T10 | `report.rs`: `build_set_chart`。 | T3, T7 |
| T11 | `countsheet.rs` 拡張: `measure_range`/`ProductionCue`/`production_sheet`（10番文書と`Document.production_cues`追加を協調）。 | — |
| T12 | `report.rs`: `build_production_sheet`（T11に依存）、`build_count_sheet`。 | T3, T11 |
| T13 | `report.rs`: `build_section_summary`（15番未着地の間は単一セクション代替、9節）。 | T3 |
| T14 | `report.rs`: `build_difficulty_report`（12番`ShowDifficulty`/13番`TransitionScan`が実装され次第。未実装の間はモック入力でスタブ実装可）。 | T3 |
| T15 | `report.rs`: `build_performer_drill_book`（`Illustrated`）、3.9節の共有部品ハイライト方式。 | T9, T10 |

### フェーズ4: 一括出力とHTML後継

| ID | 内容 | 依存 |
|---|---|---|
| T16 | `batch.rs`: `run_batch_drill_books`、`BatchDrillBookRequest`/`BatchReport`、原子的書き込み。 | T9, T15 |
| T17 | `html.rs`: `Vec<PageContent>` を HTML へ落とすシリアライザへ既存`coordinate_sheet_html`/`drill_book_html`を置き換え。 | T3 |
| T18 | `EguiPageCanvas`（`drill-app`側、印刷プレビューUI用）。43番文書の要件（3.11節）に基づき配線。 | T5 |

### フェーズ5: 検証

| ID | 内容 | 依存 |
|---|---|---|
| T19 | 単体テスト一式（7節）。 | T3, T5 |
| T20 | ゴールデンテスト一式。 | T9–T14 |
| T21 | プロパティテスト（`PageLayout`のパニック無し・境界非負）。 | T3 |
| T22 | ストレステスト（1,000人規模の時間・ページ数・ファイルサイズ実測、5節の見積りとの照合）。 | T16 |
| T23 | CI検査（`SystemTime::now()`不使用のgrep）。 | T8 |

### 並行性

```
T0 → T1 ─┬─ T2 ─┬─ T3 ─┬─ T9 ─┬─ T15 ─ T16 ─ T22
         │      │      ├─ T10(←T7)   │
         ├─ T4 ─┴─ T5 ─┴─ T6 ─ T8    │
         │              │            │
         │              └─ T18       │
         └─ T7                       │
T11 → T12                            │
T13, T14 ─────────────────────────────┤
T3 → T17                              │
T19, T21 ← T3/T5                      │
T20 ← T9..T14                         │
T23 ← T8
```

3人（3エージェント）で回す場合: **A** = T0→T1→T2→T3→T7→T18、**B** = T4→T5→T6→T8→T23、
**C** = T11→T12, T13, T14（12/13番の実装待ちなら先にT9/T10のレビューへ回る）。
合流点は T9/T10（帳票ビルダの土台）と T16（一括出力）。

## 9. 未決事項

1. **krillaのCJK/CID対応の実証**（3.4節）: ドキュメント上の明記ではなくTypst由来という間接根拠に
   基づく選定である。T0のスパイクタスクで実証し、失敗した場合の代替候補は
   `printpdf`（低レベルだが`lopdf`より新しい）である。代替に切り替える場合、`PageCanvas`抽象
   （3.5節）がkrilla固有のAPIに漏れていない設計であることが切替コストを左右する — 実装時に
   `KrillaCanvas`の実装がこの抽象境界を守っているかレビューする。
2. **krillaのメモリモデル**（5.1節末尾）: 文書全体をメモリに保持するのか、ストリーミング書き出しに
   対応するのかがドキュメントから確認できなかった。2,000ページ規模の`SingleFile`生成でメモリが
   問題になる場合、`SingleFile`もセットや演者のチャンク単位でファイルを分割する方式
   （例: 200人ごとに1ファイル）への変更が必要になる可能性がある。T22のストレステストで実測して
   判断する。
3. **PDFバイト単位の決定論の実現可能性**(3.10節): krillaが圧縮やオブジェクト順序に非決定的要素
   （並列化・ハッシュマップ走査由来の順序ゆれ等）を持ち込まないかは未検証。持ち込む場合は
   3.10節の「バイトdeterminism」を諦め、「内容determinism」のみを正式な保証とし、
   `PRODUCT_QUALITY.md`の該当要件の記述を「同じ入力から同じ**内容**が生成される
   （バイト列は圧縮実装に依存し得る）」に弱める調整が必要になる。判断はT8実装後に行う。
4. **`Document.production_cues`の追加**(3.7節): 10番文書（`Edit`enum本体）が未確定のため、
   本書の提案がそのまま採用されるかは10番との協調待ち。10番が`Edit`のバリアント追加方針
   （15章9節1で指摘されている「ID採番をEdit::apply内部に一本化するか」等）を確定させてから
   最終形にする。
5. **`SectionSummary`の暫定実装の扱い**(3.1.5節): 15番`Section`着地前の「単一セクション代替」を
   実装するコスト（T13）と、15番の実装を待って本実装のみ行うコストを比較し、待つ方が
   トータルで安ければT13を後回しにする判断はスケジューリング時に行う。
6. **`Illustrated`ドリルブックの需要**(3.1.1節・5.1節): 7〜10分規模のバッチ処理を要する機能を
   P0/P1で提供する価値があるか、実際のユーザー（マーチング指導者）への需要確認が未実施。
   本書は設計のみを行い、実装着手の優先度判断は製品側（`PRODUCT_QUALITY.md`の
   Delivery sequence）に委ねる。
7. **`fill_text_fit`のTz近似の視覚品質**(3.6節): 等幅フォント資産を持たないまま`Tz`水平スケールで
   等幅相当を近似する方式が、実際のカウントシートの数字列で見た目上十分に整列するかは
   実装後の目視レビューが必要（20章9節U7と同根の課題）。
