# 43. アプリケーション構造と UX・アクセシビリティ

## 1. 目的と範囲

`crates/drill-app/src/main.rs` は 1,838 行の単一ファイルで、起動・テーマ・状態保持・入力解釈・
業務ロジック（自動割り当て、レイアウト確定、選択境界計算）・2D描画・3D描画・ファイルI/O・
書き出しUI が全て同居している。`ARCHITECTURE.md` の「`drill-app` は表示と入力の変換に限定し、
業務ロジックを追加しない」に違反している。

この文書が決めるもの:

1. `drill-app` のモジュール分解と、既存 `DrillApp` の 28 フィールドの移送先（フィールド単位）
2. 状態の分割（ドキュメント / 再生 / 選択 / ツール / ジョブ / UI）と依存方向
3. 入力処理（ポインタ・ショートカット・ツールモード）とショートカットの一元定義
4. 画面構成（メニューバー・ツールバー・フィールド・タイムライン・インスペクタ・
   セットリスト・クリニック・ジョブ）とドッキング／レイアウト保存
5. 初回起動体験（説明書なしで「セット選択 → 演者選択 → 編集 → 再生」へ到達する導線）
6. アクセシビリティ（色以外の表現、キーボード操作、フォーカス、DPI、コントラスト、AccessKit）
7. 破壊的操作の確認方針
8. 大規模ドキュメント（1,000人 / 64セット / 2,048カウント）でのUI応答性の設計
9. 他の設計文書（23 カメラ / 30 音声 / 17 印刷 / 40 ジョブ 等）からのUI要件の受け口

**この文書が扱わないこと**:

- `Edit` コマンド代数そのもの（10）、遷移モデル（11）、解析アルゴリズム（13）
- `DisplayList` の内容と描画パイプライン（20 / 21 / 22）。本書は `DrawCmd → egui::Painter`
  の変換層の**置き場所**だけを決める
- `Job<T>` の実装（40）。本書はジョブの**表示と操作**だけを決める
- 印刷レイアウト（17）、音声エンジン（30）、カメラモデル（23）、永続化形式（41）、
  `DrillError` / `Locale` の定義（42）

---

## 2. 現状

### 2.1 ファイルと規模

| ファイル | 行数 | 内容 |
|---|---|---|
| `crates/drill-app/src/main.rs` | 1,838 | 全て |
| `crates/drill-app/Cargo.toml` | 11 | `eframe 0.35`（`default-features = false`）+ `rfd 0.17` |

`drill-app` にはこの 1 ファイル以外のソースが無い。テストも無い。

### 2.2 `DrillApp` の 28 フィールド

`main.rs:69-98`。DESIGN_GAPS C-1 は「30フィールド」と書いているが、実際は 28 である。

`document` `view_mode` `camera` `beats_per_measure` `current_set` `count_position` `playing`
`speed` `tempo_bpm` `playback_start` `playback_end` `loop_playback` `last_frame`
`frame_positions` `selected` `history` `drag_before` `drag_origin` `marquee_origin`
`current_path` `dirty` `status` `last_autosave` `show_guidance` `video_export`
`video_preset` `video_advanced` `ffmpeg_status`

### 2.3 有るもの

- `eframe::App::ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame)`（`main.rs:461`）。
  egui 0.34 で `App::update` が非推奨化され `App::ui` が主経路になった API に既に追従している。
- 日本語フォント埋め込みと暗色テーマ（`main.rs:34-61`）。
- **メニューバーは既に存在する**（`main.rs:555-643`、`egui::MenuBar::new().ui(...)`）。
  DESIGN_GAPS C-3 の「メニューバーが無い」は現在のコードに対して**古い記述**である。
  ただし項目は 5 メニュー・**操作可能項目 13 個**しかない（ヘルプメニューの 5 行は
  `ui.label` で操作不能）。
- ショートカットは 4 個のみ（`main.rs:516-539`: `Ctrl+Z` / `Ctrl+Y` / `Ctrl+S` / `Space`）。
- 未保存表示は既に文字併記（`main.rs:671-675`: `● 未保存` / `✓ 保存済み`）。
- 空状態カード（`main.rs:1441-1467`）と操作ガイドバー（`main.rs:739-755`）。
- 番号付き見出し（`1. セットを選ぶ` … `8. 音源 / カウント`）による手順の暗示。

### 2.4 無いもの・壊れているもの（行番号つき）

| # | 箇所 | 内容 |
|---|---|---|
| D-1 | `Cargo.toml:9` | `eframe` を `default-features = false` で使っており、既定機能の **`accesskit` が落ちている**。`Cargo.lock` に `accesskit_winit` が存在しない。eframe は `winit_integration.rs:168` の `InitialTreeRequested` 経路でしか `Context::enable_accesskit()` を呼ばないため、**Windows UI Automation にアクセシビリティツリーが一切出ていない**（ナレーター／NVDA から中身が見えない）。 |
| D-2 | `main.rs:465` / `main.rs:469` | 非活性ウィジェットの文字色 `(22,27,34)` と背景 `(36,45,58)` のコントラスト比は **1.26:1**。WCAG AA の 4.5:1 に対して致命的に不足。全てのボタン文字が該当する。 |
| D-3 | `main.rs:810` | 左カラムは `allocate_ui_with_layout(Vec2::new(300.0, ui.available_height()), ...)` で、**`ScrollArea` が無い**。セットリスト＋インスペクタ＋クリニック＋グリッド＋テンポ＋書き出し＋音源が縦に並ぶため、200% スケーリング（＝論理高さが半分）ではコンテンツの大半に到達できない。 |
| D-4 | `main.rs:20` | `with_inner_size([1280.0, 800.0])` は論理サイズ。200% では物理 2560×1600 を要求し、1920×1080 の画面に収まらない。`with_min_inner_size` も未設定。 |
| D-5 | `main.rs:1011` | `analyze_transition` を毎フレーム無条件呼び出し。O(n²)。1,000人で約 50 万回/フレーム。 |
| D-6 | `main.rs:1033` | `pathing::transition_stats` → `transition_moves` が毎フレーム `Vec` を確保。 |
| D-7 | `main.rs:1794` | カウントトラックの目盛りを `0..=timeline_counts()` で全数描画。2,048 カウントで 2,048 本の `line_segment` ＋ 512 個の `text`。1 画素に複数本が重なる。 |
| D-8 | `main.rs:510` `845` `877` `1165` `1573` ほか | `self.document.sets[self.current_set]` の直接添字。`current_set` は索引であり、セット削除や Undo で範囲外になると **panic** する。 |
| D-9 | `main.rs:828-841` | セット複製が `history` に積まれない（DESIGN_GAPS 既知バグ 1）。しかもここで `self.document.sets.insert(..)` と**ドキュメントを直接書き換えている**（不変条件 1 違反）。 |
| D-10 | `main.rs:1057-1132` / `1139-1183` / `1351-1398` | グリッド・テンポ・音源のウィジェットが `&mut self.document.…` を直接束縛して書き換えている。Undo 不可。 |
| D-11 | `main.rs:236-247` `430-457` `1268-1272` | 保存・読込・FFmpeg 検出が全て UI スレッド同期。`rfd` のダイアログもブロッキング。 |
| D-12 | `main.rs:245` `453` | ステータス行にフルパスを表示（`path.display()`）。画面共有時にユーザー名が露出する。 |
| D-13 | `main.rs:1015-1031` | LIVE CLINIC の可否が `Color32` の緑／赤／橙のみ。文字は `● 衝突候補: 0` で記号が状態を表していない。 |
| D-14 | `main.rs:1768-1792` | 再生範囲の IN/OUT が緑線／橙線のみ。形も文字も無い。再生ヘッドも赤線のみ（`main.rs:1814-1826`）。 |
| D-15 | `main.rs:1435` | フィールドビューは `allocate_painter(_, Sense::click_and_drag())`。**フォーカス不可**。キーボードだけでは演者を1人も選択できない。 |
| D-16 | 全体 | 発見性: 操作可能ウィジェットは概算 **120 個**（`ui.button` 23 / `small_button` 26 / `selectable_value` 21 / `checkbox` 10 / `DragValue` 23 / `Slider` 6 / `ComboBox` 3 / `add_enabled` 7 / `selectable_label` 1）。うちメニューバーから到達できるのは 13 個、**約 11%**。`PRODUCT_QUALITY.md`「全操作とショートカットはメニューバーから発見できる」を満たしていない。 |
| D-17 | `Cargo.toml:9` | `persistence` 機能が無効。ウィンドウ位置・パネル幅・ズーム倍率・最近使ったファイルが一切保存されない（`NativeOptions::persist_window` は既定 true だが `persistence` が無いと効かない）。 |
| D-18 | 全体 | 確認ダイアログが 1 つも無い。未保存のまま `×` で閉じると**無警告でデータが消える**。 |
| D-19 | 全体 | 日本語文字列が `main.rs` にリテラル直書き。`Locale` 切替の受け口が無い。 |

### 2.5 egui 0.35 / eframe 0.35 側の事実（実ソースで確認）

`~/.cargo/registry/src/*/egui-0.35.0` と `eframe-0.35.0`、および `Cargo.lock` を確認した。

- **AccessKit**: egui 0.34 で egui 側の `accesskit` フィーチャは廃止され、**常時依存**になった
  （本リポジトリの `Cargo.lock` では `accesskit 0.24.1`）。ツリー生成は遅延で、
  `Context::enable_accesskit()`（`context.rs:3599`）が呼ばれるまで一切走らない
  （`context.rs:507` の `if self.is_accesskit_enabled`）。
  プラットフォーム連携（Windows UIA / macOS NSAccessibility）は **eframe 側の `accesskit`
  フィーチャ**（既定ON、`eframe/Cargo.toml` → `egui-winit/accesskit` → `accesskit_winit`）が担う。
  ウィジェットの意味付けは `Response::widget_info`（`response.rs:849`）、
  ラベル関連付けは `Response::labelled_by`（`response.rs:983`）。
  出力は `FullOutput.platform_output.accesskit_update`（`context.rs:2608`）で、
  **ヘッドレステストから直接検証できる**。
- **DPI**: `pixels_per_point = Options::zoom_factor * ViewportInfo::native_pixels_per_point`
  （`context.rs:448`）。`Context::set_zoom_factor`（`context.rs:2269`）、
  `Context::zoom_factor`（`context.rs:2251`）。`Options::zoom_with_keyboard` は既定 true
  （`memory/mod.rs:322`）なので `Ctrl +/-/0` は最初から効く。メニュー用ヘルパ
  `egui::gui_zoom::zoom_menu_buttons(ui)`（`gui_zoom.rs:72`）がある。
  Windows の 200% は `GetDpiForWindow()==192`、`native_pixels_per_point == 2.0` に相当する。
- **レイアウト**: 0.34 で `SidePanel` / `TopBottomPanel` が `egui::Panel` に統合された。
  `Panel::left(id)` / `right` / `top` / `bottom`（`containers/panel.rs:222-247`）、
  `resizable` / `default_size` / `size_range` / `show_collapsible` / `show_animated_inside`。
  パネル幅は `PanelState` として egui memory に載る（`panel.rs:41`）ので、
  eframe の `persistence` を有効化すれば自動で永続化される。
- **モーダル**: `egui::Modal::new(Id)`（`containers/modal.rs:26`）＋ `ModalResponse::should_close()`。
- **終了阻止**: `ViewportInfo::close_requested()`（`viewport_info.rs:111`）と
  `ViewportCommand::CancelClose`（`viewport.rs:1088`）。
- **ショートカット表示**: `Context::format_shortcut`（`context.rs:1666`）が
  macOS では `⌘`、他では `Ctrl+` を出す。
- **テキスト編集中の判定**: `Context::text_edit_focused()`（`context.rs:2889`）。
- **ヘッドレス実行**: `Context::run_ui(RawInput, impl FnMut(&mut Ui)) -> FullOutput`
  （`context.rs:780`）。CI から egui を回してパネルを計測・検証できる。
