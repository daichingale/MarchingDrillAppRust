# DrillForge 不足機能の設計

> [!IMPORTANT]
> **2026-08-10 再監査:** 本文は設計開始時のスナップショットであり、記載された
> 行番号・行数・テスト数・「現状」は現在値ではない。解消済みの欠陥を未修正と誤読しないこと。
> 現行実装との照合結果と、今も残る内部／外部ゲートは
> [`docs/IMPLEMENTATION_EVIDENCE.md`](docs/IMPLEMENTATION_EVIDENCE.md) を正とする。
> 本文は欠陥の由来と設計判断を残す履歴資料として保持している。

`ARCHITECTURE.md` / `PRODUCT_QUALITY.md` / `MEDIA_PIPELINE.md` が「何を目指すか」を定義しているのに対し、
この文書は **現在のコードに実際に欠けているもの** と、それを埋めるための具体的な型・境界・実装順を定義する。

> **本書は「何が欠けているか」の一覧である。** 各領域の具体的な設計は `docs/design/` の22本に展開済み。
> クレート構成・文書間の裁定・実装順は [docs/design/90-integration-roadmap.md](docs/design/90-integration-roadmap.md) を正とする。

現状: `drill-core` 12モジュール約3,700行 + `drill-app` 1,837行、テスト124件パス。
2Dの編集・補間・保存・解析・書き出しモデルは揃っている。欠けているのは大きく3種類。

- **A. ドメインモデルの穴** — マーチングドリルとして表現できていない概念
- **B. 実行基盤の穴** — モデルはあるが動くものが無い層（音・動画・非同期）
- **C. 品質ゲート未達** — `PRODUCT_QUALITY.md` が要求しているのに満たしていない項目

---

## 0. 先に直すべき既存の不具合

設計以前に、現在のコードが壊れている箇所。Wave 0 で同時に潰す。

| # | 箇所 | 内容 |
|---|---|---|
| 1 | [main.rs:831](crates/drill-app/src/main.rs:831) | セット複製が `history` に積まれない。Undoできない。 |
| 2 | [lib.rs:407](crates/drill-core/src/lib.rs:407) | `MoveCommand.set_index` は添字。セット挿入/削除で既存履歴の指す先がずれ、Undoが**別のセットを破壊する**。 |
| 3 | [main.rs:1011](crates/drill-app/src/main.rs:1011) | `analyze_transition` は O(n²)。毎フレーム無条件呼び出し。1,000人で約50万回/フレーム、16.6ms予算を確実に超える。 |
| 4 | [main.rs:1033](crates/drill-app/src/main.rs:1033) | `transition_stats` → `transition_moves` が毎フレーム `Vec` を確保。「フレーム内ヒープ確保ゼロ」に違反。 |
| 5 | [lib.rs:434](crates/drill-core/src/lib.rs:434) | `History::push` の `Vec::remove(0)` が O(n)。`VecDeque` にする。 |
| 6 | [svg.rs:103](crates/drill-core/src/svg.rs:103) vs [main.rs:1471](crates/drill-app/src/main.rs:1471) | **画面とSVG書き出しでフィールドが上下反転している。** SVGは `off_y + (gh - fy) * scale` でy反転（フロントサイドライン=下）、画面は `rect.top() + y / height * h` で反転なし（y=0が上）。同じドリルが別物として出力される。 |
| 7 | [svg.rs:96](crates/drill-core/src/svg.rs:96) vs [main.rs:1470](crates/drill-app/src/main.rs:1470) | SVGは `scale = min(w/gw, h/gh)` でアスペクト比を保持、画面はrectへx/y独立に引き伸ばし。図の比率が一致しない。 |
| 8 | [shapes.rs:135](crates/drill-core/src/shapes.rs:135) | `bezier` が `t = i/(count-1)` のパラメータ等分で、**弧長等分になっていない**。曲線上の演者間隔が不均等になる。同ファイルの `polyline` は弧長等分で実装されており、図形の種類によって挙動が食い違う。`spiral` も同様。 |

