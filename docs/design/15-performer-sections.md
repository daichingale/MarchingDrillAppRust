# 15. 演者・セクション・サブセット・プロップ

## 1. 目的と範囲

現状 `Performer` は `{ id, label, color }` の3フィールドしか持たず、セクション（楽器編成）・
記号・カラーガード/プロップの区別・3D表示に必要な身長を一切表現できない。本文書は次を設計する。

1. `Section`（編成区分）とその標準プリセット
2. `Performer` の拡張（section・symbol・color継承・height_m・kind・equipment）
3. ドリルナンバー（label）の採番・再採番・重複検出。ラベルと `PerformerId` の分離
4. サブセット（名前付き選択集合）とその永続化
5. プロップ・装備の表現方法とその設計判断の理由
6. `Selection` 型（PerformerId集合ベース、索引集合ではない）
7. ショー途中の人数変更（追加・欠員）に対する編集設計と、全セットの `positions` 配列との整合
8. 1,000人規模でセクション別フィルタ・色解決が毎フレーム走っても性能予算を侵さないための事前解決キャッシュ
9. v1ドキュメント（セクション情報なし）からの移行既定値

**扱わないこと**（他文書の担当）:

- `Edit` enum 本体・`History`・`SetId` 安定ID化・スキーマ移行の実行機構そのもの
  （`DESIGN_GAPS.md` A-1 / A-7、将来の `docs/design/10-document-model.md` の担当）。
  本文書はその `Edit` enum に **追加される** performer/section/subset 関連のバリアントのみを定義する。
- ルート・ゲート・ホールド（`docs/design/12`〜`14` 系列 / DESIGN_GAPS A-2）。
  `Performer` の移動そのものの補間方式には触れない。
- 3D描画でどのメッシュ・シェーダーを使うか（`docs/design/22`）。本文書は `height_m` や
  `PropDetails` という**データ**を渡すところまでで、描画側の消費方法は担当外。
- `Selection` の UI 操作（ドラッグ矩形の入力処理、キーボードショートカット）は `drill-app` 側
  （`docs/design/43`）。本文書は `Selection` の**データ構造とその導出関数**のみを定義する。
- 個々のリハーサル・本番ごとの出席（一時欠席）管理。これは名簿（roster）の構造的な増減とは別問題であり、
  スコープ外とする（下記 3.7 で理由を述べる）。

## 2. 現状

- `crates/drill-core/src/lib.rs:19` — `pub type PerformerId = u32;`（型エイリアス。新規IDを払い出す
  カウンタは `Document` に存在しない）。
- `crates/drill-core/src/lib.rs:226-230`

  ```rust
  pub struct Performer {
      pub id: PerformerId,
      pub label: String,
      pub color: [u8; 3],
  }
  ```

  セクション・記号・カラーガード/プロップ区分・身長のいずれも無い。
- `crates/drill-core/src/lib.rs:233-238` — `Set { name, counts, positions: Vec<Point> }`。
  `positions` は「`Document::performers` と**索引で**対応する密配列」（doc comment 明記）。
  Section/Subset/Selection の設計は、この密配列の索引対応を崩してはならない。
- `crates/drill-core/src/lib.rs:241-252` — `Document { schema_version, title, grid, tempo, audio,
  performers, sets }`。`sections` フィールドは無い。演者ID採番カウンタも無い
  （`Document::demo` は `i as PerformerId` で直接採番、lib.rs:259）。
- `crates/drill-core/src/lib.rs:302-333` — `Document::validate` は演者数とセット数の整合、
  `PerformerId` の重複のみ検査。ラベル重複は検査しない。
- `crates/drill-core/src/svg.rs:181-201` (`field_svg`) と `svg.rs:262-278` / `svg.rs:296-319`
  （`coordinate_sheet_html` / `drill_book_html`）は、いずれも
  `doc.performers.iter().enumerate()` で **索引** を取り、`set.positions.get(i)` で対応する座標を
  引いている。演者の色・ラベルを直接読むだけで、セクションという概念を経由していない。
- `crates/drill-core/src/countsheet.rs` はセット単位の時刻のみを扱い、演者を参照しない。
  セクション別カウントシート（「Tpのみ出力」）は現状不可能（本文書 A-5 が満たすべき前提）。
- `crates/drill-app/src/main.rs:84` — `selected: BTreeSet<usize>`。選択は**索引集合**。
  `main.rs:591,861` で `(0..self.document.performers.len()).collect()` として全選択を索引で構築している。
  演者の追加・削除・並べ替えが将来入ると、この索引はズレて別演者を指してしまう
  （`00-conventions.md` 不変条件2「同一性は安定ID、順序はVecの索引。両者を混同するAPIを作らない」に抵触）。
- `crates/drill-app/src/main.rs` 全体を検索した結果、演者を追加・削除する関数は**存在しない**
  （`performers.push` / `performers.remove` 等の呼び出し箇所なし）。ショー途中の人数変更は
  現状まったく実装されていない。
- `crates/drill-core/src/camera.rs` / `crates/drill-core/src/video.rs` は `Performer` を参照しない
  （grep で0件）。3D身長やプロップ形状を渡す経路は存在しない。
- `DESIGN_GAPS.md` A-5（200-226行）が本文書のたたき台を示しているが、採番規則・サブセット・
  プロップの実体設計・`Selection`型・人数変更時の整合・移行既定値・性能キャッシュには触れていない。
  本文書はそれらを埋める。

## 3. 設計

### 3.1 型の置き場所

`Section` と拡張後の `Performer` は `Document` と一体で検証されるため引き続き
`crates/drill-core/src/lib.rs` に置く。`Selection` / `Subset` / 採番ロジック / プリセットは
新規モジュール `crates/drill-core/src/roster.rs` に置き、`lib.rs` から `pub mod roster;` で公開する
（`svg.rs` や `countsheet.rs` と同格の「ドメイン計算モジュール」として扱う）。

```rust
// lib.rs
pub mod roster;
```

### 3.2 Section とプリセット

```rust
pub type SectionId = u32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub id: SectionId,
    pub name: String,   // "Trumpet"
    pub short: String,  // "Tp" — 密な表示（ドリルブック見出し等）用
    pub color: [u8; 3],
    /// ドリルブック・パネルでの表示順。値が同じ場合は `id` 昇順にフォールバックする。
    pub order: u16,
}
```

`Document` 側:

```rust
pub struct Document {
    // ...既存フィールド...
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub subsets: Vec<roster::Subset>,
    #[serde(default)]
    next_section_id: u32,
    #[serde(default)]
    next_subset_id: u32,
    #[serde(default)]
    next_performer_id: u32,
}
```

`next_performer_id` は `DESIGN_GAPS.md` A-1 が `Document` に置くとしているカウンタと同一のもの。
セクション/サブセットのIDカウンタも同じ流儀（`Document` が所有し、`Edit` 適用時にのみ進める）で揃える。

標準編成プリセットは `roster.rs` に置く。ID は呼び出し時に `Document` のカウンタから払い出すため、
プリセット自体は ID 抜きの記述（name/short/color/order）を返し、`Document` へ追加する関数が
ID を採番する。