- **仮想化**: `ScrollArea::show_rows`（`scroll_area.rs:984`）。
- **計測フック**: 0.33 の `egui::Plugin`（`plugin.rs`）に `input_hook` / `output_hook` があり、
  フレーム計測を UI コードに散らさず差し込める。

---

## 3. 設計

### 3.1 モジュール分解

```
crates/drill-app/src/
  main.rs                     起動のみ。NativeOptions・panic hook・run_native。
  boot/
    mod.rs                    CreationContext からの初期化手順
    fonts.rs                  install_fonts（Locale でフォント優先順を切替）
    theme.rs                  DrillTheme（暗/明・コントラスト検証つき）
    window.rs                 モニタ作業領域へのウィンドウクランプ
  app.rs                      DrillApp。状態の合成とフレームパイプライン。
  state/
    mod.rs                    AppState / AppView
    document.rs               DocumentState（Document + History + revision + path）
    playback.rs               PlaybackState（Transport / Cursor / Range）
    selection.rs              Selection（PerformerId 集合 + 索引キャッシュ）
    tool.rs                   ToolState（Tool / DragSession / Marquee / Snap）
    jobs.rs                   JobState（ActiveJob 一覧・FFmpeg 検出結果）
    ui.rs                     UiState（レイアウト・ズーム・Locale・状態行・オンボーディング）
    derived.rs                DerivedCache（frame_positions / clinic / 表示文字列）
  input/
    mod.rs
    shortcuts.rs              Command 列挙と CommandSpec 表（唯一の定義箇所）
    dispatch.rs               Command → Intent
    pointer.rs                フィールド上のヒットテスト・ドラッグ・矩形選択 → Intent
    intent.rs                 Intent / IntentQueue
  view/
    mod.rs
    viewport.rs               FieldViewport（field↔screen 変換の唯一の実装）
    field2d.rs                2D フィールド（DisplayList → Painter ＋ 入力領域）
    stadium3d.rs              3D スタジアム（21/22 へ移すまでの隔離先）
    display_list.rs           DrawCmd → egui::Painter
  panels/
    mod.rs                    PanelId / PanelUi / PanelScratch / dock レイアウト
    menubar.rs                CommandSpec 表からメニューを生成
    toolbar.rs
    setlist.rs
    inspector.rs              InspectorSection の合成
    timeline.rs               カウントトラック（＋30 の波形レーン受け口）
    clinic.rs
    export.rs
    audio.rs
    jobs.rs
    statusbar.rs
    welcome.rs                初回起動・空状態
  dialogs/
    mod.rs                    DialogStack（深さ1）
    confirm.rs                ConfirmRequest / 破壊的操作の確認
    shortcuts_help.rs         ショートカット一覧（Shift+F1）
  i18n.rs                     TextId / TextTable（UI 文字列。core の Locale を使う）
  lib.rs                      統合テストから叩けるようライブラリクレート化
```

`lib.rs` を追加して `drill-app` を **lib + bin** にする。これが無いと `tests/` から
`Command` 表やパネルをヘッドレス検証できない。

**目標行数**:

| 対象 | 目標 | 根拠 |
|---|---|---|
| `main.rs` | **60 行以下** | `NativeOptions` 構築・panic hook・`run_native` のみ |
| `app.rs` | 250 行以下 | パイプライン 7 段と intent 適用 |
| `panels/*.rs`（各） | 400 行以下 | CI で機械検査 |
| `view/*.rs`（各） | 400 行以下 | 同上 |
| `state/*.rs`（各） | 250 行以下 | 同上 |
| `drill-app` 合計 | 3,400 行前後 | 1,838 行から増える。増分はメニュー網羅・ジョブUI・ダイアログ・i18n・仮想化・テスト用API |

「合計行数が増える」ことは劣化ではない。判定基準は**1ファイルの責務が1つか**と
**業務ロジックが `drill-app` に残っていないか**である（3.9）。

### 3.2 状態管理

egui は即時モードなので、フレームを跨ぐものは全て明示的に持つ。
6 つに分け、**相互参照を持たせない**。状態間の作用は必ず `Intent` を経由する。

```
        ┌──────────────┐
        │ DocumentState│  ← 唯一の真実。Edit 以外で変わらない。
        └──────┬───────┘
     read      │ read            read
  ┌────────────┼───────────────┬──────────────┐
  ▼            ▼               ▼              ▼
Selection   Playback        DerivedCache    UiState
  │            │               ▲              │
  └────────────┴───────────────┘              │
        （revision と cursor のみを鍵に再計算）  │
                                                │
                     ToolState ◀────────────────┘
                          │
                       JobState
```

依存の向きは上記の一方向のみ。`DocumentState` は他のどの状態も知らない。

```rust
// state/mod.rs
pub struct AppState {
    pub doc: DocumentState,
    pub playback: PlaybackState,
    pub selection: Selection,
    pub tool: ToolState,
    pub jobs: JobState,
    pub ui: UiState,
    pub derived: DerivedCache,
}

/// Read-only projection handed to every panel and view.
/// Panels cannot mutate anything through this type.
pub struct AppView<'a> {
    pub doc: &'a Document,
    pub revision: u64,
    pub dirty: bool,
    pub path: Option<&'a Path>,
    pub playback: &'a PlaybackState,
    pub selection: &'a Selection,
    pub tool: &'a ToolState,
    pub jobs: &'a JobState,
    pub ui: &'a UiState,
    pub derived: &'a DerivedCache,
    pub locale: Locale,
    pub text: &'a TextTable,
}

impl AppState {
    pub fn view<'a>(&'a self, text: &'a TextTable) -> AppView<'a> { /* … */ }
}
```

#### DocumentState

```rust
// state/document.rs
pub struct DocumentState {
    doc: Document,                 // private: no direct writes from anywhere
    history: History,
    revision: u64,
    saved_revision: u64,
    path: Option<PathBuf>,
    autosave_at: Instant,
}

impl DocumentState {
    pub fn doc(&self) -> &Document { &self.doc }
    pub fn revision(&self) -> u64 { self.revision }
    pub fn is_dirty(&self) -> bool { self.revision != self.saved_revision }
    pub fn can_undo(&self) -> bool { self.history.can_undo() }
    pub fn can_redo(&self) -> bool { self.history.can_redo() }

    /// The only mutation path. `Edit` and `History` are defined by design doc 10.
    pub(crate) fn apply(&mut self, edit: Edit) -> Result<(), DrillError> {
        let inverse = edit.apply(&mut self.doc)?;
        self.history.push(inverse);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub(crate) fn undo(&mut self) -> bool { /* … */ }
    pub(crate) fn redo(&mut self) -> bool { /* … */ }
    pub(crate) fn replace(&mut self, doc: Document, path: Option<PathBuf>) { /* 読込・復旧 */ }
    pub(crate) fn mark_saved(&mut self, path: PathBuf) { /* … */ }
}
```

`doc` が private で、`apply` が `pub(crate)` かつ `AppView` は `&Document` しか渡さない。
これで「パネルからドキュメントを直接書き換える」は**コンパイル時に不可能**になる。
`dirty: bool`（`main.rs:90`）はリビジョン比較に置き換えて廃止する。

#### PlaybackState

```rust
// state/playback.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transport { Stopped, Playing, Scrubbing }

/// Identity is the SetId; the index is looked up per frame and never stored.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cursor { pub set: SetId, pub local_count: f32 }

pub struct PlaybackState {
    pub transport: Transport,
    cursor: Cursor,
    pub range: PlaybackRange,
    pub looping: bool,
    pub speed: f32,
    last_frame: Instant,
}

impl PlaybackState {
    /// Resolves the cursor against the current document, falling back to the
    /// first set when the SetId no longer exists (undo of an insert, etc.).
    pub fn resolve(&self, doc: &Document) -> (usize, f32) {
        match doc.set_index(self.cursor.set) {
            Some(i) => (i, self.cursor.local_count),
            None => (0, 0.0),
        }
    }
    pub fn global_count(&self, doc: &Document) -> f32 { /* … */ }
    pub(crate) fn seek_global(&mut self, doc: &Document, count: f32) { /* … */ }
    pub(crate) fn advance(&mut self, doc: &Document, now: Instant) -> AdvanceResult { /* … */ }
}
```

`resolve` を通すことで D-8（添字 panic）が構造的に消える。

#### Selection

```rust
// state/selection.rs
pub struct Selection {
    ids: BTreeSet<PerformerId>,
    // Caches, rebuilt only when the document revision changes.
    cache_revision: u64,
    indices: Vec<u32>,             // sorted, dense, aligned to Document::performers
    bounds: Option<(Point, Point)>,
}

pub enum SelectionOp {
    Replace(Vec<PerformerId>),
    Add(Vec<PerformerId>),
    Toggle(PerformerId),
    Remove(Vec<PerformerId>),
    All,
    None,
    Invert,
    Section(SectionId),            // 15 の Section が入ったら有効
    Rect { min: Point, max: Point, additive: bool },
}

impl Selection {
    pub fn ids(&self) -> &BTreeSet<PerformerId> { &self.ids }
    pub fn indices(&self) -> &[u32] { &self.indices }
    pub fn len(&self) -> usize { self.ids.len() }
    pub fn bounds(&self) -> Option<(Point, Point)> { self.bounds }
    pub(crate) fn apply(&mut self, op: SelectionOp, doc: &Document, revision: u64);
    /// Drops ids that no longer exist. Called whenever the revision changes.
    pub(crate) fn revalidate(&mut self, doc: &Document, revision: u64);
}
```

現在の `selected: BTreeSet<usize>`（索引）から `BTreeSet<PerformerId>`（安定ID）へ変える。
`00-conventions.md` の不変条件 2 を UI 側でも守るため。
`bounds` は `drill_core::editing::bounds()` の結果をキャッシュしたもので、
毎フレーム計算しない（現在 `selection_bounds` は `main.rs:894` で毎フレーム呼ばれ、
内部で `selected_points()` が `Vec` を確保している）。

#### ToolState

```rust
// state/tool.rs
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool { Select, Move, Shape(ShapeKind), Route }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapeKind { Line, Arc, Circle, Block, Spiral }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapMode { Grid, Step, Free }

pub struct DragSession {
    pub origin_field: Point,       // field coordinates, not screen
    pub ids: Vec<PerformerId>,
    pub before: Vec<Point>,
    pub preview: Vec<Point>,       // shown by the view; not written to the Document
    pub moved: bool,
}

pub struct Marquee { pub origin_field: Point, pub current_field: Point, pub additive: bool }

pub struct ToolState {
    pub active: Tool,
    pub snap: SnapMode,
    pub drag: Option<DragSession>,
    pub marquee: Option<Marquee>,
}
```

現在のドラッグは `main.rs:1573` で**ドラッグ中フレームごとに `Document` を直接書き換え**、
`drag_stopped` でまとめて履歴に積んでいる。これは不変条件 1 違反であり、
ドラッグ中に自動保存が走ると中間状態が保存される。
新設計では `DragSession::preview` に持ち、確定時に `Edit::MovePoints` を 1 件だけ発行する。
ドラッグ座標を screen ではなく field で持つのは、ドラッグ中にウィンドウがリサイズ／
ズームされても座標が飛ばないようにするため。

#### JobState / UiState

```rust
// state/jobs.rs — Job<T> の定義は 40。ここは表示と操作だけ。
pub struct ActiveJob {
    pub id: JobId,
    pub kind: JobKind,             // Save | Export | Scan | Decode | Video | Assign
    pub title: TextId,
    pub progress: f32,             // 0.0..=1.0
    pub cancellable: bool,
    pub started: Instant,
}

pub enum FfmpegProbe { Unknown, Probing, Available { version: String }, Missing }

pub struct JobState { pub active: Vec<ActiveJob>, pub ffmpeg: FfmpegProbe, pub log: VecDeque<JobOutcome> }
```