| 9 | [main.rs:242](crates/drill-app/src/main.rs:242) | **保存が原子的でない。** `std::fs::write` は既存ファイルを切り詰めてから書くため、書き込み中のクラッシュ・電源断・ディスク満杯で**利用者のドリルが破壊される**。temp へ書いて `ReplaceFileW` で置換する必要がある（[41-persistence-recovery.md](docs/design/41-persistence-recovery.md)）。 |
| 10 | [main.rs:240](crates/drill-app/src/main.rs:240) | バックアップの `fs::copy` が OneDrive の Files On-Demand プレースホルダを実体化させ、保存のたびにネットワーク待ちで UI が固まりうる。**本リポジトリ自体が OneDrive 配下にある**ため実害が出る構成。 |

| 11 | [lib.rs:388](crates/drill-core/src/lib.rs:388) | `out.reserve(from.len().saturating_sub(out.capacity()))` の計算が誤り。`clear()` 直後は `len()==0` なので `reserve(n)` は「容量 ≥ n」の意味。容量50・演者100人だと `reserve(50)` となり既存容量で充足扱いになり、`extend` で再確保が起きる。**確保ゼロを狙った箇所が狙った場面でだけ機能しない。** 正しくは `out.reserve(from.len())`。既存テストは容量ちょうどを渡すため原理的に検出できない。 |
| 12 | [lib.rs:385](crates/drill-core/src/lib.rs:385) | `self.sets.len() - 1` は `sets` が空だと usize アンダーフローでパニック。`validate()` は読込時しか守らず、最後のセットを削除した直後のメモリ上の文書を防げない。 |
| 13 | [main.rs:1268](crates/drill-app/src/main.rs:1268) | `Command::new("ffmpeg")` の裸名指定。Windows の探索順はアプリのディレクトリとカレントディレクトリを PATH より先に見るため、悪意ある `ffmpeg.exe` が同居するフォルダでの起動が成立しうる。絶対パス限定・`.exe` 限定・`env_clear` ＋ allow-list とする（[51-security.md](docs/design/51-security.md)）。同じ箇所の UI 文言「製品版では同梱します」も、libx264 の GPL 伝播と矛盾するため要修正。 |
| 14 | `.gitignore` の `*.drill.json` | v1 フィクスチャをコミットできず、**スキーマ移行の回帰テストが成立しない**。 |
| 15 | `.gitattributes` 不在 | Windows チェックアウトで LF→CRLF 変換が起き、バイト比較のゴールデンテストが全て落ちる。本リポジトリで実際に `LF will be replaced by CRLF` の警告が出ることを確認済み。 |
| 16 | [drill-app/Cargo.toml:9](crates/drill-app/Cargo.toml:9) | `eframe` に `default-features = false` を指定した結果 **accesskit のプラットフォームアダプタが無効**。`Cargo.lock` に `accesskit_winit` が存在せず、egui がアクセシビリティノードを作っても Windows UI Automation へ渡らない。**スクリーンリーダーから一切見えない**。`PRODUCT_QUALITY.md` のアクセシビリティ要求に対する直接の違反。 |

3と4は「1,000人・60fps」の性能ゲートを今この瞬間破っているので、ベンチではなく実アプリで測り直す必要がある。
6・7は `PRODUCT_QUALITY.md` の「座標表、ドリルブック、SVG/PDF出力は同じドキュメント座標を参照する」に対する直接の違反であり、
出力を信用できないという意味で販売上のリスクが高い。いずれも `docs/design/20-display-list.md` の `FieldMap` 一本化で構造的に解消する。
**9 は利用者の制作物を失わせる唯一の経路であり、全項目中で最優先。**

## 0-b. 出荷前に必ず塞ぐ法務・コンプライアンス