```rust
pub struct SectionSpec {
    pub name: &'static str,
    pub short: &'static str,
    pub color: [u8; 3],
}

pub mod presets {
    use super::SectionSpec;

    pub const WINDS: &[SectionSpec] = &[
        SectionSpec { name: "Flute",      short: "Fl",   color: [186, 225, 255] },
        SectionSpec { name: "Clarinet",   short: "Cl",   color: [204, 229, 199] },
        SectionSpec { name: "Saxophone",  short: "Sax",  color: [255, 221, 161] },
        SectionSpec { name: "Trumpet",    short: "Tp",   color: [255, 179, 179] },
        SectionSpec { name: "Mellophone", short: "Mel",  color: [255, 200, 220] },
        SectionSpec { name: "Trombone",   short: "Tb",   color: [221, 190, 255] },
        SectionSpec { name: "Baritone",   short: "Bari", color: [190, 200, 255] },
        SectionSpec { name: "Tuba",       short: "Tuba", color: [150, 150, 220] },
    ];

    pub const BATTERY: &[SectionSpec] = &[
        SectionSpec { name: "Snare",  short: "Sn",   color: [230, 80, 80] },
        SectionSpec { name: "Tenor",  short: "Ten",  color: [230, 140, 60] },
        SectionSpec { name: "Bass",   short: "Bass", color: [200, 60, 60] },
        SectionSpec { name: "Cymbal", short: "Cym",  color: [230, 200, 60] },
    ];

    pub const FRONT_ENSEMBLE: &[SectionSpec] =
        &[SectionSpec { name: "Front Ensemble", short: "FE", color: [120, 200, 200] }];

    pub const COLOR_GUARD: &[SectionSpec] =
        &[SectionSpec { name: "Color Guard", short: "CG", color: [255, 120, 180] }];
}

/// Appends every preset section to `doc.sections`, assigning fresh IDs and
/// sequential `order` values continuing from the current max. Pure function
/// over the counter; callers wrap it in an `Edit` (see 3.6).
pub fn append_sections(doc_sections: &mut Vec<Section>, next_id: &mut u32, specs: &[SectionSpec]) -> Vec<SectionId> {
    let mut start_order = doc_sections.iter().map(|s| s.order).max().map_or(0, |o| o + 1);
    let mut ids = Vec::with_capacity(specs.len());
    for spec in specs {
        let id = *next_id;
        *next_id += 1;
        doc_sections.push(Section {
            id,
            name: spec.name.to_string(),
            short: spec.short.to_string(),
            color: spec.color,
            order: start_order,
        });
        ids.push(id);
        start_order += 1;
    }
    ids
}
```

「標準マーチングバンド編成」は `presets::WINDS` + `presets::BATTERY` + `presets::FRONT_ENSEMBLE` +
`presets::COLOR_GUARD` を順に `append_sections` する組み合わせとして UI 側（`drill-app`）が呼ぶ。
`drill-core` はプリセットの**内容**だけを持ち、「一括適用」という UI操作は持たない
（`ARCHITECTURE.md` の層分離のとおり、"何を新規作成するか" は core、"いつ作るか" は app）。

