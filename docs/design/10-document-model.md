# 10. ドキュメントモデルと編集コマンド代数

## 1. 目的と範囲

`Document` の同一性・変更・履歴・整合性・永続互換を、他の全設計文書が参照できる形で確定する。

**この文書が決めること**

1. 安定ID体系（`SetId` / `PerformerId` / `SectionId`）と索引ヘルパ
2. 編集コマンド代数 `Edit` と、その逆操作・合成・合流（coalesce）規則
3. `History`（undo/redo・上限・メモリ会計）
4. `Revision` によるキャッシュ無効化の粒度
5. `Document::validate()` の全項目と `debug_assert_invariants()`
6. スキーマ v2 とマイグレーション連鎖、信頼できない JSON への上限
7. `DESIGN_GAPS.md` §0 の #1 / #2 / #5 が構造的に消える理由

**この文書が決めないこと**（型を参照するだけで、意味論は各担当文書が正）

| 参照する型 | 意味論の所有者 |
|---|---|
| `Route` / `RouteTable` / `Gate` / `SetCounts` / `positions_at_count` | 11. 遷移モデル |
| `StepStyle` / `ClinicParams` | 12. マーチングスタイル |
| `ScanScratch` / `CollisionEvent` | 13. 解析エンジン |
| `ShapeSpec` / `ShapeAssignment` / `Shape::sample` / `assign_to_shape` | 14. フォーメーションとシェイプ |
| `Section` / `Symbol` / `PerformerKind` / `Subset` / `Selection` / `NumberingScheme` の運用 | 15. 演者・セクション |
| `HashKind` と表示名解決 | 16. 座標系と記譜規約 |
| `DisplayList` のキャッシュ実装 | 20. DisplayList |
| `CameraProgram` / `CameraTrack` / `CameraKeyframe` / `CameraCut` / `CameraId` / `LookMode` / `BoundsMode` | 23. カメラシステム |
| `SyncAnchor` | 30. 音声エンジン |
| `.drillproj` コンテナ・原子的置換・相対アセットパス | 41. 永続化 |
| `DrillError` の全変異と `Locale` 解決 | 42. エラー型と i18n |

本書は `DrillError` の**変異を追加要求**するが、`message(&self, locale)` の実装は 42 が持つ。

### 1.1 参照側文書から本書へ持ち込まれた永続化状態

14 / 15 / 23 は完成済みで、いずれも `Document` への永続化を前提にしている。本書はその 3 つを
取り込み、本書の方針（ID の newtype 化・payload の `Box` 化・`apply` が逆操作を返す・合流規則・
revision スコープ）に揃えた形で確定させる。

| 出典 | 持ち込む状態 | 本書での置き場所 |
|---|---|---|
| 15 §3.2, §3.6 | `Document.subsets: Vec<Subset>`（名前付き選択集合） | §3.2 / `Edit` 4 変異（§3.3.3.2） |
| 15 §3.4, §3.9 | 一括採番・再採番 | `Edit::RelabelPerformers`（§3.3.3） |
| 14 §3.4 | `Set.shape: Option<ShapeAssignment>`（図形を編集可能なオブジェクトとして残す） | §3.2 / `Edit::ApplyShape` と `Edit::SetShape`（§3.3.3.1） |
| 23 §3.12 | `Document.camera_program: CameraProgram` | §3.2 / `Edit` 10 変異（§3.3.3.3） |

### 1.2 参照側の型に本書が要求する派生

本書の不変条件 I-14（10,000 編集 Undo の内容一致）は `Document` の `#[derive(PartialEq)]` に依存する。
`Document` に到達可能な**全ての型が `PartialEq` を導出していなければならない**。23 の
`CameraProgram` / `CameraTrack` / `LookMode` / `FollowTarget` / `FollowDamping`、15 の `Subset`、
14 の `ShapeAssignment` / `ShapeSpec` は現状の設計文書で `PartialEq` を導出していないので、
実装時に追加する（意味論は変わらない、純粋な derive の追加）。

同様に、`Edit` の payload に入る型は `Clone + Debug + PartialEq` を要求する。
`Edit` 自体に `Serialize` は要求しない（§9 U4）。

配置クレートは `drill-core`（依存は `serde` / `serde_json` のみ、`std` 可）。

---

## 2. 現状

### 2.1 ID と索引