| # | 内容 |
|---|---|
| L1 | [main.rs:38](crates/drill-app/src/main.rs:38) が `include_bytes!` で `NotoSansJP.ttf`（9.6MB）を**実行ファイルへ直接埋め込んでいる**。`assets/OFL-NotoSansJP.txt` はリポジトリにあるが配布バイナリには付いてこない。OFL 1.1 は埋め込み配布でもライセンス文と著作権表示の添付を求めるため、アバウト画面等での表示が要る。 |
| L2 | `Cargo.toml` が `MIT OR Apache-2.0` を宣言しているのに、リポジトリルートに LICENSE ファイルが存在しない。 |
| L3 | libx264 は GPL。FFmpeg を同梱する場合はビルド構成でライセンスが伝播する。現状は外部プロセス呼び出しのみで未リンクだが、同梱に踏み切る際は判断が要る（[53-productization.md](docs/design/53-productization.md) / [31-video-export.md](docs/design/31-video-export.md)）。 |
6・7は `PRODUCT_QUALITY.md` の「座標表、ドリルブック、SVG/PDF出力は同じドキュメント座標を参照する」に対する直接の違反であり、
出力を信用できないという意味で最も販売上のリスクが高い。いずれも `docs/design/20-display-list.md` の
`FieldMap` 一本化で構造的に解消する。

---

# A. ドメインモデルの穴

## A-1. 安定IDとコマンド型Undo（最優先・全ての土台）

現状 `Set` にIDが無く、`History` は `MoveCommand` 一種類しか保持できない。
`PRODUCT_QUALITY.md` P0 の「完全なUndo/Redoコマンド化」は未達で、上記バグ1・2の原因でもある。

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct SetId(pub u32);

pub struct Document {
    // ...
    pub sets: Vec<Set>,          // 順序は Vec が持つ / 同一性は SetId が持つ
    next_set_id: u32,
    next_performer_id: u32,
}

impl Document {
    pub fn set_index(&self, id: SetId) -> Option<usize>;
    pub fn set(&self, id: SetId) -> Option<&Set>;
}
```

編集は全て単一の `Edit` を通す。可逆性を型で保証する。

```rust
pub enum Edit {
    MovePoints  { set: SetId, performers: Vec<PerformerId>, before: Vec<Point>, after: Vec<Point> },
    InsertSet   { at: usize, set: Set },
    RemoveSet   { at: usize, set: Set },      // 逆操作のため中身を保持
    ReorderSet  { from: usize, to: usize },
    RenameSet   { set: SetId, before: String, after: String },
    SetCounts   { set: SetId, before: SetCounts, after: SetCounts },
    AddPerformers    { at: usize, performers: Vec<Performer>, positions: Vec<Vec<Point>> },
    RemovePerformers { at: usize, performers: Vec<Performer>, positions: Vec<Vec<Point>> },
    SetRoutes   { set: SetId, before: RouteTable, after: RouteTable },
    SetGrid     { before: GridConfig, after: GridConfig, scaled: bool },
    SetTempo    { before: TempoMap,   after: TempoMap },
    SetAudio    { before: Option<AudioTrack>, after: Option<AudioTrack> },
    Batch(Vec<Edit>),
}

impl Edit {
    pub fn apply(self, doc: &mut Document) -> Result<Edit, DrillError>; // 逆操作を返す
    pub fn coalesce_key(&self) -> Option<CoalesceKey>;                  // ドラッグ中の連続移動を1件に畳む
}

pub struct History {
    undo: VecDeque<Edit>,   // 逆操作を積む
    redo: Vec<Edit>,
    limit: usize,
}
```

`apply` が逆操作を返す設計にすると、`before`/`after` を呼び出し側で二重に組み立てる必要がなくなり、
「applyしたが履歴に積み忘れた」（バグ1）が型として起きにくくなる。

**受け入れ基準**: 10,000回のランダム `Edit` → 全Undo → 初期ドキュメントと完全一致。

## A-2. ルート・ゲート・ホールド（Pywareとの機能差で最大の穴）

現状 `positions_at` はセット間の**単純線形補間**のみ。実際のドリルでは以下が必須。

- 曲線経路（カーブして到達する）
- ゲート（セット16カウント中、4カウント目に出発して12カウント目に到着する）
- ホールド（セット到達後、次のセットまで静止するカウント）
- イージング（加速・減速のかけ方）

```rust
pub enum RouteShape {
    Straight,
    Curve { control: Point },        // 二次ベジェ
    Path  { via: Vec<Point> },       // 折れ線・フォロー用
}

