# 11. 遷移モデル（ルート・ゲート・ホールド・追従）

## 1. 目的と範囲

セットとセットの間で演者が**どう動くか**を表現するモデルを定義する。現状の DrillForge は
セット間の単純線形補間しか持たず、これが Pyware 3D との機能差が最も大きい領域である。

この文書が解決すること:

- **経路の形**（直線・曲線・折れ線・円弧）と、その**弧長パラメータ化**（等速移動の保証）
- **ゲート**（セット内の何カウント目に出発し、何カウント目に到着するか）
- **ホールド**（到達後の静止カウント）と `SetCounts { moves, hold }` への移行
- **イージング**（step-off の加速と halt の減速）
- **スタガー**（演者ごとに時間をずらす一括ルール）
- **向き（facing）**（進行方向自動 / 固定 / キーフレーム、バックワード・スライド）
- **フォロー・ザ・リーダー**（形の共有と軌跡の追従、循環参照の検出）
- 1,000人 × 60fps で成立する評価 API `positions_at_count` と、その裏にある `TransitionPlan`
- **カウント・秒の数値精度**: `TempoMap` への `f64` 写像の追加（文書30 §3.12-3 からの依頼）と、
  「どの経路が `f32` のままでよく、どの経路が `f64` でなければならないか」の切り分け（§3.12）

この文書が扱わないこと:

| 事項 | 担当文書 |
|---|---|
| `SetId` / `Edit` 全 40 変異 / `Revisions`・`Scopes`・`SetScope`・`CacheKey` / `History` | 10（ドキュメントモデルと編集コマンド代数） |
| `StepStyle` / 歩幅評価 / 難易度スコア | 12 |
| 掃引衝突検査（`ScanScratch`）。本書は「サンプル点をどう作るか」だけを提供する | 13 |
| セット内フォーメーションの生成（`shapes.rs`） | 14 |
| ルート編集ハンドルの UI・入力 | 43 |
| `Trail` の描画コマンド | 20 |
| v1→v2 マイグレーション機構そのもの | 41（本書は写像の内容のみ指定） |
| 経路長を使った歩幅表示・コンティニュイティ文言 | 12 / 17（本書は `route_length()` を供給するだけ） |
| `AnchorMap` / サンプル索引 / 出力レイテンシ | 30（本書は `TempoMap` の f64 写像だけを供給する） |
| 有理数フレーム時刻とエンコード | 31 |

## 2. 現状

### 2.1 存在するもの

| 箇所 | 内容 |
|---|---|
| [lib.rs:384-394](../../crates/drill-core/src/lib.rs) `Document::positions_at` | `progress: f32` を 0..1 にクランプし、`from` と `to` を `Point::lerp` するだけ。形状・ゲート・イージングは無い。`out` を再利用する確保回避の作法だけは既にある。 |
| [lib.rs:136-143](../../crates/drill-core/src/lib.rs) `Point::lerp` | 唯一の補間プリミティブ。 |
| [lib.rs:232-238](../../crates/drill-core/src/lib.rs) `Set { name, counts: u16, positions }` | カウントはスカラー1個。ホールドもルートも持たない。 |
| [lib.rs:349-355](../../crates/drill-core/src/lib.rs) `timeline_counts` | **最後のセットを除いて** `counts` を合計する。最後のセットの尺はタイムラインに存在しない。 |
| [lib.rs:357-365](../../crates/drill-core/src/lib.rs) `global_count` / [lib.rs:367-382](../../crates/drill-core/src/lib.rs) `locate_count` | 同じく最後のセットを除外して走査する。`locate_count` は線形走査。 |
| [shapes.rs:110-138](../../crates/drill-core/src/shapes.rs) `bezier` | De Casteljau。ただし **パラメータ `t` について等間隔**であり弧長等間隔ではない。`Vec` を確保する。フォーメーション生成用であって経路用ではない。 |
| [shapes.rs:146-188](../../crates/drill-core/src/shapes.rs) `polyline` | 累積長の前置和を作り弧長等間隔にサンプルする。**本書の弧長パラメータ化と同じ技法の先例**。ただし `Vec` を確保し、フォーメーション生成用。 |
| [pathing.rs:8-12](../../crates/drill-core/src/pathing.rs) `path_length` / [pathing.rs:29-51](../../crates/drill-core/src/pathing.rs) `transition_moves` | 距離は**直線距離のみ**。`step_size = distance / from.counts`。曲線経路では歩幅を過小評価する。 |
| [continuity.rs:104-119](../../crates/drill-core/src/continuity.rs) | `from.counts` と直線距離から歩数を出す。ゲートもホールドも見ていない。 |
| [countsheet.rs:70,75](../../crates/drill-core/src/countsheet.rs) | `counts: set.counts` をそのまま出力し、`start_count += u32::from(set.counts)` で進む。 |
| [main.rs:510-515](../../crates/drill-app/src/main.rs) | `count_position / set_counts` で progress に戻してから `positions_at` を呼ぶ。カウント → progress → カウントの往復が発生している。 |
| [camera.rs:26-28](../../crates/drill-core/src/camera.rs) `field_to_world` | 位置しか受け取らない。3D は演者の向きを知らない。 |
| [tempo.rs:115-170](../../crates/drill-core/src/tempo.rs) `seconds_at` / `count_at` | **秒もカウントも `f32`**。区分定数 BPM を区間ごとに `seconds += span * 60.0 / bpm` で**逐次加算**する O(区間数) の走査。前置和のキャッシュは無い。§3.12 で置き換える。 |
| [benches/core_performance.rs:10](../../crates/drill-core/benches/core_performance.rs) | 現行ベースライン: 1,000人 × 60,000フレーム補間 = 9.24ms（= 154ns/フレーム）。 |

### 2.2 存在しないもの

`Route` / `RouteShape` / `RouteTable` / `Gate` / `Easing` / `SetCounts` / `Facing` / `Stagger` は
`crates/` 全体を検索して**1件もヒットしない**。演者の向きという概念自体が存在せず、
`Performer`（[lib.rs:225-230](../../crates/drill-core/src/lib.rs)）は `id` / `label` / `color` のみ。

### 2.3 前提とする他設計の成果物（文書10 最終版に整合）

本書は文書10（ドキュメントモデルと編集コマンド代数）の最終版を前提とする。整合を取った点:

| 文書10 の決定 | 本書での扱い |
|---|---|
| `PerformerId` は **`NonZeroU32`**（10 §「`PerformerId` の +1 について」）。v1 の 0 始まり ID は移行時に一律 +1 | `BTreeMap<PerformerId, Route>` はニッチ最適化の恩恵を受ける（10 §3.1 が `RouteTable` を名指ししている）。本書の `FollowShape.leader` / `FollowTrail.leader` も `NonZeroU32` |
| `Set` の `positions` は**非公開**。`Set::position(i)` / `Set::positions()` / `positions_as_f32()` 経由でのみ読む | 本書のコード片は全て accessor 経由に統一した。`Set` の定義そのものは文書10 が所有し、本書は追加フィールド `counts: SetCounts` と `routes: RouteTable` の**型だけ**を供給する |
| `Edit::SetCounts(CountsChange)`、`CountsChange { set: SetId, counts: SetCounts }`（Copy） | §6.4 |
| `Edit::SetRoutes(Box<SetRoutesChange>)`、`SetRoutesChange { set: SetId, routes: RouteTable }`（**テーブル丸ごと差し替え**） | §6.4。文書10 の未決 U2 への回答も §6.4 |
| `Edit::InsertSet` の payload が `routes: RouteTable` を含む | セット複製・削除 Undo でルートが失われない。本書側の追加要求なし |
| 検証 V12: `moves + hold <= MAX_SET_COUNTS`（= 4,096）、V16: `set.routes.validate(&performers, set.counts)` — **実装は本書** | §3.7 で `RouteTable::validate` のシグネチャと規則を定義する |
| 定数 `MAX_SET_COUNTS = 4_096` / `MAX_TIMELINE_COUNTS = 262_144` / `MAX_COORDINATE = 10_000.0` | §3.12 の精度議論と §6.1 の上限がこれらを使う |
| revision は単一の `u64` ではなく `Revisions` + `Scopes`(u16) + `SetScope` + 型付き `CacheKey` 7 種 | `TransitionPlan` の無効化は `u64` ではなく **`PlanKey`**（§3.9）で行う。`SetRoutes` は `doc` と `SetScope::One(set)` だけを上げる（10 §3.4 の行列）ので、ルート編集で衝突走査以外のキャッシュが無駄に飛ぶことはない |
| `Set.shape: Option<ShapeAssignment>` は**助言的**（I-18） | 本書は `shape` を読まない。ルートは positions からのみ導出されるので、shape が古くなっても遷移は正しい |
| `DrillError`（文書42） | 本書の公開 API は全て `Result<_, DrillError>` |

`Document::validate` が V16 でルート表を検証する以上、**「ファイルに入ってよい値」と
「編集中に一時的に生じうる値」を分ける**必要がある。本書は前者を `RouteTable::validate`
（拒否）、後者を `PlanWarning`（降格 + 警告）に割り当てる。境界は §6.2 の冒頭に書いた。

---

## 3. 設計

新規モジュール `crates/drill-core/src/transition.rs` に閉じる。依存は `serde` と `crate::{Point, Document, PerformerId}` のみ。

### 3.1 コード全体の構造

```
Set { counts: SetCounts, routes: RouteTable }   永続表現（スパース・小さい）
            │  plan()  … PlanKey が変わった時だけ
            ▼
      TransitionPlan { lanes: Vec<Lane>, tables: Vec<ArcTable> }   平坦・POD・確保済み
            │  eval() … 毎フレーム、確保ゼロ、分岐最小
            ▼
      out: &mut Vec<Point>  /  facings: &mut Vec<f32>
```

**永続表現と評価表現を分ける**のが本設計の中心である。`RouteTable` は編集しやすくファイルに
優しいスパース表現、`Lane` は 1 演者 1 レコードの平坦な POD 配列。両者の間の変換（`plan`）は
1フレームに1回も走らないのが通常であり、そこに `BTreeMap` 探索・ソート・循環検出・弧長表構築
といった重い処理を全部押し込む。

### 3.2 弦フレーム（ChordFrame）

すべての形状は、その演者自身の **開始点 → 終了点** が張る直交フレームの中で定義する。

```rust
/// A performer's own start→end frame. `along` is the unit chord direction,
/// `lateral` is that direction rotated +90 degrees. Both are stored premultiplied
/// by the chord length, so local (1.0, 0.0) is exactly `end`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChordFrame {
    pub origin: Point,
    /// Chord vector: `end - start`.
    pub along: [f32; 2],
    /// `along` rotated +90 degrees (x, y) -> (-y, x). Same magnitude as `along`.
    pub lateral: [f32; 2],
    /// `|along|`. Zero for a degenerate (start == end) lane.
    pub length: f32,
}

impl ChordFrame {
    pub fn new(start: Point, end: Point) -> Self {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        Self {
            origin: start,
            along: [dx, dy],
            lateral: [-dy, dx],
            length: (dx * dx + dy * dy).sqrt(),
        }
    }

    /// Local (along, lateral) -> field point. 4 mul + 4 add.
    #[inline]
    pub fn to_field(&self, along: f32, lateral: f32) -> Point {
        Point {
            x: self.origin.x + self.along[0] * along + self.lateral[0] * lateral,
            y: self.origin.y + self.along[1] * along + self.lateral[1] * lateral,
        }
    }

    /// Field point -> local (along, lateral). Returns `(0.0, 0.0)` for a
    /// degenerate frame. Used by the UI to convert a dragged handle into the
    /// stored, chord-relative representation.
    pub fn to_local(&self, point: Point) -> (f32, f32) {
        let l2 = self.length * self.length;
        if l2 <= f32::MIN_POSITIVE {
            return (0.0, 0.0);
        }
        let dx = point.x - self.origin.x;
        let dy = point.y - self.origin.y;
        (
            (self.along[0] * dx + self.along[1] * dy) / l2,
            (self.lateral[0] * dx + self.lateral[1] * dy) / l2,
        )
    }
}

/// A point in a `ChordFrame`. `along` runs 0..1 from start to end; `lateral` is
/// perpendicular, in units of chord length.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChordPoint {
    pub along: f32,
    pub lateral: f32,
}
```

弦フレームを採用する理由は3つあり、すべて本書の性能・メモリ主張の根拠になる。

1. **1本のルートが人数によらず意味を持つ。** 制御点をフィールド絶対座標で持つと、
   同じ `Route` を 64 人に適用したとき全員が同じ1点へ吸い寄せられる。設計者が欲しいのは
   「64本の平行な曲線」なので、弦相対が正しいドメインモデルである。
2. **弧長表が共有できる。** 弦相対の形は演者ごとに相似形なので、弧長は
   `s(t) = chord_length × ŝ(t)` と分解でき、`ŝ` の表は**ルート1本につき1個**で足りる。
   1,000人でも表は数個。これが 3.11 のメモリ主張の中核。
3. **編集操作に対して自然に追従する。** セット全体を回転・拡縮しても（`editing.rs` の
   `rotate` / `scale`）、弦が回転・拡縮するだけでルートの見た目が保たれる。
   絶対座標だと回転のたびに全ルートを書き換える `Edit` が必要になる。

フィールド絶対の経由点が本当に必要な場合（ピットやプロップを全員が同じ地点で避ける）は
`PathVia::Absolute` を用意する。3.4 参照。

