# 13. 衝突・間隔・クリニック解析エンジン

## 1. 目的と範囲

### 解決すること

1. **掃引（swept）衝突検査** — 到着セットの静止状態だけでなく、セット間の移動の**途中**で起きる接近・接触を検出する。
2. **計算量の解消** — 現在の O(n²) 総当たりを空間ハッシュによる実質 O(n) へ置き換える。
3. **確保ゼロ** — 走査は再利用可能な作業領域（`ScanScratch`）の上で行い、2回目以降のヒープ確保をゼロにする。
4. **呼び出し規約の是正** — 毎フレーム無条件で走らせる現在の実装をやめ、リビジョン差分とデバウンスで駆動する。
5. **間隔解析** — 最小間隔・平均間隔・密集ヒートマップ・等間隔性の逸脱検出。
6. **索引** — UI が「今のカウントで衝突している演者」を実質 O(1) で引ける結果構造。

### この文書が扱わないこと

| 事項 | 担当 |
|---|---|
| `SetId` / `PerformerId` の安定ID、`Edit` コマンド、`Document::revision` | 10. ドキュメントモデルと編集コマンド代数 |
| `Route` / `Gate` / `SetCounts` / `positions_at_count` の定義と補間 | 11. 遷移モデル |
| `StepStyle` / `ClinicParams` / 歩幅難易度（`StrideRating`）の定義 | 12. マーチングスタイルと歩幅 |
| フォーム（線・弧）のメタデータ | 14. フォーメーション生成とシェイプツール |
| 衝突マーカーの描画コマンド | 20. DisplayList |
| `Job<T>` の実体 | 40. 非同期ジョブ基盤 |
| `DrillError` / `Locale` の定義 | 42. エラー型・i18n |

本書は上記から**型を受け取る側**であり、それらを再定義しない。本書が要求する外部からの入力は §3.1 に列挙する。

配置先クレートは `drill-core`（新規モジュール `crates/drill-core/src/clinic.rs`）。依存追加なし（`serde` / `serde_json` / `std` のみ）。人間可読テキストは全て `Locale` 経由で解決し、`drill-core` に日本語リテラルを置かない。

---

## 2. 現状

### 2.1 いま在るもの