pub enum Easing { Linear, EaseIn, EaseOut, Smooth }

/// セット内の相対カウントで表す出発・到着タイミング。
pub struct Gate { pub depart: f32, pub arrive: f32 }

pub struct Route { pub shape: RouteShape, pub gate: Gate, pub easing: Easing }

/// 1,000人でも JSON とメモリが膨らまないようスパースに持つ。
pub struct RouteTable {
    pub default: Route,
    pub overrides: BTreeMap<PerformerId, Route>,
}

pub struct SetCounts {
    pub moves: u16,   // 次セットへ動くカウント
    pub hold:  u16,   // 到達後の静止カウント
}
```

補間APIは正規化progressではなく**セット内ローカルカウント**を受ける形に変える。
これをやらないとゲートが表現できない。

```rust
// 旧: positions_at(&self, set_index: usize, progress: f32, out: &mut Vec<Point>)
pub fn positions_at_count(&self, set_index: usize, local_count: f32, out: &mut Vec<Point>);
```

既存の `positions_at` は `local_count = progress * counts` に変換する薄いラッパとして残し、
既存テストとレンダリング呼び出しを壊さずに移行する。

## A-3. マーチングスタイルと歩幅（数値ではなくドメイン語彙で）

現状 `analyze_transition(doc, i, 0.75, 1.0)` のように閾値が**生の float リテラル**。
ドリルデザイナーは「8 to 5」「6 to 5」で考えるので、その語彙をコアに持たせる。

```rust
pub enum StepStyle {
    EightToFive,                        // 5ヤード=8歩 = 22.5 inch
    SixToFive,
    TwelveToFive,
    Custom { steps_per_five_yards: f32 },
}

impl StepStyle {
    pub fn step_inches(&self) -> f32;
    pub fn steps_for(&self, distance_yards: f32) -> f32;
}

pub enum StrideRating { Comfortable, Aggressive, Impossible }

pub struct ClinicParams {
    pub style: StepStyle,
    pub collision_radius: f32,      // 単位はグリッド単位（yd/m）
    pub aggressive_above: f32,      // step_inches 比
    pub impossible_above: f32,
}
```

`Document` に `pub style: StepStyle` を持たせ、`ClinicParams::from_document` で既定値を導く。
コンティニュイティ出力も「12カウントで8 to 5」と書けるようになる。

## A-4. 掃引衝突検査（毎フレームO(n²)の置き換え）

現状の衝突判定は**到着セットの静止状態のみ**。実際の衝突は移動の途中で起きる。
かつ O(n²) 総当たり。両方まとめて設計し直す。

```rust
pub struct CollisionEvent { pub a: PerformerId, pub b: PerformerId, pub count: f32, pub distance: f32 }

/// フレーム間で再利用する作業領域。確保はここに閉じ込める。
#[derive(Default)]
pub struct ScanScratch {
    positions: Vec<Point>,
    buckets:   Vec<u32>,     // 空間ハッシュのセル→先頭索引
    next:      Vec<u32>,     // チェイン
    events:    Vec<CollisionEvent>,
}

/// セル幅 = collision_radius の一様格子。近傍9セルのみ比較するので実質 O(n)。
pub fn scan_transition(
    doc: &Document,
    set_index: usize,
    params: &ClinicParams,
    samples_per_count: u8,        // 既定 2（半カウント刻み）
    scratch: &mut ScanScratch,
) -> &[CollisionEvent];
```

アプリ側は**毎フレーム呼ばない**。`document_revision: u64` を持ち、
リビジョンが変わった時だけ（かつドラッグ中はデバウンスして）再走査し、結果をキャッシュ表示する。

**受け入れ基準**: 1,000人・16カウントの走査が単発2ms未満、走査中のヒープ確保ゼロ（2回目以降）。

## A-5. 演者メタデータとセクション

現状 `Performer { id, label, color }` のみ。ドリルブック、セクション単位選択、3D描画の身長差、
カラーガード/プロップの区別が全て表現できない。

```rust
pub struct Section {
    pub id: SectionId,
    pub name: String,          // "Trumpet"
    pub short: String,         // "Tp"
    pub color: [u8; 3],
}