### 3.3 形状（RouteShape）

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RouteShape {
    /// Start to end in a straight line. Arc length is analytic; no table.
    Straight,

    /// Quadratic Bézier whose single control point is chord-relative.
    /// Arc length needs a precomputed table (see `ArcTable`).
    Curve { control: ChordPoint },

    /// Polyline through waypoints. Arc length is analytic per segment; the
    /// "table" is the exact cumulative-length prefix array.
    Path { via: PathVia },

    /// Circular arc through both endpoints. `bulge` is the signed sagitta
    /// divided by the chord length: 0 is a straight line, +0.5 is a half
    /// circle bulging toward `lateral+`, -0.5 the other way.
    /// Arc length is analytic; no table.
    Arc { bulge: f32 },

    /// Adopt `leader`'s resolved chord-relative shape and easing, with the gate
    /// shifted later by `delay_counts`. The follower still departs from and
    /// arrives at its OWN dots. This is the "file / ripple" move.
    FollowShape { leader: PerformerId, delay_counts: f32 },

    /// Literally retrace `leader`'s field trajectory `delay_counts` behind it.
    /// The follower's own destination dot is not used for the path; validation
    /// reports the residual when it differs from where the trail actually ends.
    /// This is the "snake / conga" move.
    FollowTrail { leader: PerformerId, delay_counts: f32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PathVia {
    /// Chord-relative waypoints. One shared arc-length table serves every
    /// performer using this route.
    Relative(Vec<ChordPoint>),
    /// Field-absolute waypoints (route around a prop). Geometry differs per
    /// performer, so each lane gets its own table. Overrides only in practice.
    Absolute(Vec<Point>),
}

impl Default for RouteShape {
    fn default() -> Self { Self::Straight }
}
```

#### 3.3.1 `Arc { bulge }` を `Arc { center, sweep }` にしない理由

DESIGN_GAPS の素案は `Arc { center, sweep }` だが、これは**過剰決定**である。円弧は始点と終点が
既に固定されているので、`center` が両端点から等距離でなければ表現不能な状態になり、
編集のたびに整合を取り直す `Edit` が必要になる。`bulge`（符号付きサジッタ / 弦長）は
自由度がちょうど1で、常に整合し、弦相対なので共有可能、しかも弧長が解析的に求まる。

UI 用に相互変換を提供する:

```rust
impl RouteShape {
    /// Build an `Arc` from a center and sweep the UI collected, projecting onto
    /// the representable family. Returns `Straight` for a degenerate sweep.
    pub fn arc_from_center_sweep(start: Point, end: Point, center: Point, sweep_rad: f32) -> Self;
    /// The center and signed sweep implied by `bulge` for a given chord.
    pub fn arc_center_sweep(bulge: f32, frame: &ChordFrame) -> Option<(Point, f32)>;
}
```

円弧の幾何（弦長 `c`、サジッタ `σ = bulge·c`）:

```
総回転角 Θ = 4·atan(2·bulge)
半径      R = c·(1 + 4·bulge²) / (8·|bulge|)      (bulge → 0 で R → ∞)
弧長      L = |R·Θ| = c·(1 + 4·bulge²)·atan(2·bulge) / (2·|bulge|)
```

`bulge → 0` の極限で `L → c`（`atan(x) ≈ x`）。実装は `|bulge| < 1e-4` で `Straight` に落とす。

#### 3.3.2 曲線の弧長パラメータ化

等速で動くには、正規化時刻ではなく**弧長**で進める必要がある。形状ごとに手段が異なる。

| 形状 | 弧長 | 手段 |
|---|---|---|
| `Straight` | `s(t) = L·t` | 解析。表なし。 |
| `Arc` | `s(t) = L·t`（角度と弧長が比例するので角度を線形に進めればよい） | 解析。表なし。 |
| `Path` | 区間ごとに解析 | 頂点数+1 の累積長前置和。**厳密**。`shapes.rs:156-165` と同じ構成。 |
| `Curve`（2次ベジェ） | 弦和による近似表 | `ArcTable`。以下で精度を示す。 |

2次ベジェの弧長には実は閉形式が存在する（被積分関数が 1 次多項式のノルムなので
`log` を含む式になる）。それでも表を使うのは次の理由による。

- 制御点が弦上に乗る（共線）と閉形式が `0/0` に退化し、`log` の引数が 0 に近づく。
  数値的に不安定で、`lateral → 0` の**連続的な**編集操作の途中で座標が跳ねる。
- 閉形式は `s(t)` を与えるが必要なのは逆写像 `t(s)` であり、結局 Newton 反復が要る。
  反復回数が入力依存になると「同じ入力から同じ出力」の説明が複雑になる。
- 表なら `Curve` / `Path` / 将来の 3 次ベジェが**同一のコード経路**を通るため、
  決定論とテストが1組で済む。

```rust
/// Normalized cumulative arc length of a chord-relative shape, sampled at
/// `RESOLUTION + 1` uniform parameter values. `s[0] == 0.0`, `s[RESOLUTION] == 1.0`.
/// Multiply by the lane's chord length to get field units.
#[derive(Clone, Debug, PartialEq)]
pub struct ArcTable {
    /// Normalized cumulative length; monotone non-decreasing, ends at 1.0.
    s: Box<[f32]>,
    /// Total normalized length (`total / chord_length` for a chord-relative
    /// shape). Needed because callers want field-unit route length.
    total_over_chord: f32,
}

impl ArcTable {
    /// 32 segments (33 samples). See the error analysis below.
    pub const RESOLUTION: usize = 32;

    /// Build from a chord-relative sampler, `f(t) -> (along, lateral)`.
    /// Chord-sum approximation: exact for polylines, O(h^2) for curves.
    pub fn build(sample: impl Fn(f32) -> (f32, f32)) -> Self;

    /// Exact table for a polyline through chord-relative waypoints. Uses one
    /// entry per segment rather than `RESOLUTION`, so it is exact.
    pub fn build_polyline(points: &[(f32, f32)]) -> Self;

    /// Inverse map: the shape parameter `t` at normalized arc fraction `e`.
    /// Binary search over `s` plus one linear interpolation inside the bucket.
    /// Monotone in `e`; returns 0.0 for `e <= 0.0` and 1.0 for `e >= 1.0`.
    #[inline]
    pub fn t_at(&self, e: f32) -> f32;

    /// Route length in field units for a lane of the given chord length.
    #[inline]
    pub fn length(&self, chord_length: f32) -> f32 { self.total_over_chord * chord_length }
}
```

**精度。** 2次ベジェの二階微分は定数 `A = 2(P0 - 2P1 + P2)`、`a = |A|`。パラメータ刻み
`h = 1/N` の区間で、曲線と弦のサジッタは `σ ≤ a·h²/8`。放物線弧の弦超過は `≈ 8σ²/(3c)`
（`c` は弦長 `≈ |B'|·h`）なので、1区間あたりの誤差は `≈ a²h³/(24·|B'|)`、全体で

```
E ≈ a² / (24 · L · N²)
```

具体値: 弦 20 yd、横オフセット 10 yd（実運用で最大級に強い曲がり）のとき
`P0-2P1+P2 = (0, -20)`、`a = 40`、`L ≈ 27.6 yd`。

| N | 絶対誤差 E | 弦長比 |
|---|---|---|
| 8 | 3.8 cm | 1.4e-3 |
| 16 | 9.5 mm | 3.4e-4 |
| **32** | **2.4 mm** | **8.5e-5** |
| 64 | 0.6 mm | 2.1e-5 |

**`RESOLUTION = 32` を採用する。** 2.4 mm は 8-to-5 の 1 歩（22.5 inch = 57 cm）の
1/240 であり、フィールド全幅 100 yd を 4K 幅で描いても 0.1 画素未満。歩幅解析（文書12）の
表示桁（0.25 歩刻み）にも一切影響しない。誤差は `1/N²` なので、将来 3 次ベジェを足して
`a` が 2 倍になっても N=64 に上げれば同じ精度に戻る。

**メモリ。** 33 × 4 = 132 B + `Box` ヘッダ。弦相対形状では**ルート1本につき1個**。
`RouteTable` に既定1本 + オーバーライド数本という通常の構成なら、1セットあたり表は
1〜8 個、つまり 132 B 〜 1 KB。`PathVia::Absolute` のみ演者ごとに個別の表が要る
（3.11 で費用を明示）。

`t_at` の計算量は `log2(32) = 5` 回の比較 + 1 除算 + 1 lerp。表は 132 B なので
L1 の 2〜3 キャッシュラインに収まり、しかも同じルートを共有する全演者が同じ表を叩くので
実質常に L1 ヒットする。

### 3.4 ゲートとホールド

```rust
/// Departure and arrival, in counts local to the set's MOVE window
/// (`0.0 ..= SetCounts::moves`).
///
/// * `local_count <= depart` -> the performer sits on its start dot.
/// * `depart < local_count < arrive` -> moving.
/// * `local_count >= arrive` -> the performer sits on its end dot (this is the
///   per-performer "early arrival hold"; the ensemble-wide hold is `SetCounts::hold`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    /// Finite, `>= 0.0`, `<= SetCounts::moves`.
    pub depart: f32,
    /// `None` means "the end of the move window", so the gate follows an edit
    /// to `SetCounts::moves` without emitting a second `Edit`. When `Some`, the
    /// value is finite and `depart <= arrive <= moves`.
    pub arrive: Option<f32>,
}

impl Gate {
    /// Depart at count 0, arrive at the end of the move window.
    pub const FULL: Self = Self { depart: 0.0, arrive: None };

    /// The concrete `(depart, arrive)` pair for a given move window. Clamps into
    /// `[0, moves]` and enforces `depart <= arrive`. Non-finite input collapses
    /// to `FULL` semantics for that field. Never returns NaN.
    pub fn resolve(self, moves: f32) -> (f32, f32);

    /// `true` for `FULL`. Used by `skip_serializing_if`.
    pub fn is_full(&self) -> bool { self.depart == 0.0 && self.arrive.is_none() }
}

impl Default for Gate {
    fn default() -> Self { Self::FULL }
}
```

**`arrive: Option<f32>` であって番兵の `f32::INFINITY` ではない理由が 2 つある。**

1. **JSON は無限大を表現できない。** `serde_json` は `f32::INFINITY` を `null` として
   書き出す（エラーにもならない）ので、番兵を使うと保存 → 読込で `arrive` が
   静かに壊れる。`Option<f32>` なら `null` が正規の表現になり、往復が保証される。
2. **文書10 の検証 V16 が「gate が `0 <= depart <= arrive <= moves`」を要求している。**
   番兵は定義上この範囲外なので、検証を通すには例外規定が要る。`None` なら
   「範囲検査の対象外」であることが型に出る。

「セットのカウント数を 16 → 24 に変えたら既定ゲートが自動追従する」という当初の狙いは
`None` でそのまま保たれる。逆に `moves` を**縮める**編集は明示的に扱う必要があり、
それは §6.2 の `RouteTable::clamped_to` で解決する。

```rust
/// Counts a set occupies on the timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SetCounts {
    /// Counts spent moving toward the NEXT set. Meaningless (and ignored by the
    /// timeline) on the last set.
    pub moves: u16,
    /// Counts held motionless on this set's dots, AFTER the move window.
    pub hold: u16,
}

impl SetCounts {
    #[inline]
    pub const fn total(self) -> u32 { self.moves as u32 + self.hold as u32 }
    #[inline]
    pub const fn from_legacy(counts: u16) -> Self { Self { moves: counts, hold: 0 } }
}
```

`hold` をゲートで代用せず独立に持つ理由:

1. **意味が違う。** `hold` は「このセットは合奏として8カウント止まる」という構造上の事実で、
   カウントシート（文書17）と音楽の小節割りに直結する。ゲートの `arrive` は
   「この演者だけ早く着く」という個人の事情。両者を1つの数に潰すと、
   カウントシートが「16カウント移動 + 8カウント静止」と印字できなくなる。
2. **速い。** ホールド中は補間が完全に不要で、`out` に到着セットの `positions` を
   `copy_from_slice` するだけで済む（3.11）。ゲートで表現すると全演者のレーンを
   評価して「たまたま全員が arrive を超えている」と気付く経路になる。

### 3.5 イージング

```rust
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Easing {
    /// Constant speed for the whole move window.
    Linear,

    /// Trapezoidal velocity profile with ramps of a FIXED NUMBER OF COUNTS.
    /// This is the marching-correct default: a step-off accelerates over about
    /// two counts and a halt decelerates over about two counts, regardless of
    /// whether the move is 8 counts or 32.
    Ramp { in_counts: f32, out_counts: f32 },

    /// Fraction-based shortcuts, kept because they are what people expect to
    /// find in the UI. `EaseIn` = u^2, `EaseOut` = u(2-u), `Smooth` = u^2(3-2u).
    EaseIn,
    EaseOut,
    Smooth,

    /// Cubic Bézier ordinates `(0, c1, c2, 1)` evaluated directly at `u`.
    /// Monotone whenever `0 <= c1 <= c2 <= 1`, which validation enforces.
    Custom { c1: f32, c2: f32 },
}

impl Default for Easing {
    /// Two counts of step-off and two counts of halt.
    fn default() -> Self { Self::Ramp { in_counts: 2.0, out_counts: 2.0 } }
}
```

**`Ramp` がマーチングとして正しい理由。** 実在の演者は速度を階段状に変えられない。
一方で「移動時間の 20% を加速に使う」という割合指定は、32 カウントの移動に 6.4 カウントの
加速区間を与えてしまい、これはドリルとしてあり得ない。加速に必要なのは**カウント数であって
割合ではない**。`Ramp` は移動窓の長さ `W`（カウント）に対して固定長の加速・減速を置く。

`a = in_counts`, `b = out_counts`, `W = arrive - depart`, `D = W - (a+b)/2` とし、
`τ = u·W` について:

```
τ < a          : e(u) = (τ² / (2a)) / D
a ≤ τ ≤ W-b    : e(u) = (a/2 + (τ - a)) / D
τ > W-b        : e(u) = (D - (W-τ)² / (2b)) / D
```

`e(0)=0`、`e(W/W)=D/D=1`、境界 `τ=a` で両式が `(a/2)/D` に一致、単調増加。
`a=b=0` で `Linear` に一致する。`a+b > W` のときは両ランプを比例縮小して `a+b = W`
にする（三角形速度プロファイル）。すべて解析式で、表も反復も不要。

イージングはプラン時に**分岐のない POD 係数**へ落とす:

```rust
/// Easing compiled into a branch-light form for the hot loop.
#[derive(Clone, Copy, Debug, Default)]
pub struct EasingCoeffs {
    kind: u8,        // 0 Linear, 1 Ramp, 2 Poly (EaseIn/Out/Smooth/Custom share the cubic)
    /// Ramp: [a, b, D, W]. Poly: cubic Bézier ordinates [c1, c2, 0, 0].
    k: [f32; 4],
}