### 3.3 Performer 拡張

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Symbol {
    Circle,
    Square,
    Triangle,
    Diamond,
    Cross,
    Star,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PerformerKind {
    Wind,
    Percussion,
    Guard,
    Prop,
}

/// Equipment carried by a performer (does not get its own field position —
/// it moves with the performer). Distinct from `PerformerKind::Prop`, which
/// is an independently positioned object; see 3.5 for the distinction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Equipment {
    Flag,
    Rifle,
    Saber,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Performer {
    pub id: PerformerId,
    pub label: String,          // ドリルナンバー "T1" — 表示用。安定同一性は `id` が持つ
    pub section: SectionId,
    #[serde(default = "default_symbol")]
    pub symbol: Symbol,
    /// `None` ならセクション色を継承する。個別に色分けしたい演者（ソリスト等）だけ `Some` にする。
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    #[serde(default = "default_height_m")]
    pub height_m: f32,
    #[serde(default = "default_kind")]
    pub kind: PerformerKind,
    /// 手持ち装備。`PerformerKind::Guard` 以外では通常 `Equipment::None`。
    #[serde(default = "default_equipment")]
    pub equipment: Equipment,
    /// `kind == PerformerKind::Prop` のときのみ意味を持つ。3.5 節参照。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prop: Option<PropDetails>,
}

fn default_symbol() -> Symbol { Symbol::Circle }
fn default_kind() -> PerformerKind { PerformerKind::Wind }
fn default_equipment() -> Equipment { Equipment::None }
/// 成人の平均身長相当。3D表示のみに影響し、2D描画・座標系には影響しない。
fn default_height_m() -> f32 { 1.7 }
```

`#[serde(default = ...)]` を全フィールドに付けるのは、`00-conventions.md` の安全性要件
「信頼できない入力」対策でもある。手編集された、あるいは部分的にしか移行されていない
JSON を読んでも `unwrap`/`expect` を経由せずに妥当な既定値へフォールバックする。

`color: Option<[u8;3]>` の解決規則:

```rust
impl Performer {
    /// Resolve the performer's effective color: explicit override, else the
    /// owning section's color, else a neutral gray for orphaned sections
    /// (section id not found — defensive, should not happen in a valid Document).
    pub fn resolved_color(&self, sections: &[Section]) -> [u8; 3] {
        self.color.unwrap_or_else(|| {
            sections
                .iter()
                .find(|s| s.id == self.section)
                .map(|s| s.color)
                .unwrap_or([128, 128, 128])
        })
    }
}
```

毎フレームこの線形探索を呼ぶのは1,000人×セクション数十件でも数十マイクロ秒程度で予算内だが、
3.8節でさらに事前解決キャッシュを用意し、描画ループでは配列参照のみにする。

`Document` に人数のヘルパーを追加する（プロップを人数に含めない、A-5末尾の要件）:

```rust
impl Document {
    /// Performers that count as people on the field (excludes props).
    pub fn performer_count(&self) -> usize {
        self.performers.iter().filter(|p| p.kind != PerformerKind::Prop).count()
    }
    pub fn prop_count(&self) -> usize {
        self.performers.iter().filter(|p| p.kind == PerformerKind::Prop).count()
    }
}
```

### 3.4 ドリルナンバー（label）の採番

`PerformerId`（不変・内部同一性）と `label`（可変・表示用のドリルナンバー）は既に別フィールドとして
分離されている（lib.rs:227-228）。本節はその**採番規則**を定義する。

```rust
/// One label-assignment rule per section: prefix + zero-padding width.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NumberingScheme {
    pub prefix_by_section: std::collections::BTreeMap<SectionId, String>, // Trumpet -> "T"
    pub start_at: u32,   // 通常 1
    pub width: u8,       // 0 = パディングなし。2 なら "T01"
}

impl NumberingScheme {
    fn prefix_for(&self, section: SectionId) -> &str {
        self.prefix_by_section.get(&section).map(String::as_str).unwrap_or("P")
    }

    fn format(&self, prefix: &str, n: u32) -> String {
        if self.width == 0 {
            format!("{prefix}{n}")
        } else {
            format!("{prefix}{n:0width$}", width = self.width as usize)
        }
    }
}

/// Assigns labels to `order` (typically performers of one section, in the
/// desired display order — e.g. sorted by current label, or by field position)
/// starting at `scheme.start_at`. Returns `(PerformerId, new_label)` pairs;
/// does not mutate the document — callers wrap the result in `Edit::RelabelPerformers`.
pub fn assign_sequential(
    scheme: &NumberingScheme,
    section: SectionId,
    order: &[PerformerId],
) -> Vec<(PerformerId, String)> {
    let prefix = scheme.prefix_for(section);
    order
        .iter()
        .enumerate()
        .map(|(i, &id)| (id, scheme.format(prefix, scheme.start_at + i as u32)))
        .collect()
}

/// Labels that appear more than once, sorted, deduplicated. O(n log n).
/// Non-fatal by design (see 4節) — used for UI warnings, not for `Document::validate`.
pub fn duplicate_labels(performers: &[Performer]) -> Vec<String> {
    let mut seen = std::collections::BTreeMap::<&str, u32>::new();
    for p in performers {
        *seen.entry(p.label.as_str()).or_insert(0) += 1;
    }
    let mut dups: Vec<String> = seen
        .into_iter()
        .filter(|&(_, count)| count > 1)
        .map(|(label, _)| label.to_string())
        .collect();
    dups.sort();
    dups
}
```

一括採番・再採番は `Edit::RelabelPerformers { changes: Vec<(PerformerId, String)> }` として
編集履歴に積む（3.6節）。初版は `(id, before, after)` の3つ組で `before` を保持していたが、
`apply` が現在のラベルを `Document` から読んで逆操作の `changes` を組み立てる設計
（doc 10 決定A）に合わせ、payload からは `after` のみを持たせる形に修正した。
`Edit::apply` が逆操作を返す設計自体は `DESIGN_GAPS.md` A-1 のとおり。

ラベル重複は**ハードエラーにしない**。理由: 実際のドリル制作では「新入部員を仮に既存演者と同じ番号で
入れ、後でまとめて振り直す」運用が普通にある。`Document::validate` は `PerformerId` の重複のみ
拒否し続け（既存動作を変えない）、ラベル重複は `duplicate_labels` を使った**UI警告**として扱う。

### 3.5 プロップ・装備の表現 — 設計判断とその理由

**判断: プロップは `Performer` の一種（`PerformerKind::Prop`）として表現し、独立したエンティティには
しない。** ただしプロップ固有のデータは `Option<PropDetails>` に閉じ込め、通常の演者では常に `None`
（コストゼロ）にする。

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropShape {
    Flagpole,
    Panel,
    Cart,
    Backdrop,
    Custom,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PropDetails {
    pub shape: PropShape,
    /// 衝突判定半径（グリッド単位）。人物のデフォルト半径と別に持つ
    /// （DESIGN_GAPS A-4 の `ScanScratch` はこの値を演者ごとの半径として読む）。
    pub collision_radius: f32,
    /// 3D表示用のバウンディングボックス寸法（幅・奥行き・高さ、メートル）。
    pub bounds_m: [f32; 3],
}
```

理由:

1. **密配列を割らない。** `Set::positions` は `Document::performers` と索引で対応する密配列という
   不変条件が既にドキュメントされている（lib.rs:236 のdoc comment）。将来の `positions_at_count`
   （DESIGN_GAPS A-2）・掃引衝突検査 `ScanScratch`（A-4）・`field_svg`／`drill_book_html`
   （svg.rs:181, 296）は、すべてこの「1つの密配列を演者と一緒に舐める」前提で書かれている、
   または書かれる予定である。プロップを別エンティティにすると、これら**全て**に第二の平行な
   配列・第二の走査コードパスが必要になり、二つの配列が食い違えば
   「同じ `(Document, config, count)` からは常に同じ出力」（00-conventions 不変条件5）が壊れる。
   `PerformerKind::Prop` として同じ配列に混ぜれば、既存・計画中の全消費側コードは変更不要か、
   最小の `kind` フィルタ追加で済む。
2. **移動の仕組みを再利用できる。** 台車やパネルもセットからセットへ「経路」を持って動く
   （DESIGN_GAPS A-2 の `RouteTable`／`Gate`）。人と全く同じ補間・イージングの仕組みが要る。
   別エンティティにすると `RouteTable` も二重管理になる。
3. **疎なコスト。** プロップは通常ショー全体の数%程度。`Option<PropDetails>` は `None` のとき
   タグ+パディング程度のコストで、1,000人中の大多数（プロップでない演者）には実質無視できる
   オーバーヘッドしかない。別エンティティ方式は `Document` に第二のID空間・第二の
   `Selection` 解決経路・第二の人数カウント意味論を要求し、複雑さで勝らない。
4. **認めるデメリット。** `PerformerKind::Prop` という命名は「プロップは演者ではない」という
   直感とは食い違う。これは `Performer` を「人」ではなく「フォーメーション上で位置を持つ実体」
   という意味で読み替えることで解消する（doc comment に明記する）。人数カウント
   （`performer_count`）・選択UI上の見た目（アイコンを人型でなくプロップ形状にする）で
   利用者向けの区別は別途表現するため、内部表現が統一されていることによる利用者側の混乱は無い。

**手持ち装備（旗・ライフル・サーベル）とは区別する。** 演者が手に持つだけの旗やライフルは
その演者自身の位置と完全に一致して動くため、独立した位置を持たない。これは
`Performer.equipment: Equipment`（3.3節）という軽量フィールドで表現し、`PropDetails` は使わない。
一方、演者から独立して（あるいは複数人で押して）動くパネル・台車・バックドロップは
`kind = PerformerKind::Prop` + `prop = Some(PropDetails{..})` として**自分の行を持つ**。

判定基準: 「その物体は特定の一人の位置と常に一致するか？」→ Yes なら `Equipment`、
No（独立した点として置きたい、あるいは複数人で操作する）なら `PerformerKind::Prop`。

### 3.6 サブセット（名前付き選択集合）

`SubsetId` は本文書の初版では `pub type SubsetId = u32;`（型エイリアス）としていたが、
`docs/design/10-document-model.md` §3.1.1 のレビューにより、`PerformerId`/`SectionId`/`SetId` と
同じ `stable_id!` newtype（`NonZeroU32` 内包）へ格上げされた。理由は同じ3点
（索引との混同防止・`0` の敵性入力拒否・`Option<T>` のニッチ最適化）で、意味論
（「保存された選択集合の同一性」）は変わらない。本文書は表現をこれに合わせる。

```rust
// SubsetId は doc 10 §3.1.1 の stable_id!(SubsetId, "named selection subset") を用いる。
// 本文書はこの型を再定義せず、doc 10 が定義したものをそのまま使う。

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Subset {
    pub id: SubsetId,
    pub name: String, // "Trumpet 1st", "全金管", "ソリスト4人"
    /// 意図的に `PerformerId` のみを持つ。存在しないIDが混ざっていても構わない
    /// （4節「ぶら下がりID」の不変条件、doc 10 の I-17 を参照）。
    pub members: std::collections::BTreeSet<PerformerId>,
}
```

`PartialEq` を derive するのは、`Document` の内容等価性検査（doc 10 の不変条件 I-14:
「10,000回の編集を適用しUndoすると初期文書と一致する」をUndo後の構造的比較で検証する）が
`Subset` の等価比較を必要とするため。derive し忘れると `Document` 全体の `#[derive(PartialEq)]`
がコンパイルできないか、比較が意図せず浅くなる。

`Document.subsets: Vec<Subset>` として文書に永続化する（3.2節ですでに `Document` へ追加済み）。
検索・再選択用のヘルパー:

```rust
impl Document {
    pub fn subset(&self, id: SubsetId) -> Option<&Subset> {
        self.subsets.iter().find(|s| s.id == id)
    }
    pub fn subset_named(&self, name: &str) -> Option<&Subset> {
        self.subsets.iter().find(|s| s.name == name)
    }
}
```

サブセットへの変更は `Edit` を通す。本文書が追加する variant（3.9節でまとめて列挙）:
`InsertSubset` / `RemoveSubset` / `RenameSubset` / `SetSubsetMembers`
（初版は `AddSubset` としていたが、`InsertSet`/`InsertSection` と綴りを揃えるため
doc 10 のレビューで `InsertSubset` に改名された）。

一括編集・出力フィルタからの利用: `Selection::by_subset`（3.7節）でサブセットを選択集合に
展開し、以降は通常の `Selection` として一括移動・一括採番・カウントシート絞り込みに使う。
つまりサブセットは「`Selection` を保存・復元する仕組み」であり、`Selection` 自体とは別の型として
持つ（サブセットは文書に保存されるデータ、`Selection` はその時点のUI状態）。

### 3.7 Selection 型

```rust
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection(std::collections::BTreeSet<PerformerId>);

impl Selection {
    pub fn new() -> Self { Self::default() }
    pub fn from_ids(ids: impl IntoIterator<Item = PerformerId>) -> Self {
        Self(ids.into_iter().collect())
    }
    pub fn ids(&self) -> impl Iterator<Item = PerformerId> + '_ { self.0.iter().copied() }
    pub fn contains(&self, id: PerformerId) -> bool { self.0.contains(&id) }
    pub fn len(&self) -> usize { self.0.len() }
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
    pub fn clear(&mut self) { self.0.clear(); }
    pub fn insert(&mut self, id: PerformerId) -> bool { self.0.insert(id) }
    pub fn remove(&mut self, id: PerformerId) -> bool { self.0.remove(&id) }
    pub fn toggle(&mut self, id: PerformerId) {
        if !self.0.remove(&id) { self.0.insert(id); }
    }

    /// Replace the selection with everything *not* currently selected,
    /// restricted to `all` (typically `doc.performers.iter().map(|p| p.id)`).
    pub fn invert(&mut self, all: &[PerformerId]) {
        let complement: std::collections::BTreeSet<PerformerId> =
            all.iter().copied().filter(|id| !self.0.contains(id)).collect();
        self.0 = complement;
    }

    pub fn by_section(doc: &Document, section: SectionId) -> Self {
        Self(doc.performers.iter().filter(|p| p.section == section).map(|p| p.id).collect())
    }

    /// Dangling IDs in the stored subset (performers since removed) are
    /// silently dropped — see 4節.
    pub fn by_subset(doc: &Document, subset: SubsetId) -> Self {
        let live: std::collections::BTreeSet<PerformerId> =
            doc.performers.iter().map(|p| p.id).collect();
        match doc.subset(subset) {
            Some(s) => Self(s.members.intersection(&live).copied().collect()),
            None => Self::default(),
        }
    }

    /// Extends the selection to include every performer sharing a section
    /// with any currently-selected performer ("同一セクション拡張").
    pub fn extend_same_section(&mut self, doc: &Document) {
        let sections: std::collections::BTreeSet<SectionId> = doc
            .performers.iter()
            .filter(|p| self.0.contains(&p.id))
            .map(|p| p.section)
            .collect();
        self.0.extend(
            doc.performers.iter().filter(|p| sections.contains(&p.section)).map(|p| p.id),
        );
    }

    /// Rectangle-select at a given set's positions. `set_index` picks which
    /// dense position array to test against — the caller resolves the active
    /// set/count outside this function (this module does not know about
    /// counts/interpolation, see 目的と範囲).
    pub fn rectangle(doc: &Document, set_index: usize, min: Point, max: Point) -> Self {
        let Some(set) = doc.sets.get(set_index) else { return Self::default() };
        Self(
            doc.performers.iter().zip(&set.positions)
                .filter(|(_, p)| p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y)
                .map(|(perf, _)| perf.id)
                .collect(),
        )
    }

    /// Resolve to dense indices into `doc.performers` / `Set::positions`, in
    /// ascending index order, for the hot move/apply loop. O(n) — call once
    /// per edit gesture, not once per frame.
    pub fn to_indices(&self, doc: &Document) -> Vec<usize> {
        doc.performers.iter().enumerate()
            .filter(|(_, p)| self.0.contains(&p.id))
            .map(|(i, _)| i)
            .collect()
    }
}
```

**索引集合ではなく `PerformerId` 集合で持つ理由**（現状 `main.rs:84` の `BTreeSet<usize>` との対比）:
セット間で演者の並び順は変わらない前提であれば索引でも動くが、3.9節で導入する
`InsertPerformers`/`RemovePerformers` が「任意位置への挿入・削除」を許す以上、索引は編集のたびに
ズレる。`PerformerId` は演者が生きている限り不変なので、`Undo`/`Redo`・追加・削除をまたいでも
選択が破壊されない。`to_indices` は都度 O(n) だが、これは**編集操作の入口**（ドラッグ開始、
プロパティパネルを開いた瞬間など）で1回だけ呼ぶものであり、毎フレーム呼ぶものではない
（3.8節で毎フレーム経路とは別に切り分ける）。

### 3.8 事前解決キャッシュ（性能）

セクション別フィルタ・色解決を毎フレーム行っても16.6ms予算を侵さないよう、
`document_revision: u64`（DESIGN_GAPS A-4 が導入する、`Document` の変更ごとに増分するカウンタ）
を使って結果をキャッシュする。

```rust
/// Index-aligned with `Document::performers`. Rebuilt only when
/// `document_revision` changes — never inside the per-frame draw loop.
#[derive(Default)]
pub struct PerformerRenderCache {
    revision: u64,
    pub resolved_color: Vec<[u8; 3]>,
    pub section_slot: Vec<u16>, // index into `Document::sections`, not a SectionId lookup
    pub is_prop: Vec<bool>,
}

impl PerformerRenderCache {
    pub fn refresh(&mut self, doc: &Document, revision: u64) {
        if self.revision == revision && self.resolved_color.len() == doc.performers.len() {
            return;
        }
        self.resolved_color.clear();
        self.section_slot.clear();
        self.is_prop.clear();
        let mut slot_of: std::collections::BTreeMap<SectionId, u16> = Default::default();
        for (i, s) in doc.sections.iter().enumerate() {
            slot_of.insert(s.id, i as u16);
        }
        for p in &doc.performers {
            let slot = slot_of.get(&p.section).copied().unwrap_or(0);
            let section_color = doc.sections.get(slot as usize).map_or([128, 128, 128], |s| s.color);
            self.resolved_color.push(p.color.unwrap_or(section_color));
            self.section_slot.push(slot);
            self.is_prop.push(p.kind == PerformerKind::Prop);
        }
        self.revision = revision;
    }
}
```

`refresh` は `document_revision` が変わったフレームでのみ O(演者数 + セクション数) を払う。
それ以外の全フレームは `resolved_color[i]` / `section_slot[i]` の配列参照のみ
（ハッシュマップ探索もBTreeSet探索もしない）。これを描画側（`drill-render`、doc 20）が
`DrawCmd::Dot` を組み立てる既存の演者ループの中でそのまま使えば、セクション色解決の追加コストは
実質ゼロになる。

同様に「特定セクションだけ表示」フィルタも `section_slot[i] == target_slot` の配列比較で足り、
`Selection`（BTreeSet探索）を経由しない。`Selection` によるハイライト表示（選択中の演者を強調）は
1,000件に対して `BTreeSet<u32>::contains` を1,000回呼んでも `O(log 1000) ≈ 10` 回の比較 × 1,000
= 1万比較で、これは1マイクロ秒未満であり、専用キャッシュを設けるまでもない
（上限規模4,000人でも4万比較、依然として無視できる）。

### 3.9 人数変更（追加・欠員）と positions 配列の整合

`DESIGN_GAPS.md` A-1 が示す `Edit` enum の骨格に、次のバリアントを **本文書の担当として** 追加する
（`Edit` enum 自体の集約・`History` 連携・ID採番は doc 10 の担当。ここでは意味論とアルゴリズムを
定義し、doc 10 のレビューで確定した正確なバリアント名・payload形に合わせる）。

初版では `AddPerformers.positions: Vec<Vec<Point>>` を**セット索引順**（外側の `Vec` の `k` 番目が
`doc.sets[k]` に対応）で持たせていたが、これは `DESIGN_GAPS.md` §0 バグ#2
（`MoveCommand.set_index` が添字だったために、セット挿入/削除で既存履歴の指す先がずれ、
Undoが別のセットを破壊する）と**同じ種類のバグを演者側で再発させる**欠陥だった。
セットの並び順は `MoveSet`/`InsertSet`/`RemoveSet` によって変わり得るため、「今の索引」で
座標列を記録すると、記録後にセット順序が変わった場合に誤ったセットへ座標を書き戻す。
doc 10 のレビューでこれを指摘され、`SetId` で行を特定する `Vec<(SetId, Vec<Point>)>` へ修正した
（`PerformerExtract::columns`。以下のバリアント定義に反映済み）。

```rust
pub enum Edit {
    // ...doc 10 が定義する MovePoints/InsertSet/RemoveSet 等...

    /// 演者の追加（新規、または `RemovePerformers` の逆操作としての復元）。
    /// `PerformerExtract` は「削除されたときに一緒に退避しなければならない、
    /// 演者キー付きデータの全体」を表す一つの構造体（3.9節末尾を参照）。
    InsertPerformers(Box<PerformerExtract>),
    /// `apply` が `Document` から現在の内容を読み取り、`PerformerExtract` を
    /// 組み立てて逆操作として返す（decision A: payload に前状態を持たない）。
    RemovePerformers { targets: Vec<PerformerId> },

    /// 一括採番・再採番（3.4節）。`(id, new_label)`。`before` は持たない —
    /// `apply` が現在のラベルを `Document` から読んで逆操作の `changes` を組み立てる。
    RelabelPerformers { changes: Vec<(PerformerId, String)> },
    /// `(id, new_section)`。演者ごとに異なる遷移先を持ち得る（不均質）ため、
    /// 逆操作も単一の `AssignSection`（`Batch` ではない）になる。`before` は持たない。
    AssignSection { changes: Vec<(PerformerId, SectionId)> },

    AddSection    { section: Section },
    RemoveSection { at: usize, section: Section },
    RenameSection { id: SectionId, before: Section, after: Section },

    /// 初版は `AddSubset` という名前だったが、`InsertSet`/`InsertSection` と綴りを
    /// 揃えるため `InsertSubset` に改名した（doc 10 レビュー）。
    InsertSubset {
        at: usize,
        /// `None` は新規採番、`Some` は `RemoveSubset` の逆操作として元のIDを復元する。
        id: Option<SubsetId>,
        name: String,
        members: std::collections::BTreeSet<PerformerId>,
    },
    /// `apply` が `Document` から該当サブセットの内容を読み、逆操作
    /// （`InsertSubset { id: Some(..), .. }`）を組み立てる。`before` は持たない。
    RemoveSubset { subset: SubsetId },
    /// `apply` が現在の名前を読んで逆操作を組み立てる。`before` は持たない。
    RenameSubset { subset: SubsetId, name: String },
    /// メンバー集合を丸ごと置き換える。`mem::swap` で逆操作を構築できるため
    /// 差分ではなく全置換にする。`before` は持たない —
    /// `apply` が現在のメンバー集合を読んで逆操作の `members` にする。
    SetSubsetMembers { subset: SubsetId, members: std::collections::BTreeSet<PerformerId> },
}

/// `Document` が演者キーで持つ、かつ **`RemovePerformers` が実際に削除する** データの全体。
/// 新しい「演者キー付きサブシステム」を足すときは、この構造体を拡張するか、
/// 3節末尾の「ぶら下がりID許容」規約を選ぶかのどちらかを選択しなければならない
/// （選ばなかった場合にテストで検出できるようにする、9節参照）。
///
/// 意図的に含まれていないもの（ぶら下がりIDを許容し、`RemovePerformers` が触らない）:
///   - `Subset::members`（本節末尾／4節不変条件4、doc 10 の I-17）
///   - `Set.shape.order`（doc 14、doc 10 の I-18）
///   - `FollowTarget::Group`（doc 23）
pub struct PerformerExtract {
    pub at: usize,
    pub performers: Vec<Performer>,
    /// `columns[k] = (set_id, points)` — `points[i]` は `performers[i]` の、
    /// セット `set_id` における座標。**セットの索引ではなく `SetId` で行を特定する**
    /// ことで、セット順序の変化に対して不変になる（本節冒頭の欠陥修正）。
    pub columns: Vec<(SetId, Vec<Point>)>,
    /// ルート個別指定（doc 11 `RouteTable`）も演者キー付きデータなので、
    /// `RemovePerformers` が削除する対象に含める。
    pub route_overrides: Vec<(SetId, PerformerId, Route)>,
}
```

`InsertPerformers` の適用アルゴリズム（`RemovePerformers` はこの逆で、`Document` から読み取って
`PerformerExtract` を組み立てる）:

```rust
fn apply_insert_performers(doc: &mut Document, extract: PerformerExtract) -> Result<Edit, DrillError> {
    let at = extract.at.min(doc.performers.len());
    for (set_id, points) in &extract.columns {
        let set_index = doc.require_set(*set_id)?; // SetId で解決。索引の陳腐化に影響されない
        if points.len() != extract.performers.len() {
            return Err(DrillError::SetSizeMismatch {
                set_index, expected: extract.performers.len(), found: points.len(),
            });
        }
        // `Set::positions` は doc 10 §3.2 で `pub(in crate::document)` — 実装は同一モジュール内
        // からの直接フィールドアクセスで splice する。ここは行の対応規則のみを示す疑似コード。
        doc.sets[set_index].splice_positions(at..at, points.iter().copied());
    }
    let targets = extract.performers.iter().map(|p| p.id).collect();
    doc.performers.splice(at..at, extract.performers.iter().cloned());
    // route_overrides の復元は doc 11 の RouteTable API を経由する（本文書の担当外）。
    Ok(Edit::RemovePerformers { targets }) // 逆操作
}

fn apply_remove_performers(doc: &mut Document, targets: &[PerformerId]) -> Result<Edit, DrillError> {
    let at = targets.iter()
        .map(|&id| doc.require_performer(id))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter().min().unwrap_or(0); // 復元位置は最小索引（doc 10 の一般則に従う）
    let performers: Vec<Performer> = targets.iter()
        .filter_map(|&id| doc.performer(id).cloned())
        .collect();
    let columns = doc.sets.iter()
        .map(|s| (s.id, targets.iter()
            .filter_map(|&id| doc.performer_index(id).and_then(|i| s.positions().get(i).copied()))
            .collect()))
        .collect();
    // 実際の削除（doc.performers.retain / 各 Set.positions からの除去）はここで行う。
    Ok(Edit::InsertPerformers(Box::new(PerformerExtract {
        at, performers, columns, route_overrides: Vec::new(), // doc 11 の RouteTable から収集
    })))
}
```

**サブセット・選択との整合**: `RemovePerformers` は `Document.subsets` の `members` を
**書き換えない**（doc 10 の I-17 と同一の規約）。`Subset.members` は「その時点で存在するとは
限らないIDの集合」として扱う不変条件（4節）を採用し、`Selection::by_subset`（3.7節）が
生存中のIDとの積を取ることでぶら下がりIDを吸収する ─ つまり**表示・展開のたびにフィルタする**のであって、
削除のたびにサブセット側を掃除するのではない。理由は単なる手抜きではなく可逆性の要請である。
もし `RemovePerformers` がサブセットを刈り込む設計にすると、その逆操作は「刈り込む前の
全サブセットのメンバー集合」を丸ごと保持しなければならず、上限規模（演者4,000人・サブセット
数百件）では逆操作の payload が最大32MB程度に膨れ、かつ「演者を削除してUndoしたらサブセットの
中身が減ったままだった」という可逆性の破れが起こり得る。触らないことで逆操作が
`PerformerExtract`（演者配列・座標配列・ルート個別指定）だけに閉じ、`Edit` 可逆性の証明が
サブセットの内容に依存しなくなる。この規約は `Set.shape.order`（doc 14）や
`FollowTarget::Group`（doc 23）にも同じ形で適用される（doc 10 の I-18）。

**欠員のスコープ**: 本文書が扱うのは「シーズン中に部員が加入・退部し、ロースター（`Document.performers`）
自体が変わる」ケースである。個々のリハーサル・本番での一時欠席（今日だけ休み）は
出席管理の領域であり、`Document` の構造とは別物として扱う。将来必要になった場合も
`Performer` に `active: bool` のような可変フラグを足すのではなく、公演ごとの出欠は
`Document` の外側（別ファイルまたは将来のroster機能）で持つべきであり、本文書はその設計を提案しない
（`Document` の同じ演者集合を使い回しながら「今日は誰が休みか」だけ変わるようにしたいのであれば、
それは `Document` の不変な入力に対する一時的なフィルタであり、`positions` を伴う構造編集
とは性質が違うため）。

## 4. 不変条件

テストで検証可能な形で列挙する。

1. **演者IDの一意性**（既存、維持）: `Document::validate` は `PerformerId` の重複を拒否する
   （lib.rs:324-331 の検査を維持。ラベル重複はここに含めない）。
2. **密配列の索引対応**（既存、維持・強化）: 任意の `Edit` 適用後も
   `doc.sets.iter().all(|s| s.positions().len() == doc.performers.len())` が成り立つ。
   `InsertPerformers`/`RemovePerformers` は演者配列と**全ての**セットの `positions` を
   同一トランザクションで変更し、行の対応は索引ではなく `SetId`（`PerformerExtract::columns`）
   で特定する（3.9節）。
3. **セクション参照の非パニック性**: `Performer.section` が `Document.sections` に存在しない
   `SectionId` を指していても `resolved_color` はパニックせず既定色 `[128,128,128]` を返す
   （3.3節）。存在しないセクションへの参照は `Document::validate` の**警告**対象になり得るが
   （将来 `DrillError` に追加してもよいが本文書では必須としない）、致命的エラーにはしない。
4. **サブセットのぶら下がりID許容**（doc 10 の I-17 と同一規約）: `Subset.members` は
   `Document.performers` に存在しない `PerformerId` を含んでよい。`Document::validate` は
   これを拒否しない。`RemovePerformers` はサブセットを一切書き換えない
   （3.9節「サブセット・選択との整合」）。ぶら下がりIDの除去は**表示・展開する側の責務**であり、
   `Selection::by_subset`（3.7節）が呼ばれるたびに生存中のIDとの積を取ってフィルタする。
   すなわち「削除時に刈り込む」のではなく「使う時に濾す」規約であり、`Subset.members` 自体は
   刈り込まれないまま文書に残り続けてよい。
5. **ラベルは同一性を持たない**: 2つの異なる `PerformerId` が同じ `label` を持つ状態を
   `Document::validate` は**許可する**（不変条件1と非対称。3.4節で理由を説明済み）。
   `duplicate_labels` は非フェイタルな検出関数として別に用意する。
6. **Selectionの安定性**: `Selection` に含まれる `PerformerId` の集合は、選択後に行われた
   移動・並べ替え・他の演者の追加/削除の `Edit` によって変化しない
   （削除された本人が選択に残っている場合を除き、変化しないことをプロパティテストで検証する。
   3.7節の型がそもそも索引を持たないため構造的に保証される）。
7. **色解決の決定論**: 同じ `(Performer.color, Performer.section, Document.sections)` の組からは
   常に同じ `resolved_color` が出る。`PerformerRenderCache::refresh` はこの純粋関数の結果を
   キャッシュするだけであり、`document_revision` が同じ限り再計算しても同じ配列になる
   （00-conventions 不変条件5に整合）。
8. **プロップは人数に含まれない**: `Document::performer_count()` は
   `kind == PerformerKind::Prop` の行を数えない。`prop_count()` はその逆。両者の和は
   常に `doc.performers.len()` に一致する。

## 5. 性能

基準規模: 演者1,000人、セクション最大20件程度（実際のマーチングバンド編成でこれを超えることは
まれ）、サブセット数十件。16.6ms予算のうち、本文書が扱うデータ層は**定常フレームでほぼ0ms**を
使う設計にする。内訳:

| 処理 | 頻度 | コスト | 備考 |
|---|---|---|---|
| `resolved_color[i]` 参照 | 毎フレーム、演者ごと | O(1) 配列参照 | 描画ループに元々あるループへ統合。追加コストは実質ゼロ |
| `section_slot[i]` 比較（セクションフィルタ） | 毎フレーム、演者ごと | O(1) 整数比較 | 同上 |
| `PerformerRenderCache::refresh` | `document_revision` 変化時のみ（編集のたび、フレームではない） | O(演者数 + セクション数) ≈ 1,000 + 20 | 1,000人で数マイクロ秒〜数十マイクロ秒。編集操作の遅延として許容範囲（Undo/Redo 2ms予算に対して無視できる） |
| `Selection::contains`（ハイライト表示） | 毎フレーム、選択中のみ | O(log 選択数) × 演者数 ≈ 1,000 × 10 | 最大でも1万回の比較、1マイクロ秒未満。専用キャッシュ不要（3.8節末尾） |
| `Selection::rectangle` / `by_section` / `by_subset` | ユーザー操作イベントごと（ドラッグ確定、パネルクリック） | O(演者数) ≈ 1,000 | 毎フレームではないため16.6ms予算の対象外。数十マイクロ秒 |
| `Selection::to_indices` | 移動編集の適用ごと（ドラッグ確定時に1回） | O(演者数) ≈ 1,000 | Undo/Redo 2ms予算に対して無視できる |
| `InsertPerformers`/`RemovePerformers` 適用 | 稀（ロースター変更時のみ） | O(セット数 × 演者数) ≈ 64 × 1,000 = 64,000 要素シフト | 数百マイクロ秒〜1ms程度。頻度が低いため16.6ms/2msどちらの予算にも実質影響しない |
| `assign_sequential` 一括採番 | 稀（採番操作時） | O(セクション内人数) | 通常数十〜数百人、無視できる |

上限規模（演者4,000人・セット256）でも `InsertPerformers` の最悪計算量は 256 × 4,000 = 約100万要素
シフトとなり、これは「稀な操作」であることを踏まえてもミリ秒オーダーに収まる
（`Vec::splice` は要素が `Point`（8バイト、`Copy`）なので `memmove` 相当、100万要素 ≒ 8MB の
移動は現代的なメモリ帯域では1ms未満）。定常フレームには一切現れないため、
60fps・16.6ms予算には算入しない。

## 6. 失敗モードと安全性

- **信頼できない入力（ファイル読込）**: `Performer` の新フィールドは全て `#[serde(default = ...)]`
  を持つため、v1相当の最小フィールド、あるいは手編集で一部フィールドを欠いたJSONを読んでも
  `serde_json::from_str` は失敗せず、既定値（Circle/Wind/None/1.7m/None）にフォールバックする。
  `unwrap`/`expect`/添字パニックはこの経路に一切現れない（3.3節のコードはすべて `unwrap_or`
  系で書く）。
- **不正な `SectionId` 参照**: 破損・改変されたファイルが存在しない `SectionId` を指す
  `Performer.section` を持っていても、`resolved_color`・`PerformerRenderCache::refresh` は
  既定色にフォールバックしパニックしない（4節不変条件3）。
- **プロップの寸法・半径の異常値**: `PropDetails.collision_radius` / `bounds_m` に
  NaN・Inf・負値が入り得る（インポート元が壊れている場合）。掃引衝突検査（DESIGN_GAPS A-4）側で
  「NaN/Inf拒否」を行う前提だが、本文書側でも `PropDetails` を構築するUI操作は正の有限値のみ
  受け付けるようバリデーションする（`0.0 < r && r.is_finite()`）。
- **`InsertPerformers`/`RemovePerformers` の長さ不一致**: `PerformerExtract.columns` の各
  `(set_id, points)` について `points.len() != extract.performers.len()` となる場合や、
  存在しない `SetId` を指す場合は `DrillError::SetSizeMismatch` / `DrillError::UnknownSet` を
  返して**適用前に**拒否する（3.9節）。部分適用によって一部のセットだけ演者数がずれた
  壊れた `Document` を作らない。
- **`at` が範囲外**: `apply_insert_performers` は `at.min(doc.performers.len())` でクランプし、
  末尾追加にフォールバックする（パニックしない）。`RemovePerformers` 側は `targets` に
  存在しない `PerformerId` が混ざっている場合 `DrillError::UnknownPerformer` を返し、
  何も削除しない（部分削除を許さない）。
- **サブセット・セクションの巨大化によるファイル肥大**: サブセットは `PerformerId`（`u32`）の
  集合のみを持ち、`BTreeSet<u32>` はシリアライズすると単純な数値配列になるため、
  4,000人規模・数百サブセットでも数百KB程度に収まる。上限を設けるかは
  `docs/design/51-security.md`（信頼できない入力の要素数上限）の担当だが、本文書としては
  「サブセット数・メンバー数に対する明示的な上限チェックが必要」であることを申し送る
  （9節未決事項）。

## 7. テスト計画

**単体テスト**（`roster.rs` 内、既存モジュールと同じ `#[cfg(test)] mod tests` 形式）:

- `resolved_color` が override あり/なし/セクション不明の3ケースで正しい色を返す。
- `assign_sequential` が `start_at`・`width` を反映したラベル列を生成する（"T1".."T12"、
  ゼロ埋め "T01" の両方）。
- `duplicate_labels` が重複ラベルのみを返し、順序が安定している（ソート済み）。
- `Selection::by_section` / `by_subset` / `extend_same_section` / `invert` / `rectangle` の
  それぞれが期待する `PerformerId` 集合を返す。
- `Selection::by_subset` がぶら下がりID（生存していない `PerformerId`）を積んだ `Subset` に対して
  パニックせず、生存IDのみを返す（4節不変条件4の直接テスト）。
- `apply_insert_performers` / 逆操作（`RemovePerformers`）が
  「追加→即削除」で元の `Document` と完全一致することを確認するラウンドトリップテスト。
- `apply_insert_performers` が `columns` 内の `points.len()` 不一致で
  `Err(DrillError::SetSizeMismatch)` を返す。
- セット順序を入れ替えてから `RemovePerformers`→Undo（`InsertPerformers`）を行っても、
  各演者が正しいセットの正しい座標へ復元される（`SetId` ベースの行対応を検証する回帰テスト。
  本節冒頭で修正した欠陥の直接テスト）。
- `Document::performer_count()` / `prop_count()` の和が `performers.len()` に一致する
  (kind混在ケース)。
- `PerformerRenderCache::refresh` が同じ `revision` の2回目呼び出しでは配列を再構築しない
  （`interpolation_reuses_output_allocation`（lib.rs:485）と同じ手法でポインタ比較する）。

**プロパティテスト**:

- 任意個数のランダム `InsertPerformers`/`RemovePerformers`/`RelabelPerformers` を適用し、
  それぞれの逆操作を続けて適用すると初期 `Document` と一致する
  （DESIGN_GAPS A-1 の「10,000回のランダムEdit→全Undo→完全一致」、doc 10 の I-14 に
  本文書のvariantも含める）。
- 任意の `Selection` は、選択に含まれないIDへの `InsertPerformers`/`RemovePerformers` を挟んでも
  中身（IDの集合）が変化しない。

**ゴールデンテスト**:

- セクション付き `Document` に対する `field_svg` 出力が、セクション色を継承した演者の描画色を
  正しく反映する（`svg.rs` 側のテスト、本文書はテストデータとしてのセクション付き `Document` を
  用意する）。

**移行回帰テスト**（A-7と連携）:

- v1フィクスチャ（セクション・サブセットなし）を読み込み、全演者が単一の "Ensemble" セクションに
  属し、`color` が元の `Performer.color` をそのまま保持していること（視覚的に変化しないこと）を
  確認する。

**ベンチマーク**（`crates/drill-core/benches/`、既存ディレクトリを利用）:

- 1,000人・20セクションでの `PerformerRenderCache::refresh` 単発コストが5節の見積り
  （数十マイクロ秒オーダー）を超えないことを計測する。
- 1,000人・64セットでの `InsertPerformers`（1人追加）が1ms未満であることを計測する。

## 8. 実装タスク

Codexに渡せる粒度（1タスク=1〜3時間）に分解する。依存がある箇所は明示する。

1. **`Section`・プリセット定義**（依存なし）: `lib.rs` に `Section`/`SectionId` を追加、
   `roster.rs` に `SectionSpec`/`presets`/`append_sections` を実装。単体テスト込み。
2. **`Performer` 拡張**（依存: 1）: `Symbol`/`PerformerKind`/`Equipment`/`PropShape`/`PropDetails`
   を追加し、`Performer` に `section`/`symbol`/`color: Option`/`height_m`/`kind`/`equipment`/`prop`
   を追加。`resolved_color` 実装。既存の `Performer { id, label, color }` 直書き箇所
   （`Document::demo`、lib.rs:255-300）を新シグネチャに合わせて更新し、`color` を
   `Some(...)` に変更（既存テストが色を直接比較している箇所の追従込み）。
3. **`Document` フィールド追加**（依存: 1, 2）: `sections`/`subsets`/`next_section_id`/
   `next_subset_id`/`next_performer_id` を `Document` に追加。`#[serde(default)]` を付け、
   既存のJSON round-tripテスト（lib.rs:495-501）が壊れないことを確認。
4. **採番モジュール**（依存: 2）: `NumberingScheme`/`assign_sequential`/`duplicate_labels` を
   `roster.rs` に実装。単体テスト込み。
5. **`Subset` とヘルパー**（依存: 3）: `Subset` 型、`Document::subset`/`subset_named` を実装。
6. **`Selection` 型**（依存: 3）: `roster.rs` に `Selection` を実装。
   `by_section`/`by_subset`/`extend_same_section`/`rectangle`/`invert`/`to_indices` を含め、
   すべて単体テスト。**この時点では `drill-app` の `selected: BTreeSet<usize>` は変更しない**
   （main.rs 側の置き換えは doc 43 の `state/selection.rs` 分解タスクと合わせて行う。
   本タスクは `drill-core` 側の型提供のみ）。
7. **`PerformerRenderCache`**（依存: 2, 3）: `roster.rs` に実装し、ポインタ比較ベンチと
   単体テストを追加。
8. **`Edit` variant草案**（依存: 2, 3, 5。**doc 10 と協調が必要**）:
   `InsertPerformers`/`RemovePerformers`/`RelabelPerformers`/`AssignSection`/`AddSection`/
   `RemoveSection`/`RenameSection`/`InsertSubset`/`RemoveSubset`/`RenameSubset`/
   `SetSubsetMembers` の適用関数とラウンドトリップテストを実装。バリアント名・payload形は
   `docs/design/10-document-model.md` §3.3.2〜§3.3.3.2 の確定版（`PerformerExtract`、
   `before` を持たない decision A の形）に従う。doc 10 の `Edit` enum・`History` が先に
   実装されていることが前提。
9. **v1→v2移行のマッピング定義**（依存: 1, 2, 3。**doc 10/A-7 と協調が必要**）:
   本文書3.3節・4節末尾で定めた既定値（"Ensemble" セクション、`color: Some(旧色)`、
   `symbol: Circle`、`kind: Wind`、`height_m: 1.7`）を `migrate_v1_to_v2` に渡す
   仕様として明文化し、回帰テスト用のv1フィクスチャに演者データを含める。

並行可能性: 1→2→3 は直列。4・5・6・7 は 3 の後で互いに並行可能。8・9 は他文書との協調が要るため
最後に回す。

## 9. 未決事項

1. ~~**`SectionId`/`SubsetId` の型**~~ **解決済み**: `docs/design/10-document-model.md` §3.1.1 が
   `PerformerId`/`SectionId`/`SubsetId`/`SetId` をすべて `stable_id!` newtype（`NonZeroU32` 内包）
   に統一した。本文書は3.6節をこれに合わせて更新済み。
2. **`DrillError` へのセクション不整合バリアント追加**: 4節不変条件3は「パニックしない」ことは
   保証するが、「存在しないセクションを参照している」ことをどこで警告するか
   （`Document::validate` に非致命エラー種別を足すか、UI側の別経路にするか）は
   `DrillError` の設計（DESIGN_GAPS A-6、doc 42）と合わせて決める。
3. **サブセット・セクション数の上限**: 6節で申し送ったとおり、信頼できない入力に対する
   要素数上限（サブセット数、1サブセットあたりのメンバー数、セクション数）は
   `docs/design/51-security.md` の担当と考えるが、具体的な数値の決定はそちらに委ねる。
4. **`next_performer_id`/`next_section_id`/`next_subset_id` の管理主体**: 本文書は
   `Document` がこれらのカウンタを持つ前提で書いたが、ID採番を `Edit::apply` の内部に
   完全に一本化する（呼び出し側がIDを組み立てずに済むようにする）か、本文書のとおり
   「呼び出し側が事前にカウンタを読んで `Performer`/`Section`/`Subset` を組み立ててから
   `Edit` に渡す」形に留めるかは、doc 10 の `Edit` 全体設計と合わせて最終決定する。
5. **カラーガードの `symbol` プリセット**: マーチングバンド全般の慣習として、カラーガードは
   ドット記号ではなく別のマーカー（例: 旗のアイコン）で表示したいことが多い。`Symbol` enum に
   専用のバリアントを足すか、`Equipment` から描画側が推測するかは、DisplayList/描画側
   （doc 20/22）のシンボル→図形マッピングと合わせて決める方が自然なため、本文書では
   `Symbol` を6種の幾何形状に留め、カラーガード専用の描画規則は描画側の担当とした。
6. **`height_m` の分布既定値**: 全演者一律1.7mとしたが、実際は学年・パート（カラーガードは
   旗の高さを含めた実効シルエットが人物より高い等）で分布がある。3D描画側（doc 22）が
   より精緻な既定値テーブルを必要とするなら、本文書の `default_height_m` を拡張するか、
   セクションごとの既定身長をプリセットに含めるかを再検討する。