pub struct Performer {
    pub id: PerformerId,
    pub label: String,         // ドリルナンバー "T1"
    pub section: SectionId,
    pub symbol: Symbol,        // Circle | Square | Triangle | Diamond | Cross
    pub color: Option<[u8;3]>, // None ならセクション色を継承
    pub height_m: f32,         // 3D表示用
}

pub enum PerformerKind { Wind, Percussion, Guard, Prop }
```

`Document.sections: Vec<Section>` を追加。これでセクション単位の選択・色分け・
ドリルブックのグループ化・カウントシートの「Tp のみ」出力が一気に実装可能になる。

## A-6. エラー型とi18n（英語UIを出す前に必須）

現状すべて `Result<_, String>` で、しかも**日本語の文字列が `drill-core` に埋まっている**
（`Document::validate`、`coordinates::readable`、`GridConfig::default` のハッシュ名など）。
`PRODUCT_QUALITY.md` は「日本語・英語UI」を要求しているので、このままでは英語化できない。
`ARCHITECTURE.md` の「coreはUIに依存しない」にも実質違反している。

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum DrillError {
    UnsupportedSchema { found: u16, supported: u16 },
    EmptySets,
    InvalidGrid { width: f32, height: f32 },
    SetSizeMismatch { set_index: usize, expected: usize, found: usize },
    DuplicatePerformerId(PerformerId),
    UnknownSet(SetId),
    Json(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Locale { Ja, En }

impl DrillError { pub fn message(&self, locale: Locale) -> String; }
```

人間可読テキストを返す関数（`coordinates::readable` / `continuity_text` / `count_sheet_text`）は
`Locale` を引数に取る。`GridLine` は `label: String` をやめて `id: HashKind` + 表示名解決に分離する。

## A-7. スキーマv2とマイグレーション（**今やる**）

`schema_version != 1` を即エラーにしているだけで、移行経路が無い。
A-1〜A-6 でドキュメント構造が大きく変わるので、**利用者のv1ファイルが増える前に**移行機構を入れる。

```rust
pub const SCHEMA_VERSION: u16 = 2;

fn migrate(value: serde_json::Value) -> Result<Document, DrillError> {
    match value.get("schema_version").and_then(|v| v.as_u64()) {
        Some(1) => migrate_v1_to_v2(value),
        Some(2) => serde_json::from_value(value).map_err(...),
        Some(v) => Err(DrillError::UnsupportedSchema { found: v as u16, supported: SCHEMA_VERSION }),
        None    => Err(DrillError::UnsupportedSchema { found: 0, supported: SCHEMA_VERSION }),
    }
}
```

v1→v2 の写像:
`SetId` を出現順に採番 / `counts` → `SetCounts { moves: counts, hold: 0 }` /
全演者をセクション `"Ensemble"` に所属させる / `RouteTable::default()`（Straight・全カウントゲート・Linear）。

**受け入れ基準**: v1ファイルをフィクスチャとしてリポジトリにコミットし、
「読み込み → v2として保存 → 再読込 → 演者数・座標・カウントが一致」を回帰テストにする。

---

# B. 実行基盤の穴

## B-1. `drill-render` クレート（新規・最重要の構造変更）

現状、描画コードは `main.rs` の egui painter の中だけにある。
一方 `svg.rs` は独自にSVGを組み立てている。**同じフィールドを2箇所が別々に描いている。**
`MEDIA_PIPELINE.md` の「同じproject/configから同じフレーム列を生成する」も、
`PRODUCT_QUALITY.md` の「座標表・SVG・PDF出力は同じドキュメント座標を参照する」も、
この状態では保証できない。動画書き出しに至っては描画側の再利用先が存在しない。

解決は**表示リスト（DisplayList）の中間表現**を1本入れること。