impl EasingCoeffs {
    /// Map normalized move time `u` in [0,1] to normalized arc fraction in [0,1].
    /// Monotone, endpoint-exact, allocation-free, no transcendental calls.
    #[inline]
    pub fn apply(&self, u: f32) -> f32;
}
```

`EaseIn` / `EaseOut` / `Smooth` / `Custom` はすべて 3 次ベジェ縦座標
`(0, c1, c2, 1)` の特別な場合として `kind = 2` に統合できる
（`EaseIn` は `(1/3, 2/3)`… ではなく厳密一致が要るので `EaseIn = (0, 1/3)`,
`EaseOut = (2/3, 1)`, `Smooth = (0, 1)` を採用し、いずれも
`e(u) = 3(1-u)²u·c1 + 3(1-u)u²·c2 + u³` が既存の意図した曲線と一致することを
単体テストで固定する）。これで熱いループの分岐は 3 通りに収まる。

### 3.6 向き（Facing）

```rust
/// Body orientation, in radians, measured from "facing the audience" and
/// increasing toward field-`+x` (the performers' right).
///
/// This matches `continuity.rs`'s direction convention: `-y` is toward the
/// audience, `+x` is 右. So `yaw = atan2(dx, -dy)`; front = 0, right = PI/2,
/// back = PI, left = -PI/2.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Facing {
    /// Face the tangent of the route. Before departure the tangent at t=0 is
    /// used; after arrival the tangent at t=1. A zero-length route yields 0.0.
    Motion,
    /// Field-absolute yaw held for the whole set. This expresses a backward
    /// march (`Fixed{0.0}` while moving toward +y) and a slide / crab
    /// (`Fixed{0.0}` while moving along ±x).
    Fixed { yaw_rad: f32 },
    /// Piecewise yaw over set-local counts, interpolated along the SHORTEST arc
    /// so a 350-degree key pair turns 10 degrees, not 350.
    Keyframed { keys: Vec<FacingKey> },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FacingKey {
    /// Set-local count. Clamped into `[0, SetCounts::total]`, sorted on load.
    pub at: f32,
    pub yaw_rad: f32,
}

impl Default for Facing {
    fn default() -> Self { Self::Motion }
}
```

派生値としてマーチングモードを出す。**保存しない**ので位置と食い違いようがない。

```rust
/// Relationship between where the body points and where it travels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarchMode { Halt, Forward, Backward, Slide }

/// `|facing - motion|` under 45 degrees is Forward, over 135 is Backward, and
/// anything between is a Slide. A stationary performer is `Halt`.
pub fn march_mode(facing_yaw: f32, motion_yaw: Option<f32>) -> MarchMode;
```

3D 表示（文書22）とドリルブック（文書17）はここを参照する。`camera.rs::field_to_world`
は位置しか取らないので、向きは別配列で供給する（3.9 の `facings_at_count`）。

### 3.7 ルートとルート表

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Route {
    #[serde(default, skip_serializing_if = "is_default_shape")]
    pub shape: RouteShape,
    #[serde(default, skip_serializing_if = "Gate::is_full")]
    pub gate: Gate,
    #[serde(default, skip_serializing_if = "is_default_easing")]
    pub easing: Easing,
    #[serde(default, skip_serializing_if = "is_default_facing")]
    pub facing: Facing,
}

impl Default for Route {
    /// Straight, full gate, two-count ramps, motion facing. This is exactly what
    /// the current `positions_at` does, apart from the ramps.
    fn default() -> Self { /* ... */ }
}

/// Sparse per-set routing: one default plus explicit exceptions.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct RouteTable {
    #[serde(default, skip_serializing_if = "is_default_route")]
    pub default: Route,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<PerformerId, Route>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stagger: Option<Stagger>,
}

impl RouteTable {
    /// O(log k) with k = `overrides.len()`, short-circuited to O(1) when empty.
    #[inline]
    pub fn route_for(&self, id: PerformerId) -> &Route {
        if self.overrides.is_empty() { return &self.default; }
        self.overrides.get(&id).unwrap_or(&self.default)
    }

    /// True when this table is byte-identical to "everyone walks straight for
    /// the whole window with linear speed". Enables the memcpy fast path.
    pub fn is_trivial(&self) -> bool;

    /// Structural validation. Called by `Document::validate` (design 10 check
    /// V16) and by `Edit::SetRoutes`'s precheck (design 10 section 3.5).
    ///
    /// Rejects only what a persisted file must never contain:
    ///
    /// * an `overrides` key that is not in `performers`
    /// * `overrides.len() > performers.len()`
    /// * a non-finite control point, bulge, delay, yaw, or easing parameter
    /// * a coordinate whose magnitude exceeds `MAX_COORDINATE`
    /// * `depart` outside `[0, counts.moves]`, or `Some(arrive)` outside
    ///   `[depart, counts.moves]`
    /// * `via` / facing-key counts over `MAX_VIA_POINTS` / `MAX_FACING_KEYS`
    ///
    /// It does NOT reject follow cycles, unknown leader ids, or unreachable
    /// gates. Those are reachable by ordinary editing (delete the leader, then
    /// the follower dangles) and design 10's invariant I-17 already establishes
    /// that dangling performer ids are tolerated rather than fatal. They surface
    /// as `PlanWarning` at plan time instead.
    pub fn validate(&self, performers: &[Performer], counts: SetCounts) -> Result<(), DrillError>;

    /// A copy with every gate clamped into `[0, counts.moves]`, or `None` when
    /// nothing would change. The app pairs this with `Edit::SetCounts` inside a
    /// single `Edit::Batch` whenever it shortens a set, so the shortening stays
    /// reversible and the document never ends up failing V16. See section 6.2.
    pub fn clamped_to(&self, counts: SetCounts) -> Option<RouteTable>;
}
```

`Set` の定義は文書10 が所有する（`positions` は非公開、`shape` は助言的）。本書が供給するのは
そのうち 2 フィールドの**型**だけである。

```rust
// design 10 owns this struct; the two fields below are what design 11 supplies.
pub struct Set {
    // ...
    pub counts: SetCounts,
    #[serde(default, skip_serializing_if = "RouteTable::is_default")]
    pub routes: RouteTable,
    // ...
}
```

本書のコード片で座標を読むときは、文書10 の accessor を使う
（`set.positions()` / `set.position(i)`）。`positions` フィールドを直接触らない。

#### スパース表現のサイズ根拠

`Route` のメモリ実サイズ（x86-64）:

| フィールド | バイト |
|---|---|
| `RouteShape`（最大変異は `Path{Absolute(Vec<Point>)}` = 24 + タグ） | 32 |
| `Gate` | 8 |
| `Easing`（最大変異 `Ramp{f32,f32}` = 8 + タグ） | 12 → 12 |
| `Facing`（最大変異 `Keyframed(Vec)` = 24 + タグ） | 32 |
| 合計（アライン込み） | **≈ 88 B** |

| 場面 | `overrides` 件数 | メモリ | JSON（pretty） |
|---|---|---|---|
| 既定のまま（v1 移行直後、全 64 セット） | 0 | 64 × 88 B = **5.6 KB** | `skip_serializing_if` で**全消滅、0 B** |
| 実運用（全セットの 25% で 5% の演者に個別ルート） | 800 | 800 × (88 + 32) ≈ **96 KB** | ≈ **120 KB** |
| 病的（全 64 セットで全 1,000 人を個別指定） | 64,000 | 64,000 × 120 ≈ **7.7 MB** | ≈ **10 MB** |
| 上限規模（256 セット × 4,000 人を全指定） | 1,024,000 | ≈ 123 MB | ≈ 154 MB |

既定のままなら JSON に**1バイトも出ない**のが要点。`serde` の `skip_serializing_if` は
`Route` の各フィールド、`RouteTable` の 3 フィールド、`Set::routes` 自身の 3 階層で効かせる。
`BTreeMap<u32, Route>` はノードあたり 11 エントリまで詰めるので、
`HashMap` と違い 1 エントリあたりのオーバーヘッドは 32 B 程度に収まり、
かつ**キー順が決定的**なので JSON 出力とゴールデンテストが安定する（`HashMap` では成立しない）。

病的ケースは 7.7 MB で「壊れないが遅い」。信頼できない入力に対しては
`overrides.len() <= performers.len()` を検証で強制する（3.12）。実運用でスタガーを
`overrides` に展開しないことも重要で、これは次節の `Stagger` が O(1) メモリである理由。

### 3.8 スタガー