| 場所 | 内容 |
|---|---|
| [lib.rs:180-184](../../crates/drill-core/src/lib.rs#L180) | `TransitionAnalysis { collisions: usize, excessive_strides: usize }`。件数のみ。 |
| [lib.rs:186-223](../../crates/drill-core/src/lib.rs#L186) | `analyze_transition(document, set_index, collision_distance, max_step_per_count)` |
| [lib.rs:199-207](../../crates/drill-core/src/lib.rs#L199) | 衝突判定本体。`to.positions`（＝**到着セットのみ**）に対する二重ループ。`for i in 0..len { for j in (i+1)..len { ... } }`。 |
| [lib.rs:208-218](../../crates/drill-core/src/lib.rs#L208) | 歩幅判定。`from`/`to` の zip で直線距離 ÷ counts を閾値比較。O(n)。 |
| [lib.rs:384-394](../../crates/drill-core/src/lib.rs#L384) | `positions_at(set_index, progress, out)`。正規化 progress の**単純線形補間**のみ。`out` 再利用あり。 |
| [pathing.rs:97-105](../../crates/drill-core/src/pathing.rs#L97) | `nearest_neighbor_interval(&[Point]) -> f32`。同じく O(n²) の二重ループ。 |
| [pathing.rs:29-51](../../crates/drill-core/src/pathing.rs#L29) | `transition_moves` が呼び出しごとに `Vec<PerformerMove>` を `collect()`。 |
| [pathing.rs:67-68](../../crates/drill-core/src/pathing.rs#L67) | `transition_stats` はその `Vec` を毎回作る。 |
| [main.rs:1011](../../crates/drill-app/src/main.rs#L1011) | `analyze_transition(&self.document, self.current_set, 0.75, 1.0)` をサイドパネル描画クロージャ内で**毎フレーム無条件**に呼ぶ。閾値は生の float リテラル。 |
| [main.rs:1033](../../crates/drill-app/src/main.rs#L1033) | `pathing::transition_stats(&self.document, self.current_set)` を同じく毎フレーム。内部で `Vec` 確保。 |
| [main.rs:1015-1031](../../crates/drill-app/src/main.rs#L1015) | 警告表示が `Color32::from_rgb` の緑/赤/橙のみ。記号は `●` 固定で、色を落とすと状態が読めない。 |
| [benches/core_performance.rs](../../crates/drill-core/benches/core_performance.rs) | 補間とJSON化のみ。衝突走査のベンチは無い。 |

### 2.2 無いもの（実測・確認済み）

- **移動途中の判定が一切ない。** [lib.rs:199](../../crates/drill-core/src/lib.rs#L199) のループは `to.positions` だけを見る。交差してすれ違う2人は、到着点が離れていれば恒久的に見逃される。
- **衝突した演者が誰かを返さない。** `collisions: usize` の件数だけで、UI は「どの2人が、いつ」を表示できない。
- **空間分割が無い。** `drill-core` 全体を `grep` しても格子・ハッシュ・BVH に相当する構造は存在しない。
- **`ScanScratch` に相当する型が無い。** 再利用される作業領域は `positions_at` の `out: &mut Vec<Point>` だけ。
- **リビジョンが無い。** `DrillApp`（[main.rs:69](../../crates/drill-app/src/main.rs#L69)）が持つのは `dirty: bool`（[main.rs:90](../../crates/drill-app/src/main.rs#L90)）のみ。差分再計算の起点になる単調増加カウンタは存在しない。
- **NaN/Inf の防壁が無い。** `Document::validate`（[lib.rs:302-333](../../crates/drill-core/src/lib.rs#L302)）は schema_version・空セット・グリッド寸法・座標数・ID重複しか見ない。`Point` の有限性検査は `drill-core` のどこにも無い（`is_finite` の出現は `audio.rs:66` / `playback.rs` / `tempo.rs:195` / `camera.rs` のテストのみ）。**JSON 経由で `NaN` / `Infinity` を含む座標が `Document` に入り得る。**
- **`ClinicParams` / `StepStyle` / `Severity` が無い。** 閾値は呼び出し側の float リテラル。

### 2.3 現在のコストの見積り

1,000人・64セットの基準規模で [main.rs:1011](../../crates/drill-app/src/main.rs#L1011) が毎フレーム行う比較回数:

```
C(1000, 2) = 499,500 回 / フレーム
```

60fps では毎秒 2,997万回。加えて [main.rs:1033](../../crates/drill-app/src/main.rs#L1033) が毎フレーム 1,000 要素の `Vec<PerformerMove>`（16 B × 1,000 = 16 KB）を確保・解放する。`PRODUCT_QUALITY.md` の「フレーム内ヒープ再確保ゼロ」と「16.6ms 以内」の両方を、**現在この2行が単独で破っている**（DESIGN_GAPS §0 の #3・#4）。

---

## 3. 設計

### 3.1 前提として受け取る外部の型

本書は次を「既に在るもの」として使う。実装順は §8 に従い、無い間は括弧内の暫定で置き換える。

```rust
use crate::{Document, DrillError, Locale, PerformerId, Point, SetId};
// 11. 遷移モデル:
//   Document::positions_at_count(&self, set_index: usize, local_count: f32, out: &mut Vec<Point>)
//   Set::counts: SetCounts { moves: u16, hold: u16 }
//   Route::curvature_bound(&self) -> f32   // |B''(t)| の上界（Straight は 0.0）
// 12. 歩幅:
//   StepStyle::step_units(&self, unit: Unit) -> f32   // 1歩の長さ（グリッド単位）
//   ClinicParams { style, collision_radius, aggressive_above, impossible_above, .. }
```

暫定運用（10/11/12 が未着地の間）: `SetId` → `usize`、`positions_at_count` → 既存 `positions_at(set, local/counts, out)`、`curvature_bound` → `0.0`。この差し替えで本モジュールは単独でコンパイル・テスト可能である。

### 3.2 定数と閾値

```rust
/// 空間ハッシュのセル総数の上限。超える場合はセル幅を倍にして再試行する。
pub const MAX_GRID_CELLS: usize = 1 << 20;      // 1,048,576 セル ≒ 8 MB（heads+stamp）
/// 1セットあたりのサンプル区間数の上限。counts が異常に大きいファイルへの防壁。
pub const MAX_SUBINTERVALS: u32 = 4_096;
/// 1回の走査で保持するイベント数の上限。超過分は件数のみ集計する。
pub const DEFAULT_MAX_EVENTS: usize = 4_096;
/// 同時に開いている（未確定の）ペア区間の上限。
pub const DEFAULT_MAX_OPEN_PAIRS: usize = 4_096;
/// これ以下の演者数なら 1 遷移の走査を UI スレッド同期で行ってよい。
pub const SYNC_SCAN_LIMIT_PERFORMERS: usize = 1_500;
/// 近傍リング探索の上限（間隔解析）。
pub const MAX_RING: u32 = 16;
```

### 3.3 severity と半径

```rust
/// 重大度。数値が大きいほど深刻。`Ord` は severity の比較にそのまま使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum Severity {
    /// 密集: フォームとして詰まりすぎているが、当たってはいない。
    Crowded = 0,
    /// 危険: 楽器・肘・進行方向次第で当たる距離。
    Danger = 1,
    /// 接触: 身体が重なる距離。
    Contact = 2,
}

impl Severity {
    /// 表示用の短いラベル。`drill-core` は文字列リテラルを持たないので `Locale` へ委譲する。
    pub fn label(self, locale: Locale) -> &'static str;
    /// 色に依存しない記号（PRODUCT_QUALITY「色だけに依存しない」）。
    pub fn glyph(self) -> char {
        match self {
            Severity::Contact => '✕',
            Severity::Danger => '⚠',
            Severity::Crowded => '△',
        }
    }
}

/// 判定半径。単位は `GridConfig::unit`（ヤードまたはメートル）。
/// 「中心間距離」であり、演者の占有円の直径に相当する。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClinicRadii {
    pub contact: f32,
    pub danger: f32,
    pub crowded: f32,
}

impl ClinicRadii {
    /// ヤード基準の既定値。
    /// contact 0.45 yd ≒ 16 in（肩幅の実測に楽器のクリアランスを足した値）。
    /// danger 0.75 yd（既存 `main.rs:1011` の 0.75 と一致させ、挙動の連続性を保つ）。
    /// crowded 1.10 yd ≒ 2 step（8 to 5 の 2 歩）。
    pub fn yards() -> Self { Self { contact: 0.45, danger: 0.75, crowded: 1.10 } }
    pub fn meters() -> Self { Self { contact: 0.41, danger: 0.69, crowded: 1.01 } }

    /// 単調性（contact ≤ danger ≤ crowded）と正値・有限性を強制する。
    /// 不正値は既定値へフォールバックし、`clamped` を立てる。
    pub fn sanitized(self) -> (Self, bool);

    #[inline]
    pub fn severity_of(self, distance: f32) -> Option<Severity> {
        if distance < self.contact { Some(Severity::Contact) }
        else if distance < self.danger { Some(Severity::Danger) }
        else if distance < self.crowded { Some(Severity::Crowded) }
        else { None }
    }

    #[inline]
    pub fn max(self) -> f32 { self.crowded }
}
```

### 3.4 走査パラメータ

```rust
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScanParams {
    pub radii: ClinicRadii,
    /// 1カウントあたりのサンプル数。既定 2（半カウント刻み）。0 は 1 に丸める。
    pub samples_per_count: u8,
    /// これ未満の severity はイベント化しない。既定 `Danger`。
    /// `Crowded` を含めると密集ブロックで数万件出るため、密集は `SpacingReport` の
    /// ヒートマップで見せ、イベント列には載せないのが既定。
    pub report_at_least: Severity,
    /// 同一ペアのイベントを畳む際に許す途切れ（カウント）。既定 1.0。
    /// 閾値付近のチャタリングで 1 区間が細切れになるのを防ぐ。
    pub merge_gap_counts: f32,
    /// 曲線経路をサンプル間で直線近似することによる誤差の補償（グリッド単位）。
    /// 既定 0.02。`Route::curvature_bound` が使えるなら §3.7 の式で自動算出する。
    pub curve_slack: f32,
    /// 1 サブ区間で 1 人が動ける距離の上限（グリッド単位）。これを超える演者は
    /// 掃引判定から除外し `Excluded::Teleport` として報告する。既定は
    /// `8.0 * StepStyle::step_units()`。格子セル幅の暴走を防ぐための必須の栓。
    pub segment_cap: f32,
    pub max_events: usize,
    pub max_open_pairs: usize,
}

impl Default for ScanParams { /* radii: yards(), samples_per_count: 2, ... */ }

impl ScanParams {
    /// 12 章の `ClinicParams` からの導出。12 章側に生やす（本書は署名のみ規定）。
    pub fn from_clinic(params: &ClinicParams, unit: Unit) -> Self;
    /// 全フィールドを安全域へ丸める。走査の入口で必ず呼ぶ。
    pub fn sanitized(self) -> (Self, ParamWarnings);
}
```

### 3.5 空間ハッシュ

セル幅 = 判定半径（+ 掃引の場合は最大移動量）の一様格子。バケット配列 + チェイン。**確保は `reshape` の中だけ**で起き、寸法が変わらない限り no-op になる。

```rust
/// 一様格子の空間ハッシュ。バケット配列 + 単方向チェイン。
/// 走査ごとに `begin()` を呼ぶ。`heads` の全消去はせず世代スタンプで無効化するので、
/// セル数に比例するコストがサンプルごとに発生しない。
#[derive(Default, Debug)]
pub struct SpatialHash {
    heads: Vec<u32>,      // len = cols * rows。stamp[c] == generation のときのみ有効。
    stamp: Vec<u32>,      // len = cols * rows
    next: Vec<u32>,       // len = capacity。u32::MAX が終端。
    point: Vec<Point>,    // len = capacity。チェイン走査時の間接参照を1段減らす。
    nonempty: Vec<u32>,   // この世代で 1 件以上入ったセル。ペア列挙はここだけを回る。
    cols: u32,
    rows: u32,
    origin: Point,
    cell_size: f32,
    inv_cell: f32,
    generation: u32,
}

impl SpatialHash {
    /// 格子の寸法を決める。`cell_size` は正の有限値。セル総数が `MAX_GRID_CELLS` を
    /// 超える場合、超えなくなるまで `cell_size` を 2 倍にする（最大 32 回で必ず収束する）。
    /// 実際に確保が起きるのは `heads`/`stamp`/`next`/`point`/`nonempty` のいずれかが
    /// 伸びるときだけ。縮む方向では確保しない（`truncate` のみ）。
    pub fn reshape(&mut self, bounds: Bounds, cell_size: f32, capacity: usize);

    /// 世代を進める。O(1)。`u32` の巻き戻り時のみ `stamp` を 0 で埋める（2^32 世代に 1 回）。
    pub fn begin(&mut self);

    /// `index` は 0..capacity。`p` は有限であること（呼び出し側が保証する）。
    #[inline]
    pub fn insert(&mut self, index: u32, p: Point);

    /// 同一セルおよび 8 近傍セルに入っている**順序なしペアをちょうど 1 回ずつ**訪問する。
    /// 実装は「非空セルを列挙 → 自セルのチェイン内の後続ペア + 前方 4 近傍
    /// (+1,0) (-1,+1) (0,+1) (+1,+1) との全組合せ」。全 9 近傍を舐めて j>i で
    /// 捨てる方式に比べ訪問コストがほぼ半分になる。
    /// `f` が `false` を返した時点で打ち切る（イベント上限に達した場合）。
    pub fn for_each_candidate_pair(&self, f: impl FnMut(u32, u32, Point, Point) -> bool);

    /// `p` から半径 `ring` セル分の正方リング上のセルだけを訪問する（間隔解析用）。
    pub fn for_each_in_ring(&self, p: Point, ring: u32, f: impl FnMut(u32, Point));
}
```

**セル索引のクランプが安全である根拠**: `cell_x = clamp(floor((x - origin.x) * inv_cell), 0, cols-1)` は `x` に対して単調非減少かつ 1-Lipschitz（`inv_cell` 倍のスケール後）である。したがって世界座標で `cell_size` 以内にある 2 点のセル索引は各軸で高々 1 しか違わない。**格子の外に居る演者を端のセルへ丸め込んでも近傍ペアを取りこぼさない。** 逆に遠く離れた点が同じ端セルに詰まることはあるが、距離判定で落ちるだけで誤検出にはならない。

**`bounds` の決め方**: `GridConfig` の寸法ではなく、そのサブ区間に実際に居る有限座標の AABB を使う。これによりフィールド外へ大きく飛ばした座標があってもセル数が `MAX_GRID_CELLS` の枠内に自動で収まる。

### 3.6 ScanScratch — 2 回目以降の確保ゼロ

```rust
/// フレーム/走査をまたいで再利用する作業領域。
/// **本モジュールでヒープ確保が起きるのはこの型の `ensure_capacity` の中だけである。**
/// 走査本体（`scan_transition` 以下）に `Vec::push` は在るが、
/// 全て `ensure_capacity` で上限まで `reserve_exact` 済みの Vec に対する push であり、
/// 上限は `max_events` / `max_open_pairs` / `performers` / セル数の関数として固定されている。
#[derive(Default, Debug)]
pub struct ScanScratch {
    prev: Vec<Point>,          // サンプル k の座標
    curr: Vec<Point>,          // サンプル k+1 の座標
    mid: Vec<Point>,           // サブ区間の中点（広域判定の代表点）
    alive: Vec<bool>,          // この区間で判定対象にする演者
    grid: SpatialHash,
    open: Vec<OpenPair>,       // 未確定のペア区間
    open_slot: Vec<u32>,       // オープンアドレス法の索引。len は 2 の冪。
    nn: Vec<f32>,              // 最近傍距離（間隔解析）
    nn_sorted: Vec<f32>,       // 分位点計算用のコピー（select_nth_unstable、確保なし）
    density: Vec<u32>,         // ヒートマップの生カウント
    density_blur: Vec<u32>,
    result: TransitionScan,    // A-4 互換の借用返却用
    dims: ScratchDims,         // 現在確保済みの寸法。一致すれば ensure_capacity は no-op。
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
struct ScratchDims {
    performers: usize,
    cells: usize,
    max_events: usize,
    max_open_pairs: usize,
    density_cells: usize,
}

impl ScanScratch {
    /// 事前確保。アプリはドキュメント読込時に 1 回呼んでおけば、
    /// 以降の走査は確保ゼロで回る。
    pub fn reserve(&mut self, performers: usize, params: &ScanParams, bounds: Bounds);

    /// 走査の入口で呼ぶ。`dims` が一致すれば即 return（no-op）。
    fn ensure_capacity(&mut self, dims: ScratchDims);

    /// テスト用。全 Vec の `as_ptr()` と `capacity()` を返す。
    #[cfg(test)]
    pub(crate) fn fingerprint(&self) -> Vec<(usize, usize)>;
}
```

**「型で担保する」の実体**（完全な型レベル証明は Rust では書けないので、確保の入口を型で 1 か所に絞る設計にする）:

1. 走査関数は `&mut ScanScratch` を取り、**結果を値で返さない**。返すのは `&[CollisionEvent]` か、呼び出し側が所有する `&mut TransitionScan` への書き込み。返り値経由の確保が構造的に起きない。
2. `Vec` を伸ばす操作（`push` / `resize` / `extend`）は `ensure_capacity` 以外では**上限が定数で決まっている Vec に対してのみ**行われる。上限超過は `push` ではなく `suppressed += 1` に落ちる（§3.9）。
3. `SpatialHash` のフィールドは全て非公開で、伸長は `reshape` 経由のみ。
4. `ScratchDims` の一致判定により、同じ寸法での 2 回目以降は `reshape` も `ensure_capacity` も分岐 1 個で抜ける。
5. 検証はテストで行う（§7 の `scan_is_allocation_free_after_warmup`。既存の `interpolation_reuses_output_allocation` と同じポインタ比較方式）。

### 3.7 掃引検査

#### サンプリングと補間

セット `i` の遷移を、`counts = sets[i].counts.moves`、`s = params.samples_per_count` として
`n_sub = counts * s` 個のサブ区間に分ける。サンプル時刻はカウントの**整数演算**から求める:

```rust
// 浮動小数の累加をしない（00-conventions 不変条件 4 と同じ方針）。
let local_count_at = |k: u32| -> f32 { k as f32 / s as f32 };
```

サンプル座標は `positions_at_count`（11 章）で `prev` / `curr` に書き、区間ごとに
`std::mem::swap(&mut prev, &mut curr)` で 1 サンプル 1 回の評価に抑える。

#### 広域判定（空間ハッシュ）

サブ区間 `k` について、各演者の線分 `a0 = prev[i]`, `a1 = curr[i]` の**中点** `mid[i]` を格子へ入れる。
セル幅は

```
cell = radii.max() + L_max + curve_slack
L_max = max_i |curr[i] - prev[i]|   （segment_cap でクランプ済み）
```

**中点 1 点だけの挿入で取りこぼさない根拠**: 相対変位を `d(t) = w + t·u`（`w = a0 - b0`, `u = (a1-a0) - (b1-b0)`, `t ∈ [0,1]`）と置くと、中点間距離は
`|mid_a - mid_b| = |w + u/2| = |d(0.5)|` である。`d` はアフィンなので任意の `t*` に対し
`|d(0.5)| ≤ |d(t*)| + |u|·|0.5 - t*| ≤ |d(t*)| + |u|/2` が成り立つ。
`|u| ≤ |a1-a0| + |b1-b0| ≤ 2·L_max` だから、最接近距離が `r` 未満のペアの中点間距離は必ず `r + L_max` 未満。
セル幅をこの値以上に取れば、両者のセル索引は各軸で高々 1 しか違わない（§3.5 のクランプ単調性）。
よって 3×3 近傍（＝ `for_each_candidate_pair` の走査範囲）で必ず捕捉される。∎

#### 狭域判定（線分同士の最接近）

サンプリングだけでは高速すれ違いを取り逃す。候補ペアには**同時刻に動く 2 点の最接近距離**を解析的に解く。
静的な線分間距離ではないことに注意する（経路が交差していても通過時刻が違えば衝突ではない）。

```rust
/// A(t) = a0 + (a1-a0)t, B(t) = b0 + (b1-b0)t を **同時に** 動かしたときの
/// t ∈ [0,1] における最接近。戻り値は (距離の二乗, その t)。
/// 平方根はここでは取らない（閾値比較は二乗のまま行う）。
#[inline]
fn closest_approach(a0: Point, a1: Point, b0: Point, b1: Point) -> (f32, f32) {
    let wx = a0.x - b0.x;
    let wy = a0.y - b0.y;
    let ux = (a1.x - a0.x) - (b1.x - b0.x);
    let uy = (a1.y - a0.y) - (b1.y - b0.y);
    let uu = ux * ux + uy * uy;
    // uu == 0 は「相対速度ゼロ = 平行移動」。t=0 の距離が全区間の距離。
    let t = if uu > 1e-12 {
        (-(wx * ux + wy * uy) / uu).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = wx + ux * t;
    let dy = wy + uy * t;
    (dx * dx + dy * dy, t)
}
```

15 flops・分岐 1・平方根なし。閾値未満だったペアにだけ `sqrt` を 1 回かけてイベントに載せる。

**曲線経路の近似誤差**: 二次ベジェをパラメータ幅 `h` の区間で弦に置き換えたときの最大偏差は `(h²/8)·|B''|`、`|B''| = 2|P0 - 2C + P2|`。半カウント刻み・16 カウントなら `h = 1/32`。制御点のオフセットが 10 yd（極端な曲がり）でも
`(1/1024)/8 × 40 = 0.0049 yd ≒ 0.18 in`。contact 半径 0.45 yd に対して 1% 未満。
`curve_slack` の既定 0.02 yd はこの 4 倍のマージンで、判定は**必ず保守側**（見逃しではなく過検出側）に倒れる。
11 章の `Route::curvature_bound()` が入ったら `curve_slack = curvature_bound * h * h / 8.0` を自動で使う。

#### 主関数

```rust
/// セット `set` から次のセットへの遷移を掃引検査する。
/// 結果は `out` に**上書き**される（`out` の Vec は clear されるだけで容量は保たれる）。
///
/// `set` が存在しない、または次のセットが無い場合は `out` を空にして `Ok(())`。
/// `DrillError` を返すのは `set` が `Document` に無い ID の場合のみ。
pub fn scan_transition(
    document: &Document,
    set: SetId,
    params: &ScanParams,
    scratch: &mut ScanScratch,
    out: &mut TransitionScan,
) -> Result<(), DrillError>;

/// DESIGN_GAPS A-4 の署名に合わせた薄いラッパ。結果は `scratch` 内に置かれる。
/// 1 遷移だけを見たい呼び出し（ライブクリニック表示）はこちらで足りる。
pub fn scan_transition_events<'s>(
    document: &Document,
    set: SetId,
    params: &ScanParams,
    scratch: &'s mut ScanScratch,
) -> Result<&'s [CollisionEvent], DrillError>;
```

擬似コード（確保が起きない形を明示する）:

```rust
let (params, warn) = params.sanitized();
let dims = ScratchDims { /* n, cells, max_events, ... */ };
scratch.ensure_capacity(dims);                 // 2 回目以降 no-op
out.clear();                                    // 容量保持

let n_sub = (counts as u32 * s as u32).min(MAX_SUBINTERVALS);
document.positions_at_count(index, 0.0, &mut scratch.prev);
mark_alive(&scratch.prev, &mut scratch.alive, out);   // 非有限を除外

for k in 0..n_sub {
    document.positions_at_count(index, local_count_at(k + 1), &mut scratch.curr);
    let l_max = fill_midpoints(scratch, &params, out); // segment_cap 超過は alive=false
    let bounds = Bounds::of_finite(&scratch.mid);
    scratch.grid.reshape(bounds, params.radii.max() + l_max + params.curve_slack, n);
    scratch.grid.begin();
    for i in 0..n { if scratch.alive[i] { scratch.grid.insert(i as u32, scratch.mid[i]); } }

    scratch.grid.for_each_candidate_pair(|i, j, _, _| {
        let (d2, t) = closest_approach(prev[i], curr[i], prev[j], curr[j]);
        if d2 < max_r2 {
            let d = d2.sqrt();
            if let Some(sev) = params.radii.severity_of(d) {
                if sev >= params.report_at_least {
                    return touch_open_pair(scratch, out, i, j, k, t, d, sev);
                }
            }
        }
        true
    });

    close_stale_pairs(scratch, out, k, &params);
    std::mem::swap(&mut scratch.prev, &mut scratch.curr);
}
close_all_pairs(scratch, out);
finalize(out, &params);      // 正準ソート + 索引構築
```

`counts.moves == 0`（ホールドのみ、または即座に到着）の場合は `n_sub = 0` になるので、
**静止状態 1 サンプルだけ**を評価する退化ケースへ落とす（`n_sub = n_sub.max(1)` とし、
`prev == curr` なら `closest_approach` は `uu = 0` 経路で `t = 0` を返す）。ゼロ除算は起きない。

### 3.8 CollisionEvent と区間へのマージ

```rust
/// カウント区間（セット内ローカルカウント）。`start <= end`。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CountSpan { pub start: f32, pub end: f32 }

/// 1 ペア × 1 連続区間 = 1 イベント。
/// 同じ 2 人が 8 サブ区間にわたって接近し続けても、イベントは 1 件になる。
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CollisionEvent {
    /// 常に a < b（正準順序）。ID であって索引ではない。
    pub a: PerformerId,
    pub b: PerformerId,
    /// 最接近が起きたローカルカウント（区間内の最悪の瞬間）。
    pub count: f32,
    /// 区間中の最小中心間距離。単位は `GridConfig::unit`。
    pub distance: f32,
    /// `distance` に対応する重大度（区間中の最悪値）。
    pub severity: Severity,
    /// 接近が継続したカウント区間。
    pub span: CountSpan,
}
```

DESIGN_GAPS A-4 の 4 フィールドに `severity` と `span` を追加している。理由:
`severity` は本課題の明示要件、`span` は「同一ペアの連続イベントを 1 区間へ畳む」の畳んだ結果そのもので、
これが無いと UI は「いつからいつまで危ないか」を出せず、タイムライン上の帯として描けない。

マージの実装:

```rust
#[derive(Clone, Copy, Debug)]
struct OpenPair {
    key: u64,           // (min_id as u64) << 32 | max_id as u64
    a: PerformerId,
    b: PerformerId,
    start: f32,
    end: f32,
    best_distance: f32,
    best_count: f32,
    worst: Severity,
    last_sub: u32,      // 最後に接近が観測されたサブ区間
}
```

- `open_slot` は容量 `max_open_pairs.next_power_of_two() * 2` のオープンアドレス法索引（線形探査）。
  `key` の混合には `wyhash` 相当のインラインな乗算＋xor（外部依存なし）を使う。
  探査回数の上限は容量とし、超えたら**新規オープンを諦めて `suppressed += 1`**（無限ループを型ではなくループ上限で断つ）。
- サブ区間 `k` でペアが観測されたら、既存エントリの `end` を伸ばし、`best_distance` / `worst` を更新する。
- `close_stale_pairs` は `open` を線形に走査し、`(k - last_sub) as f32 / s as f32 > merge_gap_counts` のものを
  `out.events` へ吐いて `swap_remove` する。`open` の長さは同時接近ペア数で、実務上は数十以下。
- `merge_gap_counts`（既定 1.0 カウント）により、閾値をまたいで出入りするチャタリングは 1 区間に畳まれる。

**正準化と決定論**: `finalize` で `out.events` を
`(span.start, a, b, severity)` の全順序で `sort_unstable_by` する（安定ソートは補助確保をするので使わない。
タイブレークを完全に入れているので不安定ソートでも出力は一意）。
浮動小数の比較には `f32::total_cmp` を使い、NaN が混じっても順序が定まるようにする。
これにより同じ入力から常にバイト同一のイベント列が出る（00-conventions 不変条件 5）。

### 3.9 除外と上限

```rust
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ExcludeReason {
    /// 座標に NaN / Inf が含まれる。
    NonFinite,
    /// 1 サブ区間の移動量が `segment_cap` を超えた（人間の移動ではない）。
    Teleport,
    /// 次セットに対応する座標が無い（セット間で演者数が不一致）。
    Unpaired,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Excluded {
    pub performer: PerformerId,
    pub reason: ExcludeReason,
    /// 最初に検出したローカルカウント。
    pub count: f32,
}

impl ExcludeReason {
    pub fn message(self, locale: Locale) -> &'static str;
}
```

除外された演者は掃引判定に参加しないが、`TransitionScan::excluded` に必ず載る。
UI は「3 名を解析から除外しました（座標が不正）」と表示できるので、**黙って落とさない**。

イベント上限超過時は `truncated = true` と `suppressed: u32` を立てる。
全員が同一座標に居る敵性ファイル（`C(4000,2) ≈ 800 万ペア`）でも、
保持は `max_events` 件、残りは件数のみ。メモリは有界。

### 3.10 間隔解析

こちらは**静止した 1 セット**に対する解析であり、遷移ではない。

```rust
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpacingReport {
    /// フォーム内の最小中心間距離。演者 2 人未満なら `f32::INFINITY`。
    pub min_interval: f32,
    pub mean_interval: f32,
    /// 下位 5% 分位。「一番詰まっている一角」の指標。平均より実務的。
    pub p05_interval: f32,
    pub tightest: Option<(PerformerId, PerformerId)>,
    /// 演者索引に整列した最近傍距離。孤立している演者は `f32::INFINITY`。
    pub nearest: Vec<f32>,
    /// 局所的な等間隔性から外れている演者。
    pub outliers: Vec<SpacingOutlier>,
    pub density: DensityGrid,
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SpacingOutlier {
    pub performer: PerformerId,
    pub nearest: f32,
    /// 近傍 6 人の最近傍距離の中央値。
    pub local_median: f32,
    /// `nearest / local_median`。1.0 から遠いほど逸脱。
    pub ratio: f32,
}

/// 密集ヒートマップ。UI/描画側はこの生データだけを受け取り、色は 20 章が決める。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DensityGrid {
    pub origin: Point,
    pub cell_size: f32,
    pub cols: u16,
    pub rows: u16,
    /// 3×3 の箱ぼかしをかけた占有数。len = cols * rows。
    pub counts: Vec<u16>,
    pub max_count: u16,
}

pub fn scan_formation(
    document: &Document,
    set: SetId,
    params: &ScanParams,
    scratch: &mut ScanScratch,
    out: &mut SpacingReport,
) -> Result<(), DrillError>;
```

- **最近傍距離**: `cell = radii.crowded` の格子に全員を入れ、リング拡大探索。
  リング `R` まで探して見つかった最良距離 `d` が `d <= R * cell` を満たしたら打ち切る（これで正しさが保証される）。
  リングは `MAX_RING` で必ず止まるので無限ループしない。密集ブロックならリング 1 で終わる。
  これが [pathing.rs:97](../../crates/drill-core/src/pathing.rs#L97) の O(n²) を置き換える。
- **分位点**: `nn` を `nn_sorted` へコピーし `select_nth_unstable_by(idx, f32::total_cmp)`。
  完全ソート不要で O(n)、かつ確保なし。
- **局所中央値**: 各演者について最近傍 8 人までを固定長 `[f32; 8]` に集め挿入ソート、上位 6 件の中央値。ヒープ不使用。
  `ratio` が `[1 - tol, 1 + tol]`（既定 tol = 0.25）を外れたら `outliers` へ。
- **ヒートマップ**: `density.cell_size = 2 * radii.crowded` の粗い格子に占有数を数え、3×3 箱ぼかしを 1 回。
  1,000人・100×53 yd・セル 2.2 yd なら 46×25 = 1,150 セル。

#### 明示的な列に対する等間隔性

自動検出は「局所的におかしい人」しか出せない。designer が「この 12 人は 1 本の線」と分かっている場合は、
順序付きの列を渡す方が精度が高い。フォーム情報（14 章）に依存しない形で受け口だけ用意する。

```rust
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EvennessReport {
    pub mean_gap: f32,
    /// |gap_i - mean_gap| の最大値。
    pub max_deviation: f32,
    /// 逸脱が最大だった隣接ペアの後ろ側の演者。
    pub worst: Option<PerformerId>,
    /// 隣接間隔。len = ordered.len() - 1。
    pub gaps: Vec<f32>,
}

/// `ordered` の順に並んでいるはずの演者列について、隣接間隔の均一性を測る。
/// 未知の `PerformerId` は `DrillError::UnknownPerformer` を返す。
pub fn evenness_along(
    document: &Document,
    set: SetId,
    ordered: &[PerformerId],
    out: &mut EvennessReport,
) -> Result<(), DrillError>;
```

O(m)、m = 列の長さ。格子不要。

### 3.11 結果と索引

```rust
/// 1 遷移分の走査結果。`ClinicCache` が所有し、再走査では中身の Vec を
/// clear して詰め直すので、2 回目以降は確保が起きない。
#[derive(Clone, Debug, Default)]
pub struct TransitionScan {
    pub set: SetId,
    /// (span.start, a, b, severity) の昇順。
    pub events: Vec<CollisionEvent>,
    pub excluded: Vec<Excluded>,
    /// このセットの**到着フォーム**に対する間隔解析。
    pub spacing: SpacingReport,
    pub worst: Option<Severity>,
    pub contact_count: u32,
    pub danger_count: u32,
    pub truncated: bool,
    pub suppressed: u32,
    pub index: CountIndex,
    /// 走査時のパラメータ指紋。パラメータが変わったら無効化するのに使う。
    pub params_hash: u64,
}

impl TransitionScan {
    pub fn clear(&mut self);   // 容量は保持する
    pub fn is_empty(&self) -> bool;
}
```

#### カウント索引 — 「今のカウントで衝突している演者」を O(1) で

```rust
/// ローカルカウント → そのカウントで進行中のイベント、の索引。
/// バケットはサブ区間と 1:1（半カウント刻みなら 1 バケット = 0.5 カウント）。
#[derive(Clone, Debug, Default)]
pub struct CountIndex {
    samples_per_count: u8,
    n_sub: u32,
    /// len = n_sub + 1。`slots` への前置和オフセット。
    bucket: Vec<u32>,
    /// イベント索引。1 イベントは自分が跨ぐバケット全てに重複して載る。
    slots: Vec<u32>,
    /// 任意。1 サブ区間あたり `words_per_sample` ワードのビットセット。
    flags: Vec<u64>,
    words_per_sample: u32,
    has_flags: bool,
}

impl CountIndex {
    #[inline]
    pub fn bucket_of(&self, local_count: f32) -> Option<u32>;

    /// そのカウントで進行中のイベントの索引スライス。
    /// 前置和 2 回の読み出しとスライス化だけ。O(1)。
    #[inline]
    pub fn events_at(&self, local_count: f32) -> &[u32];

    /// その演者がそのカウントで何らかのイベントに関与しているか。
    /// ビットセットが有効なら 1 ビットテストで真の O(1)。
    /// 無効なら `events_at` の短いスライスを線形に見る（フォールバック）。
    #[inline]
    pub fn is_flagged(&self, performer_index: u32, local_count: f32) -> bool;
}
```

- 構築コスト: イベント数 × 跨ぐバケット数。上限 `max_events × n_sub` だが、実務では 1 イベント数バケット。
- ビットセットのメモリ: `n_sub × ceil(n/64) × 8` B。
  1,000人・16カウント・半カウント刻み → `32 × 16 × 8 = 4 KB`。
  4,000人・2,048カウント全体 → `4096 × 63 × 8 ≈ 2.0 MB`。
  `params` で `n * n_sub <= 8_000_000` のときだけ有効化する（既定）。それ以上はフォールバック経路。
- 描画は `index.is_flagged(performer_index, count)` を演者ループの中で 1 回叩くだけ。
  1,000人でも 1,000 回のビットテスト（≈ 2 µs）で、赤丸の描き分けが毎フレーム可能。

### 3.12 増分再計算とキャッシュ

```rust
/// セット内容の指紋。`Document` 側に per-set revision が無くても差分検出できるよう、
/// 本モジュールが自前で持つ。座標・カウント・ルートのみを混ぜる（名前や色は無視）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SetFingerprint(u64);

impl SetFingerprint {
    /// 1,000 点で 8 KB の走査 ≒ 1 µs。乗算＋xor の逐次混合、外部依存なし。
    pub fn of(set: &Set) -> Self;
}

#[derive(Debug, Default)]
pub struct ClinicCache {
    /// 最後に見た `Document` のリビジョン（10 章が提供。無ければ 0 固定でも動く）。
    doc_revision: u64,
    params_hash: u64,
    entries: BTreeMap<SetId, CacheEntry>,
    dirty: BTreeSet<SetId>,
    scratch: ScanScratch,
}

#[derive(Debug, Default)]
struct CacheEntry {
    scan: TransitionScan,
    from_fp: SetFingerprint,   // sets[i]
    to_fp: SetFingerprint,     // sets[i+1]
    valid: bool,
}

impl ClinicCache {
    /// `Edit` の適用地点から呼ぶ。セット `set` の変更は遷移 `set` と
    /// **その 1 つ前の遷移**の両方を汚す（前の遷移の到着点が動くため）。
    pub fn invalidate(&mut self, document: &Document, set: SetId);
    pub fn invalidate_all(&mut self);

    /// 参照のみ。描画クロージャからはこれしか呼べない（`&self`）。
    pub fn get(&self, set: SetId) -> Option<&TransitionScan>;
    pub fn is_stale(&self, set: SetId) -> bool;

    /// 1 遷移を再走査する。既に有効ならそのまま返す（走査しない）。
    pub fn refresh(
        &mut self,
        document: &Document,
        set: SetId,
        params: &ScanParams,
    ) -> Result<&TransitionScan, DrillError>;

    /// 汚れているセットを最大 `max_sets` 件だけ、`focus` に近い順に再走査する。
    /// 時間ではなく件数で切るのは決定論のため（同じ入力から同じ順序で同じ結果）。
    /// 呼び出し側は「1 回の走査 ≈ 1 ms」の実測から件数へ換算する。
    pub fn refresh_nearest(
        &mut self,
        document: &Document,
        params: &ScanParams,
        focus: SetId,
        max_sets: usize,
    ) -> RefreshProgress;

    pub fn pending(&self) -> usize;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefreshProgress { pub scanned: usize, pub remaining: usize }
```

**二段の無効化判定**:

1. `doc_revision` / `params_hash` が変わっていなければ何もしない（比較 2 回）。
2. 変わっていたら、`dirty` に入っているセットについてだけ `SetFingerprint` を取り直し、
   `from_fp` / `to_fp` の両方が一致するなら再走査を**しない**。

これで「Undo で元に戻った」「別のセットを編集した」「セット名だけ変えた」ケースが自動的に走査ゼロになる。
`-0.0` と `0.0` はビット列が違うので指紋が変わり、無駄な 1 回の再走査が起きうるが、正しさには影響しない。

**ドラッグ中の戦略（デバウンス）**:

| 局面 | 動作 |
|---|---|
| ポインタ移動中の毎フレーム | 何もしない。前回のキャッシュをそのまま描く。表示に `⟳ 更新中` を添える。 |
| 最後の入力から **120 ms** 経過 | 当該遷移（と 1 つ前）を再走査。 |
| ドラッグが 120 ms 以上途切れずに続く場合 | **250 ms** ごとに 1 回だけ強制的に再走査（レート制限）。 |
| マウスボタン解放 / Undo / Redo / セット挿入・削除 | 即時に再走査。 |
| ファイル読込 / パラメータ変更 / スタイル変更 | 全遷移をジョブで再走査（§3.13）。 |

**120 ms の根拠**:
- 応答時間の古典的な区切りは 0.1 s / 1.0 s / 10 s で、0.1 s 以内なら「操作の直接の結果」と感じられ、
  それを超えると「システムが応答した」という別イベントとして知覚される。
  クリニック結果は**ドラッグ中の連続フィードバックではなく確定後の判定**なので、
  0.1 s の直結感を狙う対象ではなく、むしろ 0.1 s をわずかに超えたところで出るのが自然。
- 一方 200–250 ms を超えると「遅れて出てくる」と明確に感じられ、ドラッグとの因果が切れる。
- 60 fps のドラッグは 120 ms で約 7 フレーム分の中間状態を生む。これを全部走査すると
  7 × 1 ms = 7 ms を無駄にする。1 回に畳めば 1 ms。**捨てる中間状態は高々 7 個**で、
  最後の状態は必ず走査される（解放時の即時再走査があるため取りこぼしゼロ）。
- 走査 1 回が約 1 ms（§5）なので、デバウンス時間（120 ms）は計算コストの 120 倍。
  つまりこの値は「計算が間に合わないから待つ」のではなく「人間の知覚に合わせて出す」ための値であり、
  マシンが速くなっても短くする理由がない。定数 `DRAG_DEBOUNCE_MS: u64 = 120` として
  `drill-app` 側に置く（時間の概念は `drill-core` に持ち込まない）。

**ハイブリッド（部分走査との併用）**: ドラッグ中でも「動かしている演者だけ」の速報が欲しい。

```rust
/// 選択中の m 人だけを世界に対して照合する軽量版。
/// 空間ハッシュは前回のフル走査のものを再構築せず、m 人だけを再挿入して近傍を引く。
/// O(m · 近傍数)。m = 20 なら 1 µs 未満で、毎フレーム呼んでも問題ない。
/// 到着フォームの静止判定のみを行い、掃引はしない（掃引は debounce 後のフル走査に任せる）。
pub fn probe_static(
    document: &Document,
    set: SetId,
    performers: &[PerformerId],
    params: &ScanParams,
    scratch: &mut ScanScratch,
    out: &mut Vec<CollisionEvent>,
) -> Result<(), DrillError>;
```

これでドラッグ中は「今つかんでいる人が誰かに重なった」だけが即座に赤くなり、
本格的な掃引結果は 120 ms 後に差し替わる。速報と本判定の粒度差は UI で明示する（`⟳` の有無）。

### 3.13 アプリ側の呼び出し規約

**原則: `egui` の描画クロージャから走査関数を呼ばない。** 描画側が触ってよいのは
`ClinicCache::get`（`&self`）と `CountIndex::is_flagged` だけ。
`scan_transition` が `&mut ScanScratch` を要求することで、`&self` しか持たないクロージャからは
構造的に呼べないようにしてある（現在の [main.rs:1011](../../crates/drill-app/src/main.rs#L1011) は
`&self.document` で呼べてしまうのが問題の根）。

再走査は `eframe::App::update` の**先頭**（描画に入る前）で 1 回だけ行う。

| 規模 | 対象 | 方式 | 見積り |
|---|---|---|---|
| n ≤ 1,500 / 1 遷移 | ドラッグ後・Undo 後の再走査 | **同期**。`update` 先頭で 1 回。 | ≈ 1 ms。16.6 ms 予算のうち 2 ms を上限として確保。走査が走るのは 120 ms に 1 フレームなので、償却では 0.15 ms/frame。 |
| n ≤ 1,500 / 全 64 遷移 | 読込・パラメータ変更・スタイル変更 | **ジョブ**（40 章 `Job<ClinicReport>`）。 | 64 × 1 ms ≈ 64 ms。1 フレーム予算を超えるので UI スレッド不可。 |
| n ≤ 1,500 / 全 64 遷移（ジョブ基盤が入る前） | 同上 | `refresh_nearest(focus, 2)` を毎フレーム 1 回。現在セットから外側へ広げる。 | 2 ms/frame × 32 フレーム ≈ 0.5 秒で全完了。予算内に収まり、可視セットが最初に埋まる。 |
| n > 1,500 / 1 遷移 | 任意 | **常にジョブ**。 | 4,000人で 1 遷移 ≈ 11 ms。同期では 16.6 ms 予算の 2/3 を食う。 |
| n = 4,000 / 全 256 遷移 | 読込 | **ジョブ + 進捗 + キャンセル**。可視セット優先。 | 256 × 11 ms ≈ 2.8 秒。進捗バー必須。 |

しきい値は `SYNC_SCAN_LIMIT_PERFORMERS = 1_500`。基準規模 1,000 に 5 割の余裕を持たせた値。

```rust
// drill-app 側の骨格（drill-core には入れない）
impl DrillApp {
    fn update_clinic(&mut self, now: Instant) {
        if self.clinic.pending() == 0 { return; }
        let interactive = self.drag_active || now.duration_since(self.last_input).as_millis() < DRAG_DEBOUNCE_MS as u128;
        if interactive && now.duration_since(self.last_clinic_run).as_millis() < DRAG_MAX_INTERVAL_MS as u128 {
            return;                                   // デバウンス中。前回結果を描く。
        }
        if self.document.performers.len() > SYNC_SCAN_LIMIT_PERFORMERS || self.clinic.pending() > 2 {
            self.spawn_clinic_job();                  // 40 章
        } else {
            let _ = self.clinic.refresh_nearest(&self.document, &self.params, self.current_set, 2);
            self.last_clinic_run = now;
        }
    }
}
```

デバッグビルドでは `ScanScratch` に呼び出し回数カウンタを持たせ、
1 フレーム 3 回を超えたら `debug_assert!` で落とす（規約違反の再発を検出する）。

---

## 4. 不変条件

テストで検証できる形で列挙する。括弧内は §7 の対応するテスト名。

1. **決定論** — 同じ `(Document, ScanParams, SetId)` からは、常にバイト同一の `events` / `excluded` /
   `SpacingReport` が出る。実行順・スレッド・キャッシュの状態に依存しない。（`scan_is_deterministic`）
2. **格子は総当たりと同じ答えを出す** — `report_at_least = Crowded` かつイベント上限を無限にしたとき、
   空間ハッシュ版の結果集合は O(n²) 総当たり版と完全に一致する。（`grid_matches_brute_force`）
3. **確保ゼロ** — 同じ寸法で 2 回目以降の `scan_transition` は、`ScanScratch` のいかなる `Vec` の
   `as_ptr()` も `capacity()` も変えない。（`scan_is_allocation_free_after_warmup`）
4. **正準順序** — 全イベントで `a < b`。`events` は `(span.start, a, b, severity)` の狭義昇順で、
   同一キーの重複が無い。（`events_are_canonical`）
5. **1 ペア 1 区間** — 連続して接近しているペアは 1 イベントに畳まれる。
   `merge_gap_counts` 以下の途切れも畳まれる。（`continuous_pair_merges_into_one_event`）
6. **区間の整合** — 全イベントで `span.start <= count <= span.end` かつ
   `0.0 <= span.start` かつ `span.end <= counts as f32`。（`spans_are_within_the_set`）
7. **最悪値の一貫性** — `distance` は区間中の最小値であり、`severity == radii.severity_of(distance).unwrap()`。
   （`severity_matches_distance`）
8. **掃引が静止判定を包含する** — 到着点で接触しているペアは必ずイベントに現れる。
   すなわち新実装は現行 `analyze_transition` の検出結果を取りこぼさない。（`swept_covers_static`）
9. **全域性** — 有限・非有限・空・1 人・重複座標・`counts == 0`・巨大 `counts` のいずれでも
   パニックせず、有限時間で `Ok` を返す（除外は `excluded` に載る）。（`hostile_inputs_do_not_panic`）
10. **索引の健全性** — 任意の `local_count` と任意のイベント `e` について、
    `e.span` が `local_count` を含む ⟺ `index.events_at(local_count)` に `e` の索引が含まれる。
    また `index.is_flagged(p, c)` はその等価判定と一致する。（`count_index_agrees_with_events`）
11. **キャッシュの健全性** — `ClinicCache::get` が返す結果は、その時点の `Document` を
    直接 `scan_transition` した結果と一致する（`is_stale` が false のとき）。（`cache_matches_fresh_scan`）
12. **有界性** — `events.len() <= max_events`、`open.len() <= max_open_pairs`、
    セル数 `<= MAX_GRID_CELLS`、サブ区間数 `<= MAX_SUBINTERVALS`。入力によらず成立する。（`limits_hold_under_adversarial_input`）

---

## 5. 性能

### 5.1 基準規模の比較回数

条件: 1,000人 / 16 カウント / 半カウント刻み（`samples_per_count = 2`）。

- サブ区間数 `n_sub = 16 × 2 = 32`。サンプル座標評価は 33 回 × 1,000 = **33,000 点**。
- 密度: 最悪ケースとして 1,000人が 40 × 25 yd のブロックに収まっている場合を採る
  （フィールド全面 100 × 53.3 yd に散らばる場合はこれより 5 倍疎になるので、こちらが上界）。
  ρ = 1.0 人/yd²。
- セル幅 `cell = crowded(1.10) + L_max(0.63) + slack(0.02) ≈ 1.75 yd`。
  1 セルあたりの平均人数 `λ = ρ · cell² = 3.06`。
  非空セル数 ≈ 1000 / 3.06 ≈ **327**。
- 1 非空セルあたりのペア数 = 自セル内 `C(λ,2) = 3.15` + 前方 4 近傍 `4 · λ² = 37.5` ≈ **40.6**。
- 1 サブ区間のペア判定 = 327 × 40.6 ≈ **13,300**。
- 全体 = 32 × 13,300 ≈ **425,000 回の `closest_approach`**。

現行実装は 1 フレームあたり 499,500 回の距離比較を**移動を一切見ずに**行っている。
新実装は同じオーダーの比較で**32 サブ区間分の掃引全体**を賄い、しかもそれが 120 ms に 1 回しか走らない。

### 5.2 時間見積り

| 工程 | 回数 | 1 回あたり | 小計 |
|---|---|---|---|
| サンプル座標評価（lerp + ゲート判定） | 33,000 | 8 ns | 0.26 ms |
| 中点算出・`segment_cap` 判定 | 32,000 | 2 ns | 0.06 ms |
| 格子への挿入（chain push） | 32,000 | 3 ns | 0.10 ms |
| 非空セルの列挙とスタンプ判定 | 32 × 327 × 5 | 1 ns | 0.05 ms |
| `closest_approach`（15 flops、sqrt なし） | 425,000 | 1.2 ns | 0.51 ms |
| 閾値通過ペアの sqrt + オープン区間更新 | ≈ 2,000 | 20 ns | 0.04 ms |
| `finalize`（ソート + 索引構築） | 1 | — | 0.05 ms |
| **合計** | | | **≈ 1.07 ms** |

**2 ms 未満の根拠**: 上表は密集ブロック（ρ = 1.0 人/yd²）という上界条件での積算で 1.07 ms。
実際のドリルはフィールド全面に散るので ρ はこの 1/3〜1/5 になり、支配項の `closest_approach` は
ρ に比例して 0.1〜0.2 ms へ落ちる。**上界条件でも 1.1 ms、実測想定 0.5 ms 前後**で、
2 ms に対して最低でも 1.8 倍の余裕がある。1 回あたりの単価（1.2 ns / 8 ns）は
既存ベンチの実測（[PRODUCT_QUALITY.md](../../PRODUCT_QUALITY.md) の
「1,000人 × 60,000 フレーム補間: 9.24 ms」＝ 1 点あたり 0.154 ns、
ただしこれは連続メモリの単純 lerp で下限側）と、チェイン走査のキャッシュミス（L2 ≈ 4 ns）を
踏まえた保守的な値を採っている。実測は §7 のベンチで確定させる。

### 5.3 16.6 ms 予算の取り分

- **走査が走るフレーム**: 2.0 ms（12%）。走査は 120 ms に 1 回なので 7.2 フレームに 1 回。
- **償却**: 2.0 / 7.2 ≈ **0.28 ms/frame（1.7%）**。
- **描画時の索引参照**: 1,000 演者 × `is_flagged` 1 ビットテスト ≈ **2 µs（0.01%）**。恒常コスト。
- **ドラッグ中の速報 `probe_static`**: 選択 20 人 → **< 1 µs**。毎フレームでも無視できる。

現行の毎フレーム 499,500 回（推定 1.5〜3 ms/frame、恒常）と 16 KB/frame の確保が丸ごと消える。

### 5.4 メモリ

| 領域 | 1,000人 | 4,000人 |
|---|---|---|
| `prev` / `curr` / `mid`（`Point` = 8 B） | 24 KB | 96 KB |
| `heads` + `stamp`（セル幅 1.75 yd、100×53 yd → 58×31 = 1,798 セル） | 14 KB | 14 KB |
| `next` + `point` | 12 KB | 48 KB |
| `open`（40 B × 4,096） | 164 KB | 164 KB |
| `events`（32 B × 4,096） | 131 KB | 131 KB |
| `CountIndex.flags` | 4 KB | 16 KB |
| `SpacingReport.nearest` + `density` | 6 KB | 19 KB |
| **`ScanScratch` 合計** | **≈ 355 KB** | **≈ 488 KB** |
| `ClinicCache`（64 セット分の `TransitionScan`、イベント少数時） | ≈ 1.5 MB | ≈ 6 MB |

`ScanScratch` は全遷移で 1 個を共有するので、セット数に比例しない。
`ClinicCache` は結果のみを持ち、`Document` のコピーを持たない。

### 5.5 上限規模（4,000人 / 256 セット）での劣化

同じ 40×25 yd に 4,000 人は物理的に入らないので、密度はフィールド全面で ρ = 0.75 人/yd² と置く。
`λ = 2.3`、非空セル ≈ 1,740、ペア/セル ≈ 22.4 → 1 サブ区間 39,000、32 区間で **1,250,000 回**。
座標評価は 132,000 点。合計 **≈ 3.0 ms / 1 遷移**。
仮に不可能なほど詰め込んだ最悪密度（ρ = 3.0）でも `closest_approach` が 4 倍で **≈ 11 ms / 1 遷移**。

いずれも `SYNC_SCAN_LIMIT_PERFORMERS = 1_500` を超えるので**必ずジョブ実行**になり、
UI スレッドは停止しない。全 256 遷移でも 0.8〜2.8 秒、進捗とキャンセル付き。
メモリは §5.4 の通り 0.5 MB + 結果分で、確保ゼロの性質も保たれる。**劣化はするが壊れない。**

---

## 6. 失敗モードと安全性

信頼できない入力（他人から受け取った `.drillproj`）を前提に列挙する。

| # | 壊れ方 | 起点 | 対処 |
|---|---|---|---|
| 1 | 座標が `NaN` / `Inf` | JSON。`validate` は座標の有限性を見ていない（[lib.rs:302](../../crates/drill-core/src/lib.rs#L302)） | サンプルごとに `is_finite` を検査。非有限の演者は `alive[i] = false` にし `Excluded::NonFinite` として 1 回だけ報告。格子索引計算にも `Bounds::of_finite` にも渡さない。 |
| 2 | セル索引が `NaN` 経由で不定 | 同上 | `insert` は有限性を事前条件とし（`alive` で保証）、索引は `clamp` 後に `as u32`。Rust の float→int キャストは飽和変換なので UB にならないが、`clamp` を先に置いて意図を明示する。 |
| 3 | 座標が `1e30` などフィールド外の巨大値 | 同上 | `Bounds::of_finite` が AABB を取り、`reshape` がセル数上限に当たるまでセル幅を倍にする。結果として格子が粗くなり性能が落ちるだけで、正しさもメモリも壊れない。 |
| 4 | 1 サブ区間で数百 yd 移動（テレポート） | 悪意ある座標、または v1 からの誤変換 | `segment_cap` 超過は掃引から除外し `Excluded::Teleport`。**これが無いと `L_max` が爆発してセル幅が場全体になり、O(n²) へ退化する。**性能の栓であると同時に正しさの表明でもある（そんな移動は歩幅解析側が「不可能」として既に落としている）。 |
| 5 | 全員が同一座標（イベント爆発） | 悪意ある座標 | `max_events` で打ち切り `truncated = true` / `suppressed`。`C(4000,2) = 800 万` でもメモリは 131 KB 固定。走査時間は打ち切り後の候補列挙分だけ残るが、`for_each_candidate_pair` の早期打ち切り（`f` が `false`）で 1 サブ区間の途中で抜ける。 |
| 6 | `counts` が `u16::MAX` | JSON | `n_sub = counts * s` を `u32` で計算し `MAX_SUBINTERVALS` で飽和。`truncated` を立てる。オーバーフローしない（`65535 * 8 = 524,280 < u32::MAX`）。 |
| 7 | `samples_per_count == 0` | 設定 | `sanitized()` で 1 へ丸め、`ParamWarnings` に載せる。ゼロ除算なし。 |
| 8 | 半径が 0 / 負 / 非有限 / 逆順 | 設定・破損ファイル | `ClinicRadii::sanitized()` が正値・単調性・有限性を強制し、不正なら既定へ。`cell_size` が 0 にならないので `inv_cell` が `Inf` にならない。 |
| 9 | セット間で演者数が不一致 | v1 → v2 移行のバグ、手編集 | `min(from.len(), to.len())` までを走査し、余った演者を `Excluded::Unpaired` で報告。添字パニックを起こさない（`validate` が防ぐはずだが、防壁を二重にする）。 |
| 10 | オープンアドレス法の探査が終わらない | ハッシュの偏り | 探査回数をテーブル容量で上限化。上限到達時は新規オープンを諦め `suppressed += 1`。**`while` で回さず `for _ in 0..cap` で回す。** |
| 11 | リング拡大探索が終わらない | 全員が孤立 | `MAX_RING` で必ず停止。見つからなければ `f32::INFINITY`。 |
| 12 | 世代スタンプの巻き戻り | 2^32 回の `begin()` | `generation` が 0 に戻る回で `stamp` を 0 埋め。1 回のコストは O(cells) = 1,798 だが、頻度は 2^32 回に 1 回。 |
| 13 | 走査中にドキュメントが変わる（ジョブ実行時） | 並行編集 | ジョブは `Arc<Document>` のスナップショットを取る。完了時に `doc_revision` を照合し、古ければ結果を捨てて再投入。**部分的に新しい結果を混ぜない。** |
| 14 | ソートが NaN で不定順序になる | 除外漏れの距離 | 比較は `f32::total_cmp`。NaN も全順序に入るので `sort_unstable_by` の要求（strict weak ordering）を満たし、パニックしない。 |
| 15 | イベント数 × バケット数で索引が肥大 | 長いセット × 多イベント | `slots` の長さを `max_events * 64` で上限化。超過分は索引に載せず、`events_at` のフォールバック（線形探索）に落ちる旨を `CountIndex::has_flags = false` で示す。 |

**パニック禁止**: 本モジュールに `unwrap` / `expect` / `panic!` / 添字の直接アクセス（境界が構造的に保証される
内部ループを除く）を書かない。内部ループの添字は「`i < n` かつ全 Vec の長さが `n`」を
`ensure_capacity` が保証しており、その事実を `debug_assert_eq!` で表明する。
算術は全て `f32` か `u32`（`checked_mul` / `saturating_*` を使う箇所は §6 の #6）。

**プライバシー・外部プロセス**: 本モジュールは I/O を一切行わない。純粋関数のみ。

---

## 7. テスト計画

### 単体

| 名前 | 内容 |
|---|---|
| `swept_catches_fast_crossing` | 2 人が直交して交差し、**両端点では 10 yd 離れている**が中間で 0.1 yd まで接近する構成。現行 `analyze_transition` は 0 件、本実装は 1 件（`Contact`）を返す。この 1 本が本設計の存在理由。 |
| `swept_covers_static` | 到着点で重なる 2 人 → 必ず検出。旧実装の検出集合を包含することの確認。 |
| `crossing_paths_at_different_times_is_not_a_collision` | 経路は交差するが、片方が先に通過し終える構成 → 0 件。静的な線分交差判定との差を固定する。 |
| `parallel_motion_keeps_constant_distance` | 相対速度 0（`uu == 0` 分岐）で、`t = 0` の距離が正しく採られる。 |
| `zero_counts_falls_back_to_static_sample` | `counts.moves == 0` で退化せずに 1 サンプル分を評価する。 |
| `clamped_cells_do_not_lose_pairs` | フィールド外（負座標・幅超過）に置いた近接ペアが検出される。§3.5 のクランプ単調性の検証。 |
| `severity_matches_distance` | 3 つの半径のちょうど境界値での分類。 |
| `continuous_pair_merges_into_one_event` | 8 カウント接近し続けるペア → イベント 1 件、`span == 4.0..12.0`、`count` が最接近点。 |
| `merge_gap_bridges_chatter` | 閾値を跨いで 3 回出入りするが、途切れが `merge_gap_counts` 以下 → 1 件。 |
| `merge_gap_splits_distant_reencounter` | 途切れが `merge_gap_counts` より長い → 2 件。 |
| `spans_are_within_the_set` | 不変条件 6。 |
| `events_are_canonical` | 不変条件 4。`a < b` と昇順と重複なし。 |
| `count_index_agrees_with_events` | 不変条件 10。全バケット × 全イベントの総当たり照合。 |
| `nearest_neighbor_matches_brute_force` | リング探索版と O(n²) 版の一致。`pathing::nearest_neighbor_interval` の置換の正当性。 |
| `spacing_outliers_flag_the_off_grid_performer` | 等間隔の 20 人の線に 1 人だけずれた演者を混ぜて検出。 |
| `evenness_along_reports_the_widest_gap` | 順序付き列の最大逸脱と `worst` の同定。 |
| `excluded_performers_are_reported_not_dropped` | NaN 1 人・テレポート 1 人・不対 1 人 → `excluded.len() == 3` で理由も一致。 |

### property（`std` のみの決定論的 LCG で乱数生成。外部依存を足さない）

| 名前 | 内容 |
|---|---|
| `grid_matches_brute_force` | 200 人 × 100 通りの乱配置 × 4 カウント。空間ハッシュ版と O(n²) 掃引版の**イベント集合が完全一致**。半径・サンプル数・密度もランダムに振る。 |
| `scan_is_deterministic` | 同一入力を 10 回走査 → 全て同一バイト列（`serde_json` 経由で比較）。 |
| `hostile_inputs_do_not_panic` | `f32::from_bits(rng.next())` で座標を生成（NaN / Inf / 非正規化数が高頻度で混ざる）。10,000 ケース、パニックなし・停止すること。 |
| `limits_hold_under_adversarial_input` | 不変条件 12 を乱数入力で。 |
| `cache_matches_fresh_scan` | ランダムな `Edit` 列を適用しながら、キャッシュ経由の結果と毎回作り直した結果を照合。1,000 手。 |

### ゴールデン

- `tests/golden/clinic_demo16.expected` — `Document::demo(100, 10)` を決まった手順で崩したフィクスチャの
  イベント列をテキスト化して比較。追加依存なしの単純文字列比較（DESIGN_GAPS C-2 の方針に従う）。

### ストレス

| 名前 | 内容 |
|---|---|
| `four_thousand_performers_completes` | 4,000人 / 16 カウント / 256 セット相当。完了すること、`truncated` の扱いが正しいこと。 |
| `all_performers_at_one_point` | 4,000人が同一座標 → `max_events` で打ち切り、100 ms 以内に完了、メモリ増加なし。 |
| `ten_thousand_invalidations` | 1,000 手の `Edit` × 10 → キャッシュのメモリが単調増加しない。 |

### ベンチ（`crates/drill-core/benches/core_performance.rs` に追加）

```
1,000 performers × 16 counts × 2 samples/count, single swept scan:  目標 < 2.0 ms
  ↑ 100 回繰り返しの平均。2 回目以降の ScanScratch ポインタ不変を assert。
4,000 performers × 16 counts × 2 samples/count, single swept scan:  参考値（劣化の記録）
1,000 performers formation spacing scan:                             目標 < 0.3 ms
```

`assert!` でゲート化し、`PRODUCT_QUALITY.md` のベースライン表に追記する。

### 確保ゼロ検証

```rust
#[test]
fn scan_is_allocation_free_after_warmup() {
    let doc = Document::demo(100, 10);
    let params = ScanParams::default();
    let mut scratch = ScanScratch::default();
    let mut out = TransitionScan::default();
    scan_transition(&doc, set_id(0), &params, &mut scratch, &mut out).unwrap();
    let before = scratch.fingerprint();          // (as_ptr, capacity) の列
    let out_before = (out.events.as_ptr(), out.events.capacity());
    for _ in 0..64 {
        scan_transition(&doc, set_id(0), &params, &mut scratch, &mut out).unwrap();
    }
    assert_eq!(before, scratch.fingerprint());
    assert_eq!(out_before, (out.events.as_ptr(), out.events.capacity()));
}
```

既存の `interpolation_reuses_output_allocation` と同じポインタ比較方式で、追加依存を要さない。

---

## 8. 実装タスク

1 タスク = 1〜3 時間。`→` は依存。

### 直列の基礎（並行不可）

| ID | 内容 | 依存 | 見積 |
|---|---|---|---|
| **T1** | `clinic.rs` 新設。`Severity` / `ClinicRadii` / `ScanParams` / `Excluded` / `CountSpan` / `CollisionEvent` の型定義と `sanitized()` 群、単体テスト（境界値・丸め）。 | — | 2h |
| **T2** | `SpatialHash`。`reshape` / `begin` / `insert` / `for_each_candidate_pair` / `for_each_in_ring`。世代スタンプ、非空セルリスト、セル数上限。単体テスト（クランプ単調性、ペアの重複なし・漏れなし）。 | T1 | 3h |
| **T3** | `ScanScratch` / `ScratchDims` / `ensure_capacity` / `reserve` / `fingerprint`。確保が 1 か所に閉じていることのテスト。 | T2 | 2h |

### 並行可能（T3 の後、3 系統に分かれる）

| ID | 内容 | 依存 | 見積 | 並行 |
|---|---|---|---|---|
| **T4** | `closest_approach` と掃引ループ本体。`prev`/`curr` スワップ、`segment_cap`、非有限除外。`swept_catches_fast_crossing` ほか単体テスト。 | T3 | 3h | A |
| **T5** | `OpenPair` とオープンアドレス法索引、区間マージ、`merge_gap_counts`、`finalize` の正準ソート。マージ関連テスト 4 本。 | T4 | 3h | A |
| **T6** | `CountIndex` の構築と参照（前置和・ビットセット・フォールバック）。`count_index_agrees_with_events`。 | T5 | 2h | A |
| **T7** | `scan_formation`：リング探索の最近傍、分位点（`select_nth_unstable_by`）、局所中央値と `SpacingOutlier`。 | T3 | 3h | B |
| **T8** | `DensityGrid`（占有カウント + 3×3 箱ぼかし）と `evenness_along`。 | T7 | 2h | B |
| **T9** | `pathing::nearest_neighbor_interval` を `scan_formation` 経由の実装へ差し替え、既存テスト（[pathing.rs:243-252](../../crates/drill-core/src/pathing.rs#L243)）を通したまま O(n²) を削除。 | T7 | 1h | B |
| **T10** | `SetFingerprint` と `ClinicCache`（`invalidate` / `get` / `refresh` / `refresh_nearest`）。`cache_matches_fresh_scan`。 | T6 | 3h | C |
| **T11** | `probe_static`（ドラッグ中の速報）。 | T6 | 2h | C |

### 統合（並行分の合流後）

| ID | 内容 | 依存 | 見積 |
|---|---|---|---|
| **T12** | property テスト一式（`grid_matches_brute_force` / `hostile_inputs_do_not_panic` / 決定論 / 上限）。決定論 LCG のテストヘルパ。 | T5, T7 | 3h |
| **T13** | ベンチ 3 本を `core_performance.rs` に追加、2 ms ゲートを `assert!` 化、`PRODUCT_QUALITY.md` のベースライン表を更新。 | T12 | 2h |
| **T14** | `drill-app` 結線 その1: `DrillApp` に `ClinicCache` を持たせ、[main.rs:1011](../../crates/drill-app/src/main.rs#L1011) と [main.rs:1033](../../crates/drill-app/src/main.rs#L1033) の毎フレーム呼び出しを削除。`update` 先頭の `update_clinic` へ移す。デバウンス定数とレート制限。 | T10, T11 | 3h |
| **T15** | `drill-app` 結線 その2: LIVE CLINIC パネルを severity 別件数 + 最悪ペアの表示へ書き換え、`Severity::glyph()` で色以外の表現を入れる（DESIGN_GAPS C-3）。フィールド描画で `is_flagged` によるマーカー描画。 | T14 | 3h |
| **T16** | `Edit::apply` 地点からの `ClinicCache::invalidate` 呼び出し（10 章の `Edit` 導入後）。ゴールデンテスト追加。 | T14, 10章 | 2h |

**クリティカルパス**: T1 → T2 → T3 → T4 → T5 → T6 → T10 → T14 → T15 = 24h。
B 系（T7〜T9, 6h）と T11 は並行に消化できる。
T14 だけを先出しして「毎フレーム呼び出しの停止」（DESIGN_GAPS §0 #3・#4）を早期に潰すこともできる
— その場合、暫定的に既存 `analyze_transition` を `ClinicCache` 越しに呼ぶだけの薄いバージョンで
T1〜T3 完了時点から着手可能。**性能ゲート違反の解消だけなら 5 時間で到達する。**

---

## 9. 未決事項

| # | 内容 | 決めるために必要なもの |
|---|---|---|
| 1 | セル幅を `crowded` に取るか `danger` に取るか。`crowded` だと 1 セルの人数が約 2 倍になりペア判定が増えるが、`Crowded` イベントと間隔解析を同じ格子で賄える。`danger` だと格子が細かくなり `Crowded` 用に 2 周目が要る。 | T13 のベンチで両方を実測。差が 0.2 ms 未満なら単一格子（`crowded`）を採る。 |
| 2 | 3D の身長・プロップの高さを判定に入れるか（例: 低いプロップの上を通過するのは衝突ではない）。 | 15 章の `Performer::height_m` と 22 章のプロップ表現。少なくとも `Performer` に「衝突判定に参加するか」のフラグは要る。 |
| 3 | 進行方向を考慮した非等方な判定領域（前後より左右が狭い楕円）。トロンボーンのスライドや前向き/後ろ向きで実際の危険域は違う。 | 12 章の `StepStyle` に「体の向き」が入るか。入るなら `closest_approach` を楕円距離に一般化する（相対座標を回転行列で正規化してから同じ式を使えるので、コストは +6 flops）。 |
| 4 | 等間隔性の逸脱検出を「フォーム」単位でやるか、局所中央値だけで済ますか。 | 14 章がフォーム（線・弧・所属演者リスト）を `Document` に持たせるかどうか。持つなら `evenness_along` を自動で回せる。 |
| 5 | デバウンス 120 ms / レート制限 250 ms の妥当性。 | 実機での被験テスト。少なくとも設定で変えられるようにし、初期値の根拠は §3.12 に残す。 |
| 6 | イベントに「原因となったルート区間」を載せて、UI からワンクリックでゲートをずらせるようにするか。 | 11 章の `Route` / `Gate` の最終形。載せるなら `CollisionEvent` に `RouteHint` を追加する（16 B 増）。 |
| 7 | 全遷移スキャンをジョブ化する際の粒度（1 ジョブ = 1 遷移 か 全遷移か）。前者はキャンセル応答が良く、後者はスケジューリング負荷が低い。 | 40 章の `Job<T>` の生成コスト。 |
| 8 | スナップ有効時に等距離が大量に発生する。`tightest` のタイブレーク規則を「小さい `PerformerId` 優先」で固定してよいか（決定論のためには何かに固定する必要がある）。 | 実務上の見え方。暫定は ID 昇順で固定し、不変条件 1 のテストで縛る。 |
| 9 | `ClinicCache` を `drill-core` に置くか `drill-app` に置くか。走査は純粋ロジックだが、キャッシュは状態でありアプリの関心事でもある。 | 本書の暫定判断は `drill-core`（同じキャッシュを動画書き出しのプリフライト検査でも使いたいため）。31 章のレビューで確定させる。 |