```
Document + count + Camera + RenderOptions
            │
            ▼
      drill-render::build()        ← 決定論的・純粋関数・UI非依存
            │
        DisplayList
       ┌────┼────┬──────────┐
       ▼    ▼    ▼          ▼
     egui  SVG  RGBA raster  (将来) wgpu instancing
```

```rust
pub enum DrawCmd {
    FieldFill { color: Rgba },
    Line   { a: Vec2, b: Vec2, width: f32, color: Rgba },
    Dot    { center: Vec2, radius: f32, symbol: Symbol, fill: Rgba, stroke: Rgba },
    Text   { at: Vec2, text: TextRef, size: f32, anchor: Anchor, color: Rgba },
    Trail  { points: Range<u32>, color: Rgba },   // points_pool への参照
}

pub struct DisplayList {
    pub cmds: Vec<DrawCmd>,
    pub points_pool: Vec<Vec2>,
    pub strings: Vec<String>,
}

impl DisplayList { pub fn clear(&mut self); }   // 再利用前提

pub fn build(scene: &Scene, out: &mut DisplayList);
```

これを入れると副次的に、`svg.rs` は `DisplayList → SVG` のシリアライザに縮み、
動画書き出しの `DisplayList → RGBA` が新規に1枚書くだけで済み、
経路可視化（ロードマップ2番）も `Trail` を足すだけになる。

## B-2. `drill-audio` クレート（新規・音が鳴らない）

`audio.rs` は**同期モデルのみ**で、`drill-core` の依存は `serde` だけ。実際の音は一切鳴らない。
README ロードマップ1番、`MEDIA_PIPELINE.md` P1 の中身がここ。

依存: `symphonia`（デコード）、`cpal`（出力）、`rubato`（必要ならリサンプル）。
`drill-core` には**入れない**（コアのUI/OS非依存を維持）。

```rust
pub struct AudioAsset {
    pub pcm: Arc<[f32]>,        // インターリーブ
    pub sample_rate: u32,
    pub channels: u16,
}

/// ズーム段階別 min/max ピーク。波形描画をO(画素)にする。
pub struct PeakPyramid { levels: Vec<Vec<(f32, f32)>> }   // 256 / 1024 / 4096 / 16384 samples/bucket
impl PeakPyramid {
    pub fn build(asset: &AudioAsset) -> Self;
    pub fn range(&self, start: u64, end: u64, out_buckets: usize, out: &mut Vec<(f32,f32)>);
}

/// 音声が有る時は sample index が時間の正、無い時は単調時計。
pub struct PlaybackClock { position_samples: Arc<AtomicU64>, sample_rate: u32 }

pub struct ClickSynth { pub accent_hz: f32, pub beat_hz: f32, pub decay_s: f32 }

pub struct LatencyCalibration { pub output_latency_ms: f32 }
```

要点:
- デコードは必ずワーカースレッド。UIスレッドで `symphonia` を回さない（`MEDIA_PIPELINE.md` 品質ゲート）。
- 現在位置は `AtomicU64` のサンプル索引。UIは毎フレームこれを読み `TempoMap::count_at` でカウントに変換。
- 出力コールバック内では確保・ロック・log を一切しない。
- 同期アンカーを複数持てるよう `AudioTrack.offset_seconds`（スカラー1個）を
  `anchors: Vec<SyncAnchor { count: f32, seconds: f32 }>` に拡張する。区間ごとに線形写像。

## B-3. ジョブ（非同期）基盤

現状 `save_to`、`export_text`、解析、`optimal_assignment` が全てUIスレッド同期。
1,000人のドキュメントで保存や割り当て最適化を走らせるとUIが止まる。

```rust
pub struct Job<T> {
    progress: Arc<AtomicU32>,      // 0..=10_000
    cancel:   Arc<AtomicBool>,
    rx:       Receiver<JobMsg<T>>,
}
pub enum JobMsg<T> { Progress(u32), Done(T), Failed(String), Cancelled }

impl<T> Job<T> { pub fn poll(&mut self) -> Option<JobMsg<T>>; pub fn cancel(&self); }
```