演者ごとに 1 カウントずつずらす操作を `overrides` へ展開すると、1,000 人分の `Route` が
生えて 88 KB / セットになる。**規則として持つ。**

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stagger {
    pub order: StaggerOrder,
    /// Counts of shift per rank. May be negative.
    pub step_counts: f32,
    pub mode: StaggerMode,
    /// Absolute cap on the applied shift, in counts. Prevents a 1,000-performer
    /// ensemble from generating a 999-count delay.
    pub max_shift: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StaggerOrder {
    /// Performer order in `Document::performers`.
    ByIndex,
    /// Ascending start-dot x (left to right). Ties break by `PerformerId`.
    ByStartX,
    /// Ascending start-dot y (front to back). Ties break by `PerformerId`.
    ByStartY,
    /// Distance from a field point, nearest first. Ties break by `PerformerId`.
    ByDistanceFrom(Point),
    /// Explicit file order; performers not listed get rank 0.
    Explicit(Vec<PerformerId>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StaggerMode {
    /// Rank r departs `r * step_counts` later; arrival unchanged (compresses the
    /// move and raises the step size — the analysis in design 12 must see this).
    DelayDepart,
    /// Rank r arrives `r * step_counts` earlier; departure unchanged.
    AdvanceArrive,
    /// Shift the whole window later by `r * step_counts`, preserving duration.
    ShiftBoth,
}
```

順位付けは `plan` 時に 1 回だけ行い、`Vec<u16>` にキャッシュする。
`ByStartX` などのソートは `sort_unstable_by` に **`PerformerId` のタイブレークを必ず付ける**
（同座標の演者が複数いるのは合流形では普通に起きるので、これが無いと不安定ソートで
順位が実行ごとに変わり、決定論が壊れる）。計算量 O(n log n)、1,000 人で約 15 µs、
プラン時のみ。`Explicit` は `Vec` の位置を順位とし、未掲載は 0。

### 3.9 評価表現（TransitionPlan）と補間 API

プランの無効化は文書10 の `CacheKey` 族に合わせる。文書10 の `TransitionKey`（衝突走査用）は
`grid` と `style` を含み、かつ最後のセットで `None` を返すので、そのままでは使えない。
**8 種目の `CacheKey` として `PlanKey` を文書10 の `revision.rs` に追加することを依頼する**（§9）。

```rust
/// Key for a cached `TransitionPlan` (design 11 section 3.9).
///
/// Differs from design 10's `TransitionKey` in two ways, both deliberate:
/// it is defined for the LAST set as well (a hold-only plan is still a plan),
/// and it omits `grid` and `style` because routes are stored in field units and
/// `StepStyle` never enters the geometry. Omitting them means a grid rescale
/// does not throw away plans that a rescale cannot change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanKey {
    pub set: SetId,
    pub from: Revision,
    /// The next set's revision, or `from` again on the last set, so the key
    /// stays `Copy` and the comparison stays a single `==`.
    pub to: Revision,
    /// Performer count changes relayout every lane.
    pub topology: Revision,
}

impl PlanKey {
    /// `None` when `set` is unknown. 4 field reads and 2 index lookups, so
    /// calling it every frame costs about 15 ns (design 10 measures the very
    /// similar `TransitionKey::current` at 20 ns).
    pub fn current(doc: &Document, set: SetId) -> Option<Self>;
}
```

```rust
/// Compiled, flat, allocation-stable evaluation form of one set's transition.
/// Rebuilt only when `PlanKey::current` differs from the stored key.
#[derive(Debug, Default)]
pub struct TransitionPlan {
    set_index: usize,
    key: Option<PlanKey>,
    counts: SetCounts,
    /// One per performer, index-aligned with `Document::performers`.
    lanes: Vec<Lane>,
    /// Shared normalized arc-length tables. `Lane::table` indexes this.
    tables: Vec<ArcTable>,
    /// Non-fatal problems found while compiling (cycles, unknown leaders, ...).
    warnings: Vec<PlanWarning>,
    /// Set when the whole table is `Straight` + full gate + `Linear`.
    trivial: bool,
}

#[derive(Clone, Copy, Debug)]
struct Lane {
    frame: ChordFrame,      // 20 B
    depart: f32,
    /// `1 / (arrive - depart)`, or 0.0 for an instantaneous arrival.
    inv_span: f32,
    easing: EasingCoeffs,   // 20 B
    kind: LaneKind,         // 1 B
    /// Index into `TransitionPlan::tables`, or `NO_TABLE`.
    table: u16,
    /// Shape parameters: Curve -> [control.along, control.lateral];
    /// Arc -> [radius_over_chord, total_sweep]; Trail -> [source_lane, delay].
    param: [f32; 2],
    facing: FacingLane,     // 12 B
}
// size_of::<Lane>() == 72 B (measured by a compile-time assertion in tests)

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum LaneKind { Straight, Curve, Path, Arc, Trail }

#[derive(Clone, Debug, PartialEq)]
pub enum PlanWarning {
    /// Performers on a follow cycle; every lane listed was demoted to Straight.
    FollowCycle { performers: Vec<PerformerId> },
    FollowUnknownLeader { performer: PerformerId, leader: PerformerId },
    FollowTooDeep { performer: PerformerId },
    /// A `FollowTrail` whose trail does not end on the follower's own dot.
    TrailResidual { performer: PerformerId, distance: f32 },
    /// Gate values were clamped into `[0, moves]`.
    GateClamped { performer: PerformerId },
    /// A control point, bulge, delay, or easing parameter was non-finite and
    /// was replaced by its default.
    NonFiniteSanitized { performer: PerformerId },
}
```

```rust
impl Document {
    /// Compile the set with id `set` into `plan`. Reuses every allocation in
    /// `plan`, so a warm rebuild does not touch the allocator unless the
    /// performer count grew. Idempotent: returns immediately when
    /// `PlanKey::current(self, set) == plan.key`.
    ///
    /// Takes a `SetId`, not an index, per design 10's rule that identity is the
    /// id and order is the `Vec` index. Reordering sets with `Edit::MoveSet`
    /// must not silently repoint a live plan at different geometry.
    pub fn plan_transition(&self, set: SetId, plan: &mut TransitionPlan);

    /// Positions at `local_count` counts into set `set_index`.
    ///
    /// `local_count` is clamped to `[0, counts.total()]`. `out` is cleared and
    /// refilled with one point per performer, index-aligned with
    /// `Document::performers`. Reuses `out`'s allocation.
    ///
    /// This is the correctness reference and the API for non-realtime callers
    /// (SVG, CSV, drill book, video export). For realtime playback use
    /// `plan_transition` once and `eval` per frame; both drive the identical
    /// kernel, so their outputs are bit-identical.
    pub fn positions_at_count(&self, set_index: usize, local_count: f32, out: &mut Vec<Point>);

    /// Body yaw in radians, index-aligned with `Document::performers`.
    pub fn facings_at_count(&self, set_index: usize, local_count: f32, out: &mut Vec<f32>);

    /// Field-unit path length actually travelled by `performer_index` in
    /// `set_index`. Straight-line for `Straight`, true arc length otherwise.
    /// Design 12's stride analysis and design 17's continuity text consume this
    /// instead of `pathing::path_length`.
    pub fn route_length(&self, set_index: usize, performer_index: usize) -> f32;

    /// Compatibility wrapper. `progress` spans the WHOLE set including its hold.
    #[deprecated(note = "use positions_at_count; progress hides gates and holds")]
    pub fn positions_at(&self, set_index: usize, progress: f32, out: &mut Vec<Point>) {
        let total = self.sets.get(set_index).map_or(0, |s| s.counts.total()) as f32;
        self.positions_at_count(set_index, progress.clamp(0.0, 1.0) * total, out);
    }
}

/// Realtime evaluation. Allocation-free, deterministic, no transcendental calls
/// except for `Arc` lanes.
pub fn eval(plan: &TransitionPlan, local_count: f32, out: &mut Vec<Point>);
pub fn eval_facing(plan: &TransitionPlan, local_count: f32, out: &mut Vec<f32>);
```

**索引を取る API と `SetId` を取る API が混在している点について。**
`positions_at_count` / `facings_at_count` / `route_length` は `set_index: usize` を取り、
`plan_transition` は `SetId` を取る。これは文書10 の不変条件 2
（同一性は ID、順序は索引）に従った**意図的な区別**である。前者は
「セットを順に走査して全部出力する」非実時間の呼び出し元（SVG・CSV・ドリルブック）が使い、
索引が自然かつ寿命が 1 回の呼び出しで閉じる。後者は**フレームを跨いで保持される**
キャッシュなので、`Edit::MoveSet` でセットが並び替わったときに黙って別のセットを
指してはならない。`PlanKey.set: SetId` が入っているのはそのためで、
並び替えが起きれば `set` の不一致でプランが再構築される。

**`positions_at` の後方互換。** 既定ルート（`Straight` / `FULL` / `Linear`）かつ `hold == 0`
のとき、`positions_at(i, p)` は `local_count = p × moves`、`u = local_count / moves = p`、
`e(u) = u` となり、現行の `Point::lerp(a, b, p)` と**同じ演算列**になる。
既存テスト `interpolation_reuses_output_allocation`（[lib.rs:485-493](../../crates/drill-core/src/lib.rs)）
の期待値 `Point { x: 15.0, y: 26.0 }` はそのまま通る。ただし
**既定イージングを `Ramp{2,2}` にすると通らない**ので、移行手順を分ける:

1. まず `Route::default()` の easing を `Linear` にして本設計を入れる（既存テスト全通過）。
2. `Ramp{2,2}` を既定にする変更は独立したコミットにし、既存テストの期待値と
   v1 マイグレーションの写像（`Easing::Linear` を明示的に書き込む）を同時に更新する。

これで「v1 で作ったドリルの見た目は移行後も 1:1」を保ちつつ、新規セットは
マーチングとして正しい既定を得る。

#### 評価カーネル

`positions_at_count` と `eval` が別実装だと、SVG 出力と画面表示がずれる
（`PRODUCT_QUALITY.md`「座標表、ドリルブック、SVG/PDF出力は同じドキュメント座標を参照する」）。
**単一のカーネル関数を両者が呼ぶ**構造にして、一致を構造で保証する。

```rust
/// The single evaluation kernel. Everything else is a driver around this.
#[inline]
fn eval_lane(lanes: &[Lane], tables: &[ArcTable], index: usize, local_count: f32) -> Point {
    let lane = &lanes[index];
    // 1. Gate -> normalized move time.
    let u = ((local_count - lane.depart) * lane.inv_span).clamp(0.0, 1.0);
    // 2. Easing -> normalized arc fraction.
    let e = lane.easing.apply(u);
    // 3. Arc fraction -> shape parameter.
    let t = match lane.kind {
        LaneKind::Straight | LaneKind::Arc => e,           // arc length is linear in t
        LaneKind::Curve | LaneKind::Path   => tables[lane.table as usize].t_at(e),
        LaneKind::Trail => {
            // Flattened at plan time: exactly one hop, never recursive.
            let src = lane.param[0] as usize;
            return eval_lane(lanes, tables, src, local_count - lane.param[1]);
        }
    };
    // 4. Shape parameter -> chord-local -> field.
    let (along, lateral) = match lane.kind { /* ... */ };
    lane.frame.to_field(along, lateral)
}
```

`Trail` の再帰は**プラン時の平坦化により深さ 1 で終わる**（3.10）。`debug_assert!` で
`lanes[src].kind != LaneKind::Trail` を固定する。

### 3.10 フォロー・ザ・リーダー

#### 軌跡バッファは作らない

素朴な実装は「リーダーの通った点をリングバッファに貯め、追従者は `delay` カウント前を読む」だが、
これは (a) バッファの寿命管理、(b) シーク時に履歴が無い、(c) 再生方向を逆にすると壊れる、
(d) 前フレームに依存するので決定論を失う、という4つの問題を同時に抱える。

本設計では**すべてのレーンが `local_count` の閉形式関数**なので、リーダーの `d` カウント前の
位置は `eval_lane(leader, t - d)` を評価するだけで得られる。**バッファは存在しない。**
シークしても逆再生しても同じ値が出る。これが `positions_at_count` を弧長の閉形式に
そろえたことの副次的な最大の利得である。

#### プラン時の解決と平坦化

```rust
/// Maximum follow chain depth. Deeper chains are demoted with a warning.
pub const MAX_FOLLOW_DEPTH: usize = 16;
```

`plan_transition` の中で、フォローを次の順で処理する。

1. **リーダー索引の解決。** `PerformerId → performer index` を引く。未知の ID は
   `FollowUnknownLeader` 警告 + `Straight` へ降格。自己参照（`leader == self`）も同様。
2. **循環検出。** 追従辺だけからなる有向グラフ（各頂点の出次数は 0 か 1）に対し、
   `Vec<u8>` の 3 色（White/Gray/Black）で反復的 DFS を回す。出次数が高々 1 なので
   実質は「訪問済み集合を持ちながら鎖をたどる」だけで、計算量 O(n)、確保は
   `plan` が持ち回す `Vec<u8>` 1本のみ。Gray に再到達したら循環なので、
   **循環上の全頂点を `Straight` へ降格**し `PlanWarning::FollowCycle` に列挙する。
   A→B→A も A→A も同じ経路で捕まる。
3. **平坦化。**
   - `FollowShape { leader, delay }`: リーダーの**解決済み**の `shape` / `easing` / 弧長表索引を
     コピーし、自分のゲートを `delay` だけ後ろへずらす。自分の弦フレームは自分のもの。
     結果として `LaneKind` は `Straight` / `Curve` / `Path` / `Arc` のいずれかになり、
     **評価時のフォロー処理は消える**。
   - `FollowTrail { leader, delay }`: 鎖をたどって最初の非 Trail 祖先 `j` を求め、
     遅延を累積して `LaneKind::Trail { source: j, delay: Σd }` にする。経路圧縮付き
     union-find と同型で、償却 O(n)。
4. **残差の検査。** `FollowTrail` の追従者について、`eval_lane(j, arrive - Σd)` と
   自分の到着ドットの距離を計算し、`> 0.25 步` なら `PlanWarning::TrailResidual` を出す。
   降格はしない（軌跡追従は到着ドットを無視するのが仕様）。UI は警告として表示する。

深さが `MAX_FOLLOW_DEPTH` を超えた鎖は `FollowTooDeep` + 降格。これは循環が無くても
1,000 人の一列縦隊で `delay` が積み上がって 999 カウント遅れる病的ケースを塞ぐ。

平坦化後の不変条件（テストで検証する）:

> `plan.lanes` のうち `LaneKind::Trail` であるものの `param[0]` が指すレーンは
> `LaneKind::Trail` ではない。

これにより `eval_lane` の再帰は深さ 1 で確定し、スタックも確保も要らない。

### 3.11 タイムラインへの影響

```rust
impl Document {
    /// Counts on the global timeline: every set's move window plus its hold,
    /// except that the LAST set contributes only its hold (there is nowhere to
    /// move to). Saturating, so 256 sets of u16 counts cannot overflow.
    pub fn timeline_counts(&self) -> u32 {
        let last = self.sets().len().saturating_sub(1);
        self.sets().iter().enumerate().fold(0u32, |acc, (i, s)| {
            let span = if i == last { u32::from(s.counts.hold) } else { s.counts.total() };
            acc.saturating_add(span)
        })
    }
}
```

`saturating_add` は形式上の保険で、実際には文書10 の検証 V12 と
`MAX_TIMELINE_COUNTS = 262_144` が総和を先に縛るので飽和は起きない。

現行（[lib.rs:349-355](../../crates/drill-core/src/lib.rs)）は最後のセットを丸ごと除外していた。
これは「最後のセットで 8 カウント止まって終わる」ショーの尺が 8 カウント足りない、という
実務上の不具合でもある。新定義は `hold(last)` を算入して直す。

`locate_count` / `global_count` は各セットのスパンを `total()`（最後だけ `hold`）として
同じ走査に統一する。移行時の互換:

| 既存テスト（[lib.rs:613-620](../../crates/drill-core/src/lib.rs)） | 新定義での値 |
|---|---|
| `timeline_counts() == 16` | `total(0)=16` + `hold(1)=0` = **16** ✓ |
| `locate_count(8.0) == (0, 8.0)` | ✓ |
| `locate_count(16.0) == (1, 0.0)` | セット0のスパン 16 を使い切り、セット1のスパンは 0 → **(1, 0.0)** ✓ |
| `global_count(0, 7.0) == 7.0` | ✓ |

v1 マイグレーションが `hold = 0` を書き込むので、既存ドキュメントのタイムラインは 1 カウントも動かない。

`locate_count` の線形走査は 256 セットで最悪 256 反復。毎フレーム呼ばれるので、
セット開始カウントの累積和（`Vec<u32>`、`Scopes::TIMELINE` で無効化）を `Derived` に
キャッシュして二分探索 O(log S) にする。キャッシュの置き場は文書10 の `Derived` なので、
本書は「必要である」ことと API 形状だけを固定し、実体の所有は文書10 に委ねる。

**この累積和は f64 化（§3.12）とも噛み合う。** グローバルカウントとセット索引の対応は
整数の累積和で決まるので、二分探索を `f64` の入力に対して行っても `f32` の入力に対して行っても
**同じ整数境界**を見る。型の違いが「どのセットにいるか」の判断を割ることがない。

**波及箇所**（`set.counts` を触っている全て、文字列比較で機械的に洗い出せる）:

| 箇所 | 変更 |
|---|---|
| [countsheet.rs:70,75](../../crates/drill-core/src/countsheet.rs) | `counts: set.counts` → `moves` と `hold` の 2 列に分ける。`start_count += set.counts.total()`。 |
| [continuity.rs:104](../../crates/drill-core/src/continuity.rs) | `from.counts` → ゲート幅 `arrive - depart`。距離は `route_length` へ。（文言は文書17） |
| [pathing.rs:36](../../crates/drill-core/src/pathing.rs) | 同上。`step_size` の分母はゲート幅、分子は経路長。（文書12） |
| [svg.rs:274,314](../../crates/drill-core/src/svg.rs) / [coordinates.rs:150,172](../../crates/drill-core/src/coordinates.rs) | 表示のみ。`counts.total()`。 |
| [main.rs:510-515](../../crates/drill-app/src/main.rs) | progress への往復をやめ、`positions_at_count` を直接呼ぶ。 |
| [main.rs:618-622,765-767](../../crates/drill-app/src/main.rs) | 再生範囲の計算を `total()` に。 |

### 3.12 数値精度: どこが f32 でどこが f64 か

文書30（音声エンジン）§3.12-3 が `TempoMap` の `f32` 秒ではサンプル精度に届かないと指摘した。
検算した結果、指摘は正しく、**さらにカウント軸にも同じ問題がある**ことが分かった。

#### 3.12.1 問題の検算

`f32` の仮数は 24 ビット（暗黙の 1 を含む）なので、値 `v` 付近の刻み幅は
`ulp(v) = 2^(floor(log2 v) - 23)`。

**秒軸**（文書30 の指摘）:

| 時刻 | ulp | 48 kHz のサンプル数 |
|---|---|---|
| 60 s（1 分） | 2⁻¹⁸ = 3.81 µs | 0.18 |
| 350 s | 2⁻¹⁶ = 15.3 µs | 0.73 |
| **480 s（8 分のショー）** | **2⁻¹⁵ = 30.5 µs** | **1.46** |
| 960 s（16 分） | 2⁻¹⁴ = 61.0 µs | 2.93 |

480 秒付近で 1 サンプルを表現できなくなる。`MEDIA_PIPELINE.md` の
「音声時刻はサンプル索引」と、文書31 の「音声はサンプル索引で 1 回だけ丸める」が
`f32` の秒を経由した瞬間に成立しない。

**カウント軸**（本書が追加で確認した点）:

| グローバルカウント | ulp（カウント） | 120 BPM での秒 | 48 kHz のサンプル数 |
|---|---|---|---|
| 64（1 セット分） | 2⁻¹⁷ = 7.6e-6 | 3.8 µs | 0.18 |
| 960（8 分 @ 120 BPM） | 2⁻¹⁴ = 6.1e-5 | 30.5 µs | **1.46** |
| 2,048（基準規模の総カウント） | 2⁻¹³ = 1.2e-4 | 61 µs | 2.9 |
| 262,144（`MAX_TIMELINE_COUNTS`） | 2⁻⁵ = 0.031 | 15.6 ms | **750** |

**秒を `f64` にするだけでは足りない。** `f32` のグローバルカウントを `f64` の秒へ渡しても、
入口で既に 1.5 サンプル分の情報が落ちている。したがって
**「サンプル索引へ焼く経路はカウントも秒も `f64`」**が正しい結論である。

一方、**セット内ローカルカウントは `f32` で十分**である。文書10 の
`MAX_SET_COUNTS = 4_096` が上限を与えるので、最悪でも
`ulp(4096) = 2⁻¹¹ = 4.88e-4` カウント。1 カウントに 1 歩（0.625 yd）進む移動でも
幾何誤差は `4.88e-4 × 0.625 yd = 0.3 mm` にしかならず、§3.3.2 の弧長表誤差 2.4 mm より
1 桁小さい。実運用のセット長（16〜32 カウント）なら `ulp(32) = 3.8e-6` カウント = 2 µm。

**`positions_at_count` がグローバルではなく*セット内ローカル*のカウントを受け取る設計は、
人間工学上の選択であるだけでなく、幾何経路を安全に `f32` に留めるための構造でもある。**
仮に `positions_at_count(global_count: f32)` にしていたら、`MAX_TIMELINE_COUNTS` 近傍で
0.031 カウント = 2 cm の位置誤差が出ていた。

#### 3.12.2 `TempoMap` の f64 写像

```rust
// crates/drill-core/src/tempo.rs
impl TempoMap {
    /// Cumulative real time, in seconds, from count 0 to `global_count`.
    /// Closed form: one binary search plus one affine expression. No summation
    /// happens at query time.
    pub fn seconds_at_f64(&self, global_count: f64) -> f64;

    /// Inverse of `seconds_at_f64`, by the same construction.
    pub fn count_at_f64(&self, seconds: f64) -> f64;
}
```

**定式化。** 事象 `e_0 .. e_{n-1}` は count 昇順・重複なし（既存の `set` が維持している不変条件）。
区間 `i` の開始カウントを `s_i = max(e_i.count, 0)`（ただし `s_0` は 0 として扱う。
[tempo.rs:122-126](../../crates/drill-core/src/tempo.rs) の既存規則と同じ）、
BPM を `b_i = sanitize_bpm(e_i.bpm)` とする。区間 `i` は `[s_i, s_{i+1})` を覆い、
最終区間は無限に伸びる。

区間先頭までの経過秒の**前置和**を `f64` で 1 度だけ作る:

```
T_0 = 0
T_{i+1} = T_i + (s_{i+1} - s_i) * 60 / b_i
```

これを用いて、問い合わせは**逐次加算ではなく区間ごとの閉形式**になる:

```
i = 区間索引 = partition_point(s, |x| x <= c) - 1        (二分探索, O(log n))
seconds_at_f64(c) = T_i + (c - s_i) * 60 / b_i           (乗算1・除算1・加減算2)

j = 区間索引 = partition_point(T, |x| x <= t) - 1
count_at_f64(t)   = s_j + (t - T_j) * b_j / 60
```

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "TempoMapRepr", into = "TempoMapRepr")]
pub struct TempoMap {
    events: Vec<TempoChange>,
    /// `starts[i]` = `s_i`, `prefix[i]` = `T_i`. Both derived from `events`,
    /// rebuilt by every mutator and by the `From<TempoMapRepr>` used on load.
    /// Never serialized: it is a cache, and a hand-edited file must not be able
    /// to make the cache disagree with `events`.
    starts: Vec<f64>,
    prefix: Vec<f64>,
}

/// The on-disk shape: exactly what `TempoMap` serialized before this change,
/// so the file format does not move.
#[derive(Serialize, Deserialize)]
struct TempoMapRepr { events: Vec<TempoChange> }
```

`#[serde(from = ..., into = ...)]` にすることで、**逆直列化のたびに前置和が必ず再構築される**。
`#[serde(skip)]` + `Default` だと空の前置和を持つ `TempoMap` が生まれてしまい、
「読み込んだ直後だけ全部 0 秒」という最悪の壊れ方をする。

計算量とメモリ: 構築 O(n)、問い合わせ O(log n)。`MAX_TEMPO_EVENTS = 8_192`（文書10）なので
前置和は最大 `8_192 × 8 B × 2 = 131 KB`、二分探索は 13 段。通常のショーは
テンポ事象 10 個未満なので 4 段、160 B。現行の O(区間数) 線形走査より速い。

#### 3.12.3 既存 f32 API は f64 版のラッパにする

```rust
impl TempoMap {
    #[inline]
    pub fn seconds_at(&self, global_count: f32) -> f32 {
        self.seconds_at_f64(f64::from(global_count)) as f32
    }
    #[inline]
    pub fn count_at(&self, seconds: f32) -> f32 {
        self.count_at_f64(f64::from(seconds)) as f32
    }
}
```

「残す」でも「捨てる」でもなく**ラッパにする**判断の理由:

1. **実装が 1 つになる。** 最も危険なのは、UI が `f32` 経路で「今はセット 7」と判断し、
   音声エンジンが `f64` 経路で「今はセット 8」と判断することである。区間境界の解釈が
   2 実装に分かれている限り、この食い違いはいつか必ず起きる。実装を 1 つにすれば
   構造的に起きない。
2. **精度が悪化しようがない。** `f32 → f64` は幅拡張なので**厳密**（丸めゼロ）。
   丸めは最後の `as f32` の 1 回だけ。一方、現行の `f32` 実装は区間ごとに加算しており、
   区間数だけ丸めが累積する。したがってラッパ化は既存の全入力に対して
   **精度が同じか良くなる**。悪くなる入力は存在しない。
3. **呼び出し側を変えなくてよい。** UI 表示・タイムラインのスクラブ・
   [playback.rs:66-80](../../crates/drill-core/src/playback.rs) の既存呼び出しは
   シグネチャが変わらないので無改修で通る。

**既存テストの検算**（[tempo.rs:202-330](../../crates/drill-core/src/tempo.rs) の 11 本）:

| テスト | ラッパ化後の値 |
|---|---|
| `two_segment_hand_computed_seconds` | `T = [0, 8]`, `s = [0, 16]`, `b = [120, 60]`。`seconds_at(16)` は区間1 → `8 + 0 = 8.0`（厳密）。`seconds_at(32)` → `8 + 16·60/60 = 24.0`（厳密）。`seconds_at(8)` → `0 + 8·60/120 = 4.0`。`count_at(8)` → 区間1 → `16 + 0 = 16.0`。`count_at(24)` → `16 + 16·60/60 = 32.0`。`count_at(16)` → `16 + 8·60/60 = 24.0`。**全て厳密に一致** ✓ |
| `constant_tempo_round_trips` | 144 BPM。`7.5 → 7.5·60/144 = 3.125`（2 の冪の分母なので f64 でも f32 でも厳密）。往復も厳密 ✓ |
| `bpm_at_boundaries` | 区間の半開区間 `[s_i, s_{i+1})` と `partition_point(|x| x <= c)` の組み合わせは、`c == 16.0` を区間 1 に割り当てる。既存の `bpm_at`（境界で新テンポ）と同じ規約 ✓ |
| `counts_before_first_event_use_first_bpm` | `s_0` を 0 に潰す規則を維持するので `seconds_at(8) = 8·60/90 = 5.333…`。許容差 1e-3 ✓ |
| `empty_map_behaves_as_default` / `guards_zero_and_negative_bpm` / `negative_counts_and_seconds_clamp_to_zero` | `sanitize_bpm` と `max(0.0)` を f64 側にそのまま移す ✓ |
| `set_replaces_and_keeps_sorted` / `json_round_trip` | `events` の直列化形は不変（`TempoMapRepr`）。`json_round_trip` は `back.events() == map.events()` を見ているので通る ✓ |

`count_at` の既存実装末尾にある「到達不能」分岐
（[tempo.rs:168-169](../../crates/drill-core/src/tempo.rs)）は、最終区間が無限に伸びる
前置和 + 二分探索では構造的に消える。

#### 3.12.4 切り分け表

| 経路 | 量 | 型 | 根拠 |
|---|---|---|---|
| `positions_at_count` / `eval` / `TransitionPlan` | **セット内**ローカルカウント | **f32** | `MAX_SET_COUNTS = 4_096` で ulp = 4.9e-4 カウント → 幾何で 0.3 mm。弧長表誤差 2.4 mm より小さい |
| `Lane` の幾何量（弦・弧長・制御点） | フィールド単位 | **f32** | `MAX_COORDINATE = 10_000` で ulp = 9.8e-4 yd = 0.9 mm。フィールド内（100 yd）なら 7.6e-6 yd = 7 µm |
| `ArcTable::s` | 正規化弧長 [0,1] | **f32** | 値域が [0,1] なので ulp ≤ 6e-8。表そのものの近似誤差 8.5e-5 より 3 桁小さい |
| `Facing` の yaw / `Easing` の係数 | ラジアン・無次元 | **f32** | 0.3 mm 相当の角度分解能を遥かに下回る |
| `Gate` / `SetCounts` | セット内カウント | **f32 / u16** | 同上 |
| `Document::locate_count_f64` | **グローバル**カウント（入力） | **f64** | ulp(262,144) が f32 では 0.031 カウント = 750 サンプル |
| `Document::global_count_f64` | グローバルカウント（出力） | **f64** | 同上 |
| `Document::locate_count` / `global_count` | グローバルカウント | **f32**（f64 版のラッパ） | UI 表示とスクラブ専用。1/32 カウントの誤差が見える経路には使わない |
| `TempoMap::seconds_at_f64` / `count_at_f64` | 秒 | **f64** | 480 s で f32 ulp = 1.46 サンプル |
| `TempoMap::seconds_at` / `count_at` | 秒 | **f32**（f64 版のラッパ） | UI 表示・スクラブ |
| `playback::advance` | 秒とカウント | **f64 化を要求**（§9-7） | 現状 f32（[playback.rs:44-81](../../crates/drill-core/src/playback.rs)）。再生位置がサンプル索引と食い違うと音ズレになる |
| 音声のサンプル索引 | 整数 | **i64** | 文書30 |
| 映像のフレーム時刻 | 有理数 `frame / fps` | **整数対** | 文書31 |

#### 3.12.5 型境界を 1 か所に閉じ込める

混在で結果が揺れないことを保証する方法は「両方の型で同じ量を二度計算しない」に尽きる。
そのために、**グローバルカウントからセット内ローカルカウントへ落ちる 1 か所だけを
型の切り替え点とする。**

```rust
impl Document {
    /// The ONE place a global count is narrowed to f32.
    ///
    /// Global counts are f64 upstream of this call (playback clock, audio
    /// engine, video export). Set-local counts are f32 downstream of it
    /// (planning, evaluation, rendering, analysis). The set index is decided
    /// entirely in f64, so no consumer can disagree with another about which
    /// set the playhead is in.
    pub fn locate_count_f64(&self, global_count: f64) -> (usize, f32);

    /// f32 convenience for UI scrubbing. Widens (exactly) and delegates; there
    /// is no second implementation of the set-boundary walk.
    #[inline]
    pub fn locate_count(&self, global_count: f32) -> (usize, f32) {
        self.locate_count_f64(f64::from(global_count))
    }

    pub fn global_count_f64(&self, set_index: usize, local_count: f32) -> f64;

    /// f32 convenience. Lossy above ~2^13 counts; display only.
    #[inline]
    pub fn global_count(&self, set_index: usize, local_count: f32) -> f32 {
        self.global_count_f64(set_index, local_count) as f32
    }
}
```

これで決定論は次の 4 点で担保される。

1. **拡張は厳密、縮小は 1 回だけ。** `f32 → f64` は丸めゼロ。`f64 → f32` は
   `locate_count_f64` の出口と `TempoMap` のラッパの出口にしか無い。
   ゆえに `f32 → f64 → f32` の往復は恒等写像であり、ラッパ化で値が動くことはない。
2. **整数の判断は f64 側で確定させる。** セット索引は `usize`、区間索引も `usize`。
   これらを決める比較は必ず `f64`（またはセット開始カウントの `u32` 累積和）で行う。
   浮動小数の型によって「どのセットか」「どのテンポ区間か」が変わらない。
3. **丸めの回数が入力に依存しない。** 前置和方式では問い合わせあたりの演算列が
   区間数に依らず固定（二分探索 + 4 演算）なので、同じ入力からは常に同じ丸め列になる。
   現行の逐次加算は区間数だけ丸めが増える形だった。
4. **f64 の四則と `sqrt` は IEEE-754 で完全に規定される。** 丸めモードは既定
   （round-to-nearest-even）から変更しない。Rust は自動 FMA 融合も再結合も行わない。
   x86-64 は SSE2 で 80 ビット中間を経由しないので、`seconds_at_f64` / `count_at_f64` /
   `locate_count_f64` は**プラットフォーム間でビット同一**である。
   §5.5 で述べた超越関数の例外（`Arc` の `sin`/`cos`、facing の `atan2`）はここには入らない。

**`positions_at_count` は f32 のまま**で正しい。理由は 3.12.1 の通り、
セット内ローカルカウントの上限が `MAX_SET_COUNTS` で押さえられていて、
そこでの ulp が幾何的に無意味な大きさだから。ここを `f64` にすると
`Lane` が 72 B から 120 B へ膨らみ（1,000 人で 48 KB 増）、
SIMD 幅が半分になって §5.2 の予算を圧迫するのに、得られる精度は
弧長表の近似誤差に埋もれて観測できない。**変えない理由が積極的にある。**

## 4. 不変条件

すべてテストで検証できる形で書く。

1. **端点一致。** 任意の形状・ゲート・イージングについて
   `positions_at_count(i, 0.0)[k] == sets()[i].positions()[k]` かつ
   `positions_at_count(i, counts.total())[k] == sets()[i+1].positions()[k]`（`FollowTrail` を除く）。
2. **単調性。** `e(u)` は `u` について単調非減少、`e(0) == 0.0`、`e(1) == 1.0`。
   有効な全パラメータについて成立する。
3. **弧長等速。** `Easing::Linear` かつ `Gate::FULL` のとき、任意の `k` について
   `|dist(p(t_k), p(t_{k+1})) - L/N| <= L · 1e-3`（`Curve` の表誤差込み）。
4. **弦フレーム不変。** セット両端を同じ剛体変換 `T`（回転 + 平行移動）で写すと、
   弦相対形状の全サンプル点も `T` で写る。並進では厳密、回転では 1e-5 以内。
5. **ゲート単調。** `local_count` が単調増加すれば `u`、`e`、弧長も単調非減少。逆走しない。
6. **決定論。** 同じ `(Document, set_index, local_count)` から常に**ビット同一**の出力。
   前フレームの値・時計・イテレーション回数・ハッシュ順序に依存しない。
   累加（`pos += v * dt`）を一切使わない。
7. **駆動系一致。** `Document::positions_at_count` と `eval(plan, ...)` はビット同一。
   同一カーネルを呼ぶので構造的に保証される。
8. **有限性。** 開始点・終了点が有限なら、出力は必ず有限。NaN/Inf は `plan` の
   サニタイズで消え、評価器は非有限を生成しない。
9. **フォロー非再帰。** `LaneKind::Trail` のレーンが指す先は `Trail` ではない。
10. **確保ゼロ。** 2 回目以降の `eval` / `positions_at_count` / `plan_transition`
    （演者数不変の場合）で `out` と `plan` の `capacity` が変化しない。
11. **タイムライン整合。** 全 `i`, `c ∈ [0, total(i))` について
    `locate_count_f64(global_count_f64(i, c)) == (i, c)`（1e-9 以内）。
    f32 版でも 1e-4 以内で成立する。
12. **スタガー決定性。** 同一座標の演者が複数いても順位付けは一意
    （`PerformerId` タイブレーク）。同じ文書から常に同じ順位列。
13. **テンポ写像の往復。** 全 `c ∈ [0, MAX_TIMELINE_COUNTS]` について
    `count_at_f64(seconds_at_f64(c)) == c`（相対誤差 1e-12 以内、= f64 の 4 ulp 程度）。
    `f32` ラッパでは 1e-6 以内。
14. **f32/f64 ラッパの無害性。** `seconds_at(c) == seconds_at_f64(c as f64) as f32` が
    全 `c` について**厳密に**成立する（ラッパはそう定義されているので構造的に真だが、
    「別実装を足した」回帰を検出するためテストに残す）。
15. **型境界の一意性。** グローバルカウント → セット索引の判定は `locate_count_f64` の
    1 実装しか存在しない。`locate_count` はそれを呼ぶだけであり、独自の走査を持たない
    （§7.2 P9 が全セット境界の近傍で両者の `set_index` 一致を検査する）。

## 5. 性能

### 5.1 基準

現行ベースライン（`PRODUCT_QUALITY.md` / [benches/core_performance.rs](../../crates/drill-core/benches/core_performance.rs)）:
**1,000人 × 60,000 フレームの直線補間 = 9.24 ms、つまり 154 ns / フレーム。**
これは 1 演者あたり 0.15 ns（≒ 3 GHz で 0.5 サイクル）で、SIMD 化された `lerp` の値。

### 5.2 毎フレーム（`eval`）

| レーン種別 | 1 演者あたりの処理 | 概算 |
|---|---|---|
| `Straight` + `Linear` | ゲート 2 flop、イージング 0、`to_field` 8 flop | ≈ 12 flop、SIMD 可 |
| `Straight` + `Ramp` | + 分岐 1 + 4 flop | ≈ 18 flop |
| `Curve` | + `t_at`（5 回の比較 + 除算 + lerp）+ 2次 De Casteljau 12 flop | ≈ 60 flop、分岐あり |
| `Path` | `Curve` と同じ（表の段数だけ増える） | ≈ 60 flop |
| `Arc` | + `sin` / `cos` 各 1 回 | ≈ 40 サイクル（超越関数律速） |
| `Trail` | 上記 + 参照先 1 本の評価 | 2 倍 |

最悪の現実的構成（1,000 人全員 `Curve`）は直線の約 30 倍と見積もり、
`154 ns × 30 ≈ 4.6 µs / フレーム`。全員 `Arc`（超越関数）でも
`1,000 × 40 サイクル / 3 GHz ≈ 13 µs`。全員 `Trail` で 2 倍にしても 26 µs。

**16.6 ms 予算のうちの取り分: 0.2 ms（1.2%）を `eval` に確保する。**
実測目標は 20 µs 以下で、10 倍の安全余裕を見込んでいる。
`Facing` の評価は位置と同程度の分量なので、別途 0.1 ms を確保する。

ホールド区間（`local_count >= moves`）では `eval` はレーンを一切評価せず、
到着セットの `positions` を `out.copy_from_slice` するだけになる。
1,000 人で 8 KB の memcpy = 約 0.3 µs。8 カウントのホールドが 60 fps で
どれだけ続いても実質ゼロコスト。同様に `trivial == true`（v1 移行直後の全ドキュメント）
のときは現行と完全に同じ SIMD lerp 経路へ落ちるので、**本設計の導入で既存性能は劣化しない**。

### 5.3 プラン構築（毎フレームではない）

`PlanKey::current` が変わった時だけ走る（比較そのものは約 15 ns / フレーム）。

| 工程 | 1,000 人での計算量 | 概算 |
|---|---|---|
| 弦フレーム構築 | O(n)、`sqrt` 1 回 / 人 | 10 µs |
| `RouteTable` 探索 | O(n log k)、`overrides` 空なら O(n) | 5 µs |
| スタガー順位付け | O(n log n) ソート | 15 µs |
| フォロー解決 + 循環検出 + 平坦化 | O(n) 償却 | 5 µs |
| 弧長表構築 | 表 1 個あたり 32 サンプル × 約 10 flop = 320 flop。共有されるので通常 1〜8 個 | < 1 µs |
| サニタイズ・検証 | O(n) | 5 µs |
| **合計** | | **≈ 40 µs** |

**予算: 1.0 ms。** ドラッグ中は毎フレームセット revision が上がるので実質毎フレーム走るが、
40 µs なら 16.6 ms の 0.24% であり許容範囲。それでも
`plan` は「変わったセットだけ」再構築し、隣接セットのプランは触らない。

病的ケース: `PathVia::Absolute` を 1,000 人全員に個別指定すると表が 1,000 個必要になり、
`1,000 × 320 flop = 320 kflop ≈ 100 µs`、メモリ `1,000 × 132 B = 132 KB`。
これでも 1 ms 予算に収まるが、UI は「絶対経路は共有されません」と表示する。

### 5.4 メモリ

| 対象 | 基準規模（1,000人 / 64セット） | 上限規模（4,000人 / 256セット） |
|---|---|---|
| `RouteTable`（既定のまま） | 5.6 KB | 22 KB |
| `RouteTable`（実運用の 5% 個別指定） | 96 KB | 1.5 MB |
| `TransitionPlan`（`Lane` 72 B × n） | 72 KB / プラン | 288 KB / プラン |
| 弧長表 | 1 KB | 4 KB |
| プラン保持数（現セット + 前後 1 の先読み） | 3 × 72 KB = **216 KB** | 864 KB |

`TransitionPlan` は `Vec` を保持したまま再利用するので、2 時間再生しても常駐は増えない
（`PRODUCT_QUALITY.md`「2時間連続再生で常駐メモリの継続増加なし」）。
`plan_transition` は `lanes.clear()` → `lanes.extend()` で回し、`Vec::shrink_to_fit` を呼ばない。

### 5.5 決定論と浮動小数

- **累加を使わない。** 位置は常に `(plan, local_count)` の純関数。再生ループは
  `playback::advance`（[playback.rs:44-81](../../crates/drill-core/src/playback.rs)）が
  `tempo.seconds_at` / `count_at` を経由してカウントを求めており、既に累加していない。
  この性質を遷移モデル側でも維持する。§3.12 の前置和方式は、`TempoMap` の内部に残っていた
  最後の逐次加算も問い合わせ経路から取り除く。
- **型の切り替えを 1 か所に閉じ込める。** §3.12.5 の 4 点。グローバルカウントは `f64`、
  セット内ローカルカウントと幾何は `f32`、境界は `locate_count_f64` の出口のみ。
- **式の順序を固定する。** Rust は `-ffast-math` 相当の再結合も自動 FMA 融合も行わないので、
  `a * b + c` は常に 2 回の丸めとして評価される。整数演算と `sqrt` は IEEE-754 で
  正確に定義されるため、`Straight` / `Curve` / `Path` は **x86-64 と aarch64 でビット同一**。
- **例外は超越関数。** `Arc` の `sin` / `cos` と `Facing` の `atan2` は libm 実装依存で、
  プラットフォーム間のビット一致は保証されない。ゴールデンテストは
  `Arc` / facing を含む場合のみ 1e-4 フィールド単位の許容差で比較する。
  同一バイナリ・同一機での再現性は完全なので、再生と動画書き出しの決定論要件は満たす。

## 6. 失敗モードと安全性

信頼できない入力（他人から受け取った `.drillproj`）を前提に列挙する。

### 6.1 読込時に拒否する（`DrillError`）

これは文書10 の検証 V16 `set.routes.validate(&performers, set.counts)` の中身であり、
`Edit::SetRoutes` の precheck からも同じ関数が呼ばれる（文書10 §3.5）。

| 入力 | 対処 |
|---|---|
| `overrides.len() > performers.len()` | `DrillError::RouteTableTooLarge { set_index, found, limit }`。1,000 人のドキュメントに 10⁹ 件のオーバーライドを積む DoS を塞ぐ。 |
| `overrides` のキーが未知の `PerformerId` | `DrillError::UnknownPerformerRoute`。黙って捨てると利用者のデータが消えるので拒否する。**フォローの `leader` とは扱いが違う**（下の注記）。 |
| `PathVia` の点数 > `MAX_VIA_POINTS = 64` | `DrillError::RouteTooComplex`。1 経路に 10⁶ 点を入れる攻撃を塞ぐ。 |
| セット内の `via` 点の総数 > `4 × performers.len()` | 同上。1 経路ずつは小さくても総量で殴る攻撃を塞ぐ。 |
| `Facing::Keyframed` のキー数 > `MAX_FACING_KEYS = 64` | 同上。 |
| 制御点・経由点の座標の絶対値 > `MAX_COORDINATE`（= 10,000、文書10） | `DrillError::InvalidRoute`。`ChordPoint` は弦相対なので `lateral` の上限は別に `±64`（弦の 64 倍）とする。 |
| `gate.depart` が非有限 / `[0, moves]` の外、`Some(arrive)` が `[depart, moves]` の外 | `DrillError::InvalidRoute`。文書10 V16 が要求している範囲そのもの。 |
| 制御点・`bulge`・`delay_counts`・`yaw_rad`・イージング係数が NaN / ±Inf | `DrillError::InvalidRoute`。**ファイルには絶対に入れない。** |
| `moves` / `hold` が u16 を超える、`moves + hold > MAX_SET_COUNTS` | 前者は `serde` が型で弾き、後者は文書10 の V12 が弾く。`total()` は u32 なのでオーバーフローしない。 |

**`overrides` のキーは拒否、フォローの `leader` は許容、という非対称について。**
`overrides` のキーが実在しないルートは**到達不能なデータ**であり、読み込んでも誰の役にも立たない。
一方、`FollowShape.leader` / `FollowTrail.leader` のぶら下がりは、リーダーを削除するという
**正当な編集**の結果として日常的に生じる。文書10 の不変条件 I-17 は
`Subset::members` / `Set.shape.order` / `FollowTarget::Group` について既に
「ぶら下がり ID を許容する」と決めており、追従のリーダー参照も同じ族に属する。
そこで `leader` は `PerformerExtract`（`RemovePerformers` が実際に削除するもの）には**含めず**、
プラン時に降格 + 警告する（§6.2）。文書10 の `performer_extract_is_total` テストが
この選択を明示的に固定するので、本書はそこへ「`RouteTable` のフォロー参照は I-17 側」と
登録することを要求する。

### 6.2 プラン時にサニタイズし、警告を出す（開けなくはしない）

`PRODUCT_QUALITY.md`「音声・画像・外部ファイルが欠落してもドリル本体を開ける」の精神で、
**編集操作で普通に作れてしまう不整合は、拒否ではなく降格 + 警告**にする。

境界の原則: **`RouteTable::validate`（§6.1）が拒否した値は `plan` に到達しない。**
それでも `plan` は同じ状況を全てサニタイズする。理由は、`plan` が
「検証を通っていない `Document`」（マイグレーション途中、インポータの出力、テストの直書き）
に対しても呼ばれうるからで、`plan` がパニックしないことを検証の成否に依存させない。
検証済みの文書では下表の `GateClamped` と `NonFiniteSanitized` は**到達不能**であり、
その事実自体を §7.4 の S2 が検査する。

| 入力 | 対処 |
|---|---|
| `control` / `bulge` / `delay_counts` / `yaw_rad` が NaN・±Inf | 既定値へ置換。`NonFiniteSanitized` 警告。**評価器には非有限を渡さない**（NaN 座標は衝突走査と描画を汚染し、原因究明が極端に難しくなるので、境界で必ず止める）。 |
| `gate.depart > arrive` | `arrive = arrive.max(depart)`。等しい場合は `inv_span = 0.0` として「その瞬間に瞬間移動」と定義（0 除算なし）。`GateClamped` 警告。 |
| `gate` が `[0, moves]` の外 | クランプ。`GateClamped` 警告。**`Edit::SetCounts` でセットを縮めた直後に起きうる**（下の注記）。 |
| `moves == 0` | `inv_span = 0.0`。`local_count >= 0` で到着ドット。0 除算なし。 |
| 弦長 `< 1e-6`（開始 = 終了） | レーンをホールドに降格。`Curve` / `Arc` も 1 点に潰れる。`to_local` は `(0,0)` を返す。0 除算なし。 |
| `bulge` の絶対値 `< 1e-4` | `Straight` に降格（半径の発散を避ける）。 |
| `bulge` の絶対値 `> 8.0` | `±8.0` にクランプ（弦の 8 倍のサジッタ = フィールド外へ 800 yd 飛ぶ経路）。 |
| `Easing::Custom` の `c1` / `c2` が `[0,1]` 外、または `c1 > c2` | クランプして `c1 = c1.min(c2)`。単調性を型ではなくサニタイズで保証する。 |
| `Ramp` の `in_counts + out_counts > W` | 比例縮小して `= W`（三角形プロファイル）。負値は 0 にクランプ。 |
| フォローの自己参照・循環 | 循環上の全レーンを `Straight` に降格。`FollowCycle` 警告。 |
| フォロー鎖の深さ > 16 | 降格。`FollowTooDeep` 警告。 |
| 未知のリーダー ID（編集で演者を削除した後） | 降格。`FollowUnknownLeader` 警告。 |
| `Stagger` の `step_counts` × 順位が `max_shift` 超過 | クランプ。 |
| `FacingKey` の `at` が範囲外・未ソート・重複 | クランプ → 安定ソート → 同一 `at` は最後のキーを採用。 |

#### `Edit::SetCounts` でセットを縮めるとゲートが範囲外になる（文書10 との合わせ技の欠陥）

文書10 の `SetCounts` の precheck は `moves + hold <= MAX_SET_COUNTS` と
タイムライン総和しか見ない。ルート表は見ない。したがって

```
moves = 16, gate.arrive = Some(12.0)  →  Edit::SetCounts { moves: 8 }
```

を適用すると、その瞬間に `Document::validate` の V16（`arrive <= moves`）が**失敗する文書**が
出来上がる。保存しようとして初めてエラーになる、という最悪の壊れ方をする。

対処は文書10 が `MovePoints` + `SetShape{None}` について既に採っている方式に揃える
（文書10 §「`MovePoints` は `Set.shape` に触れない」および同 §9 の 14 との合意）。
すなわち **`Edit` 側を賢くせず、アプリが `Batch` を組む義務を負う**。

```rust
// The app builds this whenever it shrinks a set.
let mut edits = vec![Edit::SetCounts(CountsChange { set, counts: new_counts })];
if let Some(clamped) = doc.set(set)?.routes.clamped_to(new_counts) {
    edits.push(Edit::SetRoutes(Box::new(SetRoutesChange { set, routes: clamped })));
}
let edit = Edit::Batch(edits);
```

この形にする利点が 3 つある。(1) `Batch` の逆操作は逆順の逆操作なので、
Undo 1 回で `moves` とゲートが**同時に**元へ戻る。(2) `SetCounts` の precheck に
ルート検証を足す案は、ユーザーがセットを短くできなくなるので却下。
(3) クランプが「勝手に起きた」のではなく履歴に 1 エントリとして残るので、
何が変わったかを UI が説明できる。

本書は `RouteTable::clamped_to`（§3.7）を供給し、文書43 に「セット長を縮める操作は
必ずこの `Batch` を出す」ことを要求する。回帰テストは §7.4 の S5。

### 6.3 パニック禁止

- 添字は全て `get` / `get_mut` か、`lanes.len() == performers.len()` を
  `plan_transition` の出口で `debug_assert!` + リリースでは `min` クランプ、の二段構えにする。
- `plan.tables[lane.table]` は `plan_transition` が構築した索引しか入らない。
  `NO_TABLE = u16::MAX` を番兵にし、`LaneKind` が `Curve` / `Path` のときだけ参照する。
  `tables.len() <= u16::MAX` を構築時に検証（超えたら追加の表を作らず既存を再利用）。
- 除算は全て 0 除算を事前に潰す（6.2）。`sqrt` の引数は必ず非負。
- 整数はすべて `saturating_*`。`timeline_counts` は 256 × 131,070 = 33.5 M で u32 に収まる。
- `eval` は `#[inline]` かつパニック経路を持たないので、リリースビルドで
  境界チェックが除去されることをベンチで確認する。

### 6.4 データ喪失防止と履歴のサイズ（文書10 の未決 U2 への回答）

ルート情報は文書10 の `Edit::SetRoutes(Box<SetRoutesChange>)` を通してのみ変更する。
`SetRoutesChange { set: SetId, routes: RouteTable }` は**テーブル丸ごとの差し替え**で、
`apply` が旧テーブルを載せた逆操作を返すので完全可逆。`RouteTable` は
`Clone + PartialEq` なのでこれがそのまま成立する。

文書10 の未決 **U2**（「テーブル丸ごと差し替えか、override 単位か」）への回答は
**丸ごと差し替えのままでよい**。根拠は 3 つ。

1. **実運用の `overrides` は 1,000 件にならない。** §3.7 の見積りでは 1 セットあたり
   数十件で、1 エントリ 120 B として 6 KB / 履歴エントリ。文書10 が想定した
   48 KB / 編集という数字は、1,000 人全員に個別ルートを与えた場合にしか起きない。
2. **その 1,000 人ケースを作る主犯だったスタガーが、規則として持たれている。**
   §3.8 の `Stagger` は `RouteTable` に**オーバーライドを 1 件も生やさない**。
   「64 人を 1 カウントずつずらす」という最も頻繁な一括操作が、
   `Option<Stagger>` 1 個（32 B）の差分にしかならない。
   override 単位の `Edit` が必要になる主要動機がこれで消える。
3. **文書10 の履歴は既にバイト予算制。** `Entry` が `heap_bytes` を持ち
   （文書10 §3.6）、`History` は件数ではなくバイト数で古いエントリを捨てる。
   病的な 110 KB / エントリのケースは、件数上限ではなくバイト上限が自然に処理する。
   本書は `Edit::SetRoutes` の `heap_bytes()` が
   **`BTreeMap` のノード実体と `Vec<ChordPoint>` / `Vec<FacingKey>` の中身まで数える**
   ことを要求する（`size_of::<RouteTable>()` だけを返すと予算が効かない）。

ただし 1 点、文書10 に**追加を依頼する**ものがある。`CoalesceKey` に
**`Routes(SetId)` が無い**。ルートの制御点ハンドルをドラッグすると
毎フレーム `Edit::SetRoutes` が出るので、2 秒のドラッグで 120 エントリ積まれる。
これは文書10 が `CoalesceKey::ApplyShape(SetId)` を追加した理由
（「2 秒の半径ドラッグが 12 KB × 120 エントリを積む」）と**同一の状況**である。

```rust
pub enum CoalesceKey {
    // ...
    /// Dragging a route control handle re-samples every frame, exactly like
    /// `ApplyShape`. Without this, a two-second curve drag pushes 120 entries.
    Routes(SetId),
}
```

`Edit::SetCounts(CountsChange)` は既に `CoalesceKey::Counts(SetId)` を持っているので
追加不要。§6.2 の `Batch[SetCounts, SetRoutes]` は `Batch` なので合流しない
（文書10 の合流条件は単一変異が対象）。これは正しい挙動で、
セット長の変更は離散操作として 1 エントリ残るべきである。

## 7. テスト計画

すべて追加依存なしで書ける（ゴールデンは `.expected` の文字列比較、property は決定的な
パラメータ掃引）。

### 7.1 単体

| # | 内容 |
|---|---|
| U1 | `ChordFrame::to_field(1.0, 0.0) == end`、`to_field(0.0, 0.0) == start`（厳密一致） |
| U2 | `to_local(to_field(a, l)) ≈ (a, l)`、退化フレームで `(0,0)` を返し 0 除算しない |
| U3 | ゲート境界: `depart` 直前で開始ドット、`arrive` 直後で到着ドット、`depart == arrive` で瞬間移動 |
| U4 | ホールド: `local_count ∈ [moves, total]` で全演者が到着ドット |
| U5 | `Ramp`: `e(0)=0`、`e(1)=1`、`τ=a` で両式が一致、`a=b=0` が `Linear` と厳密一致、`a+b>W` で三角形化 |
| U6 | `Easing::Custom` の単調性境界: `c1=1, c2=0` を与えると `c1<=c2` にサニタイズされる |
| U7 | `Arc`: `bulge=0.5` の弧長が半円 `π·c/2` と 1e-4 で一致、`bulge→0` で弦長へ収束 |
| U8 | `ArcTable::t_at` の単調性と端点（`t_at(0)=0`, `t_at(1)=1`）、`e` が範囲外でもクランプ |
| U9 | `Path::Relative` の弧長表が `shapes::polyline` の累積長と一致（既存実装との整合） |
| U10 | `SetCounts::total` と新 `timeline_counts` / `locate_count` / `global_count` の既存期待値 4 件 |
| U11 | `march_mode`: 前進 / 後退 / スライド / 静止の 4 象限 |
| U12 | `Facing::Keyframed` の最短弧補間: 350° → 10° が 20° の回転になる |
| U13 | `size_of::<Lane>() <= 80` のコンパイル時アサーション |
| U14 | `Gate` の JSON 往復: `arrive: None` が `null`、`Some(12.0)` が `12.0`。**`f32::INFINITY` を含む `Gate` を構築できないことを型で確認**（`Option<f32>` なので構造的に不可能） |
| U15 | `RouteTable::validate`: §6.1 の各行につき 1 ケース。文書10 の V16 が要求する 3 項目（override キー実在 / gate 範囲 / 制御点有限）を必ず含む |
| U16 | `RouteTable::clamped_to`: 縮めた `moves` に対し `arrive` がクランプされる、変化が無ければ `None` を返す |
| U17 | `TempoMap::seconds_at_f64` / `count_at_f64`: 既存 11 テストと同じ入力で §3.12.3 の表の値になる（1 テストずつ f64 版を追加、f32 版はそのまま残す） |
| U18 | 前置和の再構築: `set` / `remove` / `from_changes` の後、および **JSON 逆直列化の直後**に `seconds_at_f64` が正しい（`#[serde(from)]` が効いていることの検査。`skip` + `default` にすると落ちる） |
| U19 | `TempoMap` の直列化形が変わっていないこと（`TempoMapRepr` 経由でも既存 JSON がそのまま読め、書き出しもバイト一致） |

### 7.2 property（決定的掃引・追加クレート不要）

| # | 内容 |
|---|---|
| P1 | 全 `Easing` 変異 × パラメータ 64 点 × `u` 256 点で `e` が単調、`[0,1]` に収まる |
| P2 | 全 `RouteShape` 変異について端点一致（不変条件 1） |
| P3 | 弧長等速: `Linear` + `FULL` で連続サンプル間距離のばらつきが `L·1e-3` 以内（不変条件 3） |
| P4 | 剛体変換不変: ランダムな回転 + 並進 128 通り（固定シードの線形合同法で生成）で不変条件 4 |
| P5 | `count_at` 往復: `locate_count(global_count(i, c)) == (i, c)`（不変条件 11） |
| P6 | 駆動系一致: `positions_at_count` と `eval(plan)` が全カウント半刻みでビット同一（不変条件 7） |
| P7 | ランダムアクセス一致: 0→total の順走査と、同じカウント列をシャッフルした順で評価した結果がビット同一（累加していないことの検査） |
| P8 | `positions_at`（旧 API）と `positions_at_count(p × total)` の一致 |
| P9 | **型境界の一致**（不変条件 15）。全セット境界 `c` について `c`、`c ± 1ulp(f32)`、`c ± 1ulp(f64)` の 5 点で `locate_count(x as f32).0 == locate_count_f64(x).0` を検査。一致しない `x` があれば、それは f32 が境界を表現できない領域（`MAX_TIMELINE_COUNTS` 近傍）であり、テストは**その領域の下限を報告して失敗する**（黙って許容しない） |
| P10 | **テンポ往復**（不変条件 13）。`c` を 0..`MAX_TIMELINE_COUNTS` の対数刻み 256 点、テンポ事象 0/1/3/64 個の 4 構成で `count_at_f64(seconds_at_f64(c))` が相対 1e-12 以内 |
| P11 | **ラッパ無害性**（不変条件 14）。`seconds_at(c)` が `seconds_at_f64(c as f64) as f32` と厳密一致、かつ**旧 f32 実装より真値に近いか同等**（真値は `f64` の前置和を基準とする）。旧実装をテスト内に複製して比較する |
| P12 | サンプル精度: 480 秒・960 秒・16 分のショーで、`seconds_at_f64` から求めたサンプル索引 `round(t * 48000)` が、カウントを 1 ずつ進めたときに**単調増加し、隣接カウント間で重複しない**（f32 経路では 480 秒付近で重複が出ることを、対照テストとして同時に示す） |

### 7.3 ゴールデン

`tests/golden/transition_shapes.expected` に、6 形状 × 4 イージング × 3 ゲートの
組み合わせを持つフィクスチャ文書の、全半カウントにおける全演者座標を
固定書式（`{:.4}`）でダンプして比較する。`Arc` と facing を含む行は
1e-4 の許容差で比較するヘルパを通す（5.5 の超越関数の注記）。

`svg.rs` / CSV 出力のゴールデン（C-2）が同じフィクスチャを使うことで、
「同じドキュメント座標を参照する」を出力側でも固定する。

### 7.4 ストレス・敵性入力

| # | 内容 |
|---|---|
| S1 | 1,000 人 × 256 セット、全演者オーバーライド。`plan` → `eval` を 10,000 回。2 回目以降 `capacity` 不変 |
| S2 | 敵性ルート表（表駆動）: NaN control / ±Inf bulge / `delay = -1e30` / `depart = 1e30` / 自己フォロー / 3 循環 / 深さ 100 の鎖 / 10⁶ 点の `via`。**期待: パニックしない、非有限を出力しない、拒否か降格のいずれか** |
| S3 | 10,000 回の `Edit::SetRoutes` 適用 → 全 Undo → `RouteTable` が初期値と `PartialEq` で一致 |
| S4 | 2 時間相当（432,000 フレーム）の `eval` 後に `plan` と `out` の `capacity` が不変 |
| S5 | **セット短縮の往復**（§6.2）。`moves: 16, arrive: Some(12)` のセットに `Batch[SetCounts{moves:8}, SetRoutes{clamped}]` を適用 → `Document::validate()` が `Ok`（V16 を通る）→ Undo 1 回 → 元の `counts` と `routes` に完全復帰。`Batch` を使わず `SetCounts` 単独で適用した場合に `validate()` が失敗することも同時に固定し、アプリの義務を明文化する |
| S6 | 検証済み文書に対する `plan` は `PlanWarning::GateClamped` と `NonFiniteSanitized` を**1 件も出さない**（§6.2 冒頭の主張の検査）。未検証の敵性文書に対しては出る |

### 7.5 ベンチ

`crates/drill-core/benches/core_performance.rs` に追加する（既存の 2 項目は残す）。

```
1,000 performers × 60,000 frames, all Straight+Linear   … 現行との回帰検出
1,000 performers × 60,000 frames, all Curve             … 目標 < 300 ms 合計（5 µs/フレーム）
1,000 performers × 60,000 frames, all Arc               … 目標 < 800 ms 合計
1,000 performers × 60,000 frames, all FollowTrail depth 1
1,000 performers × 10,000 plan rebuilds (stagger ByStartX) … 目標 < 400 ms 合計（40 µs/回）
TempoMap::seconds_at_f64 × 1,000,000 (64 tempo events)  … 目標 < 30 ms 合計（30 ns/回）
   対照として現行の f32 線形走査も測る。前置和 + 二分探索が遅くなっていないことの回帰検出
```

## 8. 実装タスク

1 タスク = 1〜3 時間。`⇐` は依存。

### 前提（本書の範囲外だが着手前に必要）

- **T0**: 文書10 の `Revisions` / `Scopes` / `SetScope` / `CacheKey` 群と
  `Edit::SetRoutes` / `Edit::SetCounts`。これが無いとプラン無効化が場当たりになる。
  **直列で先に必要。** あわせて文書10 へ 2 件の追加を依頼する（§9-8）:
  `CacheKey` に `PlanKey`、`CoalesceKey` に `Routes(SetId)`。

### 並行第1波（相互依存なし、4 本同時可）

| # | 内容 | 依存 |
|---|---|---|
| **T1** | `SetCounts` 型 + `total()` + `timeline_counts` / `locate_count_f64` / `global_count_f64` と f32 ラッパ。`countsheet.rs` / `svg.rs` / `coordinates.rs` / `main.rs` の呼び出し追従。既存テスト 4 件が通ることを確認 | — |
| **T1b** | **`TempoMap` の f64 化**（§3.12.2 / §3.12.3）。`starts` / `prefix` の前置和、`#[serde(from/into)]` による再構築、`seconds_at_f64` / `count_at_f64`、既存 f32 API のラッパ化。U17・U18・U19・P10・P11・P12。**文書30 の T17 がこれを待っている** | — |
| **T2** | `transition.rs` 新設。`Easing` + `EasingCoeffs` + `apply`。U5・U6・P1 | — |
| **T3** | `ChordFrame` + `ChordPoint`。`Straight` / `Arc` の解析評価と弧長。U1・U2・U7 | — |

### 並行第2波

| # | 内容 | 依存 |
|---|---|---|
| **T4** | `ArcTable` の `build` / `build_polyline` / `t_at` / `length`。`Curve` の評価。U8・P3 と精度テスト（3.3.2 の表を検証する数値テスト） | ⇐ T3 |
| **T5** | `PathVia` + `Path` の評価。`shapes::polyline` との整合テスト U9 | ⇐ T3, T4 |
| **T6** | `Gate`（`arrive: Option<f32>`）+ `Route` + `RouteTable` + serde（`skip_serializing_if` 3 階層）+ `is_trivial` + **`validate`（文書10 V16 の実体）** + `clamped_to`。JSON サイズの回帰テスト（既定表が 0 バイトになること）。U14・U15・U16 | ⇐ T2, T3 |

### 直列（プランがボトルネック）

| # | 内容 | 依存 |
|---|---|---|
| **T7** | `TransitionPlan` + `Lane` + `PlanKey` + `plan_transition`。弦フレーム構築、ルート解決、表の重複排除、サニタイズ、`PlanWarning`。確保再利用。S6 | ⇐ T4, T5, T6 |
| **T8** | `eval_lane` カーネル + `eval` + `Document::positions_at_count` + `positions_at` 互換ラッパ + ホールドの memcpy 高速路 + `trivial` 高速路。U3・U4・P2・P6・P8 | ⇐ T7 |

### 並行第3波

| # | 内容 | 依存 |
|---|---|---|
| **T9** | `Stagger` + 順位付け（タイブレーク必須）+ プランへの適用。不変条件 12 | ⇐ T7 |
| **T10** | フォロー解決: 索引化・循環検出・平坦化・残差検査。不変条件 9、S2 の循環系 | ⇐ T7, T8 |
| **T11** | `Facing` + `eval_facing` + `facings_at_count` + `march_mode`。U11・U12 | ⇐ T8 |
| **T12** | `route_length`。`pathing.rs` / `continuity.rs` へ供給する境界だけ作る（文言の変更はしない） | ⇐ T8 |

### 仕上げ

| # | 内容 | 依存 |
|---|---|---|
| **T13** | 敵性入力テーブル S2 + 検証（6.1 の上限）+ `DrillError` 変異の追加 | ⇐ T7〜T11 |
| **T14** | ゴールデン `transition_shapes.expected` + P4・P5・P7・P9 + S1・S3・S4・S5 | ⇐ T7〜T12 |
| **T15** | ベンチ 6 項目の追加と `PRODUCT_QUALITY.md` のベースライン再計測 | ⇐ T14 |
| **T16** | v1→v2 写像への追記: `counts → SetCounts{moves, hold:0}`、`routes: RouteTable::default()`（**easing は明示的に `Linear`**、gate は `Gate::FULL`）。文書41 のマイグレーションに同梱 | ⇐ T1, T6 |
| **T17** | `drill-app` 統合: `TransitionPlan` を状態に持ち `PlanKey` で無効化、[main.rs:510-515](../../crates/drill-app/src/main.rs) の progress 往復を撤去、セット短縮時に `Batch[SetCounts, SetRoutes]` を出す（§6.2）、`PlanWarning` を色以外（記号 + 文言）で表示 | ⇐ T8, T10 |

**クリティカルパス**: T0 → (T3) → T4 → T7 → T8 → T17。
T1 / T1b / T2 / T9 / T11 / T12 は並行で消化できる。
**T1b は文書30 の T17 をブロックしている**ので、第1波の中でも優先度を上げる。

## 9. 未決事項

1. **ホールド中の `Facing::Motion` の既定値。** 現案は「弦長 0 のレーンは yaw = 0（正面）」。
   実務では「直前のセットの到着時の向きを保つ」方が自然な場面が多いが、それはセットを
   跨いだ依存になり、`plan` の「1 セット独立」という前提を壊す。
   決めるのに必要な情報: ドリルデザイナー数名に、8 カウントのホールド中に演者が
   正面へ向き直るのが既定として妥当かを確認する。代替案は
   `Set` に `arrival_facing: Option<f32>` を持たせて明示させること。

2. **`Arc` のクロスプラットフォーム・ビット一致。** `sin` / `cos` が libm 依存なので、
   x86-64 で書き出した動画と aarch64 で書き出した動画が完全一致しない。
   一致が要件になるなら自前の多項式近似（Payne–Hanek 不要な範囲に限定できるので
   実現可能）を入れる。決めるのに必要な情報: 動画書き出し（文書31）が
   「別マシンで再現してバイト一致を検証する」CI を持つかどうか。

3. **`FollowTrail` の到着ドット残差の扱い。** 現案は警告のみ。
   「到着ドットへ最後の 2 カウントで寄せる」自動補正を入れるべきかは、
   実際のショーで軌跡追従がどれだけ使われるかに依存する。
   決めるのに必要な情報: 既存ショーのサンプルで、conga 的な動きの割合。

4. **3 次ベジェの追加。** 現案は 2 次のみ（制御点 1 個）。S 字カーブを 1 レーンで
   描くには 3 次が要るが、`Path::Relative` で経由点 2 個を置けば近い表現ができる。
   弧長表の枠組みは 3 次でもそのまま使える（`RESOLUTION` を 64 に上げる）。
   決めるのに必要な情報: UI（文書43）で制御点 2 個のハンドル操作が
   フィールド上で実用的に扱えるか。

5. **`locate_count` の二分探索化。** 256 セットの線形走査を毎フレーム回すのは
   0.5 µs 程度で無視できるが、セット開始カウントの累積和キャッシュは
   `Derived` を持つ側（文書10）に置くのが自然。所有者の確定が必要。
   本書は `locate_count_f64` のシグネチャと「境界判定は整数の累積和で行う」ことだけを固定した。

6. **`Stagger` と衝突走査（文書13）の相互作用。** スタガーは出発を遅らせるので、
   同じ経路上を時間差で通ることになり、静的な間隔検査では捕まらない衝突が減る一方、
   `DelayDepart` は移動窓を圧縮して歩幅を上げる。文書12 の歩幅評価がゲート幅を
   参照することは本書で決めたが、警告の閾値をスタガー適用後の実効ゲートで見るのか
   公称ゲートで見るのかは文書12 側の判断。

7. **`playback::advance` の f64 化。** 現行は秒もカウントも `f32`
   （[playback.rs:44-81](../../crates/drill-core/src/playback.rs)）。§3.12.1 の検算により、
   8 分を超えるショーでは再生位置が 1.5 サンプル以上の粒度になり、
   文書30 の `PlaybackClock`（サンプル索引が正）と食い違う。
   文書30 §3.7 の `tempo.count_at(self.position_seconds(now, speed) as f32)` も
   `count_at_f64` を使うべきである。本書は f64 API を**供給する側**なので、
   `advance` のシグネチャ変更（`playback.rs` の所有は文書43 / 30 のどちらか）と
   `PlaybackRange` の型を誰が変えるかの確定が必要。
   決めるのに必要な情報: `playback.rs` の所有者。ここが未確定のまま両者が
   別々に f64 化すると、§3.12.5 の「型境界は 1 か所」という不変条件が崩れる。

8. **文書10 への 2 件の追加依頼。** どちらも本書だけでは決められない。
   (a) `revision.rs` に 8 種目の `CacheKey` として `PlanKey`（§3.9）。
   代案は `TransitionKey` の流用だが、最後のセットで `None` を返す点と
   `grid` / `style` による過剰無効化をどう扱うかの判断が要る。
   (b) `CoalesceKey::Routes(SetId)`（§6.4）。これが無いとルートハンドルのドラッグが
   履歴を埋める。文書10 が `ApplyShape(SetId)` を足したのと同一の理由。
   決めるのに必要な情報: 文書10 の担当の可否判断のみ。技術的な障害は無い。

9. **`TempoMap` を内部まで f64 にするか。** 本書は
   `TempoChange { count: f32, bpm: f32 }` という**保存形は変えず**、
   前置和と問い合わせだけを f64 にした（直列化互換のため）。
   文書30 の未決 4 が「内部を全部 f64 にすべきか」を挙げている。
   `count: f32` のままだと、テンポ事象を `MAX_TIMELINE_COUNTS` 近傍に置いたとき
   アンカー位置そのものが 0.031 カウント刻みになる。実害があるのは 30 分超のショーで
   終盤にテンポ変化を置く場合に限られる。
   決めるのに必要な情報: `TempoChange.count` を f64 にするスキーマ変更を
   v2 に含めるか v3 送りにするか（文書41 の判断）。v2 に入れるなら**今**やるべきで、
   利用者の v2 ファイルが増えてからでは遅い。