```rust
// state/ui.rs
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PanelId { Setlist, Inspector, Timeline, Clinic, Export, Audio, Jobs, Cameras, Print }

#[derive(Clone, Serialize, Deserialize)]
pub struct UiLayout {
    pub visible: BTreeMap<PanelId, bool>,
    pub left_width: f32,
    pub right_width: f32,
    pub timeline_height: f32,
    pub zoom_factor: f32,
    pub theme: ThemeChoice,
    pub locale: Locale,
    pub recent: Vec<PathBuf>,
}

pub enum Severity { Info, Success, Warning, Error }
pub struct StatusLine { pub text: String, pub severity: Severity, pub at: Instant }

pub struct OnboardingState { pub step: u8, pub dismissed: bool }

pub struct UiState {
    pub layout: UiLayout,
    pub view_mode: ViewMode,
    pub camera: Camera,            // 23 が所有権を取るまでの暫定
    pub status: StatusLine,
    pub toasts: VecDeque<Toast>,
    pub onboarding: OnboardingState,
    pub export: ExportState,
    pub dialog: DialogStack,
}
```

`UiLayout` は `eframe::Storage` に `eframe::set_value` で保存する（3.7）。

#### DerivedCache

```rust
// state/derived.rs
#[derive(Clone, Copy, PartialEq, Eq)]
struct FrameKey { revision: u64, set_index: u32, count_q: i32 }  // count_q = (local*64.0) as i32

pub struct DerivedCache {
    frame_positions: Vec<Point>,
    frame_key: Option<FrameKey>,

    clinic: ClinicCache,
    scan_scratch: ScanScratch,     // 13 / A-4 の作業領域。確保はここに閉じ込める。

    // Pre-rendered strings so panels never call format! per frame.
    labels: LabelCache,
}

struct ClinicCache {
    key: Option<(u64, SetId)>,
    dirty_since: Option<Instant>,  // debounce during drags
    stats: TransitionStats,
    collisions: u32,
    excessive_strides: u32,
}

impl DerivedCache {
    /// Called once per frame from app.rs, before any panel runs.
    pub(crate) fn update(&mut self, doc: &Document, revision: u64, cursor: (usize, f32), now: Instant);
    pub fn frame_positions(&self) -> &[Point] { &self.frame_positions }
    pub fn clinic(&self) -> &ClinicCache { &self.clinic }
    pub fn labels(&self) -> &LabelCache { &self.labels }
}
```

**再計算の規則**:

| 導出値 | 再計算の条件 |
|---|---|
| `frame_positions` | `FrameKey` が変化したとき（再生中は毎フレーム、静止中は 0 回） |
| `clinic` | `(revision, set)` が変化し、かつ `dirty_since` が **120 ms** 以上経過したとき |
| `labels` | `(revision, cursor 整数部, locale)` が変化したとき |
| `Selection::bounds` | `revision` または選択集合が変化したとき |

クリニックのデバウンスは、ドラッグ中に revision が毎フレーム増えても O(n) 走査を
1 秒あたり最大 8 回に抑えるためのもの。D-5 / D-6 の app 側の答えである。

#### 3.2.1 `DrillApp` 28 フィールドの移送表

`main.rs:69-98` の全フィールドについて、移送先・新しい名前と型・変更点を示す。
「削除」は他の値から導出できるため保持をやめるもの。

| # | 現フィールド（行） | 現在の型 | 移送先 | 新しい名前と型 | 変更点 |
|---|---|---|---|---|---|
| 1 | `document` (70) | `Document` | `state/document.rs` | `DocumentState::doc: Document`（**private**） | 直接書き換え不可。`apply(Edit)` のみ |
| 2 | `view_mode` (71) | `ViewMode` | `state/ui.rs` | `UiState::view_mode: ViewMode` | 変更なし |
| 3 | `camera` (72) | `Camera` | `state/ui.rs` | `UiState::camera: Camera` | 23 が `CameraTrack` を定義したら移す（未決 3） |
| 4 | `beats_per_measure` (73) | `u16` | `state/ui.rs` | `ExportState::beats_per_measure: u16` | 本来は `Document`。17 の決定待ち（未決 4） |
| 5 | `current_set` (74) | `usize` | `state/playback.rs` | `PlaybackState::cursor: Cursor { set: SetId, .. }` | **索引 → 安定ID**。添字 panic（D-8）が消える |
| 6 | `count_position` (75) | `f32` | `state/playback.rs` | `Cursor::local_count: f32` | 5 と一体化 |
| 7 | `playing` (76) | `bool` | `state/playback.rs` | `PlaybackState::transport: Transport` | `bool` → 3 状態（Stopped / Playing / Scrubbing） |
| 8 | `speed` (77) | `f32` | `state/playback.rs` | `PlaybackState::speed: f32` | 変更なし |
| 9 | `tempo_bpm` (78) | `f32` | **削除** | — | `doc.tempo.bpm_at(0.0)` が唯一の真。パネルのコピー編集で置換（3.4） |
| 10 | `playback_start` (79) | `u32` | `state/playback.rs` | `PlaybackState::range: PlaybackRange`（`.start: f32`） | `drill_core::playback::PlaybackRange` を直接持つ。`u32↔f32` 変換の散在をやめる |
| 11 | `playback_end` (80) | `u32` | `state/playback.rs` | `PlaybackRange::end: f32` | 同上 |
| 12 | `loop_playback` (81) | `bool` | `state/playback.rs` | `PlaybackState::looping: bool` | 変更なし |
| 13 | `last_frame` (82) | `Instant` | `state/playback.rs` | `PlaybackState::last_frame: Instant`（private） | `advance` の内部に隠す |
| 14 | `frame_positions` (83) | `Vec<Point>` | `state/derived.rs` | `DerivedCache::frame_positions: Vec<Point>` | `FrameKey` ゲートで再計算を抑制 |
| 15 | `selected` (84) | `BTreeSet<usize>` | `state/selection.rs` | `Selection::ids: BTreeSet<PerformerId>` | **索引 → 安定ID**。`indices` / `bounds` のキャッシュを併設 |
| 16 | `history` (85) | `History` | `state/document.rs` | `DocumentState::history: History` | `Edit` ベース（10）へ。`revision` と連動 |
| 17 | `drag_before` (86) | `Option<Vec<Point>>` | `state/tool.rs` | `DragSession::before: Vec<Point>` | `DragSession` に統合 |
| 18 | `drag_origin` (87) | `Option<Pos2>` | `state/tool.rs` | `DragSession::origin_field: Point` | **screen → field 座標**。ズーム/リサイズで飛ばない |
| 19 | `marquee_origin` (88) | `Option<Pos2>` | `state/tool.rs` | `ToolState::marquee: Option<Marquee>` | 現在位置と `additive` も保持 |
| 20 | `current_path` (89) | `Option<PathBuf>` | `state/document.rs` | `DocumentState::path: Option<PathBuf>` | 表示時は `display_path()` で `~` 置換（D-12） |
| 21 | `dirty` (90) | `bool` | **削除** | — | `is_dirty() = revision != saved_revision` で導出 |
| 22 | `status` (91) | `String` | `state/ui.rs` | `UiState::status: StatusLine { text, severity, at }` | 重大度を型で持ち、色以外でも表現（3.10.4） |
| 23 | `last_autosave` (92) | `Instant` | `state/document.rs` | `DocumentState::autosave_at: Instant`（private） | ドラッグ中は延期（6章） |
| 24 | `show_guidance` (93) | `bool` | `state/ui.rs` | `UiState::onboarding: OnboardingState { step, dismissed }` | 常時バー → 4 ステップのツアー（3.8） |
| 25 | `video_export` (94) | `VideoExportConfig` | `state/ui.rs` | `ExportState::video: VideoExportConfig` | 変更なし |
| 26 | `video_preset` (95) | `ExportPreset` | `state/ui.rs` | `ExportState::video_preset: ExportPreset` | 変更なし |
| 27 | `video_advanced` (96) | `bool` | `panels/mod.rs` | `PanelScratch::video_advanced: bool` | 開閉状態のみ。失っても情報が消えない |
| 28 | `ffmpeg_status` (97) | `String` | `state/jobs.rs` | `JobState::ffmpeg: FfmpegProbe` | 文字列 → 列挙。検出はジョブ化（D-11） |

追加される状態（現在は存在しないもの）:
`DocumentState::{revision, saved_revision}` /
`Selection::{indices, bounds, cache_revision}` /
`ToolState::{active, snap}` / `JobState::{active, log}` /
`UiState::{layout, toasts, dialog}` / `DerivedCache::{clinic, scan_scratch, labels, frame_key}`。

### 3.3 Intent — 唯一の変更経路

```rust
// input/intent.rs
pub enum Intent {
    Edit(Edit),                        // drill-core。Document を変える唯一の手段。
    Undo,
    Redo,
    Seek { global_count: f32 },
    Transport(Transport),
    SetRange(PlaybackRange),
    SetLooping(bool),
    SetSpeed(f32),
    Select(SelectionOp),
    Tool(ToolIntent),
    Ui(UiIntent),
    Job(JobRequest),                   // 40 が JobRequest を定義する
    Dialog(DialogIntent),
    Status { text: String, severity: Severity },
}

#[derive(Default)]
pub struct IntentQueue { items: Vec<Intent> }

impl IntentQueue {
    pub fn push(&mut self, intent: Intent) { self.items.push(intent); }
    pub fn edit(&mut self, edit: Edit) { self.items.push(Intent::Edit(edit)); }
    pub(crate) fn drain(&mut self) -> std::vec::Drain<'_, Intent> { self.items.drain(..) }
    pub fn is_empty(&self) -> bool { self.items.is_empty() }
}
```

パネルは `&AppView` と `&mut IntentQueue` しか受け取らない。
`Document`・`PlaybackState`・`Selection` への `&mut` はパネルに到達しない。

### 3.4 パネルのインタフェース

```rust
// panels/mod.rs
/// Widget-local, non-authoritative state. Losing it must lose no user data.
#[derive(Default)]
pub struct PanelScratch {
    pub video_advanced: bool,
    pub collapsed: BTreeMap<PanelId, bool>,
    pub performer_filter: String,
    pub inspector_tab: InspectorTab,
    pub hash_being_renamed: Option<usize>,
}

pub trait PanelUi {
    const ID: PanelId;
    const TITLE: TextId;
    fn show(ui: &mut egui::Ui, view: &AppView<'_>, scratch: &mut PanelScratch, out: &mut IntentQueue);
}
```

書き換え可能な値をウィジェットに渡す必要がある場合（`DragValue` など）は
**コピーして編集し、変化したら Intent** にする。

```rust
// panels/inspector.rs 内の典型
let mut bpm = view.doc.tempo.bpm_at(0.0);
if ui.add(egui::DragValue::new(&mut bpm).range(20.0..=300.0).suffix(" BPM")).changed() {
    let mut after = view.doc.tempo.clone();
    after.set(0.0, bpm);
    out.edit(Edit::SetTempo { before: view.doc.tempo.clone(), after });
}
```

これで `tempo_bpm: f32`（`main.rs:78`）のようなドキュメントの二重保持が不要になる。

### 3.5 フレームパイプライン