対象: 保存/自動保存、CSV/SVG/HTML書き出し、動画書き出し、衝突走査、割り当て最適化、音声デコード＋ピーク生成。
アプリは `Vec<ActiveJob>` を持ち、毎フレーム `poll` するだけ。`egui::Context::request_repaint` で進捗更新。

## B-4. 動画書き出し P0 の実体

`video.rs` は**設定検証とFFmpeg引数生成まで**。実際にフレームを吐いてパイプに流す部分が無い。
B-1（DisplayList）とB-3（Job）が入って初めて実装可能になる。

```rust
pub struct ExportJob {
    doc: Document,             // スナップショット（書き出し中の編集と分離）
    config: VideoExportConfig,
    range: Range<f32>,         // グローバルカウント
}

// ワーカー内のループ:
//   for frame in 0..total {
//       let count = tempo.count_at(frame as f64 * 1.0/fps + start_seconds);  // 有理数フレーム時刻
//       drill_render::build(&scene_at(count), &mut display_list);
//       raster::draw(&display_list, &mut rgba);      // 使い回しバッファ
//       stdin.write_all(&rgba)?;                     // ffmpeg -f rawvideo -pix_fmt rgba
//   }
```

- 映像時刻は `frame / fps` の**有理数**から求める（浮動小数の累加をしない）。
- 音声は `drill-audio` から trim/gain/fade 適用済みPCMを第2入力へ。OSループバック録音は使わない。
- 完了判定は `ffprobe` で解像度・FPS・尺・音声トラック有無を検証してから。
- 途中失敗時は部分ファイルを削除し、`software fallback`（GPUエンコーダ→libx264）を1回だけ再試行。

## B-5. プロジェクトコンテナとクラッシュ復旧

現状の保存は単一JSON。音声ファイルは絶対パス文字列。プロジェクトを別マシンへ渡すと壊れる。

- `.drillproj` = zip（`document.json` + `assets/`）、または同名フォルダ形式。
- 音声・画像は**プロジェクト相対パス**。欠落しても `AssetState::Missing` として開ける
  （`PRODUCT_QUALITY.md`「音声・画像が欠落してもドリル本体を開ける」）。
- `std::panic::set_hook` でパニックを `crash-<timestamp>.drill.json` + バックトレースに変換し、
  **元ファイルは書き換えない**。次回起動時に復旧候補として提示。

---

# C. 品質ゲート未達

## C-1. `main.rs` 1,837行 / 28フィールドの god struct の分解

`ARCHITECTURE.md` は「`drill-app` は表示と入力の変換に限定」と書いているが、
現状の `main.rs` には自動割り当て、レイアウト確定、選択境界計算、3D描画が同居している。

```
crates/drill-app/src/
  main.rs            // 起動とフォント設定のみ
  app.rs             // DrillApp: 各Stateの合成と毎フレームのpoll
  state/
    document.rs      // Document + History + dirty + revision
    playback.rs      // 再生状態機械（PlaybackRange, clock, loop）
    selection.rs     // Selection型: 索引集合 + セクション/セット単位操作
    jobs.rs          // ActiveJob一覧
  view/
    field2d.rs
    stadium3d.rs
    display_list.rs  // DisplayList → egui::Painter
  panels/
    toolbar.rs  inspector.rs  timeline.rs  clinic.rs  export.rs  audio.rs
  shortcuts.rs
```

`Selection` を型にすると「セクションで選択」「セット跨ぎで同一演者を追う」が素直に書ける。

## C-2. 足りていないテスト

`PRODUCT_QUALITY.md` が要求しているのに存在しないもの。