- [lib.rs:19](../../crates/drill-core/src/lib.rs#L19) `pub type PerformerId = u32;` は **型エイリアス**。newtype ではないので `usize` の索引・`u32` の ID・`u32` のカウント値がすべて相互代入可能。コンパイラは混同を検出しない。
- [lib.rs:232-238](../../crates/drill-core/src/lib.rs#L232) `Set { name, counts, positions }` に **ID フィールドが無い**。セットの同一性を表す手段が索引しかない。
- `SectionId` に相当する概念は存在しない。[lib.rs:225-230](../../crates/drill-core/src/lib.rs#L225) `Performer { id, label, color }` のみ。
- id → index の索引ヘルパは無い。`Document` に `set_index()` / `set()` は存在しない。

### 2.2 変更経路

- [lib.rs:406-412](../../crates/drill-core/src/lib.rs#L406) `MoveCommand { set_index: usize, performer_indices: Vec<usize>, before, after }`。**セットも演者も索引で指している**。
- [lib.rs:465-478](../../crates/drill-core/src/lib.rs#L465) `MoveCommand::apply` は `get_mut(...)` が `None` のとき **黙って何もしない**。履歴の指す先がズレても失敗が観測できない。
- 座標移動以外の編集コマンドは存在しない。セット挿入・名前変更・カウント変更・グリッド変更・テンポ変更・音源変更はすべて `Document` のフィールドを直接書き換えている。
- `Document` の全フィールドが `pub`（[lib.rs:240-252](../../crates/drill-core/src/lib.rs#L240)）。UI から任意に書き換えられる。`ARCHITECTURE.md` の「編集コマンドを通す」は現状コードでは強制されていない。

`drill-app` 側の変更経路の実測（`history` に積んでいるか）:

| 箇所 | 操作 | `history.push` | 備考 |
|---|---|---|---|
| [main.rs:169-188](../../crates/drill-app/src/main.rs#L169) `commit_layout` | 整列・分布・回転・拡縮 | あり | 索引ベース |
| [main.rs:264-289](../../crates/drill-app/src/main.rs#L264) `auto_assign_next` | 自動割り当て | あり | 索引ベース |
| [main.rs:1577-1595](../../crates/drill-app/src/main.rs#L1577) `drag_stopped` | ドラッグ移動 | あり | ドラッグ中([main.rs:1562-1576](../../crates/drill-app/src/main.rs#L1562))は直接 `positions[index] = ...` |
| [main.rs:828-841](../../crates/drill-app/src/main.rs#L828) | **セット複製** | **なし** | `DESIGN_GAPS.md` §0 #1 |
| [main.rs:1043-1132](../../crates/drill-app/src/main.rs#L1043) | グリッドデザイナー全項目 | なし | `&mut self.document.grid` を UI に直接渡している |
| [main.rs:1135-1192](../../crates/drill-app/src/main.rs#L1135) | テンポマップ編集 | なし | `tempo.set/remove` を直接呼ぶ |
| [main.rs:1395-1416](../../crates/drill-app/src/main.rs#L1395) | 音源の設定・解除 | なし | `self.document.audio = ...` |
| [main.rs:846](../../crates/drill-app/src/main.rs#L846) 付近 | セット counts 表示 | — | counts の編集 UI は未実装 |

演者の追加・削除・セクション割り当ての UI は存在しない。

`dirty` は [main.rs:90](../../crates/drill-app/src/main.rs#L90) の `bool` を各所で手動 `= true` している（21 箇所）。履歴位置と連動していないため、Undo で元に戻しても `dirty` は落ちない。

### 2.3 History

- [lib.rs:414-463](../../crates/drill-core/src/lib.rs#L414) `History { commands: Vec<MoveCommand>, cursor: usize, limit: usize }`。
- [lib.rs:433](../../crates/drill-core/src/lib.rs#L433) `self.commands.remove(0)` は O(n)。`limit` は [main.rs:121](../../crates/drill-app/src/main.rs#L121) で 500。`MoveCommand` は inline 約 80 バイトなので、500 件到達後は毎編集 40 KB の memmove。`DESIGN_GAPS.md` §0 #5。
- `MoveCommand` 一種類しか保持できない。他の編集は履歴に載らない。
- 保存点（savepoint）の概念が無い。

### 2.4 revision / キャッシュ無効化

`document_revision` に相当するフィールド・型・関数は**リポジトリに存在しない**（`drill-core` / `drill-app` 全体を検索して 0 件）。結果として [main.rs:1011](../../crates/drill-app/src/main.rs#L1011) 付近の `analyze_transition` は無条件に毎フレーム呼ばれている（`DESIGN_GAPS.md` §0 #3）。

### 2.5 validate とスキーマ

- [lib.rs:302-333](../../crates/drill-core/src/lib.rs#L302) `validate()` の検査は 5 項目のみ:
  1. `schema_version != 1` を拒否
  2. `sets` が空でない
  3. `grid.width > 0 && grid.height > 0`
  4. 全セットの `positions.len()` が `performers.len()` と一致
  5. `performer.id` の重複なし
- 返り値は `Result<(), String>` で、メッセージは日本語リテラル（[lib.rs:304](../../crates/drill-core/src/lib.rs#L304), [310](../../crates/drill-core/src/lib.rs#L310), [313](../../crates/drill-core/src/lib.rs#L313), [322](../../crates/drill-core/src/lib.rs#L322), [330](../../crates/drill-core/src/lib.rs#L330)）。`GridConfig::default()` も日本語ラベルを埋めている（[lib.rs:76](../../crates/drill-core/src/lib.rs#L76), [81](../../crates/drill-core/src/lib.rs#L81)）。同種の `Result<_, String>` は `audio.rs:65`、`video.rs:103` にもある。
- **NaN / Inf の検査が無い。** `positions`・`grid.width`・`tempo.bpm` に非有限値が入った JSON を素通しする。
- **要素数の上限が無い。** `performers` / `sets` / `hashes` は無制限。
- `migrate` は存在しない。将来形式も過去形式も同じ「未対応」エラーになる。
- `schema_version` は `Document` のフィールドで、`Document::demo` が `1` を直書きしている（[lib.rs:281](../../crates/drill-core/src/lib.rs#L281)）。定数 `SCHEMA_VERSION` は無い。

### 2.6 メモリレイアウト

`Set::positions` は既に `Vec<Point>` の密配列で、`Document::performers` と索引整列している（[lib.rs:236-237](../../crates/drill-core/src/lib.rs#L236) の doc comment がその意図を明記）。`Point` は `{ x: f32, y: f32 }`（[lib.rs:130-134](../../crates/drill-core/src/lib.rs#L130)）だが `#[repr(C)]` は付いていない。

---

## 3. 設計

### 3.0 モジュール構成

```
crates/drill-core/src/document/
  mod.rs         Document, Set, Performer, Section, DocumentBuilder, 公開再エクスポート
  ids.rs         SetId / PerformerId / SectionId / IdAllocator / IdIndex
  revision.rs    Revision / Revisions / Scopes / SetScope / 各 CacheKey
  edit.rs        Edit と全 payload 構造体、precheck / apply
  history.rs     History / Entry / GestureId / PushOutcome
  validate.rs    Document::validate / limits 定数 / debug_assert_invariants
  fingerprint.rs Document::fingerprint
  migrate/
    mod.rs       migrate() 連鎖と MIGRATIONS 表
    v1_to_v2.rs  v1 → v2 の写像（v1 の日本語ラベル照合を封じ込める唯一の場所）
```

`Document` の可変フィールドは `pub(in crate::document)`。`document` モジュール外（`svg.rs` / `coordinates.rs` / `drill-app` など）からは**参照アクセサ経由でしか読めず、`Edit` 経由でしか書けない**。これが「不変条件 1: `Document` の変更は必ず `Edit` を通る」の型による強制である。

`lib.rs` は `pub use document::*;` で再エクスポートし、既存の `drill_core::Document` などのパスを維持する。

---

### 3.1 安定ID体系

#### 3.1.1 newtype

```rust
use core::num::NonZeroU32;
use serde::{Deserialize, Serialize};

macro_rules! stable_id {
    ($name:ident, $what:literal) => {
        #[doc = concat!("Stable identity of a ", $what, ". Never reused within a document.")]
        ///
        /// Ordering follows allocation order, so `Ord` is a stable tiebreaker but is
        /// **not** presentation order. Presentation order is the `Vec` index.
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(NonZeroU32);

        impl $name {
            /// Returns `None` for `0`, which is reserved as the "absent" encoding.
            pub const fn new(raw: u32) -> Option<Self> {
                match NonZeroU32::new(raw) {
                    Some(v) => Some(Self(v)),
                    None => None,
                }
            }
            pub const fn get(self) -> u32 {
                self.0.get()
            }
        }
    };
}

stable_id!(SetId, "set");
stable_id!(PerformerId, "performer");
stable_id!(SectionId, "section");
stable_id!(SubsetId, "named selection subset");
```

`SubsetId` は 15 §3.6 が `pub type SubsetId = u32;`（エイリアス）として提案しているが、`PerformerId` と
同じ理由（索引との混同防止・`0` の拒否・ニッチ最適化）で newtype に格上げする。ID 体系は本書の担当範囲
なので、15 の意味論（「保存された選択集合の同一性」）はそのまま保ったうえで表現だけを揃える。

`CameraId` は 23 §3.3 が `pub struct CameraId(pub u32)` として定義済みで、**そのまま使う**。
本書の `IdIndex` は適用しない — カメラトラックは実用上 16 本以下で、23 §3.9 の
`tracks.iter().find(|t| t.id == id)` の線形探索（最大 16 比較 ≈ 10ns）が直接アドレス表より速い。
ただし 23 の `CameraProgram::alloc_camera_id` は `self.next_camera_id += 1` で溢れを検査していないため、
本書の `Edit::InsertCameraTrack` は `alloc_camera_id` を呼ばず、`checked_add` を経由する
`CameraProgram::try_alloc_camera_id(&mut self) -> Option<CameraId>` を要求する（§9 の報告事項）。

`NonZeroU32` を選ぶ理由は 2 つ。

1. **ニッチ最適化**: `Option<SetId>` が 4 バイトになる。`RouteTable` の `BTreeMap<PerformerId, _>` や `Edit` の payload で効く。
2. **敵性入力の検出**: JSON の `0` が deserialize 時点で失敗する。「未初期化の ID」が文書内に存在し得なくなる。

`PerformerId` は現在 `pub type PerformerId = u32;`（[lib.rs:19](../../crates/drill-core/src/lib.rs#L19)）なので、この変更は破壊的である。`u32` を受けていた全箇所がコンパイルエラーになる — それが狙いで、索引と ID の混同箇所を機械的に洗い出せる。

#### 3.1.2 採番

```rust
/// Monotonic id source. Ids are never reused, including after remove + undo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdAllocator {
    /// Next raw value to hand out. Always >= 1.
    next: u32,
}

impl Default for IdAllocator {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl IdAllocator {
    /// Smallest value that has never been handed out.
    pub const fn watermark(self) -> u32 {
        self.next
    }

    /// Raises the watermark so that every id in a loaded document is below it.
    /// Used by migration and by `Document::from_repr`.
    pub fn observe(&mut self, raw: u32) {
        if raw >= self.next {
            self.next = raw.saturating_add(1);
        }
    }

    fn bump(&mut self) -> Result<u32, DrillError> {
        let raw = self.next;
        self.next = raw.checked_add(1).ok_or(DrillError::IdSpaceExhausted)?;
        Ok(raw)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdAllocators {
    #[serde(default)]
    pub(in crate::document) set: IdAllocator,
    #[serde(default)]
    pub(in crate::document) performer: IdAllocator,
    #[serde(default)]
    pub(in crate::document) section: IdAllocator,
    #[serde(default)]
    pub(in crate::document) subset: IdAllocator,
}
```

`CameraId` の採番だけは `CameraProgram::next_camera_id`（23 §3.9）が持つ。`IdAllocators` に
持ち込まないのは、`CameraProgram` が `Default` で独立に構築されて serde 往復する自己完結型で、
採番状態をそこから引き剥がすと 23 の `alloc_camera_id` の契約が壊れるため。
「ID は再利用しない」規則（下記）はカメラにも同じく適用され、`RemoveCameraTrack` の逆操作は
元の `CameraId` を復元する。

採番規則（これが `Edit` の可逆性の基礎になる）:

- **ID は再利用しない。** `RemoveSet` は watermark を戻さない。したがって `RemoveSet` → `Undo`（= `InsertSet`）は**削除前と同じ `SetId`** を復元でき、履歴に残っている他の `Edit`（`SetRoutes { set: SetId }` など）の参照が生き返る。watermark を戻す設計では、削除後に別のセットが同じ ID を得て、Undo が別物を指してしまう。
- 採番は `Edit::apply` の内部でのみ行う。UI は ID を作れない。
- `IdAllocator` は文書に serialize される。ID 空間の連続性がファイルを跨いで保たれる。
- 枯渇（2^32 - 1 回の採番）は `DrillError::IdSpaceExhausted`。1 秒に 1,000 回採番しても 49 日かかるので実用上は到達しないが、`unwrap` しない。

#### 3.1.3 id → index（`IdIndex`）

index → id は `doc.sets()[i].id` / `doc.performers()[i].id` で O(1)、補助構造は不要。逆方向のために `IdIndex` を持つ。

ID は 1 から単調に採番されるので、生きている ID は通常 `[min_live, max_live]` の狭い区間に収まる。この性質を使って**直接アドレス表**を第一表現とし、区間が疎になった場合のみ二分探索へ退化する。

```rust
const NO_SLOT: u32 = u32::MAX;

/// Maps a stable id to its current `Vec` index.
///
/// Derived state: never serialized, rebuilt on load and on every topology change.
#[derive(Clone, Debug)]
enum IdIndexRepr {
    /// `slots[raw - base]` is the index, or `NO_SLOT`.
    Dense { base: u32, slots: Vec<u32> },
    /// Sorted by raw id. Used when the live id range is sparse.
    Sparse { sorted: Vec<(u32, u32)> },
}

#[derive(Clone, Debug)]
pub struct IdIndex(IdIndexRepr);

impl Default for IdIndex {
    fn default() -> Self {
        Self(IdIndexRepr::Dense { base: 1, slots: Vec::new() })
    }
}

impl IdIndex {
    /// Density guard: switch to `Sparse` when the direct table would waste
    /// more than 4x, and the span is non-trivial.
    const DENSITY_FACTOR: u32 = 4;
    const DENSE_FLOOR: u32 = 256;
    const DENSE_CEILING: u32 = 1 << 21; // 8 MiB table, hard stop

    /// Two passes over the ids, no intermediate allocation.
    pub fn rebuild(&mut self, len: usize, raw_at: impl Fn(usize) -> u32) {
        let mut min = u32::MAX;
        let mut max = 0u32;
        for i in 0..len {
            let raw = raw_at(i);
            min = min.min(raw);
            max = max.max(raw);
        }
        if len == 0 {
            *self = Self::default();
            return;
        }
        let span = max - min + 1;
        let budget = (len as u32)
            .saturating_mul(Self::DENSITY_FACTOR)
            .max(Self::DENSE_FLOOR);
        if span <= budget && span <= Self::DENSE_CEILING {
            let mut slots = match &mut self.0 {
                IdIndexRepr::Dense { slots, .. } => std::mem::take(slots),
                IdIndexRepr::Sparse { .. } => Vec::new(),
            };
            slots.clear();
            slots.resize(span as usize, NO_SLOT);
            for i in 0..len {
                slots[(raw_at(i) - min) as usize] = i as u32;
            }
            self.0 = IdIndexRepr::Dense { base: min, slots };
        } else {
            let mut sorted = Vec::with_capacity(len);
            for i in 0..len {
                sorted.push((raw_at(i), i as u32));
            }
            sorted.sort_unstable_by_key(|&(raw, _)| raw);
            self.0 = IdIndexRepr::Sparse { sorted };
        }
    }

    pub fn get(&self, raw: u32) -> Option<usize> {
        match &self.0 {
            IdIndexRepr::Dense { base, slots } => {
                let offset = raw.checked_sub(*base)? as usize;
                match slots.get(offset).copied() {
                    Some(NO_SLOT) | None => None,
                    Some(index) => Some(index as usize),
                }
            }
            IdIndexRepr::Sparse { sorted } => sorted
                .binary_search_by_key(&raw, |&(id, _)| id)
                .ok()
                .map(|k| sorted[k].1 as usize),
        }
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            IdIndexRepr::Dense { slots, .. } => slots.iter().filter(|&&s| s != NO_SLOT).count(),
            IdIndexRepr::Sparse { sorted } => sorted.len(),
        }
    }
}
```

**保守方針: 位相が変わる編集では丸ごと再構築する。** 索引に要素を挿入すると後続の全索引が 1 ずれるので、どのみち O(n) の更新が必要になる。1,000 人の再構築は 2 パス約 2µs で、`InsertPerformers` 本体（64 セット分の `Vec::insert`）より安い。座標移動・名前変更・グリッド変更では索引に触らない。

`Document` 側のアクセサ:

```rust
impl Document {
    pub fn set_index(&self, id: SetId) -> Option<usize> {
        self.derived.sets_by_id.get(id.get())
    }
    pub fn set(&self, id: SetId) -> Option<&Set> {
        self.sets.get(self.set_index(id)?)
    }
    pub fn set_id_at(&self, index: usize) -> Option<SetId> {
        self.sets.get(index).map(|s| s.id)
    }
    pub fn performer_index(&self, id: PerformerId) -> Option<usize>;
    pub fn performer(&self, id: PerformerId) -> Option<&Performer>;
    pub fn performer_id_at(&self, index: usize) -> Option<PerformerId>;
    pub fn section_index(&self, id: SectionId) -> Option<usize>;
    pub fn section(&self, id: SectionId) -> Option<&Section>;
    pub fn subset_index(&self, id: SubsetId) -> Option<usize>;
    pub fn subset(&self, id: SubsetId) -> Option<&Subset>;
    /// Linear scan over `subsets`; names are not unique and not indexed.
    /// Mirrors 15 §3.6's `subset_named`.
    pub fn subset_named(&self, name: &str) -> Option<&Subset>;
    /// Linear scan over `camera_program.tracks` (see 3.1: cardinality is small).
    pub fn camera_track(&self, id: CameraId) -> Option<&CameraTrack>;

    /// Fallible resolution used by every `Edit::precheck`.
    fn require_set(&self, id: SetId) -> Result<usize, DrillError> {
        self.set_index(id).ok_or(DrillError::UnknownSet(id))
    }
    fn require_performer(&self, id: PerformerId) -> Result<usize, DrillError> {
        self.performer_index(id).ok_or(DrillError::UnknownPerformer(id))
    }
    fn require_subset(&self, id: SubsetId) -> Result<usize, DrillError> {
        self.subset_index(id).ok_or(DrillError::UnknownSubset(id))
    }
    fn require_camera(&self, id: CameraId) -> Result<usize, DrillError> {
        self.camera_program
            .tracks
            .iter()
            .position(|t| t.id == id)
            .ok_or(DrillError::UnknownCamera(id))
    }
}
```

**API 規約**: 索引を受ける公開関数は引数名を `*_index: usize`、ID を受ける関数は `*: SetId` とする。`usize` と `SetId` は相互変換できないので、規約違反はコンパイルエラーになる。

---

### 3.2 データ構造（v2）

```rust
/// Field-space coordinate. `#[repr(C)]` so `&[Point]` can be reinterpreted as
/// `&[f32]` for GPU upload without a dependency on bytemuck.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[repr(C)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub id: SectionId,
    pub name: String,
    pub short: String,
    pub color: [u8; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Performer {
    pub id: PerformerId,
    /// Drill number, e.g. "T1".
    pub label: String,
    pub section: SectionId,
    pub kind: PerformerKind,
    pub symbol: Symbol,
    /// `None` inherits the section color.
    pub color: Option<[u8; 3]>,
    pub height_m: f32,
}

/// A saved selection (design 15 §3.6). Purely a persisted `Selection`;
/// it carries no geometry and never affects playback.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Subset {
    pub id: SubsetId,
    /// Not unique, not validated for uniqueness: "Trumpet 1st", "全金管".
    pub name: String,
    /// May contain ids of performers that have since been removed
    /// (design 15 invariant 4). `Selection::by_subset` intersects with the
    /// live roster. See invariant I-17 for why this is deliberate.
    pub members: std::collections::BTreeSet<PerformerId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Set {
    pub id: SetId,
    pub name: String,
    pub counts: SetCounts,
    /// Dense, index-aligned with `Document::performers`.
    /// Access through `Set::position` / `Set::positions` so the storage can
    /// later become SoA without touching call sites.
    pub(in crate::document) positions: Vec<Point>,
    pub routes: RouteTable,
    pub note: String,
    /// `Some` when the formation was generated from a shape and is still
    /// re-editable (design 14 §3.4). **Advisory, not an invariant**: hand
    /// editing a single dot does not make the document invalid, it only makes
    /// the stored spec stale. See invariant I-18.
    #[serde(default)]
    pub shape: Option<ShapeAssignment>,
}

impl Set {
    #[inline]
    pub fn position(&self, performer_index: usize) -> Option<Point> {
        self.positions.get(performer_index).copied()
    }
    #[inline]
    pub fn positions(&self) -> &[Point] {
        &self.positions
    }
    /// Reinterpretation for GPU upload. Safe because `Point` is `#[repr(C)]`
    /// with two `f32` fields and no padding.
    pub fn positions_as_f32(&self) -> &[f32] {
        // SAFETY: `Point` is `#[repr(C)] { f32, f32 }`, so `[Point]` and
        // `[f32]` of twice the length have identical layout and validity.
        unsafe {
            core::slice::from_raw_parts(self.positions.as_ptr().cast::<f32>(), self.positions.len() * 2)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub(in crate::document) schema_version: u16,
    pub(in crate::document) title: String,
    pub(in crate::document) grid: GridConfig,
    pub(in crate::document) style: StepStyle,
    pub(in crate::document) tempo: TempoMap,
    pub(in crate::document) audio: Option<AudioTrack>,
    pub(in crate::document) sections: Vec<Section>,
    /// Saved selections (design 15 §3.6).
    pub(in crate::document) subsets: Vec<Subset>,
    pub(in crate::document) performers: Vec<Performer>,
    pub(in crate::document) sets: Vec<Set>,
    /// Camera tracks and program cuts (design 23 §3.12). Independent of the
    /// field geometry: see the `CAMERA` revision scope in 3.4.
    pub(in crate::document) camera_program: CameraProgram,
    pub(in crate::document) ids: IdAllocators,
    /// Derived, never serialized, always equal under `PartialEq`.
    pub(in crate::document) derived: Derived,
    /// Set when an inverse failed to apply during batch rollback.
    pub(in crate::document) poisoned: bool,
}
```

読み取りアクセサ（`document` モジュール外はこれだけ）:

```rust
impl Document {
    pub fn schema_version(&self) -> u16;
    pub fn title(&self) -> &str;
    pub fn grid(&self) -> &GridConfig;
    pub fn style(&self) -> StepStyle;
    pub fn tempo(&self) -> &TempoMap;
    pub fn audio(&self) -> Option<&AudioTrack>;
    pub fn sections(&self) -> &[Section];
    pub fn subsets(&self) -> &[Subset];
    pub fn performers(&self) -> &[Performer];
    pub fn sets(&self) -> &[Set];
    pub fn camera_program(&self) -> &CameraProgram;
    pub fn is_poisoned(&self) -> bool;
}
```

`camera_program()` が `&CameraProgram` を返すのは、23 の `CameraProgram::evaluate` /
`active_camera_at` / `cuts()` が全て `&self` メソッドだから。変更は `Edit` を通る（`camera_program`
フィールドは `pub(in crate::document)`）。23 §3.12 が暫定案として挙げていた
`Document::edit_camera(f: impl FnOnce(&mut CameraProgram) -> R) -> R` は**採用しない** —
任意のクロージャで書き換えられる穴を残すと不変条件 1 が破れるため。23 の T11（橋渡し実装）は
本書の `Edit` カメラ系 10 変異（§3.3.3.3）に直接置き換わる。

既存の読み取り側（`svg.rs` / `coordinates.rs` / `continuity.rs` / `countsheet.rs` / `pathing.rs` / `camera.rs`）は `doc.sets` → `doc.sets()`、`set.positions` → `set.positions()` の機械的な置換で通る。

#### `Derived` — 比較にも保存にも参加しない派生状態

```rust
#[derive(Clone, Debug, Default)]
pub struct Derived {
    pub(in crate::document) revisions: Revisions,
    pub(in crate::document) sets_by_id: IdIndex,
    pub(in crate::document) performers_by_id: IdIndex,
    pub(in crate::document) sections_by_id: IdIndex,
    pub(in crate::document) subsets_by_id: IdIndex,
    /// Exclusive prefix sums of set spans, length `sets.len() + 1`, in whole
    /// counts. `set_starts[i]` is the global count at which set `i` begins;
    /// `set_starts[n]` is `timeline_counts()`.
    ///
    /// Integer on purpose: `locate_count_f64` decides *which set* by comparing
    /// against these `u32`s, so the answer can never depend on a float type
    /// (design 11 §3.12.5 point 2). Also answers design 11's open item 5 —
    /// the cumulative-sum cache belongs to whoever owns `Derived`, i.e. here.
    pub(in crate::document) set_starts: Vec<u32>,
    /// Reused by `Targets::resolve` so per-frame edits allocate nothing.
    pub(in crate::document) index_scratch: Vec<u32>,
}

/// Derived state is a pure function of the model, so two documents with equal
/// models are equal regardless of how their caches were built. This makes
/// `#[derive(PartialEq)] Document` mean *content equality*, which is exactly
/// what the undo round-trip test needs.
impl PartialEq for Derived {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
```

この一手で `Document == Document` が「内容一致」になる。`revision` は毎回変わるので、比較に入れると 10,000 編集 Undo テストが書けない。逆に、`==` が revision を無視することは**意図的な落とし穴**なので、`Revisions` を比較したい箇所は `doc.revision()` を明示的に読む。

#### deserialize 後に必ず派生状態を作る

`#[serde(skip)]` + `Default` では「索引が空の `Document`」が外部に漏れる。それを型として不可能にするため、shadow struct を経由する。

```rust
#[derive(Serialize, Deserialize)]
struct DocumentRepr {
    schema_version: u16,
    title: String,
    #[serde(default)]
    grid: GridConfig,
    #[serde(default)]
    style: StepStyle,
    #[serde(default)]
    tempo: TempoMap,
    #[serde(default)]
    audio: Option<AudioTrack>,
    #[serde(default)]
    sections: Vec<Section>,
    #[serde(default)]
    subsets: Vec<Subset>,
    performers: Vec<Performer>,
    sets: Vec<Set>,
    #[serde(default)]
    camera_program: CameraProgram,
    #[serde(default)]
    ids: IdAllocators,
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let repr = DocumentRepr::deserialize(d)?;
        Ok(Document::from_repr(repr))
    }
}

impl Document {
    /// Rebuilds every derived structure and raises id watermarks so that a
    /// hand-edited or migrated file can never hand out a colliding id.
    fn from_repr(repr: DocumentRepr) -> Self {
        let mut doc = Document { /* move fields, derived: Derived::default(), poisoned: false */ };
        for s in &doc.sets { doc.ids.set.observe(s.id.get()); }
        for p in &doc.performers { doc.ids.performer.observe(p.id.get()); }
        for s in &doc.sections { doc.ids.section.observe(s.id.get()); }
        for s in &doc.subsets { doc.ids.subset.observe(s.id.get()); }
        // 23 §6: a hand-edited file may hold duplicate keyframe counts or cuts.
        // Re-normalize through the owning type's insert path (last one wins),
        // and raise the camera id watermark the same way.
        doc.camera_program.normalize_after_load();
        doc.rebuild_derived();
        doc
    }

    pub(in crate::document) fn rebuild_derived(&mut self) {
        let sets = &self.sets;
        self.derived.sets_by_id.rebuild(sets.len(), |i| sets[i].id.get());
        let performers = &self.performers;
        self.derived.performers_by_id.rebuild(performers.len(), |i| performers[i].id.get());
        let sections = &self.sections;
        self.derived.sections_by_id.rebuild(sections.len(), |i| sections[i].id.get());
        let subsets = &self.subsets;
        self.derived.subsets_by_id.rebuild(subsets.len(), |i| subsets[i].id.get());
        self.rebuild_set_starts();
        self.derived.revisions.reset_for(sets.len());
    }

    /// Cheap: O(sets), no allocation after the first call. Rerun by every edit
    /// that bumps `TIMELINE` (see the matrix in 3.4).
    pub(in crate::document) fn rebuild_set_starts(&mut self) {
        let last = self.sets.len().saturating_sub(1);
        let starts = &mut self.derived.set_starts;
        starts.clear();
        starts.reserve(self.sets.len() + 1);
        let mut acc: u32 = 0;
        for (i, set) in self.sets.iter().enumerate() {
            starts.push(acc);
            // Design 11 §3.11: the last set contributes only its hold; its
            // `moves` window has no next set to move toward.
            let span = if i == last { u32::from(set.counts.hold) } else { set.counts.total() };
            acc = acc.saturating_add(span);
        }
        starts.push(acc);
    }
}
```

`CameraProgram::normalize_after_load` は 23 §6 の表が要求している「同一 `count` に複数キーフレームを持つ
JSON を後勝ちで正規化する」「NaN を含むキーフレームはスキップして警告し、ドリル本体は開ける」を
実装する。本書はそれを `from_repr` から呼ぶ位置を確定させるだけで、中身は 23 の担当。
本書の `validate()` は正規化**後**に走るので、V20（§3.6）は「正規化に失敗した壊れ方」だけを見る。

`from_repr` は**検証しない**。検証は `validate()` の責務で、`load_json` が両方を順に呼ぶ（§3.6）。

#### 構築 API

フィールドが private になるので `Document::demo` 以外の構築手段が必要になる。

```rust
pub struct DocumentBuilder {
    doc: Document,
}

impl DocumentBuilder {
    pub fn new() -> Self;
    pub fn title(self, title: impl Into<String>) -> Self;
    pub fn grid(self, grid: GridConfig) -> Self;
    pub fn section(&mut self, name: &str, short: &str, color: [u8; 3]) -> Result<SectionId, DrillError>;
    pub fn performer(&mut self, label: &str, section: SectionId) -> Result<PerformerId, DrillError>;
    /// Positions must be index-aligned with the performers added so far.
    pub fn set(&mut self, name: &str, counts: SetCounts, positions: Vec<Point>) -> Result<SetId, DrillError>;
    /// Validates before handing out the document.
    pub fn build(self) -> Result<Document, DrillError>;
}
```

`Document::demo(rows, columns)` は `DocumentBuilder` の上に再実装する。日本語リテラル（[lib.rs:283](../../crates/drill-core/src/lib.rs#L283), [289](../../crates/drill-core/src/lib.rs#L289), [294](../../crates/drill-core/src/lib.rs#L294)）は除去し、`Document::demo` は英語の中立名（`"Untitled"` / `"Set 1"`）を入れる。UI 側で `Locale` を用いて表示名を上書きする（42 と協調）。

#### メモリレイアウト（1,000 人 × 64 セット）

| 要素 | inline | heap | 合計 |
|---|---:|---:|---:|
| `Point` | 8 B | — | — |
| `Set` inline（id 4 + name 24 + counts 4 + positions 24 + routes 72 + note 24 + shape 40 → padding 込み） | 200 B | — | 12.5 KiB（×64） |
| `Set::shape`（`ShapeSpec` ≈ 40 B + `order: Vec<PerformerId>` 1,000 × 4） | — | 4,040 B | 253 KiB（全 64 セットが図形生成の場合） |
| `Set::positions`（1,000 × 8 B） | — | 8,000 B | 500 KiB（×64） |
| `Set::name` / `note` の文字列本体 | — | ≈32 B | 2 KiB |
| `Performer` inline（id 4 + label 24 + section 4 + kind 1 + symbol 1 + color 4 + height 4） | 48 B | — | 47 KiB（×1,000） |
| `Performer::label` 本体 | — | ≈32 B | 31 KiB |
| `Section` ×20 | 64 B | ≈32 B | 2 KiB |
| `Subset` ×20（うち 4 個が全員 1,000 人、`BTreeSet` は要素あたり実測約 16 B） | 56 B | 4×16 KB + 16×1.6 KB | 90 KiB |
| `CameraProgram`（4 トラック × 平均 40 キーフレーム × 44 B + カット 32 個） | 24 B | ≈8 KiB | 8 KiB |
| `Derived`（sets/performers/sections/subsets 索引 + revisions 10×8+64×8 + `set_starts` 260 B + scratch 4 KB） | — | ≈9 KiB | 9 KiB |
| `GridConfig` / `TempoMap`（events 16 B/件 + f64 前置和 16 B/件） / `AudioTrack` | — | ≈2 KiB | 2 KiB |
| **合計（図形なし）** | | | **≈ 710 KiB** |
| **合計（全セットが図形生成）** | | | **≈ 960 KiB** |

上限規模（4,000 人 × 256 セット）では positions だけで 8.2 MB、`shape.order` が最大 4.1 MB、
`Subset` は現実的な上限（32 個 × 4,000 人）で 2 MB、文書全体で約 15 MB。

`Subset.members` が `BTreeSet<PerformerId>` なのは 15 §3.6 の設計どおり。`Vec<PerformerId>` より
1 要素あたり約 4 倍重いが、`Selection::by_subset` の `intersection` と「順序に依存しない集合等価性」
（`PartialEq` が挿入順に左右されない ＝ I-14 が成り立つ）が無償で手に入るので、そのまま採る。
1,000 人サブセット 1 個で 16 KB、20 個で 90 KiB は文書全体の 13% で許容範囲。

**SoA への移行余地**: `positions` を private にし、`Set::position()` / `positions()` / `positions_as_f32()` 経由に統一した時点で、内部表現を

```rust
pub(in crate::document) struct PositionColumn { xs: Vec<f32>, ys: Vec<f32> }
```

に差し替えても呼び出し側は `positions_as_f32()` の内部実装以外変わらない。今 AoS を維持する理由は、補間 `a.lerp(b, t)` が x と y を同時に触るためキャッシュラインの利用率が AoS の方が高く、SIMD 化しない限り SoA の利得が無いこと。移行の判断は 21（GPU レンダラ）が `xs` / `ys` を別バッファで要求するかどうかで決める（§9）。

---

### 3.3 Edit コマンド代数

#### 3.3.1 中核となる 2 つの決定

**決定 A: 前状態を payload に持たない。`apply` が文書から読んで逆操作を組み立てる。**

`DESIGN_GAPS.md` A-1 の草案は `MovePoints { before, after }` のように両方を持っていた。前状態を持たせると、(1) 呼び出し側が `before` を組み立てる責務を負い、(2) `before` が実際の文書と食い違う可能性があり、(3) メモリが 2 倍になる。`apply` が読めば全部消える。

**決定 B: 逆操作は必ず「保存された値の復元」であり、順操作の再計算ではない。**

`SetGrid { scale_positions: true }` の逆を「逆スケールを掛ける `SetGrid`」にすると、`x * sx / sx != x` で f32 が壊れ、10,000 編集 Undo テストが落ちる。逆操作は常に**適用前のビットパターンを保持した復元コマンド**を返す。

#### 3.3.2 対象指定

```rust
/// Which performers an edit touches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Targets {
    /// Every performer, in index order. Avoids a 4 KB id vector for
    /// whole-ensemble operations such as auto-assignment.
    All,
    Ids(Vec<PerformerId>),
}

impl Targets {
    pub fn len(&self, doc: &Document) -> usize {
        match self {
            Targets::All => doc.performers.len(),
            Targets::Ids(ids) => ids.len(),
        }
    }
    /// Resolves to `Vec` indices into `Set::positions`, reusing `out`.
    pub fn resolve(&self, doc: &Document, out: &mut Vec<u32>) -> Result<(), DrillError> {
        out.clear();
        match self {
            Targets::All => out.extend(0..doc.performers.len() as u32),
            Targets::Ids(ids) => {
                out.reserve(ids.len());
                for &id in ids {
                    out.push(doc.require_performer(id)? as u32);
                }
            }
        }
        Ok(())
    }
    /// O(n) exact comparison used by the coalesce guard.
    pub fn same_as(&self, other: &Targets) -> bool {
        self == other
    }
}
```

#### 3.3.3 `Edit`

Vec や String を含む payload はすべて `Box` に入れる。`Edit` を 16 バイトに保つことで `Vec<Edit>`（`Batch`）と `VecDeque<Entry>`（`History`）が小さくなり、走査が速い。追加の間接参照 1 回は座標配列のコストに埋もれる。

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Identity. Never pushed onto the history.
    Nop,
    /// Applied left to right; the inverse is the reversed list of inverses.
    Batch(Vec<Edit>),

    // ---- geometry ----
    /// Absolute target positions for `targets`, in `targets` order.
    MovePoints(Box<MovePoints>),
    /// Whole-column restore. Swaps vectors, so it is O(number of sets).
    RestorePositions(Box<RestorePositions>),

    // ---- sets ----
    InsertSet(Box<InsertSet>),
    RemoveSet(RemoveSet),
    MoveSet(MoveSet),
    RenameSet(Box<RenameSet>),
    SetCounts(CountsChange),
    SetNote(Box<SetNote>),
    SetRoutes(Box<SetRoutesChange>),

    /// Shape spec + resulting positions in one reversible unit (design 14 §3.4).
    ApplyShape(Box<ApplyShape>),
    /// Shape metadata only; no positions move.
    SetShape(Box<SetShapeChange>),

    // ---- performers ----
    InsertPerformers(Box<PerformerExtract>),
    RemovePerformers(Box<RemovePerformers>),
    SetPerformerMeta(Box<PerformerMetaChange>),
    /// Bulk (re)numbering (design 15 §3.4). One user action = one entry.
    RelabelPerformers(Box<RelabelPerformers>),
    AssignSection(Box<AssignSection>),

    // ---- sections ----
    InsertSection(Box<InsertSection>),
    RemoveSection(RemoveSection),
    SetSectionMeta(Box<SectionMetaChange>),

    // ---- subsets (design 15 §3.6) ----
    InsertSubset(Box<InsertSubset>),
    RemoveSubset(RemoveSubset),
    RenameSubset(Box<RenameSubset>),
    SetSubsetMembers(Box<SubsetMembersChange>),

    // ---- camera (design 23 §3.12) ----
    InsertCameraTrack(Box<InsertCameraTrack>),
    RemoveCameraTrack(RemoveCameraTrack),
    RenameCameraTrack(Box<RenameCameraTrack>),
    SetCameraLook(Box<CameraLookChange>),
    SetCameraBoundsMode(CameraBoundsChange),
    InsertCameraKeyframe(Box<InsertCameraKeyframe>),
    RemoveCameraKeyframe(RemoveCameraKeyframe),
    MoveCameraKeyframe(MoveCameraKeyframe),
    SetCameraCut(SetCameraCut),
    RemoveCameraCut(RemoveCameraCut),

    // ---- document-wide ----
    SetTitle(Box<TitleChange>),
    SetGrid(Box<GridChange>),
    SetStepStyle(StepStyleChange),
    SetTempo(Box<TempoChangeEdit>),
    SetAudio(Box<AudioChange>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct MovePoints {
    pub set: SetId,
    pub targets: Targets,
    /// Same length as `targets`. Every component must be finite.
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RestorePositions {
    /// One entry per affected set. Applied by `std::mem::swap`, so the
    /// inverse costs nothing beyond the vectors already in hand.
    pub columns: Vec<(SetId, Vec<Point>)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InsertSet {
    pub at: usize,
    /// `None` allocates a fresh id (user action); `Some` restores a removed
    /// set with its original id (undo of `RemoveSet`).
    pub id: Option<SetId>,
    pub name: String,
    pub counts: SetCounts,
    pub positions: Vec<Point>,
    pub routes: RouteTable,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq)] pub struct RemoveSet  { pub set: SetId }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct MoveSet     { pub set: SetId, pub to: usize }
#[derive(Clone, Debug, PartialEq)]       pub struct RenameSet   { pub set: SetId, pub name: String }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct CountsChange{ pub set: SetId, pub counts: SetCounts }
#[derive(Clone, Debug, PartialEq)]       pub struct SetNote     { pub set: SetId, pub note: String }
#[derive(Clone, Debug, PartialEq)]       pub struct SetRoutesChange { pub set: SetId, pub routes: RouteTable }

/// Everything the document stores keyed by the removed performers **and that
/// `RemovePerformers` actually deletes**. Adding a new per-performer subsystem
/// MUST either extend this struct or adopt the dangling-id rule (I-17/I-18);
/// the `performer_extract_is_total` test enforces the choice.
///
/// Deliberately absent, because those subsystems tolerate dangling ids and are
/// left untouched by `RemovePerformers`:
///   - `Subset::members`            (design 15 invariant 4)
///   - `Set::shape.order`           (design 14, see I-18)
///   - `FollowTarget::Group`        (design 23 §6)
#[derive(Clone, Debug, PartialEq)]
pub struct PerformerExtract {
    pub at: usize,
    pub performers: Vec<Performer>,
    /// `columns[set_index][k]` is the position of `performers[k]`.
    /// Ordered by current set index at extraction time; restored by `SetId`.
    pub columns: Vec<(SetId, Vec<Point>)>,
    pub route_overrides: Vec<(SetId, PerformerId, Route)>,
}

#[derive(Clone, Debug, PartialEq)] pub struct RemovePerformers { pub targets: Vec<PerformerId> }
#[derive(Clone, Debug, PartialEq)] pub struct PerformerMetaChange { pub performer: PerformerId, pub meta: PerformerMeta }

/// `(id, new label)`. Design 15 §3.4 writes the triple `(id, before, after)`;
/// the `before` is dropped because `apply` reads it from the document
/// (decision A, 3.3.1).
#[derive(Clone, Debug, PartialEq)] pub struct RelabelPerformers { pub changes: Vec<(PerformerId, String)> }

/// `(id, new section)`. Heterogeneous per design 15 §3.9, so the inverse is a
/// single `AssignSection` rather than a `Batch`.
#[derive(Clone, Debug, PartialEq)] pub struct AssignSection { pub changes: Vec<(PerformerId, SectionId)> }

#[derive(Clone, Debug, PartialEq)] pub struct InsertSection { pub at: usize, pub id: Option<SectionId>, pub section: SectionTemplate }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct RemoveSection { pub section: SectionId }
#[derive(Clone, Debug, PartialEq)] pub struct SectionMetaChange { pub section: SectionId, pub name: String, pub short: String, pub color: [u8; 3] }
#[derive(Clone, Debug, PartialEq)] pub struct TitleChange { pub title: String }
#[derive(Clone, Debug, PartialEq)] pub struct GridChange { pub grid: GridConfig, pub scale_positions: bool }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct StepStyleChange { pub style: StepStyle }
#[derive(Clone, Debug, PartialEq)] pub struct TempoChangeEdit { pub tempo: TempoMap }
#[derive(Clone, Debug, PartialEq)] pub struct AudioChange { pub audio: Option<AudioTrack> }

/// Mutable per-performer metadata, replaced as a unit so the inverse is
/// always a single `SetPerformerMeta`.
#[derive(Clone, Debug, PartialEq)]
pub struct PerformerMeta {
    pub label: String,
    pub section: SectionId,
    pub kind: PerformerKind,
    pub symbol: Symbol,
    pub color: Option<[u8; 3]>,
    pub height_m: f32,
}
```

#### 3.3.3.1 シェイプ payload（14 §3.4）

```rust
/// One reversible unit carrying BOTH the shape spec and the positions it
/// produced. Design 14 §3.4 explicitly rejects splitting these into a
/// `MovePoints` plus a non-undoable side channel.
#[derive(Clone, Debug, PartialEq)]
pub struct ApplyShape {
    pub set: SetId,
    pub shape: ShapeAssignment,
    /// Absolute positions, index-aligned with `shape.order`. Ids in `order`
    /// that no longer exist are skipped (I-18); their slots in `points` are
    /// ignored, so `points.len() == shape.order.len()` always holds and the
    /// payload stays a pure function of the shape sampling.
    pub points: Vec<Point>,
}

/// Attach or detach a shape without moving anyone. The app emits
/// `Batch[MovePoints, SetShape { shape: None }]` when freehand editing a set
/// that was shape-generated.
#[derive(Clone, Debug, PartialEq)]
pub struct SetShapeChange {
    pub set: SetId,
    pub shape: Option<ShapeAssignment>,
}
```

`MovePoints` は `Set.shape` に**触れない**。触れさせない理由は 3 つ。(1) `MovePoints` はドラッグ中の
毎フレーム経路なので、`Option<ShapeAssignment>` の退避（1,000 人で 4 KB）を毎フレーム積むと合流の
利得が消える。(2) 暗黙の副作用は逆操作の形を `MovePoints` から `Batch` へ変えてしまい、合流規則
（`coalesce_key` の一致判定）が壊れる。(3) 「1 ドット動かしたら円でなくなる」かどうかは製品判断で、
コアが決めるべきではない。代わりに**アプリ側の義務**として明文化し、テスト
`freehand_edit_of_shaped_set_clears_shape`（§7.1）で固定する。

#### 3.3.3.2 サブセット payload（15 §3.6）

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct InsertSubset {
    pub at: usize,
    /// `None` allocates; `Some` restores a removed subset with its original id.
    pub id: Option<SubsetId>,
    pub name: String,
    pub members: std::collections::BTreeSet<PerformerId>,
}

#[derive(Clone, Copy, Debug, PartialEq)] pub struct RemoveSubset { pub subset: SubsetId }
#[derive(Clone, Debug, PartialEq)]       pub struct RenameSubset { pub subset: SubsetId, pub name: String }

#[derive(Clone, Debug, PartialEq)]
pub struct SubsetMembersChange {
    pub subset: SubsetId,
    /// Replaced wholesale. Membership sets are small relative to the point
    /// arrays (16 KB for the whole ensemble), and a whole-set replacement
    /// makes the inverse a single `SubsetMembersChange` via `mem::swap`.
    pub members: std::collections::BTreeSet<PerformerId>,
}
```

15 の変異名 `AddSubset` は本書の `InsertSubset` に対応する（`InsertSet` / `InsertSection` /
`InsertPerformers` と綴りを揃えるための改名のみ。挿入位置 `at` を持つ点も同じ）。
`RemoveSubset` / `RenameSubset` / `SetSubsetMembers` は 15 と同名。

`SetSubsetMembers` の適用は `std::mem::swap` でコピーを発生させない:

```rust
fn apply_subset_members(mut change: SubsetMembersChange, doc: &mut Document) -> Edit {
    let index = doc.subset_index(change.subset).expect("prechecked");
    std::mem::swap(&mut doc.subsets[index].members, &mut change.members);
    doc.bump(Scopes::PRESENTATION, SetScope::None);
    Edit::SetSubsetMembers(Box::new(change)) // now holds the previous members
}
```

#### 3.3.3.3 カメラ payload（23 §3.12）

23 の提案は `before`/`after` と `replaced: Option<..>` を payload に持つ形だったが、本書の決定 A
（`apply` が文書から読む）に合わせて前状態を落とす。23 §3.3 / §3.9 の
`insert_keyframe` / `remove_keyframe` / `set_cut` が**置き換えられた値を返す**シグネチャに
なっているおかげで、逆操作は返り値からそのまま組める。

```rust
/// Everything in a `CameraTrack` except its id.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraTrackTemplate {
    pub name: String,
    /// Normalized through `CameraTrack::insert_keyframe` on apply, so the
    /// sorted-unique invariant (23 invariant 2) holds even for a template
    /// built by a preset generator.
    pub keyframes: Vec<CameraKeyframe>,
    pub look: LookMode,
    pub bounds: BoundsMode,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InsertCameraTrack {
    pub at: usize,
    pub id: Option<CameraId>,
    pub track: CameraTrackTemplate,
}

#[derive(Clone, Copy, Debug, PartialEq)] pub struct RemoveCameraTrack { pub camera: CameraId }
#[derive(Clone, Debug, PartialEq)]       pub struct RenameCameraTrack { pub camera: CameraId, pub name: String }
#[derive(Clone, Debug, PartialEq)]       pub struct CameraLookChange  { pub camera: CameraId, pub look: LookMode }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct CameraBoundsChange{ pub camera: CameraId, pub bounds: BoundsMode }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InsertCameraKeyframe { pub camera: CameraId, pub keyframe: CameraKeyframe }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RemoveCameraKeyframe { pub camera: CameraId, pub count: f32 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveCameraKeyframe { pub camera: CameraId, pub from_count: f32, pub to_count: f32 }

#[derive(Clone, Copy, Debug, PartialEq)] pub struct SetCameraCut    { pub cut: CameraCut }
#[derive(Clone, Copy, Debug, PartialEq)] pub struct RemoveCameraCut { pub at: f32 }
```

`InsertCameraKeyframe` は 44 バイト（`CameraKeyframe` が 40 B + `CameraId` 4 B）なので `Box` に入れる。
`RemoveCameraKeyframe` / `MoveCameraKeyframe` / `SetCameraCut` / `RemoveCameraCut` / `CameraBoundsChange`
は 12 バイト以下なので `Edit` に直接入れる（`Edit` は 16 バイトのまま）。

逆操作の作り方:

| 変異 | 逆操作 |
|---|---|
| `InsertCameraKeyframe` | `insert_keyframe` の返り値が `Some(displaced)` なら `InsertCameraKeyframe { keyframe: displaced }`（再置換）、`None` なら `RemoveCameraKeyframe { count }` |
| `RemoveCameraKeyframe` | `InsertCameraKeyframe { keyframe: 取り除いた値 }` |
| `MoveCameraKeyframe` | 移動先に既存キーフレームがあった場合は `Batch[MoveCameraKeyframe { from: to_count, to: from_count }, InsertCameraKeyframe { keyframe: 押し出された値 }]`、無ければ単一の `MoveCameraKeyframe` |
| `SetCameraCut` | `set_cut` が `Some(replaced)` を返せば `SetCameraCut { cut: replaced }`、`None` なら `RemoveCameraCut { at }` |
| `RemoveCameraCut` | `SetCameraCut { cut: 取り除いた値 }` |
| `RemoveCameraTrack` | `Batch[InsertCameraTrack { at, id: Some(元のid), track: 元の内容 }, SetCameraCut × 巻き添えで消えたカット]` |

**`RemoveCameraTrack` のカット整合**（23 T7 が要求）: トラックを削除すると、そのトラックを指す
`CameraCut` が宙に浮く。`active_camera_at` は `CameraId` を返すだけなので `evaluate` が `None` に
落ちて画が出なくなる。したがって削除時に該当カットも取り除き、逆操作でまとめて復元する。
そのために 23 の `CameraProgram` へ 2 つの API 追加を要求する（§9 の報告事項）:

```rust
impl CameraProgram {
    /// Removes the cut at `at`, returning it. Symmetric with `set_cut`.
    pub fn remove_cut(&mut self, at: f32) -> Option<CameraCut>;
    /// Removes every cut pointing at `camera`, returning them in `at` order
    /// so `Edit::apply` can rebuild them.
    pub fn drain_cuts_for(&mut self, camera: CameraId) -> Vec<CameraCut>;
    /// Overflow-checked replacement for `alloc_camera_id`.
    pub fn try_alloc_camera_id(&mut self) -> Option<CameraId>;
    /// Re-normalizes keyframe/cut ordering and drops keyframes that fail
    /// `CameraKeyframe::validate` (23 §6). Called from `Document::from_repr`.
    pub fn normalize_after_load(&mut self);
}
```

`MoveCameraKeyframe` の `f32` 一致判定について: 23 §3.3 の `insert_keyframe` /
`remove_keyframe` は `k.count == count` の厳密一致で探す。`CameraKeyframe::validate` が非有限を
拒否しているので `count` に NaN は入らず、`==` は反射的で `total_cmp` によるソートとも整合する。
`precheck` は `from_count` のキーフレームが実在することを要求する（`UnknownCameraKeyframe`）。

`AssignSection` / `RelabelPerformers` はいずれも `Vec<(PerformerId, 新値)>` の形なので、逆操作は
「同じ ID 列に対して読み取った旧値を並べた同じ変異」で、`Batch` を経由しない。15 §3.9 が
`(id, before, after)` の 3 つ組で書いていたところから `before` を落とせるのは決定 A のおかげ。

`RelabelPerformers` を `SetPerformerMeta` の複数演者版で代用しない理由: `PerformerMeta` は
label / section / kind / symbol / color / height_m を**一括置換**する型なので、複数演者版は
`Vec<(PerformerId, PerformerMeta)>` になり、1,000 人で inline 64 KB + 文字列本体 32 KB ≈ 96 KB。
ラベルだけを運ぶ専用変異なら 60 KB で済み（§5.1）、さらに合流キーと revision スコープを
`PRESENTATION` だけに絞れる（`SetPerformerMeta` は `section` を変えうるので同じ扱いにできない）。
専用変異を置く。

#### 3.3.4 `apply` — 2 相構造

```rust
impl Edit {
    /// Applies the edit and returns the exact inverse.
    ///
    /// Atomic: on `Err` the document is byte-for-byte unchanged, including
    /// its revisions.
    pub fn apply(self, doc: &mut Document) -> Result<Edit, DrillError> {
        if let Edit::Batch(items) = self {
            return Self::apply_batch(items, doc);
        }
        self.precheck(doc)?;
        let inverse = self.apply_checked(doc);
        #[cfg(debug_assertions)]
        doc.debug_assert_invariants();
        Ok(inverse)
    }

    /// Every precondition, evaluated against an immutable document.
    fn precheck(&self, doc: &Document) -> Result<(), DrillError>;

    /// Infallible mutation. Only reachable after `precheck` returned `Ok`.
    /// Bumps the revision scopes for the variant (see the matrix in 3.4).
    fn apply_checked(self, doc: &mut Document) -> Edit;

    /// `true` when the edit provably changes nothing.
    pub fn is_nop(&self) -> bool;

    /// Heap bytes owned by this edit, for history accounting.
    pub fn heap_bytes(&self) -> usize;

    /// `None` means "never merge with the previous entry".
    pub fn coalesce_key(&self) -> Option<CoalesceKey>;
}
```

`precheck` の中身（変異ごと、抜粋）:

| 変異 | 事前条件 |
|---|---|
| `MovePoints` | `set` が存在 / `points.len() == targets.len(doc)` / 全 `targets` が存在 / 全成分が `is_finite()` / `points.len() <= MAX_PERFORMERS` |
| `RestorePositions` | 全 `SetId` が存在・重複なし / 各ベクタ長 == `performers.len()` / 全成分が有限 |
| `InsertSet` | `at <= sets.len()` / `sets.len() < MAX_SETS` / `positions.len() == performers.len()` / `id` が `Some` なら**未使用**かつ watermark 未満 / `routes` の全キーが存在する演者 / `counts` が上限内 |
| `RemoveSet` | `set` が存在 / `sets.len() > 1`（`EmptySets` を作らない） |
| `MoveSet` | `set` が存在 / `to < sets.len()` |
| `SetCounts` | `set` が存在 / `moves + hold <= MAX_SET_COUNTS` / タイムライン総和が `u32` を溢れない / **`moves` を縮める場合は `routes.max_arrive() <= moves`**（§3.8.4。満たさなければ `CountsWouldStrandRoutes`。アプリは `Batch[SetRoutes{clamped}, SetCounts]` を出す） |
| `SetRoutes` | `set` が存在 / `routes.validate(performers, counts)`（11 が実装） |
| `InsertPerformers` | `at <= performers.len()` / 追加後が `MAX_PERFORMERS` 以内 / `columns` が全セットを過不足なく被覆 / 全 `id` が未使用 / 全 `section` が存在 |
| `RemovePerformers` | 全対象が存在・重複なし / 残数 >= 0 |
| `RelabelPerformers` | 全 ID が存在・重複なし / 各ラベルが `MAX_LABEL_BYTES` 以内。**ラベルの重複は許す**（15 §3.4: 「仮番号で入れて後で振り直す」運用を壊さない） |
| `AssignSection` | 全 ID が存在・重複なし / 全 `SectionId` が存在 |
| `RemoveSection` | `section` が存在 / **どの演者からも参照されていない**（参照ありなら `SectionInUse`） |
| `ApplyShape` | `set` が存在 / `shape.spec.validate()`（14 が実装、14 不変条件 9 のゲート） / `points.len() == shape.order.len()` / 全点が有限 / `order.len() <= MAX_PERFORMERS` / `order` に重複が無い。**`order` の ID が実在しなくてもよい**（I-18） |
| `SetShape` | `set` が存在 / `Some` なら上と同じ `spec.validate()` と `order` の重複なし |
| `InsertSubset` | `at <= subsets.len()` / `subsets.len() < MAX_SUBSETS` / `name` が `MAX_SET_NAME_BYTES` 以内 / `members.len() <= MAX_PERFORMERS` / `id` が `Some` なら未使用かつ watermark 未満。**メンバーの実在は要求しない**（I-17） |
| `RemoveSubset` / `RenameSubset` / `SetSubsetMembers` | `subset` が存在 / 名前長・メンバー数が上限内 |
| `InsertCameraTrack` | `at <= tracks.len()` / `tracks.len() < MAX_CAMERA_TRACKS` / 全キーフレームが `CameraKeyframe::validate()` を通る / `keyframes.len() <= MAX_CAMERA_KEYFRAMES` / `near`・`far` が有限かつ `0 < near < far` / `look` が `Follow` ならその `FollowTarget` のサイズが上限内（**演者 ID の実在は要求しない** — 23 §6 は削除済み ID を無視する設計） / `id` が `Some` なら未使用 |
| `RemoveCameraTrack` | `camera` が存在 |
| `RenameCameraTrack` / `SetCameraLook` / `SetCameraBoundsMode` | `camera` が存在 / 名前長が上限内 |
| `InsertCameraKeyframe` | `camera` が存在 / `keyframe.validate()` / 追加後のキーフレーム数が上限内 |
| `RemoveCameraKeyframe` | `camera` が存在 / `count` に厳密一致するキーフレームが存在 |
| `MoveCameraKeyframe` | `camera` が存在 / `from_count` に厳密一致するキーフレームが存在 / `to_count` が有限 |
| `SetCameraCut` | `cut.at` が有限 / `cut.camera` が**存在するトラック**を指す（宙に浮いたカットを新規に作らせない） / カット数が上限内 |
| `RemoveCameraCut` | `at` に厳密一致するカットが存在 |
| `SetGrid` | 寸法が有限かつ `(0, MAX_FIELD_EXTENT]` / `horizontal_steps >= 1` / `resolution` が `1..=16` / `hashes.len() <= MAX_HASHES` / 全 hash 位置が有限 |
| `SetTempo` | `tempo.validate()`（V14 と同一規則、11 が前置和とともに実装）/ 事象数 <= `MAX_TEMPO_EVENTS` |
| `SetAudio` | `Some` なら `track.validate()` |

**非有限値の拒否が最重要。** UI 側の計算（回転行列・0 除算・`f32::NAN` を含む重心）から NaN が 1 つでも文書に入ると、以後の `min` / `max` / 衝突走査 / バウンディングボックス / SVG 出力がすべて汚染され、しかも保存されて次回起動時にも残る。`precheck` は文書と外界の唯一の境界なので、ここで止める。

代表的な `apply_checked`:

```rust
fn apply_move_points(m: MovePoints, doc: &mut Document) -> Edit {
    let set_index = doc.set_index(m.set).expect("prechecked");
    let mut scratch = doc.derived.take_index_scratch();
    m.targets.resolve(doc, &mut scratch).expect("prechecked");

    let mut previous = Vec::with_capacity(scratch.len());
    {
        let positions = &mut doc.sets[set_index].positions;
        for (slot, &new_point) in scratch.iter().zip(&m.points) {
            let p = &mut positions[*slot as usize];
            previous.push(*p);
            *p = new_point;
        }
    }
    doc.derived.give_index_scratch(scratch);
    doc.bump(Scopes::NONE, SetScope::One(m.set));

    Edit::MovePoints(Box::new(MovePoints {
        set: m.set,
        targets: m.targets,
        points: previous,
    }))
}
```

`RestorePositions` はベクタを交換するだけなのでコピーが発生しない:

```rust
fn apply_restore_positions(mut r: RestorePositions, doc: &mut Document) -> Edit {
    for (set_id, incoming) in r.columns.iter_mut() {
        let index = doc.set_index(*set_id).expect("prechecked");
        std::mem::swap(&mut doc.sets[index].positions, incoming);
    }
    doc.bump(Scopes::NONE, SetScope::All);
    // `r.columns` now holds the previous positions.
    Edit::RestorePositions(Box::new(r))
}
```

`SetGrid { scale_positions: true }` の逆（決定 B の実例）:

```rust
fn apply_grid(change: GridChange, doc: &mut Document) -> Edit {
    let previous_grid = std::mem::replace(&mut doc.grid, change.grid);
    if !change.scale_positions {
        doc.bump(Scopes::GRID, SetScope::None);
        return Edit::SetGrid(Box::new(GridChange { grid: previous_grid, scale_positions: false }));
    }
    // Capture the exact prior bit patterns before the lossy scale.
    let columns = doc
        .sets
        .iter()
        .map(|s| (s.id, s.positions.clone()))
        .collect::<Vec<_>>();
    let sx = doc.grid.width / previous_grid.width.max(f32::EPSILON);
    let sy = doc.grid.height / previous_grid.height.max(f32::EPSILON);
    for set in &mut doc.sets {
        for p in &mut set.positions {
            p.x *= sx;
            p.y *= sy;
        }
    }
    doc.bump(Scopes::GRID, SetScope::All);
    Edit::Batch(vec![
        Edit::SetGrid(Box::new(GridChange { grid: previous_grid, scale_positions: false })),
        Edit::RestorePositions(Box::new(RestorePositions { columns })),
    ])
}
```

この逆操作は 1,000 × 64 で 512 KB を確保する。`History` は §3.5 の「大きな編集」経路で扱う。

#### 3.3.5 `Batch` とロールバック

```rust
fn apply_batch(items: Vec<Edit>, doc: &mut Document) -> Result<Edit, DrillError> {
    let mut inverses: Vec<Edit> = Vec::with_capacity(items.len());
    for item in items {
        match item.apply(doc) {
            Ok(inverse) => inverses.push(inverse),
            Err(error) => {
                // Undo what we already did, newest first.
                while let Some(inverse) = inverses.pop() {
                    if inverse.apply(doc).is_err() {
                        // Unreachable: `inverse` is the exact inverse of an edit
                        // that just succeeded against this very state.
                        debug_assert!(false, "batch rollback failed");
                        doc.poisoned = true;
                        return Err(DrillError::DocumentPoisoned);
                    }
                }
                return Err(error);
            }
        }
    }
    inverses.reverse();
    Ok(Edit::Batch(inverses))
}
```

`Batch` は `precheck` を一括で行えない（後続項目の事前条件が先行項目の結果に依存する）。ロールバックで原子性を回復する。ロールバックで revision は戻らない（単調増加のまま）が、これは安全側の誤り — キャッシュが余分に無効化されるだけ。

`poisoned` が立った文書は編集を受け付けず、`drill-app` は「復旧用に別名保存してください」を提示して元ファイルを触らない（41 と協調）。到達不能経路だが、到達したときに黙って壊れたデータを保存しないことが重要。

#### 3.3.6 合流（coalesce）

```rust
/// Identifies a run of edits that the user perceives as one action.
/// Issued by the app at gesture start (pointer down, key repeat start).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GestureId(NonZeroU32);

/// Cheap structural guard. `None` = never coalesce.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CoalesceKey {
    MovePoints(SetId),
    Counts(SetId),
    RenameSet(SetId),
    SetNote(SetId),
    Title,
    Grid,
    Tempo,
    SectionMeta(SectionId),
    PerformerMeta(PerformerId),
    /// Dragging a shape handle re-samples every frame: without this, a
    /// two-second radius drag would push 120 entries of ~12 KB each.
    ApplyShape(SetId),
    /// Dragging a route control handle, for the same reason
    /// (design 11 §6.4, requested in its §9-8b).
    Routes(SetId),
    RenameSubset(SubsetId),
    /// Live marquee editing of a saved selection.
    SubsetMembers(SubsetId),
    /// Scrubbing a keyframe along the count axis, or dragging the 3D gizmo
    /// (which re-inserts at the same `count` each frame).
    CameraKeyframe(CameraId),
    CameraCut,
    RenameCameraTrack(CameraId),
}
```

`RelabelPerformers` と `AssignSection` は `coalesce_key()` が `None`（合流しない）。どちらも
ボタン一押しの離散操作で、連続適用されることがないため。`ApplyShape` は合流するが、条件 3 に相当する
ガードとして `shape.order` の一致も要求する（`order` が変わる ＝ 割り当てをやり直した ＝ 別の操作）。

**合流規則**: 新しい編集 `E_n` の逆操作 `inv(E_n)` を積むとき、undo スタックの末尾 `top` と

1. `gesture` が両方 `Some` かつ一致し、
2. `coalesce_key()` が両方 `Some` かつ一致し、
3. `MovePoints` の場合はさらに `targets.same_as(top.targets)` が真、`ApplyShape` の場合は
   `shape.order == top.shape.order` が真、`InsertCameraKeyframe` の場合は `keyframe.count` が
   一致（同一 count への再置換 ＝ 同じキーフレームの調整）

を満たすなら、**`inv(E_n)` を捨てて `top` をそのまま残す**。

これで正しくなる理由: `top` は `inv(E_{n-1})` ではなく「ジェスチャ開始直前の状態を復元する逆操作」である。ジェスチャ内の最初の編集の逆操作をそのまま保持し続ければ、途中の編集をいくつ捨てても、Undo 一回でジェスチャ開始前に戻る。マージ処理も再構築も要らない。O(1)（`MovePoints` の targets 比較のみ O(選択数)）で、ヒープ確保はゼロ。

条件 3 を外せない理由: ジェスチャ途中で選択が変わると `top` は新しく増えた演者を復元できない。長さと ID を厳密に比較して、違えば合流せず新しいエントリを積む（Undo が 2 段になるだけで、データは壊れない）。1,000 人の比較は 4 KB の memcmp で約 200ns、ドラッグ中フレームあたり 1 回なので予算に影響しない。

`redo` スタックが空でないときは合流しない（`push` が先に redo を破棄するので、実際には常に空）。

---

### 3.4 Revision — キャッシュ無効化の粒度

```rust
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-globally unique, strictly increasing.
///
/// Uniqueness across `Document` instances is deliberate: a cache keyed by a
/// revision from a closed document can never collide with a revision from a
/// newly opened one, so "stale cache after File > Open" is impossible by
/// construction.
///
/// Never serialized, never fed into a rendered output. Determinism (invariant
/// 5) is preserved because no downstream artifact contains a revision.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Revision(u64);

static NEXT_REVISION: AtomicU64 = AtomicU64::new(1);

impl Revision {
    fn next() -> Self {
        let raw = NEXT_REVISION.fetch_add(1, Ordering::Relaxed);
        debug_assert!(raw != u64::MAX, "revision space exhausted");
        Revision(raw)
    }
}

impl Default for Revision {
    fn default() -> Self {
        Revision::next()
    }
}
```

u64 は 1ns に 1 回のペースで消費しても 584 年もつので、周回対策は不要。`fetch_add` は約 5ns、1 編集あたり 2〜3 回で 15ns。

#### スコープ

```rust
/// Bit set of document-wide revision scopes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scopes(u16);

impl Scopes {
    pub const NONE:         Self = Scopes(0);
    /// Sets or performers were added, removed, or reordered.
    pub const TOPOLOGY:     Self = Scopes(1 << 0);
    /// Count-to-set mapping changed (counts, set insert/remove/move, tempo).
    pub const TIMELINE:     Self = Scopes(1 << 1);
    pub const GRID:         Self = Scopes(1 << 2);
    pub const TEMPO:        Self = Scopes(1 << 3);
    pub const AUDIO:        Self = Scopes(1 << 4);
    /// Names, labels, colors, symbols, section membership, notes, saved
    /// subsets, shape overlays.
    pub const PRESENTATION: Self = Scopes(1 << 5);
    pub const STYLE:        Self = Scopes(1 << 6);
    /// Camera tracks, keyframes and program cuts. Deliberately separate from
    /// everything above: authoring a camera move must not invalidate the 2D
    /// field DisplayList, the collision scan, or the coordinate sheets.
    pub const CAMERA:       Self = Scopes(1 << 7);
    /// Aggregate: "some set's positions changed". Bumped automatically
    /// whenever `SetScope` is not `None`, so consumers that need a single
    /// `Copy` key for "any geometry moved" (the camera follow cache,
    /// design 23 §3.7) do not have to compare a `Vec<Revision>`.
    pub const GEOMETRY:     Self = Scopes(1 << 8);

    pub const fn or(self, other: Self) -> Self { Scopes(self.0 | other.0) }
    pub const fn has(self, other: Self) -> bool { self.0 & other.0 != 0 }
}
```

`Scopes` が 8 ビットを超えたので内部表現は `u16` にする（`Scopes(u16)`）。

```rust
/// Which per-set revisions to bump.
#[derive(Clone, Copy, Debug)]
pub enum SetScope { None, One(SetId), All }

#[derive(Clone, Debug)]
pub struct Revisions {
    doc: Revision,
    topology: Revision,
    timeline: Revision,
    grid: Revision,
    tempo: Revision,
    audio: Revision,
    presentation: Revision,
    style: Revision,
    camera: Revision,
    /// Aggregate of every per-set geometry bump.
    geometry: Revision,
    /// Index-parallel to `Document::sets`. Covers positions, routes and counts
    /// of that set.
    sets: Vec<Revision>,
}
```

```rust
impl Document {
    pub(in crate::document) fn bump(&mut self, scopes: Scopes, sets: SetScope) {
        let r = &mut self.derived.revisions;
        r.doc = Revision::next();
        if scopes.has(Scopes::TOPOLOGY)     { r.topology = Revision::next(); }
        if scopes.has(Scopes::TIMELINE)     { r.timeline = Revision::next(); }
        if scopes.has(Scopes::GRID)         { r.grid = Revision::next(); }
        if scopes.has(Scopes::TEMPO)        { r.tempo = Revision::next(); }
        if scopes.has(Scopes::AUDIO)        { r.audio = Revision::next(); }
        if scopes.has(Scopes::PRESENTATION) { r.presentation = Revision::next(); }
        if scopes.has(Scopes::STYLE)        { r.style = Revision::next(); }
        if scopes.has(Scopes::CAMERA)       { r.camera = Revision::next(); }
        match sets {
            SetScope::None => {}
            SetScope::One(id) => {
                if let Some(i) = self.derived.sets_by_id.get(id.get()) {
                    r.sets[i] = Revision::next();
                }
                r.geometry = Revision::next();
            }
            SetScope::All => {
                for slot in &mut r.sets { *slot = Revision::next(); }
                r.geometry = Revision::next();
            }
        }
        // `GEOMETRY` is never requested explicitly; it is implied by `SetScope`.
        debug_assert!(!scopes.has(Scopes::GEOMETRY));
    }

    pub fn revision(&self) -> Revision { self.derived.revisions.doc }
    pub fn topology_revision(&self) -> Revision { self.derived.revisions.topology }
    pub fn timeline_revision(&self) -> Revision { self.derived.revisions.timeline }
    pub fn grid_revision(&self) -> Revision { self.derived.revisions.grid }
    pub fn tempo_revision(&self) -> Revision { self.derived.revisions.tempo }
    pub fn audio_revision(&self) -> Revision { self.derived.revisions.audio }
    pub fn presentation_revision(&self) -> Revision { self.derived.revisions.presentation }
    pub fn style_revision(&self) -> Revision { self.derived.revisions.style }
    pub fn camera_revision(&self) -> Revision { self.derived.revisions.camera }
    pub fn geometry_revision(&self) -> Revision { self.derived.revisions.geometry }
    pub fn set_revision(&self, id: SetId) -> Option<Revision> {
        self.derived.revisions.sets.get(self.set_index(id)?).copied()
    }
}
```

#### 粒度の決定と、演者単位を採らない理由

粒度は **文書全体 / スコープ別 / セット単位** の 3 段。**演者単位は持たない。**

理由: 下流の消費者はいずれも「あるセットの全演者配列」を単位に計算する。衝突走査は空間ハッシュを全点で作り直す（13）、`DisplayList` は全ドットを積む（20）、座標表は全行を出す（16）。1 人だけ動いたときに再計算を 1 人分に絞れる消費者が存在しないので、64,000 個のカウンタを維持しても節約されるのは 0 バイトで、`bump` のコストと `Derived` のメモリだけが増える。将来、演者単位の差分描画（20 の部分更新）を導入する場合は、セット revision に加えて「変更された演者索引のリングバッファ」を `Derived` に持たせる形で拡張する（§9）。

#### 変異 × スコープ行列

`geometry` 列は書かない — `SetScope` が `None` でなければ必ず上がる（`bump` の実装が保証する）。

`timeline` が上がる変異（`InsertSet` / `RemoveSet` / `MoveSet` / `SetCounts` / `SetTempo`）は、
`bump` の直後に `rebuild_set_starts()`（§3.8.2）も呼ぶ。`SetTempo` はセットのスパンを変えないので
厳密には不要だが、`timeline` スコープと `set_starts` の再構築を 1 対 1 に縛っておくほうが
「テンポを変えたら累積和がずれていた」種類のバグを構造的に排除できる。コストは 0.1µs。

| `Edit` 変異 | doc | topology | timeline | grid | tempo | audio | presentation | style | camera | set 単位 |
|---|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|:-:|---|
| `Nop` | — | — | — | — | — | — | — | — | — | — |
| `MovePoints` | ● | | | | | | | | | `One` |
| `RestorePositions` | ● | | | | | | | | | `All` |
| `InsertSet` | ● | ● | ● | | | | | | | `All`† |
| `RemoveSet` | ● | ● | ● | | | | | | | `All`† |
| `MoveSet` | ● | ● | ● | | | | | | | `All`† |
| `RenameSet` | ● | | | | | | ● | | | `One` |
| `SetCounts` | ● | | ● | | | | | | | `One` |
| `SetNote` | ● | | | | | | ● | | | `One` |
| `SetRoutes` | ● | | | | | | | | | `One` |
| `ApplyShape` | ● | | | | | | ● | | | `One` |
| `SetShape` | ● | | | | | | ● | | | — ‡ |
| `InsertPerformers` | ● | ● | | | | | ● | | | `All` |
| `RemovePerformers` | ● | ● | | | | | ● | | | `All` |
| `SetPerformerMeta` | ● | | | | | | ● | | | — |
| `RelabelPerformers` | ● | | | | | | ● | | | — |
| `AssignSection` | ● | | | | | | ● | | | — |
| `InsertSection` | ● | | | | | | ● | | | — |
| `RemoveSection` | ● | | | | | | ● | | | — |
| `SetSectionMeta` | ● | | | | | | ● | | | — |
| `InsertSubset` | ● | | | | | | ● | | | — |
| `RemoveSubset` | ● | | | | | | ● | | | — |
| `RenameSubset` | ● | | | | | | ● | | | — |
| `SetSubsetMembers` | ● | | | | | | ● | | | — |
| `InsertCameraTrack` | ● | | | | | | | | ● | — |
| `RemoveCameraTrack` | ● | | | | | | | | ● | — |
| `RenameCameraTrack` | ● | | | | | | | | ● | — |
| `SetCameraLook` | ● | | | | | | | | ● | — |
| `SetCameraBoundsMode` | ● | | | | | | | | ● | — |
| `InsertCameraKeyframe` | ● | | | | | | | | ● | — |
| `RemoveCameraKeyframe` | ● | | | | | | | | ● | — |
| `MoveCameraKeyframe` | ● | | | | | | | | ● | — |
| `SetCameraCut` | ● | | | | | | | | ● | — |
| `RemoveCameraCut` | ● | | | | | | | | ● | — |
| `SetTitle` | ● | | | | | | ● | | | — |
| `SetGrid` (scale=false) | ● | | | ● | | | | | | — |
| `SetGrid` (scale=true) | ● | | | ● | | | | | | `All` |
| `SetStepStyle` | ● | | | | | | | ● | | — |
| `SetTempo` | ● | | ● | | ● | | | | | — |
| `SetAudio` | ● | | | | | ● | | | | — |
| `Batch` | 各項目の和 | | | | | | | | | |

† セットの挿入・削除・移動は `Revisions::sets` の長さと並びを変えるので、既存の `(SetId, Revision)` ペアが指す意味が変わりうる。`All` にして安全側に倒す。位相変更は 1 操作あたり最大 256 回の `fetch_add`（約 1.3µs）で、頻度も低い。

‡ `SetShape` は座標を動かさないので**セット revision を上げない**。上げると `TransitionKey` が変わって
1,000 人の衝突走査が無駄に再実行される。図形オーバーレイの描画は `presentation` で無効化される。

**カメラ編集がフィールド側のキャッシュを一切汚さないこと**が、この行列で最も重要な性質である。
カメラ 10 変異はいずれも `doc` と `camera` しか上げないので、`TransitionKey`（衝突走査）・
`FrameKey`（2D DisplayList）・`SheetKey`（座標表・カウントシート）・`TimelineKey`（再生マッピング）の
どれも不変のまま。ドローン軌道を 200 キーフレーム打ち込んでも 2D 側は 1 回も再計算されない。
逆向きも成立する: 演者を動かしても `camera` は上がらないので、オーサリング済みのキーフレームを
参照するキャッシュ（補間済み姿勢テーブル等）は生き残り、追従（Follow）に必要な再計算だけが
`geometry` 経由で走る。

この行列は**テストで固定する**（§7 `revision_scope_matrix`）。変異を追加してこの表を更新し忘れると落ちる。

#### 下流のキャッシュキー

```rust
/// Key for a cached sweep-collision scan of the transition out of `set`
/// (see design 13). Both endpoint sets matter, plus the grid (collision radius
/// is expressed in grid units) and topology (performer count).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransitionKey {
    pub set: SetId,
    pub from: Revision,
    pub to: Revision,
    pub topology: Revision,
    pub grid: Revision,
    pub style: Revision,
}

impl TransitionKey {
    /// `None` when `set` is the last set (no transition) or unknown.
    pub fn current(doc: &Document, set: SetId) -> Option<Self> {
        let index = doc.set_index(set)?;
        let next = doc.sets().get(index + 1)?.id;
        Some(Self {
            set,
            from: doc.set_revision(set)?,
            to: doc.set_revision(next)?,
            topology: doc.topology_revision(),
            grid: doc.grid_revision(),
            style: doc.style_revision(),
        })
    }
}

/// Key for a cached `TransitionPlan` (design 11 §3.9, requested in its §9-8a).
///
/// Deliberately NOT `TransitionKey`: a plan is defined for the last set too
/// (a hold-only plan is still a plan, so this never returns `None` for a known
/// set), and it omits `grid` and `style` because routes are stored in field
/// units and `StepStyle` never enters the geometry. Omitting them means a grid
/// rescale does not discard plans a rescale cannot change.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PlanKey {
    pub set: SetId,
    pub from: Revision,
    /// The next set's revision, or `from` again on the last set.
    pub to: Revision,
    pub topology: Revision,
}

impl PlanKey {
    pub fn current(doc: &Document, set: SetId) -> Option<Self> {
        let index = doc.set_index(set)?;
        let from = doc.set_revision(set)?;
        let to = match doc.sets().get(index + 1) {
            Some(next) => doc.set_revision(next.id)?,
            None => from,
        };
        Some(Self { set, from, to, topology: doc.topology_revision() })
    }
}

/// Key for a cached `DisplayList` (design 20).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameKey {
    pub from: Revision,
    pub to: Revision,
    pub topology: Revision,
    pub grid: Revision,
    pub presentation: Revision,
    /// Quantized local count, so float jitter does not thrash the cache.
    pub count_milli: i32,
}

/// Key for a cached coordinate sheet / count sheet / continuity text (16, 17).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SheetKey {
    pub doc: Revision,
    pub grid: Revision,
    pub presentation: Revision,
    pub timeline: Revision,
    pub locale: Locale,
}

/// Key for the count-to-seconds mapping used by playback and export (30, 31).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimelineKey {
    pub timeline: Revision,
    pub tempo: Revision,
}

/// Key for `CameraFollowCache` (design 23 §3.7), which caches per-set-boundary
/// centroids of the follow subject. It depends on where performers are, NOT on
/// how the camera is authored — hence no `camera` field. This is why the
/// `GEOMETRY` aggregate scope exists: the cache spans every set, so it cannot
/// key on a single `set` revision.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CameraFollowKey {
    pub geometry: Revision,
    pub topology: Revision,
    pub timeline: Revision,
}

/// Key for a resolved `CameraPose` table (the 3D preview in design 22 and the
/// offline renderer in design 31). Depends on both the authored program and
/// the follow inputs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CameraPoseKey {
    pub camera: Revision,
    pub follow: CameraFollowKey,
    pub tempo: Revision,
}

/// Key for a resolved `Selection` expanded from a saved subset (design 15
/// §3.7). The intersection with the live roster changes when the roster
/// changes, so `topology` participates.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubsetKey {
    pub subset: SubsetId,
    pub presentation: Revision,
    pub topology: Revision,
}
```

`CameraPoseKey` に `geometry` が（`follow` 経由で）入る一方 `FrameKey` に `camera` が入らないのが、
「カメラは幾何に依存するが、幾何はカメラに依存しない」という一方向の依存を型に落とした形。

利用形（`drill-app` 側、13 が要求する「毎フレーム呼ばない」を実現する骨格）:

```rust
pub struct CachedScan {
    key: Option<TransitionKey>,
    events: Vec<CollisionEvent>,
    scratch: ScanScratch,
    dirty_since: Option<Instant>,
}

impl CachedScan {
    /// Returns the cached events, rescanning only when the key changed and the
    /// document has been quiet for `debounce`.
    pub fn get(&mut self, doc: &Document, set: SetId, params: &ClinicParams, debounce: Duration)
        -> &[CollisionEvent];
}
```

`TransitionKey::current` は 6 回のフィールド読みと 2 回の索引参照だけなので、毎フレーム呼んでも約 20ns。無効化判定は `!=` 1 回。

---

### 3.5 History

```rust
#[derive(Clone, Debug)]
struct Entry {
    /// The edit that undoes the user action this entry represents.
    inverse: Edit,
    gesture: Option<GestureId>,
    key: Option<CoalesceKey>,
    heap_bytes: usize,
    /// Monotonic per-history sequence number, used for the savepoint.
    seq: u64,
}

#[derive(Debug)]
pub struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    limit_entries: usize,
    limit_bytes: usize,
    bytes: usize,
    next_seq: u64,
    /// `seq` of the entry that was on top when the document was last saved.
    /// `Some(0)` means "saved with an empty history".
    savepoint: Option<u64>,
    /// Set when eviction discarded the savepoint entry: dirtiness is then
    /// unknowable, so we report dirty.
    savepoint_lost: bool,
    next_gesture: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PushOutcome {
    pub coalesced: bool,
    pub evicted: usize,
    /// The single edit exceeded `limit_bytes`; the whole history was cleared.
    pub truncated: bool,
}

impl History {
    pub const DEFAULT_ENTRIES: usize = 512;
    pub const DEFAULT_BYTES: usize = 64 << 20; // 64 MiB

    pub fn new(limit_entries: usize, limit_bytes: usize) -> Self;

    /// Starts a coalescing run. The app calls this on pointer-down / key-repeat
    /// start and passes the returned id with every edit of the gesture.
    pub fn begin_gesture(&mut self) -> GestureId;

    /// Records `inverse` (the value returned by `Edit::apply`).
    pub fn push(&mut self, inverse: Edit, gesture: Option<GestureId>) -> PushOutcome;

    pub fn undo(&mut self, doc: &mut Document) -> Result<bool, DrillError>;
    pub fn redo(&mut self, doc: &mut Document) -> Result<bool, DrillError>;

    pub fn can_undo(&self) -> bool { !self.undo.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo.is_empty() }

    /// Called after a successful save.
    pub fn mark_saved(&mut self);
    pub fn is_dirty(&self) -> bool;

    /// Called when the document is replaced wholesale (File > Open / New).
    pub fn reset(&mut self);

    pub fn bytes(&self) -> usize { self.bytes }
    pub fn len(&self) -> usize { self.undo.len() }
}
```

```rust
impl History {
    pub fn push(&mut self, inverse: Edit, gesture: Option<GestureId>) -> PushOutcome {
        let mut outcome = PushOutcome::default();
        if inverse.is_nop() {
            return outcome;
        }
        self.clear_redo();

        let key = inverse.coalesce_key();
        if let (Some(g), Some(k), Some(top)) = (gesture, key, self.undo.back()) {
            if top.gesture == Some(g) && top.key == Some(k) && coalescible(&top.inverse, &inverse) {
                // Keep the older inverse: it already restores the pre-gesture state.
                outcome.coalesced = true;
                return outcome;
            }
        }

        let bytes = inverse.heap_bytes();
        if bytes > self.limit_bytes {
            // A single edit cannot fit. Undoing it is impossible; make that
            // explicit rather than silently keeping a stack we will evict anyway.
            self.clear_all();
            self.savepoint_lost = true;
            outcome.truncated = true;
            return outcome;
        }

        self.next_seq += 1;
        self.undo.push_back(Entry { inverse, gesture, key, heap_bytes: bytes, seq: self.next_seq });
        self.bytes += bytes;

        while self.undo.len() > self.limit_entries || self.bytes > self.limit_bytes {
            let Some(front) = self.undo.pop_front() else { break };
            self.bytes -= front.heap_bytes;
            if self.savepoint == Some(front.seq) {
                self.savepoint_lost = true;
            }
            outcome.evicted += 1;
        }
        outcome
    }

    pub fn undo(&mut self, doc: &mut Document) -> Result<bool, DrillError> {
        let Some(entry) = self.undo.pop_back() else { return Ok(false) };
        self.bytes -= entry.heap_bytes;
        match entry.inverse.clone().apply(doc) {
            Ok(redo_inverse) => {
                let bytes = redo_inverse.heap_bytes();
                self.redo.push(Entry { inverse: redo_inverse, gesture: None, key: None, heap_bytes: bytes, seq: entry.seq });
                Ok(true)
            }
            Err(error) => {
                // The document moved out from under the history. Refuse rather
                // than half-apply, and let the app surface it.
                self.bytes += entry.heap_bytes;
                self.undo.push_back(entry);
                Err(error)
            }
        }
    }

    pub fn is_dirty(&self) -> bool {
        if self.savepoint_lost {
            return true;
        }
        let top = self.undo.back().map_or(0, |e| e.seq);
        self.savepoint != Some(top)
    }
}
```

`redo` は対称。`redo` から取り出したエントリを適用して得た逆操作を `undo` へ `push_back` する（合流はしない）。

`entry.inverse.clone()` が入るのは `apply` が `self` を消費するため。`clone` のコストは `MovePoints` 1,000 人で 12 KB のコピー（約 1µs）。これを嫌うなら `apply(&mut self)` にする手もあるが、「apply は値を消費して逆操作を返す」という代数的な形（`DESIGN_GAPS.md` A-1 の要求）を崩す方が高くつく。1µs は 2ms 予算の 0.05%。

#### dirty の一元化

`is_dirty()` が [main.rs:90](../../crates/drill-app/src/main.rs#L90) の手動 `bool` を置き換える。Undo で保存時点まで戻れば自動的に `false` に戻る。21 箇所の `self.dirty = true;` は全部消える。

#### メモリ量の見積り

`MovePoints`（1,000 人、`Targets::Ids`）1 件:

| 部位 | バイト |
|---|---:|
| `Vec<PerformerId>` の要素 1,000 × 4 | 4,000 |
| `Vec<Point>` の要素 1,000 × 8 | 8,000 |
| `Box<MovePoints>` の中身（SetId 4 + Targets 32 + Vec 24 + padding） | 64 |
| `Entry` inline（Edit 16 + gesture 4 + key 8 + heap_bytes 8 + seq 8 + padding） | 48 |
| アロケータのヘッダ 3 件 × 16 | 48 |
| **合計** | **≈ 12,160 B（11.9 KiB）** |

`Targets::All`（全員選択・自動割り当て）なら **8,160 B（8.0 KiB）**。

| シナリオ | 1 件 | 512 件（既定上限） |
|---|---:|---:|
| 1,000 人 MovePoints（Ids） | 11.9 KiB | 5.9 MiB |
| 1,000 人 MovePoints（All） | 8.0 KiB | 4.0 MiB |
| 50 人の部分選択移動 | 0.7 KiB | 0.35 MiB |
| `RemoveSet`（1,000 人 1 セット、shape 付き） | 12.2 KiB | — |
| `RemovePerformers`（100 人 × 64 セット） | 56 KiB | — |
| `SetGrid` scale=true（1,000 × 64） | 514 KiB | 上限 64 MiB で 127 件 |
| `ApplyShape`（1,000 人: order 4,000 B + points 8,000 B + spec 40 B） | 11.9 KiB | 5.9 MiB |
| `RelabelPerformers`（1,000 人: タプル 28,000 B + 文字列本体 ≈32,000 B） | 58.7 KiB | 上限 64 MiB で 1,100 件 |
| `AssignSection`（1,000 人: 8,000 B） | 7.9 KiB | 4.0 MiB |
| `SetSubsetMembers`（1,000 人の `BTreeSet`） | 16.1 KiB | 8.0 MiB |
| `InsertCameraKeyframe` / `MoveCameraKeyframe` / `SetCameraCut` | 0.06 KiB | 0.03 MiB |
| `RemoveCameraTrack`（500 キーフレーム + 32 カット） | 22.4 KiB | — |

カメラ系の逆操作が桁違いに小さいことに注意。ドローン軌道の 200 キーフレームを 1 つずつ打っても
履歴消費は 12 KB で、1,000 人の全体移動 1 回分にすら満たない。したがってカメラ編集に対して
履歴の上限緩和や特別扱いは要らない。

`RelabelPerformers` が全変異の中で最大の「1 件あたりバイト数」になる（58.7 KiB）。それでも
512 件上限に対して 30 MiB で、バイト上限 64 MiB の半分に収まる。これを `SetPerformerMeta` の
1,000 件に分解していたら、履歴 1 回の一括採番で上限 512 件を使い切っていた — 専用変異が
必要だった実際の理由がこの数字である。

**ドラッグの効き**: 合流が無ければ 60fps × 3 秒のドラッグで 180 件 × 11.9 KiB = 2.1 MiB を積む。合流により **1 件 11.9 KiB** になる。178 倍。

**大きな編集への戦略**（3 段）:

1. `heap_bytes <= limit_bytes / 8`（8 MiB）: 通常経路。
2. `limit_bytes / 8 < heap_bytes <= limit_bytes`: 積むが、バイト上限による前方追い出しが連鎖する。`PushOutcome::evicted > 0` を `drill-app` が受けて「古い操作の取り消し履歴を破棄しました」を状態バーに出す。
3. `heap_bytes > limit_bytes`（64 MiB 超、上限規模の全体スケーリング数十回分に相当）: 履歴を全消去し `truncated = true`。UI は操作前に「この操作は取り消せません。続行しますか」を確認する。黙って壊れた履歴を残さない。

ディスクへのスピルは採らない（§9）。

#### Undo/Redo 2ms 未満の根拠

Undo の仕事は 4 つに分解できる。**文書サイズに比例する処理は 1 つも無い** — すべて編集サイズに比例する。

| 段階 | 1,000 人 `MovePoints` の内訳 |
|---|---|
| `Entry` の `clone` | 12 KB のコピー、約 1.0µs |
| `precheck`（ID 解決 1,000 回 + 有限性 2,000 回） | `IdIndex::get` は境界チェック + L1/L2 ロードで約 2ns → 2.0µs、有限性は約 0.5µs |
| `Targets::resolve`（scratch 再利用、確保なし） | 1,000 push、約 0.6µs |
| 座標書き込み 1,000 点 + 逆操作の `Vec` 構築 | 8 KB 読み + 8 KB 書き + 8 KB 確保、約 2.5µs |
| `bump`（`fetch_add` × 2） | 0.01µs |
| **合計** | **約 6.6µs** |

主要な編集の実測見込み:

| 操作 | Undo 所要 | 2ms に対する比 |
|---|---:|---:|
| 50 人ドラッグ | 0.6µs | 0.03% |
| 1,000 人整列 | 6.6µs | 0.33% |
| 1,000 人自動割り当て（`Targets::All`） | 5.0µs | 0.25% |
| セット挿入・削除（1,000 人） | 12µs（`Vec::insert` の memmove + 索引再構築 2µs） | 0.6% |
| 演者 100 人削除（64 セット） | 110µs（64 × 1,000 要素の retain） | 5.5% |
| グリッド全体スケーリングの Undo（`RestorePositions` 64 セット） | 3µs（`mem::swap` × 64、コピーなし） | 0.15% |
| 1,000 人一括採番（`RelabelPerformers`） | 95µs（1,000 回の `String` 確保が支配的） | 4.8% |
| 1,000 人シェイプ適用の Undo（`ApplyShape`） | 8µs | 0.4% |
| `SetSubsetMembers`（1,000 人、`mem::swap`） | 0.1µs | 0.005% |
| カメラキーフレーム 1 件の挿入・移動（500 件のトラック） | 3µs（`sort_by` 込み、23 §3.3） | 0.15% |
| `RemoveCameraTrack`（500 キーフレーム + カット掃除） | 6µs | 0.3% |
| **上限規模** 4,000 人 `MovePoints` | 26µs | 1.3% |
| **上限規模** 演者 4,000 人削除（256 セット） | 約 1.6ms | 80% |

最悪ケース（上限規模での全演者削除）だけが予算に接近する。これは「劣化してよいが壊れてはいけない」上限規模での破壊的操作なので許容し、`drill-app` は 100ms を超える見込みの編集を 40（ジョブ基盤）へ回す判断材料として `Edit::estimated_cost()` を持たせる余地を残す（§9）。

キャッシュ無効化を Undo の中で行わないことが本質。`bump` は整数を書くだけで、再走査も再描画も次フレームの `TransitionKey` 比較まで遅延する。

---

### 3.6 検証と上限

#### 定数

```rust
pub mod limits {
    /// Reference scale is 1,000; upper supported scale is 4,000. The hard cap
    /// exists only to bound adversarial input.
    pub const MAX_PERFORMERS: usize = 16_384;
    pub const MAX_SETS: usize = 1_024;
    pub const MAX_SECTIONS: usize = 256;
    pub const MAX_SUBSETS: usize = 512;
    /// Camera tracks are authored by hand; a show uses a handful.
    pub const MAX_CAMERA_TRACKS: usize = 64;
    pub const MAX_CAMERA_KEYFRAMES: usize = 8_192;
    pub const MAX_CAMERA_CUTS: usize = 8_192;
    /// `FollowTarget::Group` member count.
    pub const MAX_FOLLOW_MEMBERS: usize = MAX_PERFORMERS;
    /// `ShapeAssignment::order`, and `ShapeSpec::Bezier/FreePath/Text` vertex
    /// counts (the latter checked by 14's `ShapeSpec::validate`).
    pub const MAX_SHAPE_ORDER: usize = MAX_PERFORMERS;
    /// `moves + hold` for one set.
    pub const MAX_SET_COUNTS: u32 = 4_096;
    /// Sum over all sets.
    pub const MAX_TIMELINE_COUNTS: u32 = 262_144;
    pub const MAX_TEMPO_EVENTS: usize = 8_192;
    pub const MAX_HASHES: usize = 64;
    /// Coordinates far outside the field are legitimate (props staged off
    /// stage), but not astronomically far.
    pub const MAX_COORDINATE: f32 = 10_000.0;
    pub const MAX_FIELD_EXTENT: f32 = 10_000.0;
    pub const MAX_TITLE_BYTES: usize = 512;
    pub const MAX_SET_NAME_BYTES: usize = 256;
    pub const MAX_LABEL_BYTES: usize = 64;
    pub const MAX_NOTE_BYTES: usize = 8_192;
    /// Upper-scale documents serialize to about 40 MB of pretty JSON.
    pub const MAX_JSON_BYTES: usize = 128 << 20;
}
```

#### `Document::validate()`

```rust
impl Document {
    /// Full structural check. O(sets * performers). Called on load, after
    /// migration, at the end of `DocumentBuilder::build`, and in tests.
    /// **Not** called per frame.
    pub fn validate(&self) -> Result<(), DrillError>;
}
```

| # | 検査 | `DrillError` |
|---|---|---|
| V1 | `schema_version == SCHEMA_VERSION` | `UnsupportedSchema { found, supported }` |
| V2 | `!sets.is_empty()` | `EmptySets` |
| V3 | `sets.len() <= MAX_SETS` / `performers.len() <= MAX_PERFORMERS` / `sections.len() <= MAX_SECTIONS` | `TooManyElements { what, found, limit }` |
| V4 | 全セットで `positions.len() == performers.len()` | `SetSizeMismatch { set_index, expected, found }` |
| V5 | `performers` / `sets` / `sections` の ID がそれぞれ一意 | `DuplicatePerformerId` / `DuplicateSetId` / `DuplicateSectionId` |
| V6 | 全 ID が対応する `IdAllocator::watermark()` 未満 | `IdWatermarkViolation { what, id, watermark }` |
| V7 | `sections` が空でない（`performer.section` の解決可能性は V25 で扱う — こちらは致命的エラーにしない） | `NoSections` |
| V8 | 全座標成分が `is_finite()` かつ `abs() <= MAX_COORDINATE` | `NonFiniteCoordinate { set_index, performer_index }` / `CoordinateOutOfRange {..}` |
| V9 | `grid.width` / `height` が有限かつ `(0, MAX_FIELD_EXTENT]` | `InvalidGrid { width, height }` |
| V10 | `grid.horizontal_steps >= 1` / `vertical_steps >= 1` / `resolution` が `1..=16` / `horizontal_units` `vertical_units` `major_line_interval` が有限かつ正 | `InvalidGrid {..}` |
| V11 | `grid.hashes.len() <= MAX_HASHES`、各 `position` が有限かつ `0..=height`、`weight` が有限かつ正 | `InvalidGrid {..}` |
| V12 | 各 `set.counts.moves as u32 + hold as u32 <= MAX_SET_COUNTS` | `InvalidCounts { set_index }` |
| V13 | `checked_add` による総カウントの累積が `MAX_TIMELINE_COUNTS` 以内 | `TimelineOverflow` |
| V14 | `tempo.validate()`: 事象数 <= `MAX_TEMPO_EVENTS`、`bpm: f64` が有限かつ `20.0..=400.0`、`count: f64` が有限・非負・厳密昇順（重複なし）・`<= MAX_TIMELINE_COUNTS`。§3.8.1 で両フィールドが f64 になった点に注意 | `InvalidTempo {..}` |
| V15 | `audio` が `Some` なら `track.validate()`（秒数の有限性・非負、trim/fade の整合、パスが相対かつ `..` を含まない） | `InvalidAudio {..}` |
| V16 | 各 `set.routes.validate(&performers, set.counts)` — 実装は 11。全 override キーが実在、制御点が有限、`gate.depart` が有限かつ `0.0..=moves`、`gate.arrive` が `Some(a)` なら `a` が有限かつ `depart..=moves`、`None`（= 移動窓の終わりに到着、11 §3.7 の `Gate::FULL`）は常に妥当。**番兵 `f32::INFINITY` は `Option` によって型として構築不能**（`serde_json` が非有限を `null` として書き、保存→読込で値が化ける問題を回避） | `InvalidRoute {..}` |
| V17 | 文字列のバイト長が上限内（`title` / `set.name` / `set.note` / `performer.label` / `section.name` / `section.short`） | `StringTooLong { what, found, limit }` |
| V18 | `performer.height_m` が有限かつ `0.5..=2.5`、`section.short` が空でない | `InvalidPerformer {..}` |
| V19 | `!self.poisoned` | `DocumentPoisoned` |
| V20 | `subsets.len() <= MAX_SUBSETS`、ID 一意、ID が watermark 未満、各 `name` が長さ上限内、各 `members.len() <= MAX_PERFORMERS`。**メンバー ID の実在は検査しない**（I-17） | `TooManyElements` / `DuplicateSubsetId` / `IdWatermarkViolation` / `StringTooLong` |
| V21 | 各 `set.shape` が `Some` なら `spec.validate()`（14 が実装）、`order.len() <= MAX_SHAPE_ORDER`、`order` に重複なし。**`order` の ID の実在は検査しない**（I-18） | `InvalidShape {..}` |
| V22 | `camera_program.tracks.len() <= MAX_CAMERA_TRACKS`、`CameraId` が一意、各トラックの `keyframes()` が `count` 昇順・重複なし（23 不変条件 2）、各キーフレームが `CameraKeyframe::validate()` を通る、`keyframes().len() <= MAX_CAMERA_KEYFRAMES`、`near`/`far` が有限かつ `0 < near < far` | `InvalidCameraTrack {..}` / `DuplicateCameraId` / `InvalidCameraKeyframe {..}` |
| V23 | `camera_program.cuts()` が `at` 昇順・重複なし・有限（23 不変条件 5）、`cuts().len() <= MAX_CAMERA_CUTS`、**各 `cut.camera` が実在するトラックを指す** | `InvalidCameraCut {..}` |
| V24 | `LookMode::Follow` の `FollowTarget::Group` が `MAX_FOLLOW_MEMBERS` 以内、`FollowDamping::time_constant_seconds` が有限かつ非負。**演者 ID の実在は検査しない**（23 §6 のフォールバック規約） | `InvalidCameraTrack {..}` |
| V25 | 全 `performer.section` が解決可能 — **警告であり `Err` にしない**。`validate()` は `Ok` を返し、`Document::dangling_section_refs() -> Vec<PerformerId>` が別途列挙する | （なし） |

V22〜V24 は `CameraProgram::normalize_after_load`（§3.2）が済んだ後の検査なので、「正規化しても直せない
壊れ方」だけを見る。同一 count の重複や NaN キーフレームは正規化で吸収されるため、ここに到達しない。

V25 が `Err` でないのは 15 の不変条件 3 に従うため。ファイル境界では「セクション参照が壊れていても
ドリル本体は開ける」（`resolved_color` が既定色を返す）が、編集境界では `AssignSection` /
`InsertPerformers` の `precheck` が実在を要求して**新たな宙吊り参照を作らせない**。
**厳格な入口（Edit）・寛容な入口（ファイル）** の非対称は意図的で、`PRODUCT_QUALITY.md` の
「外部ファイルが欠落してもドリル本体を開ける」と「破損データを黙って開かない」の折衷点にあたる。

V6 は見落としやすいが重要で、手書き JSON が `next_performer_id` より大きい ID を含むと、その後の採番が既存 ID と衝突する。`from_repr` の `observe` が watermark を持ち上げるので通常は成立するが、`validate` でも独立に確認して二重化する。

V8 の非有限検査を `validate` と `Edit::precheck` の両方に置くのは冗長ではない。前者はファイル境界、後者は UI 境界で、入口が別だから。

計算量: V4/V8 が支配的で `sets * performers` = 64,000 回の f32 検査。約 0.15ms。上限規模（256 × 4,000 = 1,024,000）で約 2.5ms。読み込み時 1 回なので許容。

#### `debug_assert_invariants()`

```rust
impl Document {
    /// Full `validate()` plus derived-state consistency. Debug builds only.
    /// Called at the end of every successful `Edit::apply`.
    #[cfg(debug_assertions)]
    pub fn debug_assert_invariants(&self) {
        if let Err(error) = self.validate() {
            panic!("document invariant violated: {error:?}");
        }
        // Derived state must equal a fresh rebuild.
        for (index, set) in self.sets.iter().enumerate() {
            assert_eq!(self.derived.sets_by_id.get(set.id.get()), Some(index));
        }
        for (index, p) in self.performers.iter().enumerate() {
            assert_eq!(self.derived.performers_by_id.get(p.id.get()), Some(index));
        }
        for (index, s) in self.sections.iter().enumerate() {
            assert_eq!(self.derived.sections_by_id.get(s.id.get()), Some(index));
        }
        for (index, s) in self.subsets.iter().enumerate() {
            assert_eq!(self.derived.subsets_by_id.get(s.id.get()), Some(index));
        }
        assert_eq!(self.derived.revisions.sets.len(), self.sets.len());
        assert_eq!(self.derived.sets_by_id.len(), self.sets.len());
        assert_eq!(self.derived.performers_by_id.len(), self.performers.len());
    }

    #[cfg(not(debug_assertions))]
    #[inline(always)]
    pub fn debug_assert_invariants(&self) {}
}
```

デバッグビルドで 1 編集あたり約 0.3ms（1,000 × 64）。ドラッグ中は 60fps で 18% の負荷になるので、`DRILLFORGE_SKIP_INVARIANTS=1` 環境変数で無効化できるようにする（既定は有効）。ストレステストは release で回す（§7）。

#### `fingerprint`

10,000 編集 Undo テストの検証手段。JSON 化は浮動小数の書式に依存するので使わず、正準バイト列を直接ハッシュする。

```rust
impl Document {
    /// Order-sensitive content hash. Equal documents hash equal; unequal
    /// documents hash unequal with overwhelming probability.
    ///
    /// Uses `f32::to_bits`, so `0.0` and `-0.0` hash differently even though
    /// they compare equal. That is intentional: an undo that produces `-0.0`
    /// where the original had `0.0` has lost information and should fail.
    pub fn fingerprint(&self) -> u64;
}
```

FNV-1a を 8 バイト単位で回し、`u16`/`u32`/`usize`/`f32::to_bits`/文字列バイト列/`Option` のタグ、各 `Vec` の長さを順に混ぜる。`derived` と `poisoned` は含めない（`PartialEq` と同じ範囲）。1,000 × 64 で約 0.05ms。

`content_eq` は `PartialEq` そのもの。`a == b ⟹ a.fingerprint() == b.fingerprint()` を property test で固定する（§7）。

---

### 3.7 スキーマ v2 とマイグレーション

```rust
pub const SCHEMA_VERSION: u16 = 2;
```

#### 読み込みパイプライン

```rust
/// The only supported entry point for untrusted document bytes.
pub fn load_json(bytes: &[u8], locale: Locale) -> Result<Document, DrillError> {
    if bytes.len() > limits::MAX_JSON_BYTES {
        return Err(DrillError::InputTooLarge { found: bytes.len(), limit: limits::MAX_JSON_BYTES });
    }
    // serde_json enforces a nesting depth limit of 128 unless the
    // `unbounded_depth` feature is enabled. It must never be enabled.
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| DrillError::Json(e.to_string()))?;
    migrate(&mut value)?;
    let doc: Document =
        serde_json::from_value(value).map_err(|e| DrillError::Json(e.to_string()))?;
    doc.validate()?;
    let _ = locale; // messages are resolved by the caller via DrillError::message
    Ok(doc)
}

/// Atomic replacement is design 41's responsibility; this only produces bytes.
impl Document {
    pub fn to_json(&self) -> Result<String, DrillError>;
}
```

`Document::from_json(&str) -> Result<Self, String>`（[lib.rs:399](../../crates/drill-core/src/lib.rs#L399)）は削除し、`load_json` に一本化する。

#### 連鎖の形

```rust
type MigrationFn = fn(&mut serde_json::Value) -> Result<(), DrillError>;

/// (from, to, f). Append only. Never edit an entry after release, and never
/// remove one: a v1 file must still open in v9.
const MIGRATIONS: &[(u16, u16, MigrationFn)] = &[(1, 2, v1_to_v2::migrate)];

fn migrate(value: &mut serde_json::Value) -> Result<(), DrillError> {
    // Bounded: each step strictly increases the version, and there are at most
    // MIGRATIONS.len() steps. A malformed table can never loop forever.
    for _ in 0..=MIGRATIONS.len() {
        let found = read_version(value)?;
        if found == SCHEMA_VERSION {
            return Ok(());
        }
        if found > SCHEMA_VERSION {
            return Err(DrillError::UnsupportedSchema { found, supported: SCHEMA_VERSION });
        }
        let step = MIGRATIONS
            .iter()
            .find(|&&(from, _, _)| from == found)
            .ok_or(DrillError::UnsupportedSchema { found, supported: SCHEMA_VERSION })?;
        (step.2)(value)?;
        let after = read_version(value)?;
        debug_assert_eq!(after, step.1);
        if after <= found {
            return Err(DrillError::MigrationStalled { at: found });
        }
    }
    Err(DrillError::MigrationStalled { at: read_version(value)? })
}

fn read_version(value: &serde_json::Value) -> Result<u16, DrillError> {
    let raw = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(DrillError::UnsupportedSchema { found: 0, supported: SCHEMA_VERSION })?;
    u16::try_from(raw).map_err(|_| DrillError::UnsupportedSchema { found: u16::MAX, supported: SCHEMA_VERSION })
}
```

将来 v3 を足すときは `MIGRATIONS` に `(2, 3, v2_to_v3::migrate)` を追加するだけ。v1 のファイルは `v1_to_v2` → `v2_to_v3` と 2 段で通る。**既存のマイグレーション関数は絶対に変更しない**（一度出荷したファイルの読み方が変わるため）。各段にフィクスチャ対（入力 v_n、期待出力 v_{n+1}）をコミットする。

#### v1 → v2 の写像

| v1 | v2 | 規則 |
|---|---|---|
| `schema_version: 1` | `2` | 定数 |
| `title` | `title` | そのまま。> 512 B なら `StringTooLong` |
| `grid.*`（`hashes` 以外） | 同名 | そのまま |
| `grid.hashes[].label: String` | `grid.hashes[].kind: HashKind` | 後述 |
| `performers[].id: u32` | `id: PerformerId` | `0` は `InvalidId` で拒否（v1 の `Document::demo` は 0 始まりなので **全 ID に +1 する**） |
| `performers[].label` | `label` | そのまま |
| `performers[].color` | `color: Some(color)` | セクション色の継承は使わない |
| （なし） | `section` | 新設セクション 1 に全員所属 |
| （なし） | `kind` | `PerformerKind::Wind` |
| （なし） | `symbol` | `Symbol::Circle` |
| （なし） | `height_m` | `1.75` |
| （なし） | `sections` | `[{ id: 1, name: "Ensemble", short: "Ens", color: [200,200,200] }]` |
| `sets[i].name` | `name` | そのまま |
| `sets[i].counts: u16` | `counts: { moves: counts, hold: 0 }` | ホールドは v1 に概念が無い |
| `sets[i].positions` | `positions` | そのまま（数値は触らない） |
| （なし） | `sets[i].id` | 出現順に `1..=n` |
| （なし） | `sets[i].routes` | `RouteTable::default()`（Straight / 全カウントゲート / Linear） |
| （なし） | `sets[i].note` | `""` |
| （なし） | `sets[i].shape` | `None`。v1 には図形の由来情報が無く、既存セットが円に見えても**推測して `Some` を作らない**（誤った再編集の起点になる） |
| （なし） | `subsets` | `[]`。v1 に保存選択の概念が無い |
| （なし） | `camera_program` | `CameraProgram::default()`（トラック 0 本・カット 0 個・`next_camera_id: 0`）。v1 の `Camera`（`camera.rs`）は `drill-app` の一時的なプレビュー状態で文書に保存されていないため、移行元が存在しない |
| （なし） | `style` | `StepStyle::EightToFive` |
| `tempo.events[].count: f32` | `count: f64` | **写像コードは不要**（§3.8.1）。JSON に浮動小数の幅は無く、`serde_json` は数値を `f64` で保持するので、型を広げるだけで読める |
| `tempo.events[].bpm: f32` | `bpm: f64` | 同上 |
| `audio.offset_seconds` | `audio.anchors: [{ count: 0.0, seconds: offset }]` | 30 と協調。他フィールドはそのまま |
| （なし） | `ids` | `{ set: n+1, performer: max_id+2, section: 2, subset: 1 }` |

`camera_program` / `subsets` / `shape` はいずれも `#[serde(default)]` なので、v1 の JSON に無くても
`DocumentRepr` の deserialize が通る。したがって v1→v2 のマイグレーション関数はこれらのキーを
**書き足さない**（`schema_version` の更新と、ID・`counts`・`hashes` の写像だけを行う）。
`to_json` で保存し直した時点で既定値が明示的に書き出される。

**`PerformerId` の +1 について**: v1 の `Document::demo` は `id: i as PerformerId` で 0 から採番する（[lib.rs:260](../../crates/drill-core/src/lib.rs#L260)）。v2 の `PerformerId` は `NonZeroU32` なので 0 を表現できない。全 ID に一律 +1 する。v1 には `PerformerId` を参照する他フィールドが存在しない（`RouteTable` も `Section` も `Subset` も `ShapeAssignment` も `FollowTarget` も無い）ので、この再番号付けは**参照整合性を壊さない**。この事実は v1 のスキーマ全体を確認したうえでの結論であり、v2 以降は `PerformerId` を参照する場所が 5 か所に増えるので、**ID の再番号付けは今後のどのマイグレーションでも許されない**。

**ハッシュラベルの写像**: v1 の `GridLine.label` は日本語リテラル（`"フロントハッシュ"` / `"バックハッシュ"` / `"センターライン"`、[lib.rs:76](../../crates/drill-core/src/lib.rs#L76) / [81](../../crates/drill-core/src/lib.rs#L81) / [111](../../crates/drill-core/src/lib.rs#L111)）。v2 は `HashKind`（16 が所有）に持ち替える。

```rust
// migrate/v1_to_v2.rs — the ONE place in drill-core allowed to contain
// Japanese byte strings. These are v1 *data* patterns, never displayed.
fn hash_kind(label: &str, position: f32, height: f32) -> HashKind {
    match label {
        "フロントハッシュ" | "Front Hash" => HashKind::FrontHash,
        "バックハッシュ"   | "Back Hash"  => HashKind::BackHash,
        "センターライン"   | "Center Line" => HashKind::CenterLine,
        other => {
            // Positional fallback for hand-edited v1 files.
            if (position - height * 0.5).abs() < 0.01 {
                HashKind::CenterLine
            } else {
                HashKind::Custom { name: other.to_string() }
            }
        }
    }
}
```

規約「`drill-core` に日本語文字列リテラルを置かない」に対する**唯一の例外**として、`migrate/v1_to_v2.rs` にのみ許可する。理由: これは表示用テキストではなく、既存ファイルを識別するためのデータパターンであり、`Locale` 経由にできない（過去のファイルの中身は変えられない）。`v1_to_v2.rs` の先頭にこの理由を doc comment で明記し、`#![allow]` ではなく grep で見つかるよう `// I18N-EXEMPT: v1 data pattern` を各行に付ける。

**マイグレーションが「直さない」もの**: v1 で ID が重複しているファイルは、+1 しても重複したままで `validate` の V5 が拒否する。曖昧さのある破損データを黙って補修しない（`PRODUCT_QUALITY.md`「破損データを黙って開かない」）。補修してよいのは、写像が可逆かつ一意な場合（ID の一律 +1 のような）に限る。

#### 信頼できない JSON への上限

| 攻撃 | 対処 |
|---|---|
| 巨大ファイル | `MAX_JSON_BYTES = 128 MiB` を parse 前に検査 |
| 深い入れ子（スタック溢れ） | `serde_json` の既定再帰上限 128。`unbounded_depth` / `disable_recursion_limit` を**使わない**ことを CI で検査（`Cargo.toml` の feature 一覧を grep） |
| 要素数爆発 | `validate` の V3。パース中の確保は入力バイト数で押さえられる（JSON には長さ前置が無いので `Vec::with_capacity` 爆弾が成立しない）。128 MiB の入力から生成しうる数値は最大約 1,400 万個 = 112 MB の `Vec<Point>`。ピークで入力の約 2 倍という増幅率は許容範囲 |
| `NaN` / `Infinity` リテラル | JSON の文法外なので `serde_json` が拒否 |
| `1e400` のような範囲外指数 | `serde_json` は `f64::INFINITY` に丸めるので**パスする**。V8 / V9 / V14 の `is_finite()` が唯一の防壁 |
| `-0.0` | 通す（有効な座標）。`fingerprint` はビットで区別する |
| ID 0 | `NonZeroU32` の deserialize が拒否 |
| ID 重複 | V5 |
| watermark より大きい ID | `from_repr::observe` が持ち上げ、V6 が二重確認 |
| 負のカウント / 巨大なカウント | `u16` の型で下限、V12 / V13 で上限 |
| 巨大文字列 | V17（`MAX_JSON_BYTES` が第一の壁、V17 が第二） |
| 絶対パス・`..` を含む音源パス | V15（実体判定は 41） |
| 座標配列長の不一致 | V4 |
| `sets` が空 | V2（`positions_at` 系の添字パニックを防ぐ） |

パニック禁止経路の徹底: `document` モジュール全体に

```rust
#![deny(clippy::indexing_slicing, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
```

を適用する。`apply_checked` 内の `expect("prechecked")` だけは例外とし、`#[allow]` を関数単位で付けて理由を doc comment に書く（`precheck` が事前に保証しているため）。

---

### 3.8 数値精度に関わる v2 の型決定（スキーマ所有者としての裁定）

11 §3.12 と 30 §3.12-3 が `f32` の精度不足を検算で確定させた。**永続形式に現れる型は
v2 を切る前に決めなければならない**（後から変えると v3 マイグレーションが要る）。
以下は本書の裁定であり、11 の未決 5・7・8・9 への回答を兼ねる。

#### 3.8.1 `TempoChange` を `{ count: f64, bpm: f64 }` にする

11 §9-9 が本書へ回した論点。**両フィールドとも f64 にする。v2 に含める。**

**なぜ f32 では足りないか。** 11 §3.12.1 の検算より、グローバルカウント軸の `f32` ulp は
960 カウント（8 分 @ 120 BPM）で 6.1e-5 カウント = 48 kHz の 1.46 サンプル、
`MAX_TIMELINE_COUNTS = 262,144` で 0.031 カウント = 750 サンプル。
`TempoChange.count` は**まさにその軸上の位置**なので、非整数のアンカーを置いた瞬間に
保存経路が量子化する。11 の前置和 `starts`/`prefix` を `f64` にしても、
入力アンカーが `f32` なら入口で既に情報が落ちている。

整数カウントに限れば `f32` は 2²⁴ = 16,777,216 まで厳密なので `MAX_TIMELINE_COUNTS` を
余裕で覆う。したがって「テンポ変更点は必ず整数カウント」なら `f32` で足りた。
だが実際には足りない。(1) 現行 UI の `DragValue`（[main.rs:1147-1149](../../crates/drill-app/src/main.rs#L1147)）は
任意の `f32` を受ける。(2) 30 が計画している自動 BPM 推定・アンカー吸着（±60 ms）は
音声側の実測値からテンポ区間を導くので、非整数のアンカーが自然に発生する。
(3) 段階的なアッチェレランドを細かい区間列で近似する運用では、区間境界が整数に乗る保証がない。

**なぜ `bpm` も f64 にするか。** `bpm` は位置ではなく**レート**なので、誤差は
「直前のアンカーからの距離」に比例し、アンカーを密に置けば抑えられる — つまり
`count` ほど切実ではない。それでも f64 にするのは、判断の目的が「v3 を避けること」だからである。
`TempoChange` はフィールドが 2 つしかない。片方だけ広げて、あとで `bpm` も要ると判明したら
結局 v3 が要る。両方広げるコストは `MAX_TEMPO_EVENTS = 8,192` で最大 64 KB → 128 KB
（現実のショーはテンポ事象 10 個未満なので 80 B → 160 B）。
**迷う余地のある側へ倒すコストが実質ゼロなので、倒す。**

参考までに `bpm` を `f32` に留めた場合の実害の大きさ: 120 BPM 付近の相対 ulp は 6.4e-8 で、
480 秒の定テンポ区間で 3.05e-5 秒 = 1.46 サンプルのずれ。「120」のような表現可能な値なら
誤差ゼロ、自動推定の「119.7」のような値でのみ発生する。30 の `SyncAnchor` 列が
区間ごとにピン留めするので実用上は吸収されるが、吸収に頼らずに済むならその方がよい。

```rust
// crates/drill-core/src/tempo.rs (v2)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TempoChange {
    /// Global count. f64: this is a position on the timeline axis, and f32
    /// quantizes it to 1.46 samples at 960 counts (design 11 section 3.12.1).
    pub count: f64,
    /// Beats per minute. f64 for symmetry, so a future need for rate precision
    /// never forces a schema v3.
    pub bpm: f64,
}
```

**v1→v2 のマイグレーションは、この変更については恒等写像である。** JSON には浮動小数の
幅という概念が無く、数値リテラルは `serde_json` が `f64` として保持する。v1 ファイルの
`{"count": 4.0, "bpm": 120.0}` は `f64` へそのまま読める。したがって
`migrate_v1_to_v2` に `tempo` 用のコードは**1 行も要らない**（`#[serde(default)]` と
同じく、型の広がりだけで吸収される）。

ただし 1 点だけ厳密でない挙動がある。v1 が `bpm: 119.7` を保持していた場合、メモリ上の
`f32` 値は 119.69999694824219 だが、`serde_json` は往復する最短表現 `"119.7"` を書く。
これを `f64` で読むと 119.7（f64）になり、`f32` 値を単に widening した値とは
2.5e-8 相対だけ異なる。**利用者が入力した値に近づく方向のずれ**であり実害は無いが、
v1 往復のゴールデンテストは「widening 後の f32 と一致」ではなく
「JSON テキストの数値と一致」を期待値にすること。§7.5 の
`v1_tempo_widens_from_json_text_not_from_f32_bits` で固定する。

**他の永続 f32 は変えない。** 11 §3.12.4 の切り分け表を本書としても採択する。

| 永続フィールド | v2 の型 | 理由 |
|---|---|---|
| `TempoChange.count` / `.bpm` | **f64**（変更） | 上記 |
| `Point.x` / `.y` | f32（据置） | `MAX_COORDINATE = 10,000` で ulp = 0.9 mm、フィールド内なら 7 µm。f64 にすると positions が 1,000×64 で 1 MB になり、`positions_as_f32()` の GPU 直送も壊れる |
| `Gate.depart` / `Gate.arrive` | f32（据置） | セット内ローカル。`MAX_SET_COUNTS = 4,096` で ulp = 4.9e-4 カウント = 幾何 0.3 mm |
| `SetCounts.moves` / `.hold` | u16（据置） | 整数 |
| `ShapeSpec` の幾何量 | f32（据置） | フィールド単位 |
| `CameraKeyframe.count` | f32（**要確認**、§9.1） | グローバルカウント軸。ここだけ切り分け表から漏れている — 下記 |
| `CameraCut.at` | f32（**要確認**、§9.1） | 同上 |
| `AudioTrack` の秒 / `SyncAnchor` | 30 が所有 | 同上の軸なので 30 の裁定に従う |

`CameraKeyframe.count` と `CameraCut.at` は 23 が `f32` で定義しているが、**グローバルカウント軸**
なので `TempoChange.count` と同じ議論が当てはまる。ただし影響は質的に違う: テンポは
そこから秒を導く写像の定義そのものであるのに対し、カメラのキーフレームは
「その count で姿勢がこう」というオーサリング値で、1/32 カウント（15.6 ms）ずれても
映像として観測できない（60 fps の 1 フレームは 16.7 ms）。したがって
**`f32` のままでよい**と裁定する。ただし `MAX_TIMELINE_COUNTS` 近傍では
`insert_keyframe` の `k.count == keyframe.count` による同一判定が 0.031 カウント刻みに
なる（＝ 近接する 2 つのキーフレームが同一とみなされうる）ので、23 の
`CameraKeyframe::validate` に `count.abs() <= MAX_TIMELINE_COUNTS` を足すことを依頼する（§9.1）。

#### 3.8.2 グローバルカウントの f64 API と整数境界

11 §3.12.5 の「型境界を 1 か所に閉じ込める」を本書側で実装する。11 の未決 5
（累積和キャッシュの所有者）はここで決着 — `Derived` を持つ本書が持つ。

```rust
impl Document {
    /// Total counts on the timeline. Exact integer sum, from `Derived::set_starts`.
    pub fn timeline_counts(&self) -> u32 {
        self.derived.set_starts.last().copied().unwrap_or(0)
    }

    /// The ONE place a global count is narrowed to f32.
    ///
    /// The set index is decided by comparing against the integer prefix sums in
    /// `Derived::set_starts`, so no float type can change which set the
    /// playhead is in. Only the returned set-local remainder is f32, and it is
    /// bounded by `MAX_SET_COUNTS` where f32 is exact to 0.3 mm of geometry.
    pub fn locate_count_f64(&self, global_count: f64) -> (usize, f32) {
        let total = f64::from(self.timeline_counts());
        let clamped = if global_count.is_finite() { global_count.clamp(0.0, total) } else { 0.0 };
        let starts = &self.derived.set_starts;
        // starts is sorted and has len == sets.len() + 1.
        let i = starts
            .partition_point(|&s| f64::from(s) <= clamped)
            .saturating_sub(1)
            .min(self.sets.len().saturating_sub(1));
        let local = clamped - f64::from(starts[i]);
        (i, local as f32)
    }

    /// f32 convenience for UI scrubbing. Widening is exact, so there is no
    /// second implementation of the set-boundary walk.
    #[inline]
    pub fn locate_count(&self, global_count: f32) -> (usize, f32) {
        self.locate_count_f64(f64::from(global_count))
    }

    pub fn global_count_f64(&self, set_index: usize, local_count: f32) -> f64 {
        let base = self.derived.set_starts.get(set_index).copied().unwrap_or(0);
        f64::from(base) + f64::from(local_count)
    }

    /// f32 convenience. Lossy above about 2^13 counts; display only.
    #[inline]
    pub fn global_count(&self, set_index: usize, local_count: f32) -> f32 {
        self.global_count_f64(set_index, local_count) as f32
    }
}
```

現行の `locate_count`（[lib.rs:367-382](../../crates/drill-core/src/lib.rs#L367)）は 256 セットの線形走査を
`f32` の減算で行っており、走査するほど丸めが積む。二分探索 + 整数基点にすると、
探索は 8 段（256 セット）で丸めはゼロ、`(i, local)` は入力から一意に決まる。

#### 3.8.3 `playback.rs` の所有者と `advance` の裁定（11 未決 7）

**所有者は本書（doc 10）とする。** `playback.rs` は `drill-core` の時間モデルであり、
30 は `drill-audio` を、43 は `drill-app` を所有する。30 §3.7.3 は
「`drill_core::playback::advance` は削除しない。`Transport` の単調時計経路が内部でこれを呼ぶ」
と明記しているので、`drill-core` 側に残る以上、中核モデルの所有者が持つのが筋である。

**そのうえで、現行の `advance` は署名を変える。** 単に f64 化するだけでは足りない。

現行（[playback.rs:44-81](../../crates/drill-core/src/playback.rs#L44)）は
`advance(current_count, elapsed_seconds /* = フレームの dt */, ...)` で、
**毎フレーム前回位置に dt を足し込む**形になっている。これは
`00-conventions.md` の不変条件 4「浮動小数の累加で時間を進めない」に正面から反する。
f64 にしても累加であることは変わらず、60 fps × 2 時間 = 432,000 回の加算になる
（f64 なら誤差は 1e-11 秒で無害だが、問題は精度ではなく**壁時計 dt のジッタが
再生位置に永久に residue として残る**こと ＝ 決定論の喪失）。
`PRODUCT_QUALITY.md`「全カウント、セット境界、可変BPM、部分ループで再生位置が決定論的である」
を満たさない。

裁定: **再生位置を「再生開始からの経過時間の純関数」にする。**

```rust
// crates/drill-core/src/playback.rs (v2)

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackRange {
    pub start: f64,
    pub end: f64,
}

impl PlaybackRange {
    pub fn new(start: f64, end: f64, total_counts: f64) -> Self;
    pub fn is_empty(self) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AdvanceResult {
    Running(f64),
    Looped(f64),
    Stopped(f64),
}

/// Playback position as a pure function of elapsed time since the transport
/// last anchored.
///
/// Replaces the previous `advance(current_count, dt, ...)`, which integrated a
/// per-frame wall-clock delta. That form violated `00-conventions.md` invariant
/// 4 (never advance time by accumulating floats) and made the position depend
/// on frame pacing, so two runs of the same show did not agree.
///
/// `origin_count` and `elapsed_seconds` are re-anchored by the caller whenever
/// the user seeks or changes speed — the same `epoch` concept design 30's
/// `ClockShared` already carries.
///
/// Deterministic and allocation-free. Looping wraps the *total* elapsed span,
/// so a 30-minute loop is exact rather than accumulating one rounding per lap.
pub fn position_after(
    origin_count: f64,
    elapsed_seconds: f64,
    speed: f64,
    range: PlaybackRange,
    looping: bool,
    tempo: &TempoMap,
) -> AdvanceResult;
```

`advance` は**削除する**（残さない）。引数の並びが同じまま意味だけ変わると呼び出し側が
黙って壊れるので、改名して全呼び出し箇所をコンパイルエラーにする。対象は
[main.rs:488-509](../../crates/drill-app/src/main.rs#L488) の 1 か所と、30 の `Transport` 単調時計経路。

この変更は 30 に**得**をもたらす。30 §7 の property test P-1
（「`advance` の結果と `wrap_timeline` の結果が ±1 サンプル以内」）は、
`wrap_timeline` が絶対サンプル位置に対する厳密な整数演算であるのに対し
`advance` が毎フレーム累加する形のままでは長時間再生で必ず乖離して落ちる。
両者を「絶対経過量の関数」に揃えて初めて成立する。

30 §3.7.2 の `PlaybackClock::position_count` が
`tempo.count_at(self.position_seconds(now, speed) as f32)` としている箇所は、
11 の指摘どおり `count_at_f64(self.position_seconds(now, speed))` に直し、
戻り値を `f64` にすることを依頼する（§9.1）。

`PlaybackRange` の f64 化にともない、`Document::timeline_counts()` が `u32` を返すこと
（上記 3.8.2）が効いてくる: 範囲の端点は整数カウントで指定されるのが通常なので、
`PlaybackRange::new(start, end, f64::from(doc.timeline_counts()))` は丸めを一切生まない。

#### 3.8.4 `Gate::arrive: Option<f32>` と `SetCounts` の整合（11 §6.2 との衝突の解消）

11 §3.7 は `Gate { depart: f32, arrive: Option<f32> }` とし、`None` を「移動窓の終わりに到着」
と定義した。番兵 `f32::INFINITY` を使わない理由（`serde_json` が非有限を `null` として書き、
保存 → 読込で `arrive` が化ける）は妥当で、本書の V16 もこれに合わせる。

しかし 11 §6.2 と本書のあいだに**実在する矛盾**が見つかった。11 は

```
moves = 16, gate.arrive = Some(12.0)  →  Edit::SetCounts { moves: 8 }
```

について「適用すると `Document::validate` の V16 が失敗する文書ができるので、アプリは
`Batch[SetCounts, SetRoutes{clamped}]` を出す義務がある」とし、テスト S5 で
「`SetCounts` 単独なら `validate()` が失敗すること」を固定しようとしている。

これは本書の **I-11（検証の冪等性: valid な文書に成功適用した結果もまた valid）** に反する。
本書は全 `apply` の末尾で `debug_assert_invariants()` を走らせるので、
「適用は成功したが文書が invalid」という状態はデバッグビルドでパニックする。

裁定: **`SetCounts` の `precheck` で拒否する。** 適用させてから検証で落とすのではなく、
最初から適用させない。

```rust
// Edit::SetCounts precheck, in addition to the bounds in 3.3.4
// Rejects a shrink that would strand a gate, so the document can never be
// left in a state `validate()` rejects (invariant I-11).
if change.counts.moves < current.counts.moves
    && set.routes.max_arrive() > f32::from(change.counts.moves)
{
    return Err(DrillError::CountsWouldStrandRoutes {
        set: change.set,
        moves: change.counts.moves,
    });
}
```

`RouteTable::max_arrive()`（既定ルートと全 override の `arrive` の最大値、`None` は
`moves` とみなさず 0 を返す）は 11 に追加を依頼する。これは `overrides` の線形走査で、
数十件なら 100 ns 未満。`SetCounts` は離散操作なので予算に影響しない。

アプリの義務は 11 の記述どおり `Batch[SetCounts, SetRoutes{ routes.clamped_to(moves) }]`。
`Batch` は先頭から適用されるので、`SetRoutes` を**先**に置く必要がある点に注意
（`SetCounts` を先に置くと上の `precheck` で弾かれる）。正しい順序は

```rust
Edit::Batch(vec![
    Edit::SetRoutes(Box::new(SetRoutesChange { set, routes: clamped })),
    Edit::SetCounts(CountsChange { set, counts: shorter }),
])
```

で、逆操作は自動的に `Batch[SetCounts{元}, SetRoutes{元}]`（逆順）になり、こちらも
順序として正しい（カウントを戻してからルートを戻す）。`Batch` の逆操作が逆順になる
本書の規則が、そのままこの順序制約を満たす。11 のテスト S5 は「`SetCounts` 単独で
`validate()` が失敗する」から「`SetCounts` 単独が `CountsWouldStrandRoutes` で失敗し、
文書は無変更」に読み替える（§9.1 で依頼）。

### 3.9 既存バグが構造的に消える理由

#### #1 セット複製が履歴に積まれない（[main.rs:828-841](../../crates/drill-app/src/main.rs#L828)）

現状は `self.document.sets.insert(...)` を直接呼べる。v2 では `Document::sets` が `pub(in crate::document)` になるので、**`drill-app` からはこの行がコンパイルできない**。セットを増やす手段は `Edit::InsertSet` だけになる。

さらに `Edit::apply` は逆操作を**返り値として**返す。返り値を捨てるコードは `#[must_use]` で警告になる:

```rust
impl Edit {
    #[must_use = "the returned inverse must be pushed onto the history"]
    pub fn apply(self, doc: &mut Document) -> Result<Edit, DrillError>;
}
```

「適用したが履歴に積み忘れた」が警告として可視化される。加えて `drill-app` 側は `Edit` を直接 `apply` せず、`DocumentState::edit(&mut self, edit: Edit, gesture: Option<GestureId>)` という一箇所のヘルパを通す（43 が所有）。ヘルパの中で `apply` → `history.push` が必ず対になる。同じ理由で、グリッド編集（[main.rs:1043-1132](../../crates/drill-app/src/main.rs#L1043)）・テンポ編集（[main.rs:1135-1192](../../crates/drill-app/src/main.rs#L1135)）・音源設定（[main.rs:1395-1416](../../crates/drill-app/src/main.rs#L1395)）も自動的に Undo 可能になる。

#### #2 `MoveCommand.set_index` が添字（[lib.rs:407](../../crates/drill-core/src/lib.rs#L407)）

3 段階で消える。

1. **全変異が `SetId` で対象を指す。** 履歴に `usize` のセット索引は 1 つも入らない。セットを挿入・削除・移動しても、履歴中の `SetId` の指す先は変わらない。
2. **解決が失敗すると `Err(UnknownSet)` になる。** 現状の `MoveCommand::apply`（[lib.rs:465-478](../../crates/drill-core/src/lib.rs#L465)）は `get_mut` が `None` のとき黙って何もしない。新設計の `precheck` は `require_set` でエラーを返し、`History::undo` はエントリをスタックに戻して呼び出し側にエラーを渡す。「別のセットを破壊する」代わりに「何もせず失敗を報告する」。
3. **ID は再利用されない。** `RemoveSet` の逆操作は元の `SetId` を持つ `InsertSet` なので、削除 → Undo の後も履歴の他のエントリが指す `SetId` が有効に戻る。watermark を戻さない採番規則がこれを保証する。

同じ問題の**演者版**（[main.rs:169-188](../../crates/drill-app/src/main.rs#L169) の `commit_layout` が `BTreeSet<usize>` の索引を履歴に積んでいる）も、`Targets::Ids(Vec<PerformerId>)` になることで同時に消える。現状は演者の追加・削除 UI が無いためまだ顕在化していないが、A-5 で `Section` と演者編集が入った瞬間に #2 と同じ壊れ方をする箇所だった。

#### #5 `History::push` の `Vec::remove(0)`（[lib.rs:433](../../crates/drill-core/src/lib.rs#L433)）

`VecDeque::pop_front()` が O(1)。500 件 × 80 バイトの memmove（約 2µs）が毎編集から消える。

加えて、現状の `Vec` + `cursor` の単一スタック構造そのものを廃し、`undo: VecDeque` / `redo: Vec` の 2 スタックにする。`cursor` を持たないので、「上限到達で先頭を削ったときに cursor が指す位置がずれる」種類の添字バグが構造的に存在しない。`truncate(self.cursor)`（[lib.rs:431](../../crates/drill-core/src/lib.rs#L431)）に相当する redo 破棄は `self.redo.clear()` になる。

---

## 4. 不変条件

テストで検証できる形で述べる。括弧内は §7 のテスト名。

**I-1（変更経路の単一性）** `document` モジュール外のクレート内に `Document` の可変参照を取ってフィールドを書く経路が存在しない。`Edit::apply` と `History::undo/redo` のみが `Document` を変更する。
検証: `pub(in crate::document)` によるコンパイル時強制 + `drill-app` に `&mut doc.` を含む行が無いことの grep テスト（`no_direct_document_mutation`）。

**I-2（同一性と順序の分離）** ID から索引への変換は `Document` のアクセサのみが行い、`Edit` の payload に `usize` のセット索引・演者索引が含まれるのは `InsertSet.at` / `PerformerExtract.at` / `InsertSection.at`（挿入位置＝順序の指定）と `MoveSet.to` だけである。
検証: 型による強制 + レビュー用チェックリスト。

**I-3（ID の非再利用）** 任意の編集列に対し、`IdAllocator::watermark()` は単調非減少であり、生きている ID は常に watermark 未満。同一 ID が 2 度採番されることはない。
検証: `id_alloc_never_reuses`、`remove_set_undo_restores_same_id`。

**I-4（索引の整合）** 全 `i` について `sets_by_id.get(sets[i].id.get()) == Some(i)`、かつ `sets_by_id.len() == sets.len()`。演者・セクションも同様。
検証: `debug_assert_invariants` + `index_matches_linear_scan`（property）。

**I-5（apply の原子性）** `e.apply(&mut d)` が `Err` を返したとき、`d.fingerprint()` は呼び出し前と等しい。
検証: `apply_is_atomic_on_error`（全変異について、事前条件を 1 つずつ壊した入力で）。

**I-6（逆操作の正確性）** `e.apply(&mut d)` が `Ok(inv)` を返したとき、`inv.apply(&mut d)` は `Ok(_)` を返し、その後の `d` は呼び出し前と `==` かつ `fingerprint` が一致する。
検証: `apply_returns_exact_inverse`（変異ごとの表駆動）。

**I-7（逆操作は復元であって再計算ではない）** 順操作が浮動小数を破壊的に変換する変異（`SetGrid { scale_positions: true }`）について、I-6 が f32 のビット単位で成立する。
検証: `grid_scale_undo_is_bitwise_exact`。

**I-8（対合性）** `inv.apply` が返す逆操作を `inv2` とすると、`inv2` は `e` と意味的に等価（同じ状態遷移を起こす）。
検証: `inverse_is_involutive`（`d0 --e--> d1 --inv--> d0 --inv2--> d2` で `d1 == d2`）。

**I-9（revision の単調性と網羅性）** 成功した非 `Nop` の `apply` は必ず `doc` revision を厳密に増加させる。かつ、§3.4 の行列が指定するスコープ**だけ**が増加する。
検証: `revision_monotonic`、`revision_scope_matrix`（全変異 × 全スコープの直積）。

**I-10（キャッシュ健全性）** ある `Edit` の適用前後で、その `Edit` が §3.4 で影響を与えるとされる下流の出力が変化しうるならば、対応する `CacheKey` も変化する。
検証: `cache_key_covers_output`（各キー型について、キーが不変なら `DisplayList` / 走査結果 / 座標表のバイト列も不変であることを property test で確認）。

**I-11（検証の冪等性）** `validate()` が `Ok` を返す文書に任意の `Edit` を成功適用した結果もまた `validate()` が `Ok`。
検証: `debug_assert_invariants` を全 apply の末尾で実行 + `random_edits_preserve_validity`。

**I-12（往復同型）** `validate()` が `Ok` の文書 `d` について、`load_json(d.to_json()?.as_bytes())? == d` かつ fingerprint が一致。
検証: `json_round_trip_is_identity`。

**I-13（マイグレーションの決定性）** 同じ v1 バイト列は常に同じ v2 `Document` を生む。かつ `migrate` は高々 `MIGRATIONS.len()` 段で停止する。
検証: `v1_fixture_golden`、`migration_terminates`。

**I-14（10,000 編集 Undo — 前置閉性）**

初期文書を `D₀`、成功適用した編集列を `e₁, e₂, …, e_N`（`N = 10,000`）、`Dᵢ = eᵢ(Dᵢ₋₁)`、`apply` が返した逆操作を `uᵢ` とする。このとき

> **∀k ∈ [0, N]:  (u_{k+1} ∘ u_{k+2} ∘ … ∘ u_N)(D_N) ≟ D_k**

が `==`（content equality）と `fingerprint` の両方で成立する。

`k = 0` だけを見る「全部 Undo して初期状態に戻る」より強い。途中で 2 つの誤りが打ち消し合って最後だけ一致する状態を検出できる。実装は `D₀..D_N` の fingerprint 列（10,001 × 8 B = 80 KB）を往路で記録し、復路で 1 段ごとに照合する。

さらに Redo についても

> **∀k ∈ [0, N]:  Undo を N-k 回、続いて Redo を N-k 回行うと `D_N` に戻る**

を、`k` を 100 点サンプリングして検証する。
検証: `stress_10000_edits_prefix_undo`。

**I-15（メモリの有界性）** 任意の編集列に対し `History::bytes() <= limit_bytes` かつ `History::len() <= limit_entries`。ドラッグ 1 回（同一 `GestureId` の連続 `MovePoints`）が生む `Entry` は高々 1 件。
検証: `history_respects_limits`、`coalesce_drag_produces_single_entry`、2 時間相当のソーク（§7）。

**I-16（非有限値の非侵入）** `Document` の中に `is_finite()` が偽の f32 は決して存在しない
（座標・グリッド・テンポ・身長・シェイプ仕様・カメラキーフレーム・カット位置の全て）。
検証: V8 / V9 / V14 / V18 / V21 / V22 / V23 / V24 + `nan_edit_is_rejected` + `inf_json_is_rejected`
+ `nan_camera_keyframe_is_rejected`。

**I-17（ぶら下がり ID の許容 — サブセット）** `Subset.members` は `Document::performers` に存在しない
`PerformerId` を含んでよい。`validate()` はそれを拒否せず、`RemovePerformers` はサブセットを
**書き換えない**。展開時（`Selection::by_subset`、15 §3.7）に生存 ID との積を取る。

これは怠慢ではなく設計上の要請である。もし `RemovePerformers` がサブセットを刈り込むなら、その逆操作は
「刈り込む前の全サブセットのメンバー集合」を保持しなければならず、512 サブセット × 4,000 人で最大 32 MB の
逆操作 payload になる。しかも「演者を消して Undo したらサブセットの中身が減っていた」という
可逆性の破れが起きうる。触らないことで、逆操作が演者配列と座標配列だけに閉じ、I-14 の証明が
サブセットに依存しなくなる（15 §3.6 の結論と一致）。
検証: `remove_performers_leaves_subsets_untouched`、`dangling_subset_member_passes_validate`、
`remove_then_undo_restores_subset_semantics`。

**I-18（ぶら下がり ID の許容 — シェイプと追従、および `Set.shape` が助言的であること）**
`Set.shape.order` と `LookMode::Follow` の `FollowTarget::Group` も I-17 と同じ規則に従う。
加えて、`Set.shape` は「positions が現在も spec の標本と一致する」ことを**主張しない**。
手編集で 1 点だけずらした文書は valid であり、`shape` は「次に半径を変えたときに何を基準に
再標本するか」を覚えているだけの助言的メタデータである。

`shape.order` にぶら下がりを許してよい根拠: 再編集は `spec.sample(order.len())` で標本を作り、
`scratch[i]` を `order[i]` に配る。死んだ `order[i]` を飛ばしても、`i` 以外の添字の対応は変わらないので、
**生存している演者の座標は影響を受けない**（図形にドットの抜けができるだけ）。これはむしろ望ましい挙動で、
刈り込んで添字を詰めると全員が 1 つずつずれてしまう。
検証: `hand_edited_shaped_set_is_valid`、`shape_reedit_skips_dead_ids_without_shifting_survivors`。

**I-19（カメラ編集がフィールド側のキャッシュを汚さない）** カメラ 10 変異のいずれを適用しても、
`TransitionKey::current` / `FrameKey` / `SheetKey` / `TimelineKey` の値は変化しない。逆に、
カメラ以外のいずれの変異を適用しても `camera_revision()` は変化しない。
検証: `camera_edits_do_not_touch_field_keys`（10 変異 × 4 キーの直積）、
`field_edits_do_not_touch_camera_revision`。

**I-20（カットの参照整合）** `camera_program.cuts()` の全 `cut.camera` は実在するトラックを指す。
`RemoveCameraTrack` はそのトラックを指すカットを同一トランザクションで取り除き、逆操作でまとめて復元する。
検証: `remove_camera_track_drops_and_restores_its_cuts`、V23。

---

**I-21（セット境界の判定が浮動小数型に依存しない）** 任意の `global_count` について、
`locate_count_f64(c)` が返すセット索引は `Derived::set_starts`（`u32` の累積和）との
比較だけで決まる。`locate_count(c as f32)` と `locate_count_f64(c)` は、`c` が f32 で
表現可能な限り同じ索引を返す。また `locate_count_f64(global_count_f64(i, l)) == (i, l)`
がセット内ローカル `l` について厳密に成立する（基点が整数なので加減算に丸めが無い）。
検証: `locate_count_round_trip_is_exact`、`set_boundary_index_is_integer_decided`（property）。

**I-22（再生位置が経過時間の純関数）** `position_after(origin, e, speed, range, looping, tempo)`
の戻り値は `(origin, e, speed, range, looping, tempo)` のみに依存する。同じ引数列を
どの順序で・何回呼んでも同じ値を返し、フレーム分割の仕方（1 回の `e = 10.0` と
600 回の 60 fps 刻み）によらず終端の値が一致する。
検証: `playback_is_frame_rate_independent`（同一区間を 1 分割 / 60 分割 / ランダム分割で
評価して終端一致）、`playback_loop_is_exact_over_1000_laps`。

**I-23（永続 f64 の往復厳密性）** `TempoChange` の `count` / `bpm` は
`to_json` → `load_json` でビット一致する（`serde_json` の f64 出力は最短往復表現）。
検証: `tempo_f64_json_round_trip_is_bitwise_exact`。

## 5. 性能

### 5.1 メモリ

| 対象 | 基準規模 1,000 × 64 | 上限規模 4,000 × 256 |
|---|---:|---:|
| `Document` 常駐 | 600 KiB | 9.0 MiB |
| `Derived`（索引 + revision） | 4.8 KiB | 33 KiB |
| pretty JSON | 2.6 MB | 42 MB |
| `History` 512 件（全員移動のみ） | 5.9 MiB | 23 MiB |
| `History` バイト上限 | 64 MiB | 64 MiB |
| 10,000 編集ストレスの fingerprint 列 | 80 KB | 80 KB |

`MovePoints` 1 件（1,000 人）は **12,160 バイト**（内訳は §3.5）。`Targets::All` なら 8,160 バイト。

### 5.2 時間

| 操作 | 基準規模 | 上限規模 |
|---|---:|---:|
| `IdIndex::get` 1 回 | 2ns | 2ns |
| `IdIndex::rebuild`（演者） | 2.0µs | 8µs |
| `Edit::apply`（1,000 人 `MovePoints`） | 5.6µs | 22µs |
| `History::undo`（同上、`clone` 込み） | 6.6µs | 26µs |
| `bump`（2 スコープ） | 10ns | 10ns |
| `bump`（`SetScope::All`） | 0.4µs | 1.3µs |
| `TransitionKey::current` / `PlanKey::current` | 20ns | 20ns |
| `locate_count_f64`（`set_starts` の二分探索） | 8ns（64 セット = 6 段） | 12ns（256 セット = 8 段） |
| `rebuild_set_starts` | 0.1µs | 0.4µs |
| `position_after`（`seconds_at_f64` 2 回 + `count_at_f64` 1 回） | 0.05µs | 0.05µs |
| `Document::validate` | 0.15ms | 2.5ms |
| `Document::fingerprint` | 0.05ms | 0.8ms |
| `to_json`（既存ベースライン 100 回 28.75ms より） | 0.29ms | 4.6ms |
| `load_json`（parse + migrate + validate） | 約 12ms | 約 190ms |
| coalesce 判定（1,000 人 targets 比較） | 0.2µs | 0.8µs |

### 5.3 16.6ms 予算の取り分

このモジュールがフレーム内で実行するのは、**編集が起きたフレームだけ**の以下である。

| フレーム内の処理 | 実行条件 | 所要 |
|---|---|---:|
| `Edit::apply`（ドラッグ 1 フレーム分、選択 1,000 人） | ドラッグ中の毎フレーム | 5.6µs |
| `History::push` + coalesce 判定 | 同上 | 0.3µs |
| `TransitionKey::current` / `PlanKey::current` × 表示中のセット数（最大 3） | 毎フレーム | 0.12µs |
| `SheetKey` / `FrameKey` / `CameraPoseKey` の構築と比較 | 毎フレーム | 0.07µs |
| `position_after` + `locate_count_f64`（再生中） | 毎フレーム | 0.06µs |
| **合計（ドラッグ中の最悪フレーム）** | | **約 6.2µs** |

**16.6ms のうち 0.04%。** 毎フレーム走る処理（`TransitionKey` 等の比較）だけなら 0.11µs で 0.0007%。

`validate()`（0.15ms）と `fingerprint()`（0.05ms）はフレーム内で呼ばない。前者は読み込み時と `DocumentBuilder::build` のみ、後者はテストのみ。デバッグビルドの `debug_assert_invariants`（0.3ms）はフレーム予算の対象外（release でゼロコスト）。

Undo/Redo は 6.6µs で **2ms 予算の 0.33%**、300 倍の余裕。根拠の骨子は「Undo のコストは編集サイズに比例し、文書サイズには比例しない」こと。キャッシュ再構築を Undo の中で行わず revision の整数書き込みに留める設計がこれを可能にしている。

### 5.4 確保回数

| 操作 | ヒープ確保回数 |
|---|---|
| ドラッグ中の `MovePoints` 適用 | 1（逆操作の `Vec<Point>`）。`Targets::resolve` の scratch は `Derived` から借りて返すので 0 |
| `History::push`（合流時） | 0 |
| `History::push`（新規） | 0（`VecDeque` は容量到達時のみ） |
| `RestorePositions` の適用 | 0（`mem::swap`） |
| `TransitionKey::current` | 0 |
| `validate` | 0（重複検出はソート済み走査ではなく、`Derived` の索引長との突き合わせで行う） |

`Derived` に `index_scratch: Vec<u32>` を持たせて `take` / `give` で貸し出すことで、`Targets::resolve` の確保をゼロにする。これは規約 3「毎フレーム走る関数は `&mut` の作業領域を受け取り、内部でヒープ確保しない」の適用。

---

## 6. 失敗モードと安全性

| # | 壊れ方 | 検出 | 対処 |
|---|---|---|---|
| F1 | 履歴が消えたセット/演者を指す | `precheck` の `require_*` | `Err(UnknownSet/UnknownPerformer)`。`History::undo` はエントリをスタックに戻し、文書は無変更。UI は「この操作は取り消せません」を表示して該当エントリを破棄 |
| F2 | `Batch` の途中で失敗 | `apply_batch` | 適用済みを逆順にロールバック。文書は原子的に無変更 |
| F3 | ロールバック自体が失敗（到達不能） | `debug_assert` + 戻り値 | `poisoned = true`、`Err(DocumentPoisoned)`。以後の編集を拒否し、41 の「復旧用に別名保存」経路へ。**元ファイルは触らない** |
| F4 | UI の数値計算から NaN/Inf が入る | `precheck` の有限性検査 | `Err(NonFiniteCoordinate)`。文書に入る前に止まる |
| F5 | ID 空間の枯渇（2^32 回の採番） | `checked_add` | `Err(IdSpaceExhausted)` |
| F6 | 単一編集が 64 MiB を超える | `heap_bytes` | 履歴を全消去し `truncated`。UI は事前確認ダイアログ |
| F7 | 履歴の追い出しで保存点が失われる | `savepoint_lost` | `is_dirty()` が `true` に固定（安全側） |
| F8 | 文書を差し替えたのに履歴が残っている | — | `History::reset()` を `DocumentState::replace` が必ず呼ぶ。加えて revision がプロセス全体で一意なので、古いキャッシュキーは新しい文書と決して一致しない |
| F9 | 敵性 JSON（巨大・深い・NaN・重複 ID・上限超過） | §3.7 の上限表 | 全て `DrillError`。パニックしない |
| F10 | カウント総和の整数溢れ | V13 の `checked_add` | `Err(TimelineOverflow)` |
| F11 | 移行後の文書が不正 | `load_json` が `migrate` の後に `validate` | `Err`。移行が壊れたファイルを作れない |
| F12 | 移行表の誤り（循環・停滞） | `migrate` のループ上限とバージョン単調増加検査 | `Err(MigrationStalled)`。無限ループしない |
| F13 | `positions` の長さ不一致による添字パニック | V4 + `clippy::indexing_slicing` | 到達不能 |
| F14 | 並行アクセス（書き出しジョブが編集中の文書を読む） | 型 | `Document: Send`。ジョブへは `Document` のスナップショットを `clone` して渡す（40 の責務）。`Document` は `Sync` だが内部可変性を持たないので、共有参照からは変更できない |

`Revision` のプロセス全体一意性が F8 を型ではなく値のレベルで潰している点が設計上の要。「File > Open 後に前の文書のキャッシュが効いてしまう」は本来テストしにくいクラスのバグで、単調な大域カウンタで構造的に消せる。

`drill-core` の依存は `serde` / `serde_json` のみのまま（`std::sync::atomic` は std）。UI・GPU・OS・音声デバイスに触れない。

---

## 7. テスト計画

`crates/drill-core/src/document/` の各モジュール内 `#[cfg(test)]` と `crates/drill-core/tests/`。**新規の依存は入れない** — property test 用の乱数は 32 行の xorshift64* をテストモジュール内に置く。

### 7.1 単体

| テスト | 対象 |
|---|---|
| `id_alloc_never_reuses` | I-3 |
| `id_zero_is_rejected_by_deserialize` | `NonZeroU32` |
| `id_index_dense_and_sparse_agree` | `IdIndex` の 2 表現が同じ結果を返す |
| `id_index_switches_to_sparse_above_threshold` | 密度ガード |
| `index_matches_linear_scan` | I-4（property、ランダムな挿入・削除列） |
| `apply_returns_exact_inverse` | I-6、全 38 変異（`Nop` / `Batch` を除く）の表駆動 |
| `inverse_is_involutive` | I-8 |
| `apply_is_atomic_on_error` | I-5、事前条件を 1 つずつ壊す |
| `batch_rollback_restores_fingerprint` | F2 |
| `move_points_undo_is_bitwise_exact` | f32 ビット比較 |
| `grid_scale_undo_is_bitwise_exact` | I-7 |
| `restore_positions_does_not_copy` | `as_ptr()` 比較で `mem::swap` を確認 |
| `remove_set_undo_restores_same_id` | I-3 と #2 の回帰 |
| `remove_last_set_is_rejected` | V2 |
| `remove_section_in_use_is_rejected` | `SectionInUse` |
| `assign_section_inverse_is_batch_per_section` | 逆操作の形 |
| `nan_edit_is_rejected` | I-16、F4 |
| `nop_is_not_pushed` | `History::push` |
| `relabel_1000_is_one_entry` | `RelabelPerformers` の粒度（履歴長が 1 増えるだけ） |
| `relabel_allows_duplicate_labels` | 15 §3.4 の運用要件 |
| `assign_section_inverse_is_single_variant` | 異種セクションでも `Batch` にならない |
| `apply_shape_carries_spec_and_points_in_one_entry` | 14 §3.4 の要求（1 操作 = 1 Undo） |
| `freehand_edit_of_shaped_set_clears_shape` | アプリ側の義務（`Batch[MovePoints, SetShape{None}]`） |
| `hand_edited_shaped_set_is_valid` | I-18（`shape` は助言的） |
| `shape_reedit_skips_dead_ids_without_shifting_survivors` | I-18 |
| `remove_performers_leaves_subsets_untouched` | I-17 |
| `dangling_subset_member_passes_validate` | I-17、V20 |
| `subset_members_swap_does_not_copy` | `mem::swap` のポインタ比較 |
| `remove_subset_undo_restores_same_id` | I-3 をサブセットへ拡張 |
| `insert_camera_keyframe_at_existing_count_replaces` | 23 不変条件 2、逆操作が再置換になること |
| `move_camera_keyframe_displacing_another_restores_both` | `Batch` 逆操作 |
| `remove_camera_track_drops_and_restores_its_cuts` | I-20 |
| `set_camera_cut_to_unknown_track_is_rejected` | V23 の編集側ゲート |
| `nan_camera_keyframe_is_rejected` | I-16 |
| `camera_track_undo_restores_same_id` | I-3 をカメラへ拡張 |
| `set_counts_shrink_stranding_a_gate_is_rejected` | §3.8.4。`SetCounts` 単独で `CountsWouldStrandRoutes`、文書は無変更（I-5） |
| `batch_routes_then_counts_shrinks_cleanly` | §3.8.4 の正しい順序。Undo 1 回で完全復帰（11 の S5 の読み替え版） |
| `gate_arrive_none_round_trips_as_json_null` | V16。`f32::INFINITY` を含む `Gate` が型として構築不能であること |
| `locate_count_round_trip_is_exact` | I-21 |
| `set_boundary_index_is_integer_decided` | I-21（property: 境界近傍の f32/f64 で索引が一致） |
| `locate_count_binary_search_matches_linear_scan` | 11 の未決 5 の決着（既存 `locate_count` との等価性） |
| `playback_is_frame_rate_independent` | I-22。1 分割 / 60 分割 / ランダム分割の終端一致 |
| `playback_loop_is_exact_over_1000_laps` | I-22 |
| `tempo_f64_json_round_trip_is_bitwise_exact` | I-23 |

### 7.2 History

| テスト | 対象 |
|---|---|
| `coalesce_drag_produces_single_entry` | I-15。同一 `GestureId` で 200 回 `MovePoints` → `len() == 1`、Undo 1 回でドラッグ前に戻る |
| `coalesce_does_not_merge_across_gestures` | `begin_gesture` を挟むと 2 件 |
| `coalesce_does_not_merge_when_targets_differ` | 条件 3 |
| `coalesce_does_not_merge_across_sets` | `CoalesceKey::MovePoints(SetId)` |
| `push_discards_redo_branch` | 既存 `history_discards_redo_branch`（[lib.rs:536](../../crates/drill-core/src/lib.rs#L536)）の移植 |
| `history_respects_limits` | I-15、件数上限とバイト上限の両方 |
| `oversized_edit_truncates_history` | F6 |
| `savepoint_tracks_dirtiness` | 編集 → dirty、Undo で clean、Redo で dirty |
| `savepoint_lost_reports_dirty` | F7 |
| `undo_on_missing_target_returns_error_and_keeps_entry` | F1 |

### 7.3 revision とキャッシュ

| テスト | 対象 |
|---|---|
| `revision_monotonic` | I-9 前半 |
| `revision_scope_matrix` | I-9 後半。§3.4 の表をテストコード内の表として持ち、全変異について「増えるべきスコープが増え、増えるべきでないスコープが増えていない」を確認 |
| `cache_key_covers_output` | I-10。`TransitionKey` / `FrameKey` / `SheetKey` / `CameraPoseKey` / `SubsetKey` それぞれについて、ランダム編集列でキー不変 ⇒ 出力バイト列不変 |
| `camera_edits_do_not_touch_field_keys` | I-19 前半。カメラ 10 変異 × フィールド系 4 キーの直積 40 ケース |
| `field_edits_do_not_touch_camera_revision` | I-19 後半 |
| `geometry_revision_bumps_iff_set_scope_nonempty` | `GEOMETRY` が `SetScope` から自動導出されること |
| `set_shape_does_not_bump_set_revision` | 表の ‡（衝突走査を無駄に無効化しない） |
| `revisions_are_globally_unique` | 2 つの `Document` を交互に編集して revision の重複が無いこと |
| `revision_not_in_serialized_output` | `to_json` の出力に revision が現れないこと（決定論、規約 5） |

### 7.4 validate と敵性入力

各 V 項目に 1 本ずつ（`validate_rejects_empty_sets` … `validate_rejects_long_label`、19 本）。加えて `crates/drill-core/tests/fixtures/adversarial/` に:

| フィクスチャ | 期待 |
|---|---|
| `deep_nesting.json`（200 段） | `Json` エラー、パニックしない |
| `huge.json`（129 MiB、CI では生成） | `InputTooLarge`、パースしない |
| `inf_coordinate.json`（`1e400`） | `NonFiniteCoordinate` |
| `duplicate_performer_id.json` | `DuplicatePerformerId` |
| `id_above_watermark.json` | 読み込みは成功（watermark が持ち上がる）、以後の採番が衝突しない |
| `zero_id.json` | `Json`（`NonZeroU32`） |
| `negative_counts.json` | `Json`（`u16`） |
| `mismatched_positions.json` | `SetSizeMismatch`。既存 `mismatched_set_size_is_rejected`（[lib.rs:513](../../crates/drill-core/src/lib.rs#L513)）の移植 |
| `absolute_audio_path.json` | `InvalidAudio` |
| `truncated.json` | `Json`、パニックしない |
| `duplicate_camera_keyframe_counts.json` | `normalize_after_load` が後勝ちで正規化し、読み込みは成功（23 §6） |
| `nan_camera_keyframe.json` | 該当キーフレームのみスキップして警告、ドリル本体は開ける（23 §6） |
| `cut_to_unknown_camera.json` | V23 で `InvalidCameraCut` |
| `dangling_subset_members.json` | **読み込み成功**（I-17）。展開時に無視される |
| `dangling_shape_order.json` | **読み込み成功**（I-18） |
| `dangling_performer_section.json` | **読み込み成功**（V25）。`dangling_section_refs()` が列挙する |
| `huge_subset_members.json`（16,385 件） | `TooManyElements` |
| `huge_camera_track_count.json`（65 本） | `TooManyElements` |

`no_unbounded_depth_feature`: `Cargo.lock` / `Cargo.toml` に `serde_json` の `unbounded_depth` が有効化されていないことを確認する CI テスト。

### 7.5 マイグレーション

- `crates/drill-core/tests/fixtures/v1/basic.drill.json`（4 人 × 2 セット、手書き）と `v1/football_hashes.drill.json`（既定グリッドの日本語ハッシュラベル入り）をコミット。
- `v1_fixture_golden`: v1 を読む → v2 として `to_json` → `fixtures/v2/basic.expected.json` と単純文字列比較（追加依存なし、`PRODUCT_QUALITY.md` C-2 の方針どおり）。
- `v1_round_trip_preserves_data`: 読み込み → v2 保存 → 再読込 → **演者数・全座標・全カウント・グリッド寸法が一致**（`DESIGN_GAPS.md` A-7 の受け入れ基準）。
- `v1_performer_ids_are_shifted_by_one`: v1 の id 0 が v2 の 1 になり、順序が保たれる。
- `v1_hash_labels_map_to_kinds`: 3 種の日本語ラベルと未知ラベルの写像。
- `v1_tempo_widens_from_json_text_not_from_f32_bits`: v1 の `{"bpm": 119.7}` を読んだ結果が
  `119.7_f64` であって `f64::from(119.7_f32)` ではないこと（§3.8.1 の注記）。
  `count` についても同様。マイグレーション関数が `tempo` に一切触れないことの確認も兼ねる。
- `v1_gains_empty_subsets_shape_and_camera`: v1 を読んだ結果が `subsets == []`、
  全セット `shape == None`、`camera_program == CameraProgram::default()` になること
  （マイグレーション関数がこれらのキーを書き足さず `#[serde(default)]` に任せていることの確認）。
- `unsupported_future_version`: `schema_version: 9999` → `UnsupportedSchema { found: 9999, supported: 2 }`。既存 `malformed_and_future_documents_are_rejected`（[lib.rs:504](../../crates/drill-core/src/lib.rs#L504)）の移植（メッセージ文字列ではなく `DrillError` の変異で判定）。
- `missing_version_is_rejected`。
- `migration_terminates`: 意図的に壊した `MIGRATIONS` 表（自己ループ）で `MigrationStalled` になること。

### 7.6 ストレスと property

- **`stress_10000_edits_prefix_undo`**（I-14）: 1,000 人 × 8 セットの文書に、xorshift で重み付けした
  38 変異（`Nop` / `Batch` を除く全て）をランダム生成して 10,000 回適用。カメラ系
  （`InsertCameraKeyframe` / `RemoveCameraKeyframe` / `MoveCameraKeyframe` / `SetCameraCut` /
  `InsertCameraTrack` / `RemoveCameraTrack`）を含めることは 23 §7 の要求でもあり、シェイプ・
  サブセット・一括採番も同じ列に混ぜる。往路で fingerprint 列を記録、復路で 1 段ごとに照合。`--release` 前提（デバッグでは 500 回版 `stress_500_edits_debug` を回す）。所要見込み: 適用 10,000 × 6µs + fingerprint 20,000 × 0.05ms ≈ 1.1 秒。
- `random_edits_preserve_validity`（I-11）: 上記の各段で `validate()` が `Ok`。
- `json_round_trip_is_identity`（I-12）: ランダム文書 100 個。既存 `json_round_trip_is_valid`（[lib.rs:496](../../crates/drill-core/src/lib.rs#L496)）を強化。
- `fingerprint_agrees_with_eq`: `a == b ⟺ a.fingerprint() == b.fingerprint()`（⟸ は確率的、反例が出たら衝突として調査）。
- `soak_two_hours_of_dragging`（`#[ignore]`、手動実行）: 60fps × 7,200 秒相当（1,555,200 回）の合流 `MovePoints` を流し、`History::bytes()` と `History::len()` が上限内に留まり、プロセス RSS が単調増加しないことを確認。`PRODUCT_QUALITY.md`「2 時間連続再生で常駐メモリの継続増加なし」の編集側の対応物。

### 7.7 ベンチ（`benches/core_performance.rs` に追加）

| ベンチ | 測るもの |
|---|---|
| `edit_apply_move_1000` | 5.6µs 主張の検証 |
| `history_undo_redo_1000` | 2ms ゲート |
| `id_index_get_hot` | 2ns 主張 |
| `id_index_rebuild_1000` | 位相変更のコスト |
| `document_validate_1000x64` | 0.15ms 主張 |
| `document_fingerprint_1000x64` | 0.05ms 主張 |
| `load_json_1000x64` | 読み込み全体 |
| `edit_apply_zero_alloc` | 既存 `interpolation_reuses_output_allocation`（[lib.rs:485](../../crates/drill-core/src/lib.rs#L485)）と同じポインタ比較で、2 回目以降の `Targets::resolve` が確保しないこと |

計測結果は `PRODUCT_QUALITY.md` のベースライン節へ追記する（Wave 0 完了時）。

---

## 8. 実装タスク

1 タスク = 1〜3 時間相当。`Wave 0` の 2〜4 に対応する。

| ID | 内容 | 依存 | 並行 |
|---|---|---|---|
| **T-10-01** | `document/ids.rs`: 4 つの newtype（`SetId`/`PerformerId`/`SectionId`/`SubsetId`）、`IdAllocator`、`IdAllocators`、`IdIndex`（Dense/Sparse）と単体テスト 5 本 | — | — |
| **T-10-02** | `document/mod.rs`: `Section` / `Subset` / `Performer` / `Set`（`shape` 込み）/ `Document`（`subsets` / `camera_program` 込み）の v2 構造、フィールドの `pub(in crate::document)` 化、読み取りアクセサ、`DocumentRepr` + `from_repr` + `rebuild_derived`、`Derived` の always-equal `PartialEq`。14/15/23 の型への `PartialEq` derive 追加（§1.2） | T-10-01、doc 14/15/23 の型実装 | — |
| **T-10-03** | `DocumentBuilder` と `Document::demo` の再実装（日本語リテラル除去）、既存モジュール（`svg` / `coordinates` / `continuity` / `countsheet` / `pathing` / `camera`）の `doc.sets` → `doc.sets()` 機械置換 | T-10-02 | T-10-04, T-10-05 と並行可 |
| **T-10-04** | `document/revision.rs`: `Revision` / `Scopes`（`CAMERA` / `GEOMETRY` 込み、`u16`）/ `SetScope` / `Revisions` / `bump` / **8 種の `CacheKey`**（`PlanKey` 込み、11 §9-8a）と `current()` | T-10-02 | T-10-03, T-10-05 と並行可 |
| **T-10-04b** | `Derived::set_starts` と `timeline_counts` / `locate_count_f64` / `global_count_f64` / f32 ラッパ（§3.8.2）。既存 `locate_count`（[lib.rs:367-382](../../crates/drill-core/src/lib.rs#L367)）との等価性テストと I-21 | T-10-02 | T-10-03, T-10-05 と並行可 |
| **T-10-04c** | `playback.rs` の f64 化と `advance` → `position_after` の置換（§3.8.3）。`PlaybackRange` / `AdvanceResult` の f64 化、既存テスト 5 本の書き換え、I-22 の 2 本を追加。30 の `Transport` / 43 の `main.rs:488-509` への影響を通知 | T-10-04b、11 の `TempoMap` f64 化 | T-10-05 と並行可 |
| **T-10-05** | `document/validate.rs`: `limits` 定数、`validate()` の V1〜V25、`dangling_section_refs`、`debug_assert_invariants`、`DrillError` 変異の追加（42 と調整） | T-10-02 | T-10-03, T-10-04 と並行可 |
| **T-10-06** | `document/edit.rs` 骨格: `Edit` enum、全 payload 構造体、`Targets`、`precheck` / `apply_checked` のディスパッチ、`is_nop` / `heap_bytes` / `coalesce_key` | T-10-02, T-10-04 | — |
| **T-10-07** | 幾何とセットの変異: `MovePoints` / `RestorePositions` / `InsertSet` / `RemoveSet` / `MoveSet` / `RenameSet` / `SetCounts` / `SetNote` の precheck と apply | T-10-06 | T-10-08 と並行可 |
| **T-10-08** | 演者・セクションの変異: `InsertPerformers` / `RemovePerformers` / `PerformerExtract` / `SetPerformerMeta` / `RelabelPerformers` / `AssignSection` / `InsertSection` / `RemoveSection` / `SetSectionMeta` | T-10-06 | T-10-07, T-10-08b, T-10-08c と並行可 |
| **T-10-08b** | サブセットの変異 4 本（`InsertSubset` / `RemoveSubset` / `RenameSubset` / `SetSubsetMembers`）と I-17 のテスト。15 §3.7 の `Selection::by_subset` との結線確認 | T-10-06 | T-10-07, T-10-08, T-10-08c と並行可 |
| **T-10-08c** | カメラの変異 10 本と I-19 / I-20 のテスト。23 側へ `remove_cut` / `drain_cuts_for` / `try_alloc_camera_id` / `normalize_after_load` を追加（§9 の報告事項） | T-10-06、doc 23 の T7 | T-10-07, T-10-08, T-10-08b と並行可 |
| **T-10-09** | シェイプと文書全体の変異: `ApplyShape` / `SetShape` / `SetTitle` / `SetGrid`（可逆な非スケーリング版と Batch 逆操作のスケーリング版）/ `SetStepStyle` / `SetTempo` / `SetAudio`、`Batch` とロールバック、`poisoned` | T-10-07, T-10-08, T-10-08b, T-10-08c | — |
| **T-10-10** | `document/history.rs`: `History` / `Entry` / `GestureId` / `PushOutcome` / 合流（16 種の `CoalesceKey`）/ 上限 / savepoint、§7.2 のテスト 10 本 | T-10-09 | T-10-11 と並行可 |
| **T-10-11** | `document/migrate/`: `migrate` 連鎖、`v1_to_v2`、`load_json`、v1/v2 フィクスチャ、§7.5 のテスト 8 本、§7.4 の敵性フィクスチャ 10 本 | T-10-05, T-10-09 | T-10-10 と並行可 |
| **T-10-12** | `document/fingerprint.rs` と `stress_10000_edits_prefix_undo`、`random_edits_preserve_validity`、`soak_two_hours_of_dragging` | T-10-10 | — |
| **T-10-13** | `revision_scope_matrix` / `cache_key_covers_output` / `apply_returns_exact_inverse` の表駆動テスト一式 | T-10-09, T-10-04 | T-10-12 と並行可 |
| **T-10-14** | `benches/core_performance.rs` に §7.7 の 8 本を追加、`PRODUCT_QUALITY.md` のベースライン更新 | T-10-12 | — |
| **T-10-15** | `drill-app` の全変更経路を `Edit` に集約（`DocumentState::edit` ヘルパ、`dirty` の削除、グリッド/テンポ/音源/セット複製の `Edit` 化、ドラッグの `GestureId` 化）。43 と協調 | T-10-10 | — |

依存グラフ:

```
T-01 ─▶ T-02 ─┬─▶ T-03
              ├─▶ T-04 ─┐
              └─▶ T-05 ─┤
                        ▼
                      T-06 ─┬─▶ T-07  ─┐
                            ├─▶ T-08  ─┤
                            ├─▶ T-08b ─┤
                            └─▶ T-08c ─┤
                                       ▼
                                     T-09 ─┬─▶ T-10 ─┬─▶ T-12 ─▶ T-14
                                           │         └─▶ T-15
                                           ├─▶ T-11
                                           └─▶ T-13
```

並行可能な最大幅は T-07 / T-08 / T-08b / T-08c の 4 本、次いで T-03 / T-04 / T-05 の 3 本、
最後に T-10 / T-11 / T-13 の 3 本。

T-08c は doc 23 の T3（`CameraTrack` 実装）と T7（`CameraProgram` 実装）に依存する。
T-08b は doc 15 の `roster.rs`、T-09 のシェイプ部分は doc 14 の `ShapeSpec` / `ShapeAssignment` に依存する。
これら 3 本の外部依存は互いに独立なので、14 / 15 / 23 の実装が並行して進むかぎり本書側の
4 本並行は維持できる。

T-10-15 が完了するまで `drill-app` はビルドできない（`Document` のフィールドが private になるため）。T-02 と T-15 の間はブランチを分けず、T-02 で `drill-app` を一時的に `#[cfg(FALSE)]` で切るのではなく、T-02 の中で最小限のコンパイル修正（アクセサ置換）まで行い、意味のある `Edit` 化は T-15 で行う。

---

## 9. 未決事項

| # | 保留した点 | 決めるために必要な情報 |
|---|---|---|
| U1 | ~~`PerformerExtract` に含めるべき「演者キーのデータ」の最終形~~ **決着**: `route_overrides` のみ。サブセット（15 §3.6）・`shape.order`（14 §3.4）・`FollowTarget::Group`（23 §6）はいずれもぶら下がり ID を許容する規約（I-17 / I-18）を採ったので、`RemovePerformers` が触らず、逆操作にも入らない。今後の演者キー付きサブシステムは「`PerformerExtract` に足す」か「ぶら下がりを許す」かを設計時に選び、`performer_extract_is_total` テストで明示する | — |
| U2 | `SetRoutes` の粒度（テーブル丸ごと差し替えか、override 単位か） | 11 が `RouteTable` のサイズを決めてから。1,000 人分の override が常在するなら丸ごと差し替えは 48 KB/編集になり、override 単位の `SetRouteOverride { set, performer, route: Option<Route> }` が必要 |
| U3 | `AudioTrack.offset_seconds` → `anchors: Vec<SyncAnchor>` の移行を v1→v2 で行うか、v2→v3 に回すか | 30 が `SyncAnchor` を確定させる時期。v2 に含めた方がファイル形式の版数が減る |
| U4 | `Edit` を `Serialize` にしてクラッシュレポートへ直近 N 件を書き出すか | 41 のクラッシュレポート仕様。有用だが、全 payload 型に `Serialize` を要求するので `Route` / `Section` の確定待ち |
| U5 | 大きな編集（> 64 MiB）の履歴をディスクへスピルするか | 実運用で 64 MiB を超える単発編集がどれだけ出るか。現状は「取り消せません」の確認ダイアログで足りると判断。上限規模の全体スケーリングが 8 MiB なので、超えるのは 4,000 人 × 256 セットで 8 回連続の全体変形といった稀なケース |
| U6 | 演者単位の変更追跡（差分描画用リングバッファ） | 20 が `DisplayList` の部分更新を要求するかどうか。要求しないならセット単位で十分 |
| U7 | SoA への移行時期 | 21 の GPU インスタンシングが `xs` / `ys` を別バッファで要求するか、`&[f32]` のインターリーブで足りるか。後者なら `positions_as_f32()` で済み、移行は不要 |
| U8 | `IdIndex` の Dense/Sparse 閾値（`DENSITY_FACTOR = 4`、`DENSE_FLOOR = 256`） | 実セッションでの ID 分布の実測。長時間セッションで演者の追加削除を繰り返したときの span を計測してから調整 |
| U9 | `History` の既定上限（512 件 / 64 MiB） | 利用者テスト。Pyware の実運用で「もっと戻りたい」がどの程度出るか。バイト上限を上げる方が件数上限を上げるより体感に効くはず |
| U10 | `Document` をジョブへ渡すときに `clone` するか `Arc<Document>` にするか | 40 のジョブ基盤。`clone` は 600 KiB で約 0.1ms なので許容範囲だが、動画書き出しのように長時間保持する用途では `Arc` の方が素直。`Arc<Document>` にすると `Edit::apply` が `Arc::make_mut` 経由になり、書き出し中の編集で丸ごとコピーが走る（copy-on-write）— これは望ましい挙動かもしれない |
| U11 | v2 のバイナリ表現（座標を base64 の f32 配列にする等） | 41。上限規模の pretty JSON が 42 MB になるので、保存時間と可読性のトレードオフを測ってから |
| U12 | `debug_assert_invariants` の既定コスト（デバッグビルドで 1 編集 0.3ms） | 開発者の体感。`DRILLFORGE_SKIP_INVARIANTS` を用意するが、既定を有効のままにするか、ドラッグ中だけ軽量版 `cheap_invariants()`（セット数と長さのみ、O(sets)）に落とすか |
| U13 | 手編集で崩れた `Set.shape` を UI にどう見せるか（「この円は古くなっています / 再適用しますか」） | 43 の製品判断。本書は `shape` を助言的（I-18）と決めただけで、鮮度判定（`spec.sample(order.len())` と現在の positions の距離）を core に置くかは 14 / 43 の担当 |
| U14 | サブセットのぶら下がり ID を「掃除する」明示的な操作を用意するか | 15 / 43。I-17 は自動での刈り込みを禁じるが、利用者が明示的に実行する `Edit::SetSubsetMembers`（生存 ID との積を渡すだけ）なら整合する。UI に出す価値があるかは運用次第 |
| U16 | `Point` を f64 にする日が来るか | 来ないと判断している（§3.8.1 の表）。ただし屋内展示の mm 精度作図など、想定外の用途が出た場合は positions が 2 倍（1,000×64 で 1 MB）になり `positions_as_f32()` の GPU 直送も設計し直しになるので、**v3 相当の大改修**として扱う。判断材料: `MAX_COORDINATE` を実際にどこまで使うか |
| U17 | `SyncAnchor` の型 | 30 の所有。`{ count, seconds }` はどちらもタイムライン軸上の位置なので `TempoChange.count` と同じ議論が当てはまり、f64 が自然。本書は `Document.audio` を保持するだけなので裁定しないが、v2 に含めるなら**今**決める必要がある（30 へ照会済み、§9.1） |
| U15 | `CameraProgram` を `Document` から分離して別ファイル（`.drillcam`）に置く案 | 41 / 53。カメラワークだけを他のショーへ流用したいという要求が出た場合に検討する。現状は 23 §3.12 の決定どおり `Document` 内。分離すると I-14 の対象から外れ、`Edit` のカメラ 10 変異が別の履歴を持つことになるので、要求が実在してから判断する |

### 9.1 参照側文書（14 / 15 / 23）への報告事項

いずれも完成済み文書の**意味論は変えない**。本書の統合にあたって必要になった追加・調整のみ。
本書は他ファイルを変更しないので、実装時に該当文書の担当が取り込む。

| 宛先 | 内容 | 種別 |
|---|---|---|
| 14 / 15 / 23 | `ShapeAssignment` / `ShapeSpec` / `Subset` / `CameraProgram` / `CameraTrack` / `LookMode` / `FollowTarget` / `FollowDamping` に `PartialEq` の derive が無い。`Document` の内容等価性（I-14 の 10,000 編集 Undo 検証）が成立しないので追加が必要（§1.2） | 追加（derive のみ） |
| 23 | `CameraProgram` に `remove_cut(at) -> Option<CameraCut>` が無い。`set_cut` はあるが対の削除操作が無く、`Edit::RemoveCameraCut` を実装できない | API 追加 |
| 23 | `CameraProgram::drain_cuts_for(camera) -> Vec<CameraCut>` が必要。23 の T7 が「トラック削除時のカット整合」を挙げているが、削除したカットを**逆操作のために返す**手段が無いと `RemoveCameraTrack` が可逆にならない（I-20） | API 追加 |
| 23 | `CameraProgram::alloc_camera_id` の `self.next_camera_id += 1` は溢れを検査していない。`u32` 枯渇時に debug でパニック・release でラップして ID が衝突する。`try_alloc_camera_id() -> Option<CameraId>` を要求（規約「パニック禁止経路」） | **欠陥** |
| 23 | §6 の表が「重複 count の後勝ち正規化」「NaN キーフレームのスキップ」を要求しているが、それを行う関数名が定義されていない。本書は `CameraProgram::normalize_after_load()` として `Document::from_repr` から呼ぶ位置を確定させた | 命名の確定 |
| 23 | §3.12 の橋渡し案 `Document::edit_camera(f: impl FnOnce(&mut CameraProgram) -> R)` は**採用しない**。任意のクロージャで文書を書き換えられる穴は規約の不変条件 1 に反する。23 の T11 は本書のカメラ 10 変異に置き換わる | 方針の上書き |
| 15 | `SubsetId` を `pub type SubsetId = u32;` から newtype へ格上げした（`PerformerId` と同じ扱い）。意味論は不変 | 表現の統一 |
| 15 | `AddSubset` を `InsertSubset { at, id, .. }` に改名（`InsertSet` / `InsertSection` と綴りを揃える）。他 3 変異は同名のまま | 改名 |
| 15 | §3.9 の `RelabelPerformers { changes: Vec<(id, before, after)> }` と `AssignSection { changes: Vec<(id, before, after)> }` から `before` を落とした（`apply` が文書から読む、決定 A）。履歴メモリが半減する | payload の簡約 |
| 15 | §3.9 の `AddPerformers`/`RemovePerformers` の `positions: Vec<Vec<Point>>`（セット索引順）は、セット順序が変わりうるため `Vec<(SetId, Vec<Point>)>` に変更した（`PerformerExtract`）。§0 バグ #2 と同じ添字ズレを演者側で再発させないため | **潜在的な欠陥の修正** |
| 14 | §3.4 の `EditAddition::ApplyShape { before_shape, after_shape, before_positions, after_positions }` から前状態を落とし、`ApplyShape { set, shape, points }` にした。14 が懸念していた「before/after 二重管理」は決定 A で構造的に消える | payload の簡約 |
| 14 | `Set.shape` は**助言的**（I-18）と確定した。手編集で positions と spec がずれても `validate()` は `Ok` を返す。`MovePoints` は `shape` を自動で `None` にしない（アプリが `Batch[MovePoints, SetShape{None}]` を出す義務を負う） | 責務の確定 |
| 14 | `ShapeAssignment.order` は doc comment で「索引 or `PerformerId`」と両論併記だったが、`PerformerId` に確定した（安定 ID が着地するため） | 確定 |
| 11 / 30 | **`TempoChange` を `{ count: f64, bpm: f64 }` に変更**（§3.8.1）。11 §9-9 と 30 §9-4 への回答。v2 に含める。前置和 `starts` / `prefix` は f64 events から直接作れるので 11 §3.12.2 のアルゴリズムは変更不要（widening が消えるぶん簡単になる）。`bpm_at()` は f32 ラッパを残し `bpm_at_f64()` を追加 | **スキーマ確定** |
| 11 | §9-8a `PlanKey` を **受理**。`revision.rs` に 8 種目の `CacheKey` として追加（§3.4） | 受理 |
| 11 | §9-8b `CoalesceKey::Routes(SetId)` を **受理**（§3.3.6） | 受理 |
| 11 | §9-5 `locate_count` の二分探索化と累積和キャッシュの所有者を **本書が引き取る**。`Derived::set_starts: Vec<u32>` として実装（§3.8.2）。11 は `locate_count_f64` のシグネチャを前提にしてよい | 受理 |
| 11 | §6.2 / テスト S5 の期待値を変更する必要がある。`SetCounts` 単独でゲートを取り残す縮小は **`precheck` で拒否**（`CountsWouldStrandRoutes`）する。「適用は成功するが `validate()` が失敗する」は本書の I-11 と `debug_assert_invariants` に反するため採れない。`Batch` の順序も `[SetRoutes{clamped}, SetCounts]` が正（§3.8.4） | **不整合の解消** |
| 11 | `RouteTable::max_arrive() -> f32`（既定と全 override の `arrive` の最大、`None` は 0 として扱う）の追加を依頼。`SetCounts` の `precheck` が使う | API 追加 |
| 11 / 30 | `playback.rs` の**所有者は本書**と裁定。`advance` は f64 化するだけでなく `position_after(origin_count, elapsed_seconds, ...)` に置換し、**フレーム dt の累加をやめる**（§3.8.3）。現行形は `00-conventions.md` 不変条件 4 と `PRODUCT_QUALITY.md` の再生決定論に反する。30 §7 の property test P-1（`advance` と `wrap_timeline` が ±1 サンプル以内）は、この置換で初めて長時間再生でも成立する | **裁定・欠陥の修正** |
| 30 | §3.7.2 `PlaybackClock::position_count` の `tempo.count_at(self.position_seconds(now, speed) as f32)` を `count_at_f64(self.position_seconds(now, speed))` に変更し、戻り値を `f64` にすることを依頼（11 §9-7 の指摘どおり）。`as f32` が入口で 1.46 サンプル分を落としている | **欠陥** |
| 23 | `CameraKeyframe.count` / `CameraCut.at` は **f32 のままでよい**と裁定（§3.8.1）。1/32 カウント = 15.6 ms のずれは 60 fps の 1 フレーム 16.7 ms 未満で観測できない。ただし `insert_keyframe` の `count` 一致判定が粗くなるのを防ぐため、`CameraKeyframe::validate` に `count.abs() <= MAX_TIMELINE_COUNTS` の検査を追加してほしい | 裁定 + 検査追加 |
| 30 | `SyncAnchor { count, seconds }` の型を**照会**（§9 U17）。どちらもタイムライン軸上の位置なので `TempoChange.count` と同じ議論が当てはまる。`Document.audio` は永続状態なので、f64 にするなら v2 に含めなければならない。30 の裁定を待って本書の migration 表と V15 を最終化する | **v2 締切前の要決定** |
| 全体 | 事業方針の変更（完全オープンソース化、有料ティア・権限機構なし、`drill-license` 廃止）は本書の設計に影響しない。`Document` は権限・ライセンス・機能フラグに相当するフィールドを 1 つも持たず、`Edit` にもゲートは無い。品質ゲート（I-1〜I-23、V1〜V25、性能予算）はいずれも据え置く | 影響なし |