```rust
// app.rs
impl eframe::App for DrillApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();

        // 1. jobs: non-blocking poll; may enqueue intents (load finished, export failed, …)
        self.state.jobs.poll(ui.ctx(), &mut self.intents);

        // 2. transport
        if self.state.playback.transport == Transport::Playing {
            let result = self.state.playback.advance(self.state.doc.doc(), now);
            if matches!(result, AdvanceResult::Stopped(_)) {
                self.state.playback.transport = Transport::Stopped;
            }
            ui.ctx().request_repaint();
        }

        // 3. derived caches (revision-gated)
        let cursor = self.state.playback.resolve(self.state.doc.doc());
        self.state.derived.update(self.state.doc.doc(), self.state.doc.revision(), cursor, now);

        // 4. shortcuts -> commands -> intents
        input::shortcuts::poll(ui.ctx(), &mut self.commands);

        // 5. read-only UI pass: nothing below this line mutates AppState
        {
            let view = self.state.view(&self.text);
            for command in self.commands.drain(..) {
                input::dispatch::dispatch(command, &view, &mut self.intents);
            }
            panels::show_all(ui, &view, &mut self.scratch, &mut self.intents);
            dialogs::show(ui.ctx(), &view, &mut self.intents);
        }

        // 6. window close / autosave / recovery
        self.handle_close_request(ui.ctx());
        self.autosave(now);

        // 7. mutation
        self.apply_intents();
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "ui_layout", &self.state.ui.layout);
    }
}
```

`view` は `&self.state` を、`self.intents` / `self.scratch` / `self.commands` は
別フィールドを可変に借りるので、同一メソッド内の分離借用として成立する。
ステップ 5 のあいだ `AppState` は不変なので、**どのパネルも同じスナップショットを見る**。
「パネルAが値を変え、下に描かれるパネルBが1フレームずれた値を見る」という
即時モード特有のバグが構造的に起きない。

`apply_intents` は投入順に適用し、`Edit` の失敗で以降を打ち切る:

```rust
fn apply_intents(&mut self) {
    for intent in self.intents.drain() {
        let result = match intent {
            Intent::Edit(edit) => self.state.doc.apply(edit),
            Intent::Undo => { self.state.doc.undo(); Ok(()) }
            /* … */
        };
        if let Err(error) = result {
            self.state.ui.status = StatusLine::error(error.message(self.state.ui.layout.locale));
            break;                         // never leave a half-applied batch
        }
    }
    let revision = self.state.doc.revision();
    self.state.selection.revalidate(self.state.doc.doc(), revision);
}
```

複数の変更を不可分にしたいときは `Edit::Batch`（DESIGN_GAPS A-1）を 1 件だけ積む。

### 3.6 入力処理

#### 3.6.1 ショートカットとコマンドの一元定義

```rust
// input/shortcuts.rs
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Command {
    FileNew, FileOpen, FileOpenSample, FileSave, FileSaveAs, FileRevert, FileQuit,
    EditUndo, EditRedo, EditCut, EditCopy, EditPaste,
    SelectAll, SelectNone, SelectInvert, SelectFind,
    SetDuplicate, SetInsertAfter, SetDelete, SetRename, SetCounts,
    ShapeLine, ShapeColumn, ShapeDiagonal, ShapeArc, ShapeCircle, ShapeBlock, ShapeSpiral,
    AlignHorizontal, AlignVertical, DistributeHorizontal, DistributeVertical,
    FlipHorizontal, FlipVertical, RotateLeft, RotateRight, ScaleUp, ScaleDown,
    NudgeLeft, NudgeRight, NudgeUp, NudgeDown,
    AutoAssignNext,
    PlayPause, PlayFromRangeStart, SeekRangeStart, SeekRangeEnd,
    StepBack, StepForward, JumpBack, JumpForward, PrevSet, NextSet,
    RangeInHere, RangeOutHere, RangeCurrentSet, RangeWholeShow, ToggleLoop,
    ToolSelect, ToolMove, ToolShape, ToolRoute, ToggleSnap,
    View2D, View3D, ViewFit, ViewStepGrid, CameraAudience, CameraPressBox, CameraOverhead,
    ZoomIn, ZoomOut, ZoomReset, ToggleFullscreen,
    PanelSetlist, PanelInspector, PanelTimeline, PanelClinic, PanelExport, PanelAudio, PanelJobs,
    PanelResetLayout,
    ExportCsv, ExportSetSvg, ExportCoordinateSheet, ExportDrillBook, ExportCountSheet,
    ExportContinuity, ExportDotBook, ExportVideo,
    LocaleJa, LocaleEn,
    HelpTour, HelpShortcuts, HelpAbout,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuTop { File, Edit, Select, Set, Format, Playback, View, Panel, Export, Help }

pub struct CommandSpec {
    pub command: Command,
    pub menu: MenuTop,
    pub group: u8,                       // separators are drawn between groups
    pub label: TextId,
    pub shortcut: Option<egui::KeyboardShortcut>,
    /// Glyph shown in toolbar/menu. Information must never be carried by colour alone.
    pub glyph: &'static str,
    pub destructive: bool,
    pub toolbar: Option<u8>,             // Some(order) if it appears in the toolbar
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        command: Command::FileSave, menu: MenuTop::File, group: 1,
        label: TextId::CmdFileSave,
        shortcut: Some(egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S)),
        glyph: "💾", destructive: false, toolbar: Some(2),
    },
    // …（以下、全 Command について 1 行ずつ）
];
```

**この表が唯一の定義箇所**であり、メニューバー・ツールバー・ショートカット処理・
ショートカット一覧ダイアログ・（採用するなら）コマンドパレットの全てがここから生成される。
新しい操作を足すとき、表に 1 行足す以外の作業は無い。

```rust
pub fn poll(ctx: &egui::Context, out: &mut Vec<Command>) {
    let typing = ctx.text_edit_focused();
    ctx.input_mut(|input| {
        for spec in COMMANDS {
            let Some(shortcut) = spec.shortcut else { continue };
            // Un-modified single keys must not fire while a text field has focus.
            if typing && shortcut.modifiers.is_none() { continue; }
            if input.consume_shortcut(&shortcut) { out.push(spec.command); }
        }
    });
}
```

主要な割り当て（`Modifiers::COMMAND` は macOS で `⌘`、他で `Ctrl`）:

| キー | Command | キー | Command |
|---|---|---|---|
| `Cmd+N` / `O` / `S` / `Shift+S` | 新規 / 開く / 保存 / 名前を付けて保存 | `Space` | 再生・一時停止 |
| `Cmd+Z` / `Cmd+Shift+Z` | Undo / Redo | `Cmd+Space` | 範囲先頭から再生 |
| `Cmd+A` / `Cmd+Shift+A` / `Cmd+I` | 全選択 / 選択解除 / 選択反転 | `Home` / `End` | 範囲先頭 / 範囲末尾 |
| `Cmd+F` | 演者を検索 | `←` / `→` | 1カウント移動 |
| `Cmd+D` | セットを複製 | `Shift+←` / `Shift+→` | 4カウント移動 |
| `Cmd+X` / `C` / `V` | ドット座標の切取／コピー／貼付 | `PageUp` / `PageDown` | 前セット / 次セット |
| `Alt+矢印` | 選択を1ステップ移動 | `I` / `O` | 再生範囲 IN / OUT を現在位置に |
| `Alt+Shift+矢印` | 選択を4ステップ移動 | `L` | ループ切替 |
| `Cmd+[` / `Cmd+]` | 選択を 15° 回転 | `1` `2` `3` `4` | 選択／移動／図形／経路ツール |
| `Cmd+Plus` / `Minus` / `0` | UI ズーム（egui 既定） | `G` | スナップ切替 |
| `Cmd+E` | 書き出しパネル | `V` | 2D / 3D 切替 |
| `F` | フィールドにフィット | `F1` / `Shift+F1` | ツアー / ショートカット一覧 |
| `F11` | 全画面 | `Esc` | ドラッグ中止 → モーダル閉 → 選択解除（この順） |

`Esc` の段階的な意味づけは `dispatch` 側で `AppView` を見て決める（表には 1 行しか無い）。

#### 3.6.2 メニューバーの自動生成

```rust
// panels/menubar.rs
pub fn show(ui: &mut egui::Ui, view: &AppView<'_>, out: &mut IntentQueue) {
    egui::MenuBar::new().ui(ui, |ui| {
        for top in MenuTop::ALL {
            ui.menu_button(view.text.menu(top), |ui| {
                let mut group = 0;
                for spec in COMMANDS.iter().filter(|s| s.menu == top) {
                    if spec.group != group && group != 0 { ui.separator(); }
                    group = spec.group;
                    let label = format!("{} {}", spec.glyph, view.text.get(spec.label));
                    let mut button = egui::Button::new(label);
                    if let Some(shortcut) = spec.shortcut {
                        button = button.shortcut_text(ui.ctx().format_shortcut(&shortcut));
                    }
                    let enabled = dispatch::is_enabled(spec.command, view);
                    if ui.add_enabled(enabled, button).clicked() {
                        dispatch::dispatch(spec.command, view, out);
                        ui.close();
                    }
                }
            });
        }
        egui::gui_zoom::zoom_menu_buttons(ui);   // Ctrl +/-/0 と同じものをメニューに出す
    });
}
```

現在の実装（`main.rs:555-643`）は各項目を手書きし、ショートカットを
`"保存    Ctrl/Cmd+S"` のように文字列へ埋め込んでいる（macOS で誤表示）。
`Context::format_shortcut` を使えばプラットフォームに応じた表記になる。

#### 3.6.3 ポインタ処理

```rust
// view/viewport.rs — field↔screen 変換の唯一の実装
#[derive(Clone, Copy)]
pub struct FieldViewport { rect: egui::Rect, width: f32, height: f32 }

impl FieldViewport {
    pub fn new(rect: egui::Rect, grid: &GridConfig) -> Self { /* … */ }
    pub fn to_screen(&self, p: Point) -> egui::Pos2 { /* … */ }
    pub fn to_field(&self, p: egui::Pos2) -> Point { /* … */ }
    /// Pixel radius converted to field units, so hit tests scale with zoom and DPI.
    pub fn pick_radius_field(&self, pixels: f32) -> f32 { /* … */ }
}
```

現在 `to_screen` は `main.rs:1468` のクロージャ、3D 側は `main.rs:316-321` に別実装、
`draw_field` は `main.rs:1625-1626` に 3 つ目、`draw_count_track` は 4 つ目を持つ。
逆変換は `main.rs:1566-1567` にドラッグ用の差分計算として 5 つ目がある。
`FieldViewport` に一本化する。

```rust
// input/pointer.rs
pub fn handle(
    response: &egui::Response,
    viewport: FieldViewport,
    view: &AppView<'_>,
    out: &mut IntentQueue,
) {
    let Some(pointer) = response.interact_pointer_pos() else { return };
    let field = viewport.to_field(pointer);
    let additive = response.ctx.input(|i| i.modifiers.command || i.modifiers.shift);
    let hit = pick(view, field, viewport.pick_radius_field(18.0));
    // click / drag_started / dragged / drag_stopped -> Intent::Select / Intent::Tool / Intent::Edit
}

/// O(n) nearest-neighbour over the cached frame positions. No allocation.
fn pick(view: &AppView<'_>, at: Point, radius: f32) -> Option<PerformerId> { /* … */ }
```

**ツールモードごとの意味**:

| ツール | クリック | 空白からドラッグ | 演者からドラッグ | 修飾 |
|---|---|---|---|---|
| Select | 選択／トグル | 矩形選択 | 矩形選択 | `Cmd`/`Shift` = 追加、`Alt` = 除外 |
| Move | 選択 | 矩形選択 | 選択を移動 | `Shift` = 軸拘束、`Alt` = スナップ無効 |
| Shape | 図形の始点 | 図形の対角 | 同左 | `Shift` = 正方形／正円 |
| Route | 経路の制御点を掴む | 何もしない | 制御点を移動 | 11 の `RouteShape` に依存 |

現在は Move 相当の挙動が固定で、しかも `count_position == 0.0` のときだけ編集できる
（`main.rs:1528` / `1563`）。この制約は残す（セット境界でのみ編集）が、
`main.rs:867-872` の警告ラベルではなく、**カウントが 0 でないときはツールバーの編集ボタンを
無効化し、`Cursor` を最寄りのセット境界へ寄せるボタンを出す**（禁止ではなく脱出路を出す）。

### 3.7 画面構成とレイアウト