| テスト | 対象ゲート |
|---|---|
| v1フィクスチャの移行往復 | 「未対応の将来形式や破損データを黙って開かない」 |
| 10,000編集ストレス + 不変条件検査 | 「10,000回の編集コマンドを含むストレステスト」 |
| SVG / CSV / HTML のゴールデン比較 | 「出力は同じドキュメント座標を参照する」 |
| 再生決定論（同一doc+config→同一カウント列） | 「再生位置が決定論的」 |
| `count_at(seconds_at(c)) ≈ c` の property test | テンポマップの往復整合 |
| 1,000人衝突走査ベンチ + 確保ゼロ検証 | 「フレーム内ヒープ再確保ゼロ」 |
| 2時間再生のメモリ推移 | 「常駐メモリの継続増加がない」 |

ゴールデンテストは `.expected` ファイルをコミットして単純文字列比較で足りる（追加依存なし）。
確保ゼロ検証は既存の `interpolation_reuses_output_allocation` と同じくポインタ比較で行う。

## C-3. アクセシビリティと発見性

- 警告表示が色のみ（`Color32::from_rgb` の緑/赤/橙）。**文字と形**を併記する
  （`● 衝突候補: 0` → `✓ 衝突候補なし` / `⚠ 衝突候補 3件`）。
- メニューバーが無い。「全操作とショートカットはメニューバーから発見できる」が未達。
- Windows 100–200% スケーリングの検証手順が無い。

---

# 実装順（Codexへ渡す単位）

依存関係が実在するので、Wave 0 は直列。Wave 1 以降は並行可能。

### Wave 0 — 土台（直列・これ抜きで他を進めると全部やり直しになる）

1. `DrillError` 導入、`Result<_, String>` を全廃、`Locale` を人間可読テキスト関数に導入
2. `SetId` / `PerformerId` の安定ID化、`Document` に採番カウンタ
3. `Edit` enum + `apply→逆操作` + `VecDeque` History、`main.rs` の全変更経路を `Edit` に集約（既存バグ1・2・5が消える）
4. `SCHEMA_VERSION = 2` + `migrate_v1_to_v2` + v1フィクスチャ回帰テスト

### Wave 1 — 並行3本

| 系統 | 内容 |
|---|---|
| **A: ドメイン** | `RouteTable` / `Gate` / `SetCounts` / `positions_at_count` / `StepStyle` / `ScanScratch` による掃引衝突検査 / `Section`・`Performer` 拡張 |
| **B: 描画** | `drill-render` クレート新設、`DisplayList` 定義、egui backend と SVG backend を載せ替え（`svg.rs` をシリアライザに縮小） |
| **C: 音声** | `drill-audio` クレート新設、symphonia デコード + `PeakPyramid` + cpal 出力 + `ClickSynth` + 複数 `SyncAnchor` |

A と B は `Route` の描画（Trail）で最後に合流するので、`DrawCmd::Trail` の形だけ先に固定しておく。

### Wave 2

5. `Job<T>` 基盤、保存/書き出し/走査/割り当てをワーカーへ退避
6. 動画書き出し P0（`DisplayList → RGBA → ffmpeg pipe → ffprobe 検証`、進捗・キャンセル・fallback）
7. `.drillproj` コンテナ、相対アセットパス、欠落許容、パニックフック + 復旧画面

### Wave 3

8. `main.rs` 分解（C-1のツリー）、メニューバー、色以外の警告表現、`Selection` 型
9. C-2 のテスト・ベンチ一式、`PRODUCT_QUALITY.md` のベースライン再計測

---

## 設計上の不変条件（レビュー時のチェックリスト）

- `drill-core` は `serde` / `serde_json` 以外に依存しない。UI・GPU・OS・音声デバイスを知らない。
- `drill-core` は日本語文字列リテラルを持たない。全ての人間可読テキストは `Locale` 経由。
- `Document` の変更は必ず `Edit::apply` を通る。UI から直接フィールドを書き換えない。
- セットの同一性は `SetId`、順序は `Vec` の索引。両者を混同する API を作らない。
- 毎フレーム走る関数（補間・描画・走査）は `&mut` の作業領域を受け取り、内部で確保しない。
- 音声時刻はサンプル索引、映像時刻は有理数フレーム時刻。浮動小数の累加で時間を進めない。
- 同じ `(Document, config, count)` からは常に同じ `DisplayList` が出る。