```
┌─ MenuBar ────────────────────────────────────────────────────────────┐
├─ Toolbar（CommandSpec.toolbar から生成。アイコン＋文字ラベル）────────┤
├──────────┬───────────────────────────────────────────┬───────────────┤
│ Setlist  │                                           │  Inspector    │
│ (left,   │            FieldView (2D / 3D)            │  (right,      │
│  可変幅) │                                           │   可変幅)     │
│          │                                           │  ├ 選択       │
│  Set 1   │                                           │  ├ 座標       │
│  Set 2   │                                           │  ├ 整形       │
│   …      │                                           │  ├ グリッド   │
│          │                                           │  ├ テンポ     │
│  Clinic  │                                           │  ├ 音源(30)   │
│ (下に固定)├───────────────────────────────────────────┤  ├ カメラ(23) │
│          │  Timeline（カウントトラック＋波形レーン）  │  └ 印刷(17)   │
├──────────┴───────────────────────────────────────────┴───────────────┤
│ StatusBar（状態行 ＋ ジョブ進捗 ＋ 演者数 ＋ 現在カウント）           │
└──────────────────────────────────────────────────────────────────────┘
```

```rust
// panels/mod.rs
pub fn show_all(ui: &mut egui::Ui, view: &AppView<'_>, scratch: &mut PanelScratch, out: &mut IntentQueue) {
    Panel::top("menubar").resizable(false).frame(theme::menubar_frame()).show(ui, |ui| {
        menubar::show(ui, view, out);
    });
    Panel::top("toolbar").resizable(false).show(ui, |ui| toolbar::show(ui, view, out));
    Panel::bottom("statusbar").resizable(false).show(ui, |ui| statusbar::show(ui, view, out));

    if view.ui.layout.visible[&PanelId::Setlist] {
        Panel::left("setlist")
            .resizable(true)
            .default_size(view.ui.layout.left_width)
            .size_range(200.0..=480.0)
            .show(ui, |ui| { setlist::show(ui, view, scratch, out); clinic::show(ui, view, scratch, out); });
    }
    if view.ui.layout.visible[&PanelId::Inspector] {
        Panel::right("inspector")
            .resizable(true)
            .default_size(view.ui.layout.right_width)
            .size_range(240.0..=560.0)
            .show(ui, |ui| inspector::show(ui, view, scratch, out));
    }
    if view.ui.layout.visible[&PanelId::Timeline] {
        Panel::bottom("timeline")
            .resizable(true)
            .default_size(view.ui.layout.timeline_height)
            .size_range(90.0..=320.0)
            .show(ui, |ui| timeline::show(ui, view, scratch, out));
    }
    view::field2d::show(ui, view, out);      // 残り全部を占める
}
```

**ドッキングと保存**:

- パネルの幅・高さは egui の `PanelState`（`panel.rs:41`）が保持する。
  `eframe` の `persistence` フィーチャを有効化し、`App::persist_egui_memory()`（既定 true）で
  自動的に復元される。追加コードは不要。
- 表示／非表示と `zoom_factor` / `locale` / `recent` は `UiLayout` として
  `App::save` で `eframe::set_value(storage, "ui_layout", …)` に書く。
- `Command::PanelResetLayout` で `UiLayout::default()` に戻し、
  同時に `ui.ctx().memory_mut(|m| m.reset_areas())` でパネル幅も初期化する。
- **タブのドラッグ＆ドロップによる自由配置は行わない**（`egui_dock` 等の外部依存が要る。9 参照）。
  代わりに「左／右／下のどこに出すか」をパネルごとに設定で選べるようにする。

**大量要素を含むパネルの規則**（3.8 で予算化）:

- セットリスト（最大 256 セット）は `ScrollArea::show_rows` で可視行のみ。
- 演者リスト（最大 4,000 人）は同じく仮想化し、既定では**セクション単位に畳む**。
- インスペクタの選択詳細は先頭 20 件＋集約値のみ。`… 他 N 人` を出す。
- カウントトラックの目盛りは `stride = ((total / rect.width()).ceil() as u32).max(1)` で間引き、
  1 画素あたり 1 本を超えないようにする（D-7 の修正）。

### 3.8 初回起動体験

**目標**: 起動から「セット選択 → 演者選択 → 編集 → 再生」まで、説明書なし・90 秒以内。

1. **起動時の分岐**
   - 前回のクラッシュ復旧候補がある（41）→ 復旧ダイアログを最優先で出す。
   - `UiLayout::recent` が空でない → 最近のプロジェクトを並べたウェルカム画面。
   - 完全な初回 → ウェルカム画面（テンプレート主体）。
2. **ウェルカム画面**（`panels/welcome.rs`。モーダルではなく中央領域に描く。
   Esc やクリックで消えず、いずれかを選ぶまで留まる。ただしメニューバーは常に使える）
   - `新しいドリル`: フィールド（フットボール100yd / インドア / サッカー）×
     演者数（16 / 32 / 64 / 128）× セット数（4 / 8 / 16）を 3 つのボタン列で選ぶ。
     既定は「フットボール・64人・8セット」。
   - `サンプルを開く`: `assets/samples/showcase.drill.json`（64人・16セット・
     テンポ変化 2 箇所・衝突が 1 件わざと残してある）。クリニックの意味がすぐ分かる。
   - `ファイルを開く` / `最近使ったファイル`。
3. **コーチマーク**（`OnboardingState`）: 初回のみ 4 ステップ。
   各ステップは対象パネルを枠で囲み、`1/4 セットを選びます` と番号を出す。
   `次へ` / `スキップ` / `もう表示しない`。`Storage` に保存。
   現在の常時表示ガイドバー（`main.rs:739-755`）はこれに置き換え、既定で消える。
4. **空状態**
   - 選択ゼロ → フィールド上のカード（`main.rs:1441` の資産を流用）。
     ただし**カードの中身をボタンにする**（現在は `painter.text` で操作不能・フォーカス不可）。
     `[全員を選択 (Cmd+A)]` `[前列だけ選択]` `[サンプルを開く]`。
   - セットが 1 つだけ → タイムラインに `[＋ セットを追加 (Cmd+D)]` を出す。
   - 音源なし → タイムラインの波形レーンに `[音源を読み込む]`。
5. **番号付き手順の維持**: 現在の `1. セットを選ぶ` … の番号は発見性に効いているので、
   初回のみ各パネル見出しに番号バッジを出し、ツアー完了後は消す。

### 3.9 「drill-app は業務ロジックを持たない」の機械的検査

#### 判定基準（どちらへ置くか）

関数は、次の**いずれか 1 つでも該当したら `drill-core` / `drill-render` へ移す**:

1. 引数と戻り値に `egui` / `eframe` / `rfd` / `winit` の型が 1 つも現れない
2. 同じ入力から常に同じ結果が出る（決定論）で、スクリーン座標・DPI・フォントに依存しない
3. 単体テストに `egui::Context` が不要
4. 書き出し（SVG / CSV / PDF / 動画）や印刷が同じ計算を必要とする（二重実装の危険がある）

逆に `drill-app` に残してよいのは、`egui::Ui` / `Painter` / `Response` / `Pos2` / `Rect` /
`Modifiers` / `Id` / OS ダイアログ / フォーカス / DPI のいずれかに触れるものだけである。

#### 現時点の違反一覧と移送先

| 現在地 | 内容 | 移送先 |
|---|---|---|
| `main.rs:190-210` `selection_bounds` | 点群の AABB | `drill_core::editing::bounds(&[Point]) -> Option<(Point, Point)>` |
| `main.rs:212-234` `transform_selection` | 重心・回転・スケール・フィールド内クランプ | `drill_core::editing::rotate_scale(&[Point], scale, angle, &GridConfig) -> Vec<Point>` |
| `main.rs:169-188` `commit_layout` | スナップ・差分検出・コマンド生成 | `Edit::move_points(doc, set, ids, after)`（10） |
| `main.rs:264-289` `auto_assign_next` | 割り当て最適化と `MoveCommand` 生成 | `drill_core::pathing::reassign_edit(doc, set) -> Option<Edit>` |
| `main.rs:950-953` | ブロック配置の cols/rows 決定 | `drill_core::shapes::block_auto(min, max, n) -> Vec<Point>` |
| `main.rs:925-926` `940-941` `961-962` | 円弧・円・螺旋の半径決定 | `drill_core::shapes::*_fit(min, max, n)` |
| `main.rs:388-400` | 3D の深度ソートと焦点距離からの半径 | `drill-render` の 3D パス（21 / 22） |
| `main.rs:1617-1710` `draw_field` | ヤード線・ステップグリッド・ハッシュの幾何 | `drill-render::build`（20）。app には `DrawCmd → Painter` だけ残す |
| `main.rs:1712-1837` `draw_count_track` | セット区間・目盛り・再生範囲の幾何 | 同上（`DrawCmd` として） |
| `main.rs:236-247` `save_to` | バックアップ付き保存 | 41 の永続化層 ＋ 40 のジョブ |
| `main.rs:477-487` | 自動保存 | 同上 |
| `main.rs:1256-1262` | 動画のフレーム数・サイズ推定の呼び出し方 | 既に `drill_core::video`。呼び出しだけ残す |
| `main.rs:1268-1272` | FFmpeg 検出（`std::process::Command`） | 40 のジョブ |

#### CI で回す検査（`crates/drill-app/tests/architecture.rs`。追加依存なし）

```rust
const MAX_LINES: &[(&str, usize)] = &[("src/main.rs", 60), ("src/app.rs", 250)];
const MAX_LINES_GLOB: &[(&str, usize)] = &[("src/panels", 400), ("src/view", 400), ("src/state", 250)];

#[test] fn main_rs_is_only_boot() { /* MAX_LINES を検査 */ }
#[test] fn no_file_exceeds_its_budget() { /* MAX_LINES_GLOB */ }

/// Panels must not do I/O, spawn processes, or open OS dialogs; they emit intents instead.
#[test]
fn panels_do_no_io() {
    for path in rust_files("src/panels") {
        let src = std::fs::read_to_string(&path).unwrap();
        for banned in ["std::fs", "std::process", "rfd::", "std::thread"] {
            assert!(!src.contains(banned), "{}: {banned}", path.display());
        }
    }
}

/// Panels and views must never obtain a mutable Document.
#[test]
fn panels_never_mutate_the_document() {
    for path in rust_files("src/panels").chain(rust_files("src/view")) {
        let src = std::fs::read_to_string(&path).unwrap();
        for banned in ["&mut Document", "&mut AppState", "DocumentState", ".apply("] {
            assert!(!src.contains(banned), "{}: {banned}", path.display());
        }
    }
}

/// Analysis thresholds belong to ClinicParams, not to UI literals.
#[test]
fn panels_contain_no_analysis_thresholds() {
    for path in rust_files("src/panels") {
        let src = std::fs::read_to_string(&path).unwrap();
        assert!(!src.contains("analyze_transition("), "{}", path.display());
    }
}
```

型による保証（grep より強い、こちらが本体）:

- `DocumentState::doc` が private、`apply` が `pub(crate)` → パネルから `&mut Document` は取れない。
- `PanelUi::show` の引数が `&AppView` と `&mut IntentQueue` のみ → 変更手段が Intent しかない。
- `dispatch::dispatch(command: Command, …)` は `match command { … }` を
  **ワイルドカード無しで**書く → 新しい `Command` を足したらコンパイルが通らない。
- `PanelScratch` は `#[derive(Default)]` で、全フィールドが `bool` / `f32` / `String` /
  `Option<usize>` に限る（ドメイン型を持たせない）。

### 3.10 アクセシビリティ

#### 3.10.1 AccessKit の有効化

```toml
# crates/drill-app/Cargo.toml
eframe = { version = "0.35.0", default-features = false, features = [
  "accesskit",     # ← 追加。これが無いと Windows UIA にツリーが出ない（D-1）
  "persistence",   # ← 追加。パネル幅・ズーム・最近使ったファイルの保存（D-17）
  "wgpu", "wayland", "x11",
] }
```

`accesskit` を足しても、`InitialTreeRequested` が来るまで
`Context::enable_accesskit()` が呼ばれないので、支援技術を使わない利用者への
恒常的なコストはゼロである（`context.rs:507`）。

#### 3.10.2 自前描画部分のアクセシビリティ

egui の標準ウィジェットは自動でノードを作るが、`allocate_painter` で描いたものは作らない。

```rust
// view/field2d.rs
let response = ui.allocate_response(size, egui::Sense::click_and_drag());
response.widget_info(|| {
    egui::WidgetInfo::labeled(
        egui::WidgetType::Other,
        ui.is_enabled(),
        view.text.field_summary(view),   // 「フィールド。セット3、カウント8、選択12人」
    )
});
```

```rust
// panels/timeline.rs
let response = ui.allocate_response(size, egui::Sense::click_and_drag());
response.widget_info(|| {
    egui::WidgetInfo::slider(true, global_count as f64, view.text.get(TextId::TimelineCount))
});
```

**演者 1,000 人に 1,000 ノードは作らない**（ツリー生成コストと読み上げの実用性の両方で破綻する）。
代わりに:

- フィールドビュー全体を 1 ノードとし、要約を読み上げる。
- インスペクタの**演者リストが正規のアクセシブル表面**であり、
  `SelectableLabel` として仮想化されたリストに出す（可視行のみノードが作られる）。
- 選択が変わったら `ui.ctx().request_repaint()` に加えて、
  状態行のテキストを更新する（`aria-live` 相当。状態行は `WidgetType::Label`）。

#### 3.10.3 キーボードだけで全操作

- フィールドビューを `Sense::click_and_drag()` から
  **フォーカス可能**にする（`Sense::focusable_noninteractive()` を合成するか、
  `ui.memory_mut(|m| m.interested_in_focus(id, layer))` を使う）。
  フォーカスがある間、`矢印` = カーソル演者の移動、`Space` = 選択トグル、
  `Tab` / `Shift+Tab` = 次／前の演者、`Enter` = 移動モード開始、
  `Alt+矢印` = 選択を 1 ステップ移動、`Esc` = 中止。
- フォーカスリングは egui 既定の `visuals.selection.stroke` に依存せず、
  **2 px の実線＋1 px の対比色オフセット**を自前で描く（暗色テーマで既定リングが埋もれるため）。
- `Cmd+F`（`Command::SelectFind`）でインスペクタの演者検索にフォーカスを移し、
  ラベル前方一致で選択できるようにする。マウスに触れずに任意の演者へ到達できる経路を保証する。
- 全ての `Command` はショートカットかメニューから実行できる（3.6.1）。
  ツールバーのみに存在する操作を作らない。これは 4. の不変条件で検査する。

#### 3.10.4 色以外での情報伝達

| 情報 | 現在 | 変更後 |
|---|---|---|
| 衝突候補 | 緑／赤の `● 衝突候補: 0`（`main.rs:1015-1023`） | `✓ 衝突なし` / `⚠ 衝突候補 3件（カウント 6.5 で T4–T9 が最接近）` |
| 過大歩幅 | 緑／橙の `● 過大歩幅: 0人` | `✓ 歩幅は全員 8 to 5 以内` / `⚠ 6 to 5 超 2人` / `✕ 到達不能 1人` |
| 再生範囲 IN/OUT | 緑線／橙線のみ（`main.rs:1779-1792`） | `▸IN 32` / `OUT 96◂` のブラケット字形＋数値ラベル |
| 再生ヘッド | 赤い縦線（`main.rs:1814`） | 上端に▼マーカー＋カウント数値。線は破線と実線で範囲内外を区別 |
| セット境界 | 黄色の縦線（`main.rs:1754`） | 実線＋セット名＋カウント。範囲外セットは点線 |
| 選択中の演者 | 白い円ストローク（`main.rs:1488`） | 二重リング（内側 1px 暗色 / 外側 2px 明色）＋ラベルを反転表示 |
| 未保存 | `● 未保存` / `✓ 保存済み`（既に文字） | 維持。加えてウィンドウタイトルに `*` を付ける |
| ジョブ進捗 | 無し | `ProgressBar` ＋ `12% · 座標CSVを書き出し中` の文字 |
| 検証エラー | 赤文字（`main.rs:1265` `1390`） | `✕` 記号＋文言＋該当ウィジェットへのフォーカス移動リンク |

原則: **状態は「記号 ＋ 語 ＋ 数」の 3 点で表す**。色は 4 番目の冗長な手がかりに限る。

#### 3.10.5 コントラスト

`boot/theme.rs` に色を集約し、単体テストで比を検証する。

```rust
/// WCAG 2.1 relative luminance / contrast ratio. Colours are sRGB 8-bit.
pub fn relative_luminance(c: egui::Color32) -> f32 { /* … */ }
pub fn contrast_ratio(a: egui::Color32, b: egui::Color32) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}
```

現在の値を実測すると `widgets.inactive` の文字 `(22,27,34)` と背景 `(36,45,58)`
（`main.rs:465` / `main.rs:469`）で **1.26:1** しかない（D-2）。
本文の `Color32::from_gray(225)` と `panel_fill (13,17,23)` は約 14:1 で良好。
基準は **本文・ラベル 4.5:1 以上、大きい文字と図形境界 3.0:1 以上**とし、
テーマの全ペアをテストで走査する。

#### 3.10.6 Windows 100–200% スケーリング

- `pixels_per_point = zoom_factor × native_pixels_per_point`。100% で 1.0、200% で 2.0。
  レイアウトは論理ポイントで書くので、**寸法をハードコードしても比率は保たれる**。
  問題になるのは (a) 初期ウィンドウが画面に入らない、(b) 縦に伸びるパネルが切れる、の 2 点。
- (a) の対処（D-4）:

```rust
// boot/window.rs
/// The monitor size is only known once the window exists, so clamp on the first frame.
pub fn clamp_to_monitor(ctx: &egui::Context) {
    let (monitor, inner) = ctx.input(|i| {
        (i.viewport().monitor_size, i.viewport().inner_rect.map(|r| r.size()))
    });
    let (Some(monitor), Some(inner)) = (monitor, inner) else { return };
    let max = monitor * 0.92;
    if inner.x > max.x || inner.y > max.y {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(inner.min(max)));
    }
}
```

  併せて `ViewportBuilder::with_min_inner_size([900.0, 560.0])` を設定する。
  900 pt はパネル最小幅 200 + 240 とフィールド最小 400 の和に余裕を足した値。
- (b) の対処（D-3）: 左右パネルの中身を必ず `egui::ScrollArea::vertical()` で包む。
  ツールバーは `ui.horizontal_wrapped` ではなく
  `ScrollArea::horizontal().auto_shrink([false, true])` にして、
  狭いときに折り返さず横スクロールへ退避させる（ボタン位置の記憶を壊さないため）。
- `zoom_factor` はメニューの `egui::gui_zoom::zoom_menu_buttons(ui)` と
  `Ctrl +/-/0` で変更でき、`UiLayout` に保存される。
  ディスプレイを跨いでも UI 倍率が飛ばない（`zoom_factor` は永続、`native` だけが変わる）。
- 検証手順（50 へ渡す手動チェックリスト）:
  100 / 125 / 150 / 175 / 200% の各設定で、
  (1) 起動ウィンドウが作業領域に収まる、(2) メニューの全項目が読める、
  (3) 左右パネルの最下部の項目に到達できる、(4) フィールドの演者ラベルが読める、
  (5) 高DPI と低DPI のモニタ間でウィンドウを往復させても崩れない。

#### 3.10.7 日本語・英語 UI

- 文字列は `i18n.rs` の `TextTable` に集約する。
  `drill-core` 側の人間可読テキストは `Locale` 引数で取得する（42 / A-6）。
- フォント: 現在は Noto Sans JP を `Proportional` と `Monospace` の**先頭**に挿入している
  （`main.rs:42-48`）。英語 UI では Latin 用フォントを先頭にして、
  日本語フォントをフォールバックに回す。`Command::LocaleEn` / `LocaleJa` で
  `install_fonts(ctx, locale)` を呼び直す。
- レイアウトは文字幅に依存しないこと（英語ラベルは日本語より 1.4 倍程度長くなる）。
  固定幅ボタンを使わず、`ui.add_sized` は最小幅の指定にのみ使う。

### 3.11 破壊的操作の確認

原則: **Undo で戻せるものは確認しない。戻せないものだけ確認する。**
確認の代わりに「実行 ＋ 取り消しトースト」を既定とする。

| 操作 | 可逆 | 扱い |
|---|---|---|
| 演者・セットの削除 | Undo 可 | 確認しない。トースト `セット3を削除しました [元に戻す]`（5 秒） |
| 整列・回転・図形適用・自動割り当て | Undo 可 | 確認しない |
| グリッド変更（座標スケールあり） | Undo 可 | 確認しない。ただしスケールの有無は**事前に**トグルで選ばせる |
| 上書き保存 | 元ファイルは `.backup` へ退避（`main.rs:238-241`） | 確認しない |
| 書き出し先の上書き | OS ダイアログが確認する | 追加の確認をしない |
| **未保存のままウィンドウを閉じる** | 不可逆 | **確認する**。`保存して終了 / 保存せず終了 / キャンセル` |
| **未保存のまま新規・開く・復旧** | 不可逆 | **確認する**（同上） |
| **v1 → v2 マイグレーション後の上書き保存** | 旧版アプリで開けなくなる | **確認する**。既定ボタンは `別名で保存` |
| **実行中ジョブがある状態での終了** | 不可逆 | **確認する**。実行中ジョブ名を列挙 |
| **プロジェクト内アセットの削除** | 不可逆 | **確認する** |
| 履歴上限超過で最古の Undo が消える | 不可逆 | 確認しない。インスペクタに `履歴 500/500` と表示するのみ |

「確認疲れ」を避けるための規則:

1. 確認ダイアログは上の **5 種類のみ**。これ以外を追加するときはこの表を更新し、
   理由（Undo で戻せない理由）を書く。
2. `DialogStack` の深さは **1** に固定する。ダイアログの上にダイアログを出さない。
3. `今後表示しない` を付けてよいのはマイグレーション確認だけ。
   データ喪失に直結する終了確認には付けない。
4. 既定ボタン（Enter）は常に安全側（`保存` または `キャンセル`）。`Esc` はキャンセル。
5. 破壊的コマンドは `CommandSpec::destructive == true` を持ち、
   `dispatch` は `Intent::Dialog(DialogIntent::Confirm(..))` を出す。
   直接 `Intent::Edit` を出さないことを 4. の不変条件で検査する。

```rust
// dialogs/confirm.rs
pub struct ConfirmRequest {
    pub title: TextId,
    pub body: String,
    pub danger: TextId,                    // e.g. 「保存せず終了」
    pub safe: TextId,                      // e.g. 「保存して終了」
    pub cancel: TextId,
    pub on_danger: Box<[Intent]>,
    pub on_safe: Box<[Intent]>,
}
```

```rust
// app.rs
fn handle_close_request(&mut self, ctx: &egui::Context) {
    if !ctx.input(|i| i.viewport().close_requested()) { return; }
    if self.state.doc.is_dirty() || !self.state.jobs.active.is_empty() {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        self.intents.push(Intent::Dialog(DialogIntent::Confirm(confirm::unsaved_exit(&self.state))));
    }
}
```

### 3.12 他の設計文書からの UI 要件の受け口

他文書が UI を要求するとき、**次の 4 形式のいずれかに落とす**。
それ以外の形（独自モーダル、独自ショートカット処理、独自の状態フィールド追加）は受け付けない。

1. `CommandSpec` の行を追加する（メニュー・ショートカット・ツールバー・
   ショートカット一覧に自動で載る）
2. `PanelId` に列挙子を 1 つ足し、`panels/<name>.rs` に `impl PanelUi` を書く
3. `InspectorSection` を実装してインスペクタに節を足す
4. `JobRequest` に列挙子を足す（進捗・キャンセル・失敗表示は共通の `panels/jobs.rs`）

```rust
// panels/inspector.rs
pub trait InspectorSection {
    const ID: SectionId;
    const TITLE: TextId;
    fn is_relevant(view: &AppView<'_>) -> bool;
    fn show(ui: &mut egui::Ui, view: &AppView<'_>, scratch: &mut PanelScratch, out: &mut IntentQueue);
}
```

| 文書 | 投げてくる UI 要件 | 受け口 |
|---|---|---|
| 23 カメラ | カメラ一覧、キーフレーム編集、追従対象の選択、プリセット | `PanelId::Cameras` ＋ `InspectorSection::Camera` ＋ `Command::Camera*`。`UiState::camera` の所有権は 23 が確定したらそちらへ移す |
| 40 ジョブ | 進捗・キャンセル・失敗通知・完了時のドキュメント差し替え | `JobState` ＋ `panels/jobs.rs` ＋ `StatusLine`。`Intent::Job(JobRequest)` |
| 17 印刷 | 印刷プレビュー、ページ設定、出力対象（演者・セット範囲）の選択 | `PanelId::Print` ＋ `Command::Export*` ＋ `JobRequest::Print` |
| 30 音声 | 波形レーン、メトロノーム、同期アンカー編集、レイテンシ校正 | `panels/timeline.rs` の `WaveformLane` ＋ `InspectorSection::Audio` ＋ `JobRequest::DecodeAudio` |
| 20 / 21 / 22 描画 | `DrawCmd` の追加 | `view/display_list.rs` の `match` に 1 腕追加 |
| 41 永続化 | 復旧候補の提示、自動保存状態 | `dialogs/confirm.rs` の復旧ダイアログ ＋ `StatusLine` |
| 42 i18n | `Locale` 切替 | `UiLayout::locale` ＋ `TextTable` ＋ `boot/fonts.rs` |
| 13 クリニック | 検査結果の表示と該当箇所へのジャンプ | `panels/clinic.rs` ＋ `Intent::Seek` ＋ `Intent::Select` |

---

## 4. 不変条件

テストで検証できる形で書く。番号は 7. のテスト項目と対応する。

1. **パネルはドキュメントを変更できない**。`AppView` は `&Document` のみを公開し、
   `DocumentState::doc` は private、`apply` は `pub(crate)`。（型 ＋ `architecture.rs`）
2. **1 フレーム中に `Document` が変わるのは `apply_intents` の 1 箇所のみ**。
   `DocumentState::apply` の呼び出し箇所がソース全体で 1 つ。
3. **`PanelScratch::default()` に戻しても** `Document` / `PlaybackState` / `Selection` /
   `UiLayout` は変化しない。
4. **すべての `Command` はちょうど 1 つのメニュー項目に現れる**
   （`COMMANDS` 内で `command` が一意、かつ `menu` が必ず設定されている）。
5. **すべての `Command` にハンドラがある**（`dispatch` の `match` にワイルドカード無し）。
6. **ショートカットは重複しない**（`(modifiers, key)` の組が `COMMANDS` 内で一意）。
7. **`destructive == true` の `Command` は `Intent::Edit` を直接出さない**
   （`Intent::Dialog` を経由する）。
8. **`Selection` は常に現在の `Document` に存在する `PerformerId` のみを含む**
   （`revalidate` が毎フレーム revision を見て保証）。
9. **`DerivedCache` は `(revision, cursor, locale)` の関数である**。
   同じ鍵からは同じ `frame_positions` / `labels` が出る（決定論）。
10. **`Cursor` は `SetId` で保持し、解決できないときは先頭セットへフォールバックする**。
    ドキュメント上のどんな `Edit` 列を適用しても添字 panic が起きない。
11. **フレーム内で `DerivedCache` はヒープ確保しない**（2 フレーム目以降）。
    `frame_positions` と `ScanScratch` のポインタが変わらない。
12. **`main.rs` は 60 行以下**、`panels/*.rs` と `view/*.rs` は各 400 行以下、
    `state/*.rs` は各 250 行以下。
13. **`panels/` は `std::fs` / `std::process` / `rfd` / `std::thread` を import しない**。
14. **テーマの全 (前景, 背景) ペアのコントラスト比が 4.5:1 以上**
    （図形境界のみ 3.0:1 以上）。
15. **フォーカス可能なウィジェットは全てアクセシブルなラベルを持つ**
    （`accesskit_update` のノードに空ラベルが無い）。
16. **1 フレームで生成する egui ウィジェットは 400 個以下**（1,000 人・64 セット時）。
17. **`zoom_factor` が 0.5〜2.0 のどこでも、パネル最小幅の合計が
    `min_inner_size` を超えない**。

---

## 5. 性能

### 5.1 16.6 ms の配分

| 区分 | 担当文書 | 予算 |
|---|---|---|
| 入力処理・ヒットテスト・Intent 適用 | 43 | 1.0 ms |
| `DerivedCache` 更新（`frame_positions` 1,000 点ほか） | 43 | 0.3 ms |
| **非フィールドパネルの描画（メニュー・ツールバー・セットリスト・インスペクタ・タイムライン・クリニック・ジョブ・状態行）** | **43** | **1.5 ms** |
| フィールドビュー（`DrawCmd → Painter` 変換と発行） | 43 | 3.0 ms |
| 43 の予備 | 43 | 0.2 ms |
| **`drill-app` 小計** | | **6.0 ms** |
| `DisplayList` 構築 | 20 | 0.6 ms |
| egui tessellation ＋ wgpu 提出・描画 | 21 | 4.0 ms |
| `drill-core`（補間・クリニックの償却分） | 10 / 13 | 0.4 ms |
| OS 合成・present・余裕 | — | 5.6 ms |
| **合計** | | **16.6 ms** |

**質問への直接の回答: 非フィールドパネルの描画に使ってよいのは合計 1.5 ms**。内訳:

| パネル | 予算 | 主なコスト要因 |
|---|---|---|
| メニューバー | 0.05 ms | 10 個の `menu_button`。開いていない限り中身は評価しない |
| ツールバー | 0.10 ms | `CommandSpec::toolbar` が `Some` のもの（12 個程度） |
| セットリスト | 0.15 ms | 64 セットのうち可視 20 行のみ（`show_rows`） |
| インスペクタ | 0.30 ms | 集約値 ＋ 先頭 20 件 ＋ 開いている節のみ |
| タイムライン | 0.50 ms | 間引き後の目盛り最大 240 本 ＋ セット区間 64 ＋ 波形 1 レーン |
| クリニック | 0.10 ms | キャッシュ済みの数値 4 個と文字列 3 本 |
| ジョブ | 0.05 ms | 実行中ジョブ数（通常 0〜2） |
| 状態行 | 0.05 ms | 文字列 3 本（キャッシュ済み） |
| 予備 | 0.20 ms | |

これを満たすための具体的な規則:

1. **1 フレームのウィジェット生成は 400 個以下**。egui のウィジェットは概ね 1〜3 µs なので、
   400 個 ≈ 0.4〜1.2 ms。仮想化しなければ現在の実装は
   セットリスト 64 ＋ 演者 1,000 ＋ 目盛り 2,048 で軽く 3,000 個を超える。
2. **`format!` を毎フレーム呼ばない**。`DerivedCache::labels` に
   `(revision, cursor 整数部, locale)` を鍵にキャッシュする。
   現在は `main.rs:723` `788` `843` `1038` `1187` `1258` `1426` で毎フレーム `format!` している。
3. **全走査を毎フレーム行わない**。`analyze_transition`（`main.rs:1011`）と
   `transition_stats`（`main.rs:1033`）は revision ゲート ＋ 120 ms デバウンス。
   静止時の呼び出し回数は 0、ドラッグ中は最大 8 回/秒。
4. **目盛りの間引き**: `stride = ceil(total_counts / rect.width())`。
   2,048 カウント × 幅 1,200 px なら stride = 2 で 1,024 本 …
   さらに「主目盛りのみ描画し、副目盛りは stride が 1 のときだけ」とし、上限 240 本に固定する。
5. **`Selection` の集約はキャッシュ**。1,000 人選択時に境界・重心・平均歩幅を
   毎フレーム計算しない。

### 5.2 メモリ

| 対象 | 1,000 人 / 64 セット | 4,000 人 / 256 セット |
|---|---|---|
| `DerivedCache::frame_positions` | 8 KB | 32 KB |
| `Selection::ids`（`BTreeSet<u32>`） | 最悪 ~48 KB | ~192 KB |
| `Selection::indices` | 4 KB | 16 KB |
| `ScanScratch` | 13 の設計に従う（~100 KB 想定） | ~400 KB |
| `LabelCache` | ~8 KB | ~16 KB |
| `PanelScratch` | < 1 KB | < 1 KB |

いずれも 1 回確保して使い回す。フレーム内の再確保はゼロ（不変条件 11）。

### 5.3 計測方法

- `egui::Plugin` の `input_hook` / `output_hook` に `FrameBudget` を差し込み、
  ゾーンごとの経過を `VecDeque<[f32; ZONES]>`（256 フレーム分）に記録する。
  `F12` でデバッグパネルに p50 / p95 / max を出す。UI コードにタイマを散らさない。
- CI では `Context::run_ui` によるヘッドレス実行で、
  1,000 人ドキュメントを 120 フレーム回し、各パネルの p95 を閾値と比較する。

---

## 6. 失敗モードと安全性

| 失敗 | 原因 | 対処 |
|---|---|---|
| セット削除・Undo 後の添字 panic | `current_set: usize` を保持している（D-8） | `Cursor` を `SetId` で持ち `resolve` でフォールバック（不変条件 10）。パネルは `doc.sets.get(i)` のみ使う（`architecture.rs` で `sets[` の直接添字を禁止） |
| ドラッグ中に自動保存が走り中間状態が保存される | ドラッグ中に `Document` を直接書き換えている（`main.rs:1573`） | `DragSession::preview` に保持し、確定時に `Edit` 1 件。自動保存はドラッグ中は延期する |
| ジョブ完了時のドキュメント差し替えで編集が消える | 非同期読込中に編集できる | 差し替え前に `revision` を比較し、変わっていたら確認ダイアログ |
| 同一フレームの Intent が部分適用される | `Edit` が途中で失敗 | 不可分にしたいものは `Edit::Batch` 1 件にする。`apply_intents` は最初の失敗で打ち切る |
| パネル内 panic でアプリが落ちる | 添字・`unwrap` | `architecture.rs` で `panels/` 内の `unwrap()` / `expect(` を禁止。`std::panic::set_hook` は 41 のクラッシュレポートへ |
| 悪意ある `Performer::label` / `Set::name` で UI が壊れる | 改行・制御文字・U+202E（RTL override）・極端な長さ | 表示直前に `sanitize_label(&str) -> Cow<str>`：制御文字と双方向制御文字を除去、64 文字で省略。**表示層の責務なので `drill-app` に置く**（`drill-core` は生データを保持する） |
| ステータス行からユーザー名が漏れる | フルパス表示（`main.rs:245` / `453`、D-12） | `display_path()` でホームディレクトリを `~` に置換。完全パスはツールチップにのみ出す |
| `rfd` の同期ダイアログで UI が固まる | `save_file()` がブロッキング（D-11） | ファイル選択は `rfd::AsyncFileDialog` で 40 のジョブへ。ダイアログ中は自動保存タイマを止める |
| 4,000 人でパネルが固まる | O(n) の文字列生成 | インスペクタは先頭 20 件 ＋ `… 他 N 人`。演者リストは仮想化。集約は `DerivedCache` |
| `zoom_factor` を上げすぎてウィンドウにパネルが入らない | 200% ＋ 狭いウィンドウ | パネルは `size_range` の下限を持ち、下回るときは自動的に折りたたむ（`Panel::show_collapsible`）。`min_inner_size` を設定 |
| 200% で起動ウィンドウが画面外 | 論理サイズ固定（D-4） | `boot::window::clamp_to_monitor` を初回フレームで実行 |
| 支援技術から中身が見えない | eframe の `accesskit` 未有効（D-1） | フィーチャ追加 ＋ `widget_info` 付与。テストで `accesskit_update` を検証 |
| 未保存で終了しデータ喪失 | 確認が無い（D-18） | `close_requested` → `CancelClose` → 確認ダイアログ（3.11） |
| 信頼できないプロジェクトファイルを開いた直後に UI が落ちる | 巨大な演者数・セット数 | 読込は 41 / 51 の上限検証を通す。UI 側は「開いた結果」しか見ないので、パネルは常に上限（4,000 人 / 256 セット）を前提に仮想化しておく |

---

## 7. テスト計画

### 単体（`crates/drill-app/tests/`）

| # | 項目 | 対応する不変条件 |
|---|---|---|
| U1 | `COMMANDS` のショートカットが重複しない | 6 |
| U2 | `COMMANDS` の `command` が一意でメニューが必ず設定されている | 4 |
| U3 | `destructive` な `Command` の `dispatch` が `Intent::Dialog` を出す | 7 |
| U4 | `Selection::revalidate` が消えた `PerformerId` を落とす | 8 |
| U5 | `Cursor::resolve` が未知の `SetId` で `(0, 0.0)` を返す | 10 |
| U6 | `sanitize_label` が制御文字・U+202E を除去し 64 字で省略する | 6章 |
| U7 | `display_path` がホームディレクトリを `~` にする | 6章 |
| U8 | `contrast_ratio` の実装が既知の値と一致（白/黒 = 21:1） | 14 |
| U9 | テーマの全ペアが 4.5:1 以上 | 14 |
| U10 | `FieldViewport::to_field(to_screen(p)) ≈ p`（property） | — |
| U11 | `DerivedCache` が同じ鍵で再計算しない（呼び出し回数カウンタ） | 9 |
| U12 | `PanelScratch::default()` 復元後に状態が一致 | 3 |

### アーキテクチャ検査（`tests/architecture.rs`）

| # | 項目 | 不変条件 |
|---|---|---|
| A1 | `main.rs` ≤ 60 行、各ファイルの行数上限 | 12 |
| A2 | `panels/` が `std::fs` / `std::process` / `rfd` / `std::thread` を含まない | 13 |
| A3 | `panels/` `view/` が `&mut Document` / `DocumentState` を含まない | 1 |
| A4 | `DocumentState::apply` の呼び出しがソース全体で 1 箇所 | 2 |
| A5 | `panels/` に `analyze_transition(` / `unwrap()` / `.sets[` が無い | 6章 |

### ゴールデン

| # | 項目 |
|---|---|
| G1 | メニュー構造のテキスト表現を `.expected` と比較（日本語・英語の 2 本）。発見性の回帰防止 |
| G2 | ショートカット一覧のテキスト表現を `.expected` と比較 |
| G3 | 1,000 人ドキュメントの 1 フレーム `FullOutput.shapes` の個数が閾値以下 |

### ヘッドレス UI（`Context::run_ui`）

| # | 項目 | 不変条件 |
|---|---|---|
| H1 | 1,000 人 / 64 セットで 1 フレーム描画してパニックしない | — |
| H2 | 生成ウィジェット数 ≤ 400（`accesskit_update` のノード数で代用） | 16 |
| H3 | `ctx.enable_accesskit()` 後、`accesskit_update` に主要コマンドのノードが存在し、空ラベルが無い | 15 |
| H4 | `zoom_factor` = 0.5 / 1.0 / 1.5 / 2.0 の各値でレイアウトが破綻しない（水平オーバーフローが無い） | 17 |
| H5 | 空ドキュメント・セット 1 個・演者 0 人でパニックしない | — |
| H6 | 2 フレーム目以降 `DerivedCache` のバッファポインタが変化しない | 11 |

### property / ストレス

| # | 項目 |
|---|---|
| P1 | ランダムな `Intent` 列 10,000 件を適用しても不変条件 8 / 10 が保たれる |
| P2 | ランダムな `Command` 列を `dispatch` してもパニックしない |
| P3 | 10,000 回の編集後に Undo を全て実行して初期ドキュメントに一致（DESIGN_GAPS C-2 と共有） |

### ベンチ（`benches/panels.rs`）

| # | 項目 | 閾値 |
|---|---|---|
| B1 | 1,000 人 / 64 セットでの非フィールドパネル合計 | p95 ≤ 1.5 ms |
| B2 | 各パネル個別 | 5.1 の表の値 |
| B3 | フィールドビューの `DrawCmd → Painter` | p95 ≤ 3.0 ms |
| B4 | 4,000 人 / 256 セット（上限規模。劣化してよいが 33 ms 以内） | ≤ 33 ms |

### 手動（`docs/qa/` のチェックリストとして 50 へ渡す）

| # | 項目 |
|---|---|
| M1 | Windows 100 / 125 / 150 / 175 / 200% での 5 項目チェック（3.10.6） |
| M2 | ナレーター（Windows）でメニュー・ツールバー・インスペクタ・タイムラインを読み上げ、演者を選択して移動できる |
| M3 | マウスを一切使わずに「開く → セット選択 → 演者選択 → 移動 → 再生 → 保存」を完走 |
| M4 | 初回起動テスト: マーチング経験はあるが本アプリ未経験の被験者 5 人が、説明書なしで「セット選択 → 演者選択 → 編集 → 再生」を平均 90 秒以内・離脱ゼロ |
| M5 | 日本語 UI と英語 UI の両方で全パネルの文字が枠に収まる |
| M6 | 高DPI / 低DPI モニタ間でウィンドウを往復させる |

---

## 8. 実装タスク

Wave 3（DESIGN_GAPS の実装順 8 に相当）。1 タスク = 1〜3 時間。

| # | タスク | 依存 | 並行 |
|---|---|---|---|
| T1 | `lib.rs` 追加でライブラリ化。`boot/`（fonts / theme / window）を抽出。D-2 のコントラスト修正、D-4 のウィンドウクランプ、`min_inner_size` 設定。`contrast_ratio` と U8 / U9 | — | — |
| T2 | `state/` 6 モジュールの型定義と、`DrillApp` の 28 フィールドの移送（3.1 の表どおり）。振る舞いは変えない。`dirty` と `tempo_bpm` を廃止 | T1 | — |
| T3 | `AppView` / `Intent` / `IntentQueue` / `PanelUi` / `PanelScratch` の定義と `app.rs` のフレームパイプライン 7 段。既存 UI をそのまま 1 パネルとして動かす | T2 | — |
| T4 | `input/shortcuts.rs`: `Command` 列挙と `COMMANDS` 表の全行、`poll`、`dispatch`（ワイルドカード無し `match`）。U1 / U2 / U3 | T3 | ◯ |
| T5 | `panels/menubar.rs`: 表からのメニュー生成、`format_shortcut`、`gui_zoom::zoom_menu_buttons`。G1 / G2 | T4 | ◯ |
| T6 | `panels/` の分割（toolbar / setlist / inspector / timeline / clinic / export / audio / jobs / statusbar）。400 行上限。仮想化（`show_rows`）と `ScrollArea` 包み（D-3） | T3 | ◯ |
| T7 | `view/viewport.rs` の `FieldViewport` に 5 つの座標変換を統合。`view/field2d.rs` 抽出。フォーカス可能化とキーボード選択（D-15） | T3 | ◯ |
| T8 | `view/stadium3d.rs` 抽出（`main.rs:292-414`）。21 / 22 へ移送するまでの隔離。深度ソートの毎フレーム `Vec` 確保を `DerivedCache` へ | T3 | ◯ |
| T9 | `state/derived.rs`: `FrameKey` ゲート、クリニックの 120 ms デバウンス、`LabelCache`（D-5 / D-6 の app 側）。U11 / H6 | T2 | ◯ |
| T10 | `input/pointer.rs`: ツールモード表の実装、`DragSession::preview`（ドラッグ中に `Document` を触らない）、矩形選択、Esc の段階的中止 | T3, T7 | — |
| T11 | `dialogs/`: `DialogStack`（深さ1）、`ConfirmRequest`、`close_requested` → `CancelClose`、Undo トースト（D-18） | T3, T4 | ◯ |
| T12 | `persistence` フィーチャ有効化、`UiLayout` の保存／復元、パネル表示切替、`PanelResetLayout`（D-17） | T3, T6 | ◯ |
| T13 | `i18n.rs`: `TextId` / `TextTable`、`main.rs` の日本語リテラル全移送、フォント優先順の切替（D-19） | T5, 42 | ◯ |
| T14 | `accesskit` フィーチャ有効化、`widget_info` 付与、色以外の表現（3.10.4 の 9 項目）、フォーカスリング（D-1 / D-13 / D-14） | T5, T6, T7 | — |
| T15 | `panels/welcome.rs`: ウェルカム画面、テンプレート、サンプルプロジェクト（`assets/samples/showcase.drill.json` の作成を含む）、コーチマーク | T3, T12 | ◯ |
| T16 | 業務ロジックの移送（3.9 の表のうち `drill-app` 側の呼び出し変更）。`selection_bounds` / `transform_selection` / `commit_layout` / `auto_assign_next` / 図形の寸法決定 | T3, 10 の `Edit` | — |
| T17 | `tests/architecture.rs`（A1〜A5）、ヘッドレステスト（H1〜H6）、`benches/panels.rs`（B1〜B4） | T6, T14 | — |

**直列**: T1 → T2 → T3。ここまでは分割できない。
**並行**: T3 完了後、T4〜T9・T11・T12・T15 は独立に進められる。
T10 は T7 に、T14 は T5 / T6 / T7 に、T17 は全体に依存する。
T13 は 42（`Locale` / `DrillError`）、T16 は 10（`Edit`）の完了を待つ。

---

## 9. 未決事項

| # | 内容 | 決めるために必要なもの |
|---|---|---|
| 1 | 自由なドッキング（タブのドラッグ移動）を入れるか。`egui_dock` を入れると依存が増え、`00-conventions.md` のクレート表にも載らない | 実利用者がパネル配置を変えたがるかの検証。まず固定ドック＋表示切替で出荷し、要望を見てから判断する |
| 2 | コマンドパレット（`Cmd+P`）を入れるか。`COMMANDS` 表があるので実装は 1 パネル分だが、Pyware には無い操作系 | M4 の初回起動テストで「操作が見つからない」が観測されるかどうか |
| 3 | `UiState::camera` の所有権。暫定で 43 が持つが、キーフレームを持つなら 23 が持つべき | 23 の `CameraTrack` の型が決まり次第 |
| 4 | `beats_per_measure`（`main.rs:73`）を `Document` に入れるか、UI 設定のままにするか。カウントシート出力に影響する | 17 の決定 |
| 5 | `accesskit` フィーチャ有効化による Windows 起動時間とメモリの増分。遅延生成なので小さいはずだが未計測 | T14 後に実測 |
| 6 | フィールドビューの 3.0 ms が妥当か。21 の GPU インスタンシングが入ると `DrawCmd → Painter` が不要になり、43 の取り分は減る | 21 の実測後に 5.1 の表を再配分 |
| 7 | `rfd` の同期ダイアログをジョブ化するか、`AsyncFileDialog` を直接使うか | 40 のジョブ基盤が `!Send` な future を扱えるか |
| 8 | Undo トーストの表示時間（5 秒）と、連続操作時の集約方法 | M4 の観察 |
| 9 | 明色テーマを出荷するか。屋外・プロジェクタ利用の要望次第 | 販売前のユーザー調査（53） |
| 10 | フィールドビューのアクセシブル表現として、演者ごとにノードを作る「詳細モード」を用意するか（100 人以下のときだけ有効にする等） | M2 のナレーター実地評価 |
