# 52. 相互運用（インポート/エクスポート）とプラグイン API

## 1. 目的と範囲

DrillForge の売上は「既に他製品でドリルを作っている指導者が、既存の資産を捨てずに乗り換えられるか」で決まる。
本書はその**乗り換え経路**と、外部からの拡張手段を設計する。

この文書が設計するもの:

1. インポート経路の棚卸しと採否判断（何を取り込むと価値があり、何は取り込めないか）。
2. CSV / 表計算 / 汎用テキスト座標表の**列マッピングを伴う汎用インポータ**（`drill-interop` 新クレート）。
3. MusicXML / MIDI からの **TempoMap・拍子・リハーサル記号**取り込み。クレート選定を含む。
4. エクスポートの全体像と、外部ツールから読める**安定した書式契約**。
5. プラグインの**導入時期の判断**と、**後から入れられることを今の設計で保証する条件**。
6. 公開後に壊さないための安定性保証（何を semver で守り、何を守らないか）。
7. スクリプティング層の要否。
8. 乗り換え支援の具体的な導線。

**この文書が扱わないこと**:

- `DrillError` / `Locale` の定義（第42番）。本書は必要な variant の追加要求だけを出す。
- `Edit` コマンド代数の定義（第10番）。インポータは `Vec<Edit>` を返す契約だけを守る。
- `parse_coordinate` の文法そのもの（第16番7章）。本書は**呼び出し契約**と、実務上必要な API 分割の要求を出す。
- `Section` / `Performer` / `Subset` の型定義（第15番）。インポート時のセクション推定はこれらに整合させるだけ。
- `ShapeSpec` / `Shape` トレイトの定義（第14番）。本書は**外部提供シェイプの永続化に必要な追加要求**だけを出す（3.5.4）。
- プロジェクトコンテナ `.drillproj` とスキーマ移行（第41番 / `DESIGN_GAPS.md` A-7、B-5）。本書は「自形式の読み書きは第41番が正」とし、外部形式のみを扱う。
- SVG/PDF の版面設計（第17番）、動画エンコード（第31番 / `MEDIA_PIPELINE.md`）。本書はそれらを「書き出し経路の一覧」として参照するだけ。
- **プラグインのサンドボックス方式（第51番 3.7 が決定済み）。** WebAssembly コンポーネント（wasmtime）のみ・
  ネイティブ DLL は永久不採用・`Capability` 語彙・`PluginManifest` / `PluginLimits` / `PluginHost` /
  `Plugin` の型・epoch 割り込み・`StoreLimits`・WASI 不供給・署名は**すべて第51番が正**であり、
  本書はこれを再決定しない。本書が決めるのは**その上に載る API の形**（拡張点・WIT ワールド・
  プラグイン向け編集語彙・バージョニング）である。第51番と本書が食い違う箇所は 3.5.7 に裁定を明記した。

### 1.0 所有者決定（2026-08-09）と本章の位置づけ

`docs/design/90-integration-roadmap.md` §5 に記録されたプロジェクト所有者の決定のうち、本書に効くもの:

| # | 決定 | 本書での扱い |
|---|---|---|
| 1 | **全面オープンソース**（open-core 不採用） | 3.5.2 で「fork という拡張経路が最初から存在する」ことを前提に、プラグイン API の必要範囲を引き直した |
| 4 | **将来プラグインを入れられるようにする** | 3.5 を「保留」から「**後から入れられることを今の設計で保証する**」へ格上げした。3.5.4 が「今 decide しないと手遅れになるもの」、3.5.5 が「閉ざさない条件」、3.5.6 が着手判定条件 |

初版の「P0/P1 では入れない」という**判断そのものは維持**する（根拠は 3.5.1）。
変えたのはその先の扱いであり、`drill-sandbox` / `drill-plugin` は**破棄しない**。

### 1.1 法的・倫理的境界（本書全体の前提、覆さない）

`ARCHITECTURE.md`「境界」節および `docs/design/00-conventions.md`「模倣しないもの」に従い、
本書の相互運用は次の3つだけを対象とする。

| 対象 | 例 | 採否 |
|---|---|---|
| (a) 公開された標準形式 | MusicXML 4.0（W3C Music Notation CG、Community Final Specification Agreement で自由に実装可）、Standard MIDI File、CSV (RFC 4180)、ZIP、PNG/JPEG | 採用 |
| (b) 他社製品が公式に書き出せる汎用形式 | 他社ツールの「CSV 書き出し」「座標表テキスト書き出し」機能の出力 | 採用 |
| (c) 利用者自身が所有するデータ | 利用者が自分で作った Excel の座標表、自校の名簿、自分で買った音源 | 採用 |

**明示的に行わないこと**（技術的可否とは無関係の設計判断）:

- 他社の独自バイナリ形式のリバースエンジニアリング、およびその内部仕様の複製。
- 他社の商標・製品名を DrillForge の機能名・ファイル形式名・UI ラベルに使うこと。
- 他社の同梱アセット（フィールド画像、スタジアムモデル、フォント、シェイプライブラリ）の取り込み・再配布。
- 他社製品の UI 意匠の模倣（インポートウィザードの画面設計を含む）。

UI 上でも正直に書く。「独自形式は読めません」ではなく
**「読みません（意図的な設計判断です）。代わりに元製品の CSV 書き出しを使ってください」**と提示し、
一般的な操作手順の説明のみを案内する（他社の画面キャプチャ・商標は転載しない）。3.7.5 節参照。

## 2. 現状

### 2.1 リポジトリに有るもの

エクスポート側は既に一定量が動いており、これが本書の出発点になる。

- `crates/drill-core/src/coordinates.rs:159-181` `coordinates_csv(doc) -> String`
  ヘッダ `performer,label,set,counts,x,y,side_to_side,front_to_back`、`(performer, set)` の直積で1行。
  `csv_escape`（33-40行）は RFC 4180 準拠のクォートを行うが、**改行は `\n` 固定**（Excel 互換の CRLF ではない）、
  **BOM を付けない**（Excel で開くと日本語列が文字化けする）。
- `crates/drill-core/src/coordinates.rs:136-155` `performer_sheet(doc, index) -> String`（演者別ドットブックのプレーンテキスト）。
- `crates/drill-core/src/svg.rs:66/218/247/286` `field_svg` / `set_svg` / `coordinate_sheet_html` / `drill_book_html`。
  外部クレート依存ゼロの文字列組み立て。
- `crates/drill-core/src/countsheet.rs:90` `count_sheet_text`、`crates/drill-core/src/continuity.rs:174` `continuity_text`。
- `crates/drill-core/src/video.rs` FFmpeg 引数生成と設定検証まで（実フレーム出力は無し。`DESIGN_GAPS.md` B-4）。
- `crates/drill-core/src/audio.rs:104` `click_track(tempo, total_counts) -> Vec<f32>`（クリック時刻の秒配列。MIDI 化はされていない）。
- `crates/drill-core/src/tempo.rs` `TempoMap`（可変 BPM、`seconds_at` / `count_at` / `measure_beat`）。
  **音楽ファイルから TempoMap を作る経路は一切無い**。
- `crates/drill-app/src/main.rs:249-260` `export_text(default_name, filter_name, ext, contents)`
  — 全ての書き出しがこの1関数を通り、`std::fs::write` を **UIスレッドで同期実行**している。
- `crates/drill-app/src/main.rs:1285-1340` 書き出しメニュー（座標CSV / ドットブックTXT / セットSVG / 座標シートHTML /
  ドリルブックHTML / カウントシートTXT / コンティニュイティTXT）。

### 2.2 リポジトリに無いもの

- **インポータが1つも無い。** `crates/drill-app/src/main.rs:430-457` `open_dialog` が
  `Document::from_json` を呼ぶだけで、これが唯一の入力経路である
  （`crates/drill-core/src/lib.rs:399-403`、`schema_version != 1` は即エラー、移行経路なし）。
- 列マッピング、文字コード判定、区切り文字判定、行スキップ報告に相当する型・関数は存在しない。
- MusicXML / MIDI / 画像を扱うコードは存在しない。
- プラグイン、スクリプト、CLI、外部プロセス連携（FFmpeg 呼び出しの引数生成を除く）は存在しない。
- 依存クレートは workspace 全体で `serde` / `serde_json` / `eframe` / `rfd` の4つのみ
  （`Cargo.toml`、`crates/drill-core/Cargo.toml`、`crates/drill-app/Cargo.toml`）。
  **`drill-core` の依存は serde/serde_json のみという規約**（00-conventions.md「クレート境界」）があるため、
  パーサクレートを `drill-core` に足すことはできない。
- 第16番文書が定義した `parse_coordinate`（16-coordinates-notation.md:510-512, 540-541 タスク7）は**未実装**。
  本書のテキスト座標インポータはこれに依存する。
- `Edit` / `DrillError` / `Locale` / `Section` / `Subset` も未実装（`DESIGN_GAPS.md` A-1/A-5/A-6、第10/15/42番）。

### 2.3 本書が依存する未完了の他文書

| 依存先 | 必要なもの | 無い場合の代替 |
|---|---|---|
| 第10番 / A-1 | `Edit` 列挙とその `apply` | インポータは暫定的に `Document` を直接構築する経路（新規作成のみ）だけ実装し、既存文書へのマージは待つ |
| 第15番 | `Section` / `SectionId` / `Performer.section` | セクション列は無視して報告に載せる |
| 第16番 | `parse_coordinate` / `CoordinateNotation` / `Locale` | テキスト座標列を無効化し、数値 x/y 列のみ受け付ける |
| 第41番 / A-7 | `.drillproj`、schema migration | 自形式インポートは `Document::from_json` のまま |
| 第42番 | `DrillError` / `Locale` | 本書の型は `DrillError` を前提に書き、暫定期間だけ `String` を許す（新規 `Result<_, String>` は作らない） |
| 第51番 | `drill_sandbox::plugin::*`（機構）・`Document::validate_untrusted(&Limits::DERIVED)` | プラグインを実装しない。ただし 3.5.4 の「今やる」項目は第51番の完成を待たずに着手できる |
| 第14番 | `ShapeSpec` / `Shape` トレイト | 外部提供シェイプの永続化（3.5.4 D1）が入らない。**これは後から足すと破壊的変更になるので待てない** |

### 2.4 プラグインに関して他文書が既に決めていること（本書は再決定しない）

初版執筆時には未読だった文書が完成しているため、事実として記録する。

- **第51番 3.7**（`docs/design/51-security.md:1064-1180`）が**サンドボックス方式を決定済み**:
  WebAssembly コンポーネント（wasmtime）のみ／ネイティブ DLL は永久不採用（DLL 探索順ハイジャックと
  ABI 固定が理由）／`Capability` 6 種／`PluginManifest`（Ed25519 署名）／`PluginLimits`
  （メモリ 256 MiB、対話 250 ms・ジョブ 30 s の epoch 締め切り、出力 64 MiB）／
  `Config::epoch_interruption(true)` + 1 ms 周期の ticker ／`wasm_threads(false)` ／
  `wasm_nan_canonicalization(true)` ／**WASI 不供給** ／返り値の `Vec<Edit>` は
  複製に適用 → `validate_untrusted(&Limits::DERIVED)` → 不合格なら**全件破棄** ／
  適用は `Edit::Batch` 1 件としてプラグイン ID 付きで履歴へ。
  型の置き場所は `crates/drill-sandbox/src/plugin.rs`（`feature = "plugins"`）。
- **第10番**が `Edit` を **40 バリアント**に確定（`docs/design/10-document-model.md:779-841`）。
  `apply(self, doc) -> Result<Edit, DrillError>` が**逆操作を返す**（前状態を payload に持たない）。
  `Edit` は `#[derive(Clone, Debug, PartialEq)]` のみで**`Serialize` を持たない**。
  `validate()` は V1〜V25（同 1959-1988 行）。V25（セクション参照の宙吊り）だけは `Err` にしない。
- **第14番**が `ShapeSpec` を **17 バリアントの閉じた列挙**として確定し、`#[serde(tag = "kind")]` で
  `Set.shape` に永続化する（`docs/design/14-formations-shapes.md:294-317`）。
  `Shape` トレイト（`sample` / `arc_length` / `validate`、同 320-336 行）が既に**拡張の継ぎ目の形をしている**。
- **第90番 §1** がクレートの実在時期を確定し、`drill-sandbox` / `drill-plugin` を
  「P0/P1 では実装しないが**破棄しない**」と裁定した。
- **第90番 §5** が全面オープンソースを記録した。

## 3. 設計

### 3.0 クレート構成

00-conventions.md「クレート境界」の表に**3クレートを追加**する（`drill-sandbox` は第51番 3.0 が既に追加済み）。
依存は必ず上から下へ。確定版のクレート構成と実在時期は `docs/design/90-integration-roadmap.md` §1 が正。

```
drill-core      ドキュメントモデル・時間・座標・解析・保存検証。依存は serde/serde_json のみ。（変更なし）
drill-sandbox   信頼できない入力の外殻（第51番 3.0 が定義・所有）。zip / パス / 画像・音声の入口ゲート /
                外部プロセス / 原子的書き込み / (feature = "plugins") wasmtime ホスト。
drill-render    DisplayList 中間表現。drill-core にのみ依存。（変更なし）
drill-audio     デコード・再生・波形。（変更なし）
drill-export    SVG/PNG/PDF/動画。drill-render + drill-sandbox に依存。（変更なし）
drill-interop   ★新規。外部形式の読み書き。drill-core + drill-sandbox に依存。
                第三者パーサ（csv / encoding_rs / roxmltree / midly）はここにだけ入る。
                zip は drill-sandbox のコンテナゲート経由で使う（.mxl も同じ検査を通す。6.2）。
                UI・GPU・OS ダイアログ・ファイルダイアログを知らない（&[u8] と Write を受け取る）。
drill-plugin    ★新規・P2（3.5.6 の着手条件を満たしてから）。**破棄しない**。
                プラグイン API の表面: WIT ワールド / 能力→ホスト関数の結線 /
                `PluginEdit` → `Edit` の写像 / 提供者レジストリの WASM 実装。
                依存: drill-core, drill-sandbox（feature = "plugins"）, drill-interop。
                **wasmtime を直接持つのは drill-sandbox だけ**で、drill-plugin は
                `drill_sandbox::plugin::{PluginHost, Plugin, Capability}` を使う（第51番 3.7 の型）。
drill-cli       ★新規。ヘッドレス実行。drill-core/interop/export に依存。drill-app には依存しない。
drill-app       egui/wgpu の表示と入力変換のみ。上記全てに依存。
```

**`drill-sandbox` と `drill-plugin` の責務分界**（第51番との重複を避けるための裁定）:

| | `drill-sandbox`（第51番が所有） | `drill-plugin`（本書が所有） |
|---|---|---|
| 何を決めるか | **機構**: 実行方式・隔離・上限・署名・同意 | **表面**: 何を拡張できるか・関数の形・語彙・版 |
| 型 | `Capability` / `PluginManifest` / `PluginLimits` / `PluginHost` / `Plugin` | WIT ワールド / `PluginEdit` / `ProviderId` / 各 `*Provider` の WASM 実装 |
| 変わる頻度 | 低（wasmtime LTS に追随するときだけ） | 中（拡張点を足すたび） |

`drill-interop` が `drill-render` / `drill-export` に依存しないことを明示する。
インポータは描画を知らず、エクスポータのうち**描画を伴うもの（SVG/PNG/PDF/動画）は `drill-export` の責務**、
**描画を伴わないもの（CSV/テキスト/MIDI/JSON）は `drill-interop` の責務**という分担にする。
既存の `drill-core::coordinates::coordinates_csv` と `drill-core::svg` は、依存ゼロで書ける限り
`drill-core` に残す（移動によるリグレッションを避ける）。`drill-interop` は
「第三者クレートかバイト列レベルの符号化が要るもの」だけを引き受ける。

Cargo フィーチャで各インポータを切り離せるようにする（攻撃面を削れるビルドを常に用意する。6章）。

```toml
# crates/drill-interop/Cargo.toml
[dependencies]
drill-core   = { path = "../drill-core" }
csv          = { version = "1.4",  optional = true }
encoding_rs  = { version = "0.8",  optional = true }
chardetng    = { version = "1.0",  optional = true }
roxmltree    = { version = "0.21", optional = true }
zip          = { version = "8",    optional = true, default-features = false, features = ["deflate"] }
midly        = { version = "0.5",  optional = true, default-features = false, features = ["std", "alloc"] }

[features]
default  = ["csv-io", "musicxml", "midi"]
csv-io   = ["dep:csv", "dep:encoding_rs", "dep:chardetng"]
musicxml = ["dep:roxmltree", "dep:zip", "dep:encoding_rs"]
midi     = ["dep:midly"]
xlsx     = ["dep:calamine"]   # P1。9章の未決事項
```

`midly` は `default-features = false` にして `parallel`（rayon）を落とす。
`midly` の通常依存は rayon **1つだけ**であり、これを外すと依存ゼロのパーサになる。

### 3.1 インポート経路の棚卸し

「実現可能性」＝ 公開仕様/汎用形式だけで実装できるか。「価値」＝ 乗り換え1件あたりに節約できる手作業時間。

| # | 経路 | 実現可能性 | 価値 | 判断 |
|---|---|---|---|---|
| 1 | **CSV 座標表**（他社の CSV 書き出し、自作 Excel の CSV 保存） | 高。RFC 4180 + 文字コード判定だけ | **最大**。1,000人×64セット＝64,000ドットの手入力を消す | **P0 採用** |
| 2 | **DrillForge JSON / .drillproj** | 高。自形式 | 高（版間移行） | **P0 採用**（実装は第41番） |
| 3 | **汎用テキスト座標シート**（1行1セットのドットブック） | 中。`parse_coordinate` に全面依存、表記ゆれが本体 | 高。CSV を吐けない製品からの唯一の道 | **P1 採用** |
| 4 | **Excel .xlsx 座標表** | 中。`calamine` で読める。結合セル・複数シート・書式が実務の障害 | 高。日本の吹奏楽部の座標表は Excel が実態 | **P1 採用**（9章で最終判断） |
| 5 | **MIDI (SMF)** | 高。公開仕様、`midly` で完結 | 中〜高。TempoMap 手入力の全廃 | **P1 採用** |
| 6 | **MusicXML (.musicxml / .mxl)** | 高。W3C CG 公開仕様、XSD 公開 | 中〜高。譜面ソフトからテンポ・小節・リハーサル記号 | **P1 採用** |
| 7 | **画像（フォーメーションの下敷き）** | 高。PNG/JPEG デコードのみ | 中。手描き構想図をなぞる用途に効く | **P1 採用（表示のみ）**。自動ベクトル化はしない |
| 8 | **PDF のドリルチャート** | 低。ベクタ抽出は製品ごとに構造が違い、ラスタは OCR 頼み。誤読が座標という**正確性が命の領域**に入る | 低（誤りの検算コストが手入力を上回る） | **不採用** |
| 9 | **音源ファイル（WAV/MP3/AAC）** | 高 | 高 | 採用（第30番 / `drill-audio` の担当。本書の対象外） |
| 10 | **他社独自バイナリ形式** | — | — | **不採用（1.1節の設計判断）** |
| 11 | **MusicXML からの演者・パート推定** | 高（技術的には） | **負**。楽譜のパート（Trumpet 1）とフィールド上の演者は1対1ではない。推定は必ず外れ、利用者は全件検証を強いられる | **不採用**。取り込むのは時間軸だけ |

8番・11番を「できるがやらない」と明記するのは重要である。相互運用は**正しさを保証できる範囲**でだけ自動化し、
それ以外は手作業に戻す方が、結果として乗り換えは速い。

### 3.2 表形式インポータ（CSV / TSV / xlsx）

#### 3.2.1 全体の流れ — 3フェーズに分ける

```
bytes (&[u8])
   │  ① sniff  — 先頭 64 KiB のみ。文字コード・区切り・ヘッダ有無・先頭50行を推定
   ▼
TabularPreview ──► guess_mapping() ──► ColumnMapping （既定案）
   │                                        │
   │                       ② 利用者が列マッピングUIで修正・保存・再利用
   ▼                                        ▼
② plan_import(preview, mapping, grid) -> ImportPlan   （全行を読まずに矛盾を検出）
   │
   ▼
③ run_import(bytes, plan, progress) -> ImportOutcome { edits | document, report }
```

①だけは UI スレッドで同期実行してよい（64 KiB 上限、目標 2ms 未満。5章）。
②③は `DESIGN_GAPS.md` B-3 の `Job` に載せる。`drill-interop` 自身はスレッドを作らず、
進捗コールバックを受け取るだけにする（クレート境界: OS を知らない）。

#### 3.2.2 上限（入力を読む前に確定する）

```rust
/// Hard caps applied to every tabular import. Every field is checked *before*
/// the corresponding resource is consumed, never after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportLimits {
    pub max_bytes: u64,
    pub max_rows: u64,
    pub max_columns: usize,
    pub max_field_bytes: usize,
    pub max_performers: usize,
    pub max_sets: usize,
    /// Detailed per-row diagnostics retained in the report; beyond this only a
    /// counter is kept, so a 1M-bad-row file cannot exhaust memory via the report.
    pub max_reported_rows: usize,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_bytes: 256 * 1024 * 1024,
            max_rows: 2_000_000,
            max_columns: 512,
            max_field_bytes: 4096,
            // 00-conventions.md「上限規模」: 演者4,000人 / セット256。
            max_performers: 4_000,
            max_sets: 256,
            max_reported_rows: 500,
        }
    }
}
```

`max_rows` の 2,000,000 は「4,000人 × 256セット = 1,024,000」に倍の余裕を持たせた値。
`max_performers` / `max_sets` は 00-conventions.md の上限規模と一致させる。
これを超える入力は**部分的に読まず、即座に `Err`** にする（半端に読んで壊れた文書を作らない）。

#### 3.2.3 フェーズ① — sniff

```rust
/// How the source bytes were decoded to text. Always shown in the UI so the
/// user can catch a misdetection (CP932 vs UTF-8 mojibake in labels) before
/// importing 64,000 rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// Shift_JIS superset written by Japanese Excel — the dominant real case.
    Cp932,
    Windows1252,
    /// encoding_rs label for anything chardetng returned that is not above.
    Other(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding { Lf, CrLf, Cr }

#[derive(Clone, Debug)]
pub struct TabularPreview {
    pub encoding: SourceEncoding,
    /// `false` when the encoding was chosen by chardetng rather than by a BOM
    /// or an explicit user override. The UI must surface this as "推定".
    pub encoding_is_certain: bool,
    pub line_ending: LineEnding,
    pub delimiter: u8,
    pub has_header: bool,
    pub headers: Vec<String>,
    /// At most 50 rows, each at most `limits.max_columns` fields.
    pub sample_rows: Vec<Vec<String>>,
    pub column_count: usize,
    /// Extrapolated from the sniffed window; only for progress display.
    pub estimated_rows: u64,
    pub source_bytes: u64,
}

/// Read at most 64 KiB of `bytes` and infer how to read the rest. Never fails
/// on garbage input: an undecodable or structureless file yields a preview with
/// `column_count == 0`, which `plan_import` then rejects with a precise error.
pub fn sniff(bytes: &[u8], limits: &ImportLimits) -> Result<TabularPreview, DrillError>;

/// Re-run the sniff with a user-chosen encoding and/or delimiter.
pub fn resniff(
    bytes: &[u8],
    limits: &ImportLimits,
    encoding: Option<SourceEncoding>,
    delimiter: Option<u8>,
    has_header: Option<bool>,
) -> Result<TabularPreview, DrillError>;
```

判定アルゴリズム（決定論的。同じバイト列からは常に同じ `TabularPreview`）:

1. **BOM**: `EF BB BF` → `Utf8Bom`、`FF FE` → `Utf16Le`、`FE FF` → `Utf16Be`。確定 (`encoding_is_certain = true`)。
2. BOM 無し: 先頭 64 KiB が妥当な UTF-8 なら `Utf8`（`std::str::from_utf8` が成功、かつ
   U+FFFD を含まない）。確定扱いにする。
3. それ以外: `chardetng::EncodingDetector` に 64 KiB を食わせ、返った `&'static Encoding` を
   `Cp932` / `Windows1252` / `Other(name)` に写像。`encoding_is_certain = false`。
4. **区切り文字**: 候補 `b','` / `b'\t'` / `b';'` それぞれで先頭 20 行をパースし、
   「フィールド数が最頻値と一致する行の割合」が最大のものを選ぶ。同率なら `,` → `\t` → `;` の順。
5. **ヘッダ有無**: 先頭行の全フィールドが非数値で、かつ2行目以降に1つでも数値フィールドがある列が存在すれば
   `has_header = true`。
6. **改行**: 最初に現れた `\r\n` / `\n` / `\r`。

デコードは `encoding_rs::Decoder` をストリーミングで使い、再利用する `String` バッファへ書く。
不正バイト列は U+FFFD に置換して**継続する**（1バイトの破損で64,000行を捨てない）。
ただし U+FFFD を含むフィールドは `ImportWarning::ReplacementCharacter` として報告する。

#### 3.2.4 列の役割（`ColumnRole`）と座標系の解釈

```rust
/// The unit a numeric coordinate column is expressed in. `Steps` uses the
/// document grid's per-axis step length (`horizontal_units / horizontal_steps`,
/// `vertical_units / vertical_steps`) — the same arithmetic
/// `drill_core::coordinates` uses, never re-derived here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LengthUnit { Yards, Meters, Feet, Steps }

/// Where (0,0) sits in the source file's numeric coordinates, and which way
/// depth grows. Products differ on all three axes of this choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// DrillForge native: x=0 at the Side-1 endzone, y=0 at the front sideline.
    FrontSide1Corner,
    /// x=0 at the 50 yard line (negative toward Side 1), y=0 at the front sideline.
    FiftyFrontSideline,
    /// x=0 at the 50, y=0 at the front hash.
    FiftyFrontHash,
    /// x=0 at the 50, y=0 at the field's vertical center.
    FieldCenter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepthDirection {
    /// Larger value = farther from the audience. DrillForge native.
    TowardBack,
    /// Larger value = closer to the audience.
    TowardAudience,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisConvention {
    pub origin: Origin,
    pub depth: DepthDirection,
    pub x_unit: LengthUnit,
    pub y_unit: LengthUnit,
}

impl AxisConvention {
    pub const fn native() -> Self {
        Self {
            origin: Origin::FrontSide1Corner,
            depth: DepthDirection::TowardBack,
            x_unit: LengthUnit::Yards,
            y_unit: LengthUnit::Yards,
        }
    }

    /// Convert one source pair into document space. Pure; returns `None` for a
    /// non-finite result so the caller records a `SkipReason::NonFinite` rather
    /// than writing NaN into a `Point` (00-conventions.md「NaN/Inf 拒否」).
    pub fn to_point(self, x: f64, y: f64, grid: &GridConfig) -> Option<Point>;
}
```

```rust
/// What one source column means. Index-aligned with the source columns in
/// `ColumnMapping::roles`.
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnRole {
    Ignore,
    /// Drill number shown to performers ("T1"). Also the join key when no
    /// `PerformerKey` column exists.
    PerformerLabel,
    /// A stable per-performer key from the source file. Used only for joining
    /// rows; never becomes `PerformerId` (IDs are minted by the Document).
    PerformerKey,
    /// Matched against `Document::sections` by name, then by `short`,
    /// case-insensitively; unmatched names create new `Section`s (第15番).
    SectionName,
    SetName,
    /// 1-based ordinal of the set within the show.
    SetOrdinal,
    Counts,
    /// Numeric coordinate, interpreted through `ColumnMapping::axes`.
    X,
    Y,
    /// Free text through `coordinates::parse_lateral` (第16番、3.2.7).
    LateralText,
    /// Free text through `coordinates::parse_depth`.
    DepthText,
    /// One column holding the whole readout ("サイド1 45ヤードラインの内側に2歩、フロントハッシュの2歩後ろ").
    CoordinateText,
    /// Free-form note attached to the set (第17番のセット注釈へ渡す).
    SetNote,
}
```

`X`/`Y` の単位を `ColumnRole` ではなく `AxisConvention` に置いたのは、x と y で単位が食い違う入力を
実務で見ないこと、そして「原点・向き・単位」は必ずセットで理解すべき1つの規約だからである。

#### 3.2.5 表の形（long / wide）

```rust
#[derive(Clone, Debug, PartialEq)]
pub enum TableLayout {
    /// One row per (performer, set). DrillForge's own `coordinates_csv` shape.
    Long,
    /// One row per performer; each set occupies its own group of columns.
    /// `set_groups[i]` lists the column indices belonging to set `i`; the
    /// per-column roles still come from `ColumnMapping::roles`.
    Wide { set_groups: Vec<SetGroup> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetGroup {
    pub name: String,
    pub counts: Option<u16>,
    /// Column indices in this group. Their roles must be a subset of
    /// {X, Y, LateralText, DepthText, CoordinateText, SetNote}.
    pub columns: Vec<usize>,
}
```

`Long` を P0、`Wide` を P1 とする。理由: 自形式の往復（不変条件5）と他社 CSV の大半は Long であり、
Wide は Excel 手作りの座標表に多いが、列グループの推定 UI が別途要るため。

#### 3.2.6 `ColumnMapping` と自動推定

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnMapping {
    pub layout: TableLayout,
    /// Index-aligned with source columns. Length must equal
    /// `TabularPreview::column_count`.
    pub roles: Vec<ColumnRole>,
    pub axes: AxisConvention,
    /// Used for `LateralText` / `DepthText` / `CoordinateText` columns.
    pub notation: CoordinateNotation,
    /// Primary locale for text-coordinate parsing. The other locale is tried as
    /// a fallback and produces `ImportWarning::LocaleFallback` (3.2.7).
    pub locale: Locale,
    /// Applied when no `Counts` column exists.
    pub default_counts: u16,
    pub ragged: RaggedPolicy,
    pub on_duplicate: DuplicatePolicy,
}

/// What to do when a performer has no row for some set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RaggedPolicy {
    /// Refuse the whole import. Safest; the default.
    Reject,
    /// Copy the performer's previous set position (a hold). Reported per gap.
    HoldPrevious,
}

/// What to do when two rows carry the same (performer, set).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicatePolicy { Reject, LastWins, FirstWins }
```

`ColumnMapping` は `Serialize + Deserialize` にし、**TOML/JSON として保存・共有できる**ようにする
（7章のスクリプティング判断とつながる: コードではなくデータで拡張する）。
拡張子は `.drillmap.toml`。学校や団体が自分たちのフォーマットに対する写像を1度作れば以後は再利用できる。

```rust
/// Header-name and value-shape heuristics producing a starting point for the
/// mapping UI. Never used without user confirmation — a wrong guess that
/// silently imports 64,000 wrong dots is worse than no guess at all.
pub fn guess_mapping(
    preview: &TabularPreview,
    grid: &GridConfig,
    locale: Locale,
) -> ColumnMapping;
```

推定規則（決定論。辞書は `&'static [(&'static str, ColumnRole)]` の固定配列、`BTreeMap` で検索）:

1. ヘッダ名の正規化: NFKC 相当の全角→半角、小文字化、`_`/`-`/空白の除去。
2. 辞書一致（多言語）:
   `performer|player|member|演者|番号|drill number|dot` → `PerformerLabel`
   `set|セット|page|chart` → `SetName`（値が全て整数なら `SetOrdinal`）
   `count|counts|カウント|beats` → `Counts`
   `x|side to side|sidetoside|lateral|左右` → `X` または `LateralText`
   `y|front to back|fronttoback|depth|前後` → `Y` または `DepthText`
   `section|instrument|パート|セクション|楽器` → `SectionName`
3. `X`/`LateralText` の判別は**値の形**で決める: サンプル50行のうち90%以上が
   `f64` としてパースできれば `X`、そうでなければ `LateralText`。
4. ヘッダ名が DrillForge 自身の出力（`performer,label,set,counts,x,y,side_to_side,front_to_back` …）に
   完全一致した場合は、辞書を飛ばして**自形式プロファイル**を即採用する（往復の確実性を最優先）。
5. `axes` は既定で `AxisConvention::native()`。ただしサンプルの x 値に負値があれば `Origin::FiftyFrontSideline` を提案する。

#### 3.2.7 テキスト座標のパース — 第16番との接続

第16番文書（16-coordinates-notation.md:510-512, 540-541）は逆変換パーサ
`parse_coordinate(text, grid, notation, locale) -> Result<Point, DrillError>` を定義し、
往復テスト（`readable` → `parse_coordinate` → 元の `point`）を要求している。本書はこれを**そのまま使う**。
座標の算術を `drill-interop` 側に再実装しない（第16番 不変条件1「単一計算経路」を跨クレートで維持する）。

そのうえで、本書から第16番へ**2点の追加要求**を出す。理由は実際の入力の形にある。

**要求1: 左右・前後を独立にパースできる API を分割公開すること。**
根拠: DrillForge 自身の `coordinates_csv`（coordinates.rs:160）が
`side_to_side` と `front_to_back` を**別々の列**に出しており、他社 CSV も同様に2列に割るものが多い。
連結してから `parse_coordinate` に渡すには区切り文字（`、` / `, `）を `drill-interop` 側で組み立てる必要があり、
それはロケール依存の文字列組み立てを core の外に漏らすことになる（第16番 不変条件3 に反する）。

```rust
// 第16番 coordinates.rs への追加要求（本書は要求のみを出し、定義はしない）
pub struct LateralReading { pub side: Side, pub yard_line: u16, pub lateral_steps: RoundedStep }
pub struct DepthReading   { pub reference: HashKind, pub depth_steps: RoundedStep }

pub fn parse_lateral(text: &str, grid: &GridConfig, n: CoordinateNotation, l: Locale)
    -> Result<LateralReading, DrillError>;
pub fn parse_depth(text: &str, grid: &GridConfig, n: CoordinateNotation, l: Locale)
    -> Result<DepthReading, DrillError>;
/// Existing contract; composes the two above and converts to a Point.
pub fn parse_coordinate(text: &str, grid: &GridConfig, n: CoordinateNotation, l: Locale)
    -> Result<Point, DrillError>;
/// Needed by the importer to build a Point from two separately-parsed halves.
pub fn reading_to_point(lat: &LateralReading, depth: &DepthReading, grid: &GridConfig) -> Point;
```

**要求2: `DrillError::CoordinateParse` は「どこで失敗したか」を持つこと。**
64,000行のうち37行が落ちたとき、「パース失敗」だけでは利用者は直せない。

```rust
// 第42番 DrillError への追加要求
CoordinateParse { byte_offset: usize, kind: CoordinateParseKind }

pub enum CoordinateParseKind {
    UnknownSideWord, UnknownReferenceLine, MissingNumber,
    NumberOutOfRange, TrailingGarbage, Empty,
}
```

インポータ側のロケール取り扱い:

```rust
/// Try `mapping.locale` first, then the other locale. A successful fallback is
/// not an error (mixed-language sheets are common when a Japanese school
/// imports a US template) but is always reported.
fn parse_lateral_bilingual(
    text: &str, grid: &GridConfig, n: CoordinateNotation, primary: Locale,
) -> Result<(LateralReading, Option<ImportWarning>), DrillError>;
```

#### 3.2.8 実行と結果

```rust
#[derive(Clone, Debug)]
pub struct ImportPlan {
    pub mapping: ColumnMapping,
    pub limits: ImportLimits,
    pub grid: GridConfig,
    /// Columns the mapping never consumes. Surfaced up-front so the user is
    /// told what will be dropped *before* the import, not after.
    pub ignored_columns: Vec<String>,
}

/// Validate a mapping against a preview without reading the whole file.
/// Catches: missing required roles, duplicate exclusive roles (two `Counts`
/// columns), a `Wide` layout whose groups reference out-of-range columns,
/// text-coordinate roles when the `parse_*` API is unavailable.
pub fn plan_import(
    preview: &TabularPreview,
    mapping: ColumnMapping,
    grid: &GridConfig,
    limits: ImportLimits,
) -> Result<ImportPlan, DrillError>;

/// Progress in ten-thousandths, matching `DESIGN_GAPS.md` B-3's `Job`.
pub type Progress<'a> = &'a mut dyn FnMut(u32) -> ControlFlow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlFlow { Continue, Cancel }

/// Build a brand-new document from the source. Streaming: one reusable
/// `csv::ByteRecord`, one reusable decoded `String`, one `BTreeMap` accumulator.
pub fn import_tabular_as_document(
    bytes: &[u8],
    plan: &ImportPlan,
    progress: Progress<'_>,
) -> Result<ImportOutcome<Document>, DrillError>;

/// Merge into an existing document. Returns edits, never a mutated document —
/// 00-conventions.md 不変条件1「`Document` の変更は必ず `Edit` コマンドを通る」.
pub fn import_tabular_into(
    bytes: &[u8],
    plan: &ImportPlan,
    target: &Document,
    progress: Progress<'_>,
) -> Result<ImportOutcome<Vec<Edit>>, DrillError>;

#[derive(Clone, Debug)]
pub struct ImportOutcome<T> {
    pub value: T,
    pub report: ImportReport,
}
```

```rust
#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    pub rows_read: u64,
    pub rows_accepted: u64,
    pub rows_skipped: u64,
    /// At most `limits.max_reported_rows` entries; `rows_skipped` is the true total.
    pub skipped_detail: Vec<SkippedRow>,
    pub performers_created: usize,
    pub performers_matched: usize,
    pub sets_created: usize,
    pub sections_created: usize,
    pub warnings: Vec<ImportWarning>,
    /// Source columns that carried data but had no role. Named, not counted.
    pub ignored_columns: Vec<String>,
    /// Concepts this importer structurally cannot carry, listed so the user
    /// knows to redo them by hand. Static per source kind — see 8.2.
    pub not_imported: &'static [NotImported],
}

#[derive(Clone, Debug)]
pub struct SkippedRow {
    pub line: u64,
    pub column: Option<usize>,
    pub reason: SkipReason,
    /// Truncated to 120 chars for display; never the whole row.
    pub excerpt: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SkipReason {
    EmptyRow,
    FieldCountMismatch { expected: usize, found: usize },
    MissingRequired(ColumnRole),
    NumberParse,
    NonFinite,
    OutOfField { x: f32, y: f32 },
    CoordinateParse(CoordinateParseKind),
    DuplicateKey { performer: String, set: String },
    FieldTooLong { bytes: usize },
    PerformerLimit,
    SetLimit,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImportWarning {
    EncodingGuessed(SourceEncoding),
    ReplacementCharacter { line: u64, column: usize },
    LocaleFallback { line: u64, used: Locale },
    HeldPreviousSet { performer: String, set: String },
    SectionCreated(String),
    CountsDefaulted { set: String, value: u16 },
    /// The point was inside the field but more than 8 steps outside a hash /
    /// sideline reference — likely an axis-convention mistake, not a typo.
    SuspiciousOrigin { line: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotImported {
    Routes, Gates, Holds, MarchingStyle, Cameras, AudioSync,
    PrintLayout, PerformerSymbols, Props, Annotations,
}
```

`not_imported` を**静的な列挙**として持つのが要点である。「取り込めなかったもの」を
実行時の検出に任せると、元ファイルにそもそも無かった概念は報告されない。
CSV には経路（ルート/ゲート/ホールド）もカメラも構造的に載らないのだから、
毎回無条件に「これらは入っていません。必要なら手で付けてください」と言う方が正直で、実務的にも役に立つ。

`SuspiciousOrigin` は座標系の取り違えを捕まえるための警告である。
`AxisConvention` を間違えると全 64,000 点が一様にずれ、しかも「フィールド内」には収まってしまうので
`OutOfField` では検出できない。「ハッシュ/サイドラインから8歩以上離れた点」の比率が
異常に高い（例: 全体の70%超）ときにまとめて1件警告する。

#### 3.2.9 セクション推定（第15番との整合）

`SectionName` 列の値 → `Section` の対応付けは次の順で行い、**新規作成した場合は必ず報告する**。

1. `Document::sections` の `name` と完全一致（大文字小文字・全角半角を正規化して比較）。
2. `short` と完全一致。
3. `roster::presets`（第15番 3.2節: `WINDS` / `BATTERY` / `FRONT_ENSEMBLE` / `COLOR_GUARD`）の
   `name` / `short` と一致 → そのプリセット定義で新規 `Section` を作る。
4. どれにも一致しない → `SectionSpec { name: <元の文字列>, short: <先頭4文字>, color: 既定色 }` で
   新規作成し、`ImportWarning::SectionCreated` を出す。

ID の採番は `roster::append_sections(&mut doc.sections, &mut next_id, specs)` に委ね、
`drill-interop` は ID を自分で払い出さない（第15番 3.2節の「`Document` がカウンタを所有する」規約）。
`PerformerKind` / `Symbol` / `Equipment` は**推定しない**（`not_imported` に載せる）。
楽器名から `PerformerKind::Percussion` を推定するのは当たりそうに見えて外れ、
外れたことに気づけないまま印刷物へ流れるほうが害が大きい。

### 3.3 MusicXML / MIDI — 時間軸だけを取り込む

#### 3.3.1 何を取り込むか

| 取り込む | 写像先 | 理由 |
|---|---|---|
| テンポ変化 | `TempoMap`（`tempo::TempoChange { count, bpm }`） | 手入力が最も苦痛かつ間違えやすい |
| 拍子変化 | `Vec<MeterChange>` → `countsheet` の `beats_per_measure`（第17番） | 小節・拍表示の正しさに直結 |
| リハーサル記号 | `Vec<MusicalMark>` → セット名候補 / カウントシートの注記 | 「Letter C から」の共通言語 |
| 小節線位置 | `Vec<MeasureStart>`（カウント値） | タイムライン上の小節グリッド表示 |
| 総カウント数 | `u32` | 範囲検証 |
| **取り込まない** | 音符・歌詞・強弱・パート・楽器・調号 | 3.1 の 11 番の判断 |

#### 3.3.2 共通の中間表現

```rust
/// The musical time axis extracted from a score or a MIDI file. Contains no
/// notes — only what the count timeline needs.
#[derive(Clone, Debug, Default)]
pub struct MusicalTimeline {
    pub tempo: TempoMap,
    pub meters: Vec<MeterChange>,
    pub measures: Vec<MeasureStart>,
    pub marks: Vec<MusicalMark>,
    pub total_counts: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterChange { pub measure: u32, pub count: f32, pub numerator: u8, pub denominator: u8 }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeasureStart { pub measure: u32, pub count: f32, pub implicit: bool }

#[derive(Clone, Debug, PartialEq)]
pub struct MusicalMark { pub measure: u32, pub count: f32, pub text: String, pub kind: MarkKind }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind { Rehearsal, Marker, Text, Segno, Coda }
```

**「1カウント」の定義**が最重要の意味決定である。マーチングでは通常 1 count = 4分音符だが、
6/8・9/8・12/8 の複合拍子では付点4分音符が1カウントになる。既定を持ちつつ選ばせる。

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountUnit { Whole, Half, Quarter, Eighth, DottedQuarter }

impl CountUnit {
    /// Length of one count in quarter notes.
    pub const fn quarters(self) -> f32 {
        match self {
            CountUnit::Whole => 4.0,
            CountUnit::Half => 2.0,
            CountUnit::Quarter => 1.0,
            CountUnit::Eighth => 0.5,
            CountUnit::DottedQuarter => 1.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicalImportOptions {
    pub count_unit: CountUnit,
    /// When true, 6/8, 9/8 and 12/8 measures use `DottedQuarter` regardless of
    /// `count_unit`. Default true — matches how marching shows are actually counted.
    pub compound_meter_is_dotted: bool,
    /// 1-based; measures before this are dropped and counts start at 0 here.
    pub start_measure: u32,
    pub max_measures: u32,
    pub max_marks: usize,
    pub max_bytes: u64,
}

impl Default for MusicalImportOptions {
    fn default() -> Self {
        Self {
            count_unit: CountUnit::Quarter,
            compound_meter_is_dotted: true,
            start_measure: 1,
            max_measures: 4_000,
            max_marks: 4_000,
            max_bytes: 32 * 1024 * 1024,
        }
    }
}
```

BPM の写像: MusicXML の `<sound tempo="...">` と MIDI の tempo meta は**どちらも4分音符あたり**の値である。
1カウント = `unit.quarters()` 四分音符なので、

```
count_bpm = quarter_bpm / unit.quarters()
```

（付点4分カウントなら `quarter_bpm / 1.5`。6/8 で「四分音符=180」は「カウント=120」になる。）
これを `TempoMap::set(count, count_bpm)` で積む。`TempoMap` は既に非有限値・0以下を
`sanitize_bpm`（tempo.rs:194-200）で吸収するので、異常値でパニックしない。

#### 3.3.3 MIDI — クレート選定と実装

**選定: `midly` 0.5.3**（Unlicense、累計 355,760 DL / 直近 115,080 DL、リポジトリ negamartin/midly）。

| 候補 | 判断 |
|---|---|
| **midly 0.5.3** | **採用**。通常依存は optional な `rayon` **1つだけ**で、`default-features = false, features = ["std","alloc"]` にすると**依存ゼロ**になる。ライフタイム借用で元バイト列を指すゼロコピー設計、`no_std` 対応。`Smf::parse(&[u8])`、`Timing::{Metrical, Timecode}`、`MetaMessage::{Tempo, TimeSignature, Marker, Text, TrackName}` と、必要なものが全て揃っている |
| nodi 1.0.3 | 不採用。再生抽象が主目的で、直近DLが 723 と実質メンテ停止圏。midly の上に乗るだけで得るものがない |
| midir 0.11 | 不採用。リアルタイム MIDI **ポート** I/O（デバイス入出力）であって、SMF パーサではない。用途が違う。将来「MIDI キーボードでテンポタップ」をやるなら再検討 |

```rust
/// Parse a Standard MIDI File into a `MusicalTimeline`.
/// Rejects (never panics on) : SMPTE timing, zero division, files over
/// `opts.max_bytes`, and track counts / event counts beyond the caps.
#[cfg(feature = "midi")]
pub fn import_midi(
    bytes: &[u8],
    opts: &MusicalImportOptions,
) -> Result<ImportOutcome<MusicalTimeline>, DrillError>;
```

アルゴリズム:

1. `bytes.len() > opts.max_bytes` なら即 `Err`。
2. `midly::Smf::parse(bytes)?`。
3. `smf.header.timing`:
   - `Timing::Metrical(tpq)` → `ticks_per_quarter = u16::from(tpq)`。`0` なら `Err`。
   - `Timing::Timecode(fps, subframe)` → `Err(DrillError::UnsupportedMidiTiming)`。
     SMPTE タイムコードのファイルは絶対時刻で書かれており、テンポマップという概念を持たない。
     エラーメッセージで「小節・テンポを持つ形式（拍単位）で書き出し直してください」と案内する。
4. 全トラックを走査し、`(absolute_ticks, event)` を集める。format 1 ではテンポは通常トラック0だが
   規格上の保証はないので**全トラックの meta を対象にする**。`delta` は `u28` なので
   累積は `u64` で行い、`saturating_add` する（オーバーフロー禁止、00-conventions.md）。
5. tick → count: `count = ticks as f64 * unit_ticks_recip` where
   `unit_ticks = ticks_per_quarter as f64 * unit.quarters() as f64`。
   除算ではなく事前計算した逆数を掛け、`f64` で計算してから `f32` へ落とす
   （64,000 カウント規模で `f32` 累積誤差を出さないため。00-conventions.md 不変条件4 の精神）。
6. `MetaMessage::Tempo(us_per_quarter)` → `quarter_bpm = 60_000_000.0 / us_per_quarter as f64`。
   `us_per_quarter == 0` はスキップして `ImportWarning` を出す。
7. `MetaMessage::TimeSignature(num, denom_pow2, _, _)` → `denominator = 1u32 << denom_pow2.min(7)`。
   複合拍子判定（`compound_meter_is_dotted && denominator == 8 && num % 3 == 0`）でその区間の
   `CountUnit` を `DottedQuarter` に切り替える。
8. `MetaMessage::Marker(b)` / `Text(b)` / `CuePoint(b)` → `MusicalMark`。バイト列は
   MIDI 規格上 Latin-1 だが実務では UTF-8 と CP932 が混在するので、
   `encoding_rs` で「UTF-8 として妥当なら UTF-8、でなければ CP932、それも駄目なら Latin-1」の順に解釈する。
   長さは 256 バイトで切り詰める。
9. 同一 count に複数のテンポ変化があれば最後を採る（`TempoMap::set` の既存挙動 tempo.rs:78-80 と一致）。

#### 3.3.4 MusicXML — クレート選定と実装

**選定: `roxmltree` 0.21.1 + `zip` 8.x を使い、抽出は自前で書く。`musicxml` クレートは採用しない。**

| 候補 | 判断 |
|---|---|
| `musicxml` 1.1.2 | **不採用**。(a) 累計 20,248 DL / 直近 1,995 DL と実績が薄く、単一ベンダ（hedgetechllc）の単独メンテ。(b) `regex` + `crc32fast` + `miniz_oxide` を引き込む。(c) MusicXML 4.0 の**全要素**を型で表現する設計で、我々が必要とするのは `divisions` / `sound@tempo` / `metronome` / `time` / `rehearsal` の5要素だけ。信頼できない入力を食わせる面積が必要の 100 倍になる。(d) 我々のフィーチャ（`musicxml`）を落としたビルドでの依存削減効果が薄れる |
| **`roxmltree` 0.21.1 + `zip` 8.x** | **採用**。roxmltree の通常依存は `memchr` **1つだけ**。`ParsingOptions` が `allow_dtd`（既定 `false`）と `nodes_limit`（既定 `u32::MAX`）を持ち、DTD を既定で拒否したうえで billion laughs 対策も入っている — **信頼できない XML に対して欲しい防御が既定で有効**。抽出対象が5要素なので自前実装は 300 行程度に収まり、その全てを我々がテスト・fuzz できる |
| `quick-xml` 0.41 | 次点。ストリーミング（`Reader::read_event`）でツリーを作らない分メモリが要らないが、`<direction>` が対応する `<measure>` の文脈を自前で持ち回る必要があり、5要素抽出では roxmltree のツリー走査の方が誤りにくい。ファイル上限を 32 MiB に切ってあるのでツリー化のコストは許容範囲 |

トレードオフを正直に書く: 自前実装は MusicXML 仕様の変更に自分で追随する義務を負う。
ただし本書が触るのは MusicXML 3.0 から安定している最小部分集合であり、
仕様追随のコストは低いと判断する。もし将来「楽譜そのものを表示する」機能が要るなら
その時点で `musicxml` クレートを再評価する（9章）。

```rust
#[cfg(feature = "musicxml")]
pub fn import_musicxml(
    bytes: &[u8],
    opts: &MusicalImportOptions,
) -> Result<ImportOutcome<MusicalTimeline>, DrillError>;
```

アルゴリズム:

1. `bytes.len() > opts.max_bytes` なら即 `Err`。
2. コンテナ判定: 先頭が `PK\x03\x04` なら `.mxl`（ZIP）。そうでなければ生 XML。
3. `.mxl` の場合（**zip bomb 対策を先に置く**）:
   - エントリ数 ≤ 64。
   - 各エントリの**宣言展開後サイズ**の合計 ≤ 64 MiB。宣言を信じず、実際の読み出しも
     `std::io::Read::take(64 MiB)` で打ち切る。
   - エントリ名に `..` / 絶対パス / ドライブレターを含むものは拒否（パス・トラバーサル。展開はしないが
     名前をログや報告に出す前に弾く）。
   - `META-INF/container.xml` を読み、`<rootfile full-path="...">` の最初の1件を対象にする。
     無ければ拡張子 `.xml` / `.musicxml` の最初のエントリにフォールバック。
4. XML パース:
   ```rust
   let opt = roxmltree::ParsingOptions {
       allow_dtd: false,          // 既定値。明示して意図を残す
       nodes_limit: 500_000,      // 既定は u32::MAX。必ず絞る
       ..Default::default()
   };
   let doc = roxmltree::Document::parse_with_options(text, opt)?;
   ```
   `entity_resolver` は `None` のまま（外部実体を解決しない）。
5. ルートが `score-partwise` か `score-timewise` かを見る。`score-timewise` は
   `<measure><part>` の入れ子が逆になるだけなので、走査順を切り替えて同じ抽出器に流す。
6. **小節グリッドの構築**（第一 `<part>` のみを使う）:
   - `<attributes><divisions>` を追跡（0 や非数は前の値を保持、初期値 1、`ImportWarning` を出す）。
   - `<attributes><time><beats>` / `<beat-type>` を追跡。小節の長さ（4分音符単位）は
     `beats * 4 / beat_type`。
   - `<measure implicit="yes">`（弱起・小節分割）は拍子から長さを決められないので、
     その小節に限り `<note><duration>` を第1ボイスについて合計し（`<chord>` を持つ音符は加算しない、
     `<backup>` / `<forward>` を符号付きで反映）、`divisions` で割って4分音符長を得る。
     この推定を使った小節は `MeasureStart { implicit: true }` として記録し、
     報告に「弱起小節の長さは音符長から推定しました」と出す。
   - 小節数が `opts.max_measures` を超えたら打ち切り、`ImportWarning::Truncated` を出す（`Err` にはしない）。
7. **テンポ**: 全 `<part>` の `<direction>` を走査する。
   - `<sound tempo="X">` があればそれを4分音符 BPM として採用（最優先）。
   - 無ければ `<direction-type><metronome><beat-unit>` + `<per-minute>` から換算
     （`beat-unit` が `half` なら ×2、`eighth` なら ÷2、`<beat-unit-dot>` があれば ×1.5）。
   - 位置は `<direction>` の親 `<measure>` の開始カウント +
     直前までの `<note>` 累積（`offset` 要素があれば加算）。ボイス追跡の複雑さを避けるため、
     P1 では**小節先頭に丸める**。小節途中のテンポ変化は稀であり、丸めたことを報告する。
8. **リハーサル記号**: `<direction-type><rehearsal>` のテキスト。`<words>` は
   `MarkKind::Text` として取るが既定では**無効**（`opts` で有効化）。演奏指示の文字列が
   セット名候補に大量に混ざるのを防ぐ。
9. `max_marks` を超えたら打ち切り。

#### 3.3.5 取り込んだ時間軸の適用

```rust
/// Turn a `MusicalTimeline` into edits against a document. Never mutates.
/// `set_naming` decides whether rehearsal marks rename sets.
pub fn apply_timeline(
    doc: &Document,
    timeline: &MusicalTimeline,
    policy: TimelinePolicy,
) -> Result<Vec<Edit>, DrillError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimelinePolicy {
    pub replace_tempo: bool,
    /// Create one set boundary per rehearsal mark, preserving existing dots by
    /// splitting the enclosing set. Off by default — it changes the drill's
    /// structure, which the user should opt into deliberately.
    pub split_sets_at_marks: bool,
    /// Rename set *i* to the rehearsal mark that starts at its first count.
    pub rename_sets_from_marks: bool,
    pub import_meters: bool,
}
```

テンポだけを入れる（既定）なら `Edit::ReplaceTempoMap { before, after }` 1件で済み、Undo も1手で戻る。

### 3.4 エクスポート

#### 3.4.1 一覧と所有クレート

| 出力 | 用途 | 所有 | 状態 |
|---|---|---|---|
| `.drill.json` / `.drillproj` | 自形式・保存 | drill-core + 第41番 | 一部実装（lib.rs:396-403） |
| **座標 CSV**（安定方言） | 表計算・他ツール・自形式往復 | drill-core（生成）+ drill-interop（符号化） | 実装済み・本書で方言を確定 |
| 汎用座標テキスト（ドットブック） | 印刷・配布 | drill-core `performer_sheet` | 実装済み |
| カウントシート TXT / コンティニュイティ TXT | 印刷 | drill-core | 実装済み |
| SVG / HTML（印刷 → ブラウザで PDF） | 印刷 | drill-core `svg.rs` → 将来 drill-export | 実装済み（第17番が版面を再設計） |
| PDF（直接） | 印刷 | drill-export（第17番） | 未 |
| 動画 | 配布・確認 | drill-export（第31番） | 引数生成のみ（video.rs） |
| **MIDI クリックトラック** | 練習・DAW 連携 | drill-interop（本書） | 未 |
| `.drillmap.toml`（列マッピング） | 共有・再利用 | drill-interop（本書） | 未 |
| MusicXML 書き出し | — | — | **不採用**（3.4.4） |
| xlsx 書き出し | — | — | **不採用**（CSV で足りる。9章） |

#### 3.4.2 座標 CSV 方言の確定 — 「列名は追加のみ」

これが本書で最も重要な安定性の約束である。第16番文書 9章が
「CSV スキーマ拡張は既存テストを壊す破壊的変更であり、第41番または第52番とバージョニングの調整が必要」
として本書へ差し戻していた件への回答でもある。

**規則:**

1. **ヘッダ行は必須**。読む側は**列名で写像し、列位置に依存しない**。
2. **列名は凍結**。一度出した列名は改名しない、削除しない、意味を変えない。
3. **新しい列は末尾に追記する**。位置の変更をしない。
4. これにより「列の追加」は読む側（我々自身の往復インポータを含む）にとって**非破壊**になる。
   よって CSV 自体にバージョン番号を埋め込まない（先頭にコメント行を置くと Excel と
   多くの CSV ライブラリを壊すため、この選択は実利的でもある）。
5. **符号化**: 既定で **UTF-8 + BOM + CRLF**。日本語 Excel が BOM 無し UTF-8 CSV を CP932 として
   誤読する問題（実務で最も多い苦情）を構造的に消す。`CsvProfile` で切り替えられる。

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CsvProfile {
    pub encoding: SourceEncoding,   // 既定 Utf8Bom
    pub line_ending: LineEnding,    // 既定 CrLf
    pub delimiter: u8,              // 既定 b','
    pub columns: CsvColumns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsvColumns {
    /// Every frozen column. Round-trippable through `import_tabular_*`.
    Full,
    /// performer,label,set,counts,x,y only — for tools that choke on wide rows.
    Minimal,
    /// Frozen columns minus the raw x/y, for handing to performers.
    Readable,
}

/// Encode a CSV produced by `drill_core::coordinates` under `profile`.
/// `drill-core` builds the text (it owns the coordinate arithmetic);
/// `drill-interop` owns the byte-level encoding, BOM and line endings.
pub fn write_coordinates_csv<W: std::io::Write>(
    out: &mut W,
    doc: &Document,
    notation: CoordinateNotation,
    locale: Locale,
    profile: CsvProfile,
) -> Result<(), DrillError>;
```

凍結する列名（第16番 8章タスク5 の拡張後の姿を、本書が正として確定する）:

```
performer, label, set, counts, x, y, side_to_side, front_to_back,
side, yard_line, lateral_steps, reference, depth_steps, section, set_ordinal
```

先頭6列 `performer,label,set,counts,x,y` は現行 coordinates.rs:160 と**同一**であり、
既存のテスト `csv_has_header_and_one_row_per_pair`（coordinates.rs:302-313）は
`lines[0].starts_with("performer,label,set,counts,x,y")` に緩めるだけで通る。
`lines[0]` の完全一致はゴールデンテストへ移す（7章）。

`x` / `y` は `fmt_num`（coordinates.rs:22-31）で丸められた表示値である。
これは**往復の精度を落とす**（`45.15625` が `45.16` になる）。往復の正しさを守るため、
`CsvColumns::Full` では `x` / `y` を**丸めずに** `{:.6}` で出す。表示用の丸め値が欲しい用途は
`Readable` プロファイルが担当する。この非互換は `x`/`y` の**精度が上がる**方向であり、
既存の読み手を壊さない。

#### 3.4.3 MIDI クリックトラックの書き出し

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClickExportOptions {
    pub ticks_per_quarter: u16,   // 既定 480
    pub count_unit: CountUnit,    // 既定 Quarter
    pub beats_per_measure: u16,
    pub accent_note: u8,          // 既定 76 (Hi Wood Block)
    pub beat_note: u8,            // 既定 77 (Low Wood Block)
    pub channel: u8,              // 既定 9 (GM percussion)
    pub velocity_accent: u8,
    pub velocity_beat: u8,
    /// Emit an SMF `Marker` meta event at every set boundary.
    pub set_markers: bool,
}

/// Write a format-1 SMF: track 0 carries the tempo map, time signature and set
/// markers; track 1 carries the click notes. Deterministic — the same
/// (Document, options) always produces byte-identical output.
#[cfg(feature = "midi")]
pub fn write_click_midi<W: std::io::Write>(
    out: &mut W,
    doc: &Document,
    opts: &ClickExportOptions,
) -> Result<(), DrillError>;
```

`drill_core::audio::click_track`（audio.rs:104）は**秒**の配列を返すが、MIDI に要るのは
tick である。秒を経由すると丸め誤差が入るので、ここでは `TempoMap::events()` と
セットの `counts` から**カウント→tick を整数で**組み立てる（`tick = count * tpq * unit.quarters()`、
`tpq=480`・`unit=Quarter` なら 1 カウント = 480 tick の整数演算）。
これは 00-conventions.md 不変条件4「浮動小数の累加で時間を進めない」の直接の適用である。

#### 3.4.4 MusicXML 書き出しを行わない理由

DrillForge は音符を持たない。テンポと小節線しか無い MusicXML は譜面ソフトで開いても
空の小節が並ぶだけで、誰の役にも立たない。同じ情報は MIDI（3.4.3）の方が
DAW・メトロノームアプリ・譜面ソフトのいずれからも読める。**不採用**。

### 3.5 プラグイン — 「今は入れない、しかし必ず入る」ための設計

所有者決定（1.0 節、`docs/design/90-integration-roadmap.md` §5 の #4）により、
プラグインは**将来必ず入れる**。したがって本節の課題は「入れるか否か」ではなく、
**入れる時期の判断**と、**今の設計が将来の導入を不可能にしていないことの保証**である。

構成:

| 節 | 内容 |
|---|---|
| 3.5.1 | P0/P1 では入れない根拠（初版から**維持**） |
| 3.5.2 | 全面オープンソースが変えること — fork で足りる範囲と足りない範囲 |
| 3.5.3 | P0/P1 で出す3つの代替拡張点 |
| **3.5.4** | **先送りできない決定（今 decide しないと手遅れになるもの）** |
| **3.5.5** | **「閉ざさない条件」— 検証可能なチェックリスト** |
| 3.5.6 | 着手判定条件と、最小の第一弾 |
| 3.5.7 | 実装時の API 表面と、第51番との裁定 |

#### 3.5.1 P0/P1 では入れない（根拠は維持）

「拡張可能にする」は聞こえがよいが、公開した瞬間に**永久の互換性債務**と**新しい攻撃面**を同時に買う。
時期を遅らせる根拠を数字で示す。

**入れないことを支持する事実:**

1. **ホストランタイムの更新速度が速い。** Wasmtime は毎月20日に**メジャー版**を出す。
   サポートは通常版が2か月、LTS（版番号が12の倍数）が24か月。
   常時サポートされるのは LTS 2本 + 通常版 2本のみ。
   埋め込む側は「LTS に固定して24か月ごとに大移行」か「毎月追随」の二択になる。
2. **ラッパを噛ませても解決しない。** Extism 1.30.0（2026-06-04 公開）の依存は `wasmtime ^43` であり、
   現行の 47 から4メジャー遅れている。ラッパは追随の遅延を**継承**するうえ、
   自前の ABI・マニフェスト・ホスト関数群という第二の互換性表面を増やす。
3. **凍結すべきモデルが今まさに動いている。** `Performer` は第15番で7フィールド増え、
   `GridLine.label` は第16番で `HashKind` になり、`Set.counts` は A-7 で `SetCounts` になり、
   `schema_version` は 1 から 2 へ上がる。**移行の最中に ABI を凍結するのは最悪のタイミング**である。
4. **WASM の安全性はメモリ安全性であって、認可ではない。** サンドボックスが守るのは
   「プラグインがホストのメモリを壊さないこと」だけで、「プラグインが利用者のドリルを壊さないこと」は
   ホスト関数の設計（能力ベース権限）が守る。難しいのは後者であり、ランタイム選定では解決しない。
5. **需要の証拠がまだ無い。** 現時点で DrillForge には利用者がいない。
   「誰が何を拡張したいか」の実データ無しに API 表面を決めると、要らないものを永久に保守する。

これらはいずれも**時期の議論**であって、「入れない」の議論ではない。1〜3 は時間が解決し
（スキーマは v2 で凍る、LTS は選べる）、5 は利用者が付けば解決する。4 は解決しないが、
第51番 3.7 が能力ベース権限として既に答えを出している。

#### 3.5.2 全面オープンソースが変えること — fork と plugin の分界

コードが公開される以上、**プラグインを待たずに fork・直接改造という拡張経路が最初から存在する**。
これはプラグイン API の必要性を変える。どこまで fork で足り、どこから足りないかを線引きする。

**fork で足りる範囲:**

- ワンオフの改造、社内・自校専用の機能、実験的な試み。
- Rust のビルド環境を持ち、upstream 追随のコストを自分で負える人。
- 「本体に入れるべきか分からない機能」の先行実装（後で PR にできる）。
- 我々自身のドッグフーディング。**我々は拡張点を経由せずに機能を足せる**ので、
  「自社機能のために拡張点を作る」という動機は生じない。これは拡張点を**最小に保てる**ことを意味する。

**fork では絶対に足りない範囲（プラグインでしか解けないもの）:**

| # | 課題 | なぜ fork で解けないか |
|---|---|---|
| F1 | **配布** | fork を他人に渡すと、受け手は非公式ビルドを信用する必要がある。署名も更新も別系統になる（doc 90 §5.1 は署名済みビルドを有償提供の中核に置いている）。プラグインなら本体は公式ビルドのまま |
| F2 | **合成** | A・B・C の3人が別々に fork した機能を1人が同時に使うにはマージが要る。プラグインなら並べるだけ |
| F3 | **利用者層** | 顧客は吹奏楽・マーチングの指導者であり、`cargo build` をしない。「拡張したければ fork せよ」は事実上「拡張できない」と同義 |
| F4 | **追随コスト** | fork は upstream に追随し続ける義務を永久に負う。プラグインは API が安定していれば放置できる |
| F5 | **安全性** | fork したバイナリにサンドボックスは無い。**「他人が書いた拡張を安全に動かす」は fork では原理的に解けない**。第51番 3.7 の脅威 T2（悪意あるプラグイン作者）は、fork の世界では「悪意ある fork 配布者」としてもっと悪い形で現れる |

**逆向きの作用 — OSS だからこそプラグインが要る。**
オープンにすると「この機能を入れてほしい」という PR と要望が来る。全てを本体にマージすると
`drill-app` は肥大し、`PRODUCT_QUALITY.md` の「初回起動から説明書なしで到達できる」が壊れる。
プラグインは**「本体には入れません、拡張として出してください」と言うための受け皿**でもある。
所有者決定 #1（全面オープン）と #4（将来プラグイン）は、この点で互いを必要としている。

**設計への具体的な影響:**

1. **優先度は下がる。** 上級者には fork という道が最初からあるので、P0/P1 で急ぐ理由が減る（3.5.1 を補強する）。
2. **必要性は消えない。** F1〜F5 は時間で解決しない。したがって「後で入れられること」の保証が要る（3.5.4/3.5.5）。
3. **拡張点は最小で始めてよい。** 自社都合の拡張点を作る動機が無い（上記）ので、
   最初は `shape-generator` 1 つだけで出せる（3.5.6）。
4. **API 文書化の負担が下がる。** ソースが読めるので、WIT 定義 + 動く例 1 つで実用に足りる。
   閉じた製品なら必要だった網羅的リファレンスを最初から書かなくてよい。
5. **プラグイン機能を有償ビルド限定にしない。** オープンの精神に反するうえ、fork で自明に回避される。
   署名鍵は「製品リリース署名鍵」と「プラグイン審査署名鍵」を**分離**する（用途と失効の粒度が違う）。

#### 3.5.3 P0/P1 で出す、安価で効果の大きい3つの拡張点

| 代替 | 何を可能にするか | コスト |
|---|---|---|
| **(1) 安定した書式契約**（3.4.2 の CSV 方言 + JSON スキーマの公開） | 任意の言語で外部ツールが書ける。Python で座標を加工して読み戻す、が今日から可能 | ゼロ（既に出している出力を約束に変えるだけ） |
| **(2) ヘッドレス CLI `drillforge-cli`** | `convert` / `export` / `analyze` / `render` / `validate`。CI・バッチ・他システム連携。**プロセス境界がそのままサンドボックス**になる | 1クレート。UI ロジックを持たないので薄い |
| **(3) データとしての拡張**（`.drillmap.toml` の列マッピング、シェイプのパラメータプリセット、帳票テンプレート） | 「うちの学校の CSV 形式」「うちの定番シェイプ」「うちの座標表の版面」をコード無しで共有 | 小。全てシリアライズ可能な既存型 |

(2) の CLI は特に重要である。「プラグイン」で本当に欲しがられるものの大半は
**バッチ処理**であり、それはプロセスを呼べば済む。しかも失敗しても親プロセスは死なず、
権限は OS が管理し、我々は ABI を凍結しなくてよい。

```rust
// crates/drill-cli — 表面の輪郭のみ（詳細は第53番の配布設計と協調）
// drillforge export coords  --in show.drillproj --out coords.csv --profile full
// drillforge export click   --in show.drillproj --out click.mid
// drillforge import csv     --in dots.csv --map school.drillmap.toml --out show.drillproj --report r.json
// drillforge analyze        --in show.drillproj --json
// drillforge validate       --in show.drillproj
//
// 機械可読出力は必ず {"api": 1, ...} を先頭フィールドに持つ（3.8 の安定性契約）。
```

#### 3.5.4 先送りできない決定 — 今 decide しないと手遅れになるもの

「後から足せるもの」と「後から足すと破壊的変更になるもの」を分ける。
**後者だけ**を今の設計で確定させる。判定基準は次の3つのいずれかに当てはまるかである。

- (α) **永続化データの形**に触れる（スキーマ v2 凍結後に変えると、既存ファイルが開けなくなる）。
- (β) **既存コードの構造**に触れる（後から入れるには広範囲の書き換えが要る）。
- (γ) **語彙の凍結**に触れる（公開後に意味を変えられない）。

##### D1 (α)(γ) 外部提供シェイプの永続化枠 — **最も手遅れになりやすい項目**

第14番の `ShapeSpec` は**17 バリアントの閉じた列挙**で、`#[serde(tag = "kind")]` により
`Set.shape` へ永続化される（`docs/design/14-formations-shapes.md:294-317`）。
プラグインが作ったシェイプはこのどれにも当てはまらない。

**後から `External` バリアントを足すと何が起きるか:**
`#[serde(tag = "kind")]` の列挙は、未知のタグに対して**デシリアライズ失敗**する。
v2 出荷後に足すと、プラグイン利用者が作った文書を**古い DrillForge が開けなくなる**。
「音声・画像が欠落してもドリル本体を開ける」（`PRODUCT_QUALITY.md`）に真っ向から反する。
一方、**v2 を凍結する前に枠だけ入れておけば、その被害は構造的に発生しない**。

したがって第14番・第10番へ次を要求する（本書は要求のみを出し、定義は両文書が行う）。

```rust
// 第14番 ShapeSpec への追加要求（v2 凍結前に入れること）
    /// A shape produced by an extension provider. Kept editable when the
    /// provider is present, and openable — with identical rendering — when it
    /// is not, because `baked` is authoritative for geometry.
    External(Box<ExternalShape>),

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExternalShape {
    pub provider: ProviderId,
    /// Provider-local shape name, e.g. "hexagon". Opaque to drill-core.
    pub name: String,
    /// The provider API major version this was authored against.
    pub api_major: u16,
    /// Provider-defined parameters. `BTreeMap` (not `HashMap`) so
    /// serialization is deterministic (00-conventions.md 不変条件5).
    pub params: std::collections::BTreeMap<String, ParamValue>,
    /// Points produced when the shape was last applied. **Authoritative.**
    /// The provider is only ever consulted to re-derive these after a
    /// parameter edit — never during load, render or export (D6).
    pub baked: Vec<Point>,
}

/// The only value type crossing the provider boundary. Deliberately tiny:
/// anything richer belongs in `baked`, not in parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v")]
pub enum ParamValue { Flag(bool), Number(f64), Text(String) }
```

`impl Shape for ExternalShape`（第14番の `Shape` トレイトを満たす）:

- `sample(count, out)`: `count == baked.len()` なら `baked` をコピー。
  異なれば `baked` を折れ線とみなして弧長で再標本化する（第14番が既に `FreePath` 用に持つ経路を再利用）。
  **プロバイダを呼ばない。**
- `arc_length()`: `baked` の折れ線長。
- `validate()`: `provider` が妥当、`name` / `params` が長さ上限内、`baked` が有限かつ
  `MAX_SHAPE_ORDER` 以内。

これで **「プラグインを入れていない DrillForge でプラグイン製の文書を開いても、
見た目が1ピクセルも変わらない」** が型で保証される。

##### D2 (α)(γ) `ProviderId` — 文書に残る出所

文書に入る外部由来の成果物は、**誰が作ったかを名乗る**必要がある。
無いと「このシェイプを編集するには何を入れればよいか」を利用者に案内できず、
将来の provenance（第51番 STRIDE の R = 否認防止）も成立しない。

```rust
/// Reverse-DNS-ish namespaced provider identity. Appears in saved documents,
/// so its grammar is frozen at schema v2.
///
/// Grammar: one or more `[a-z0-9]([a-z0-9-]*[a-z0-9])?` segments joined by
/// '.', total <= MAX_PROVIDER_ID_BYTES. ASCII only — this is an identifier,
/// not a display name, and must not vary by locale or normalization form.
/// `"builtin"` is reserved for DrillForge's own providers.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProviderId(String);

impl ProviderId {
    pub const BUILTIN: &'static str = "builtin";
    pub fn parse(s: &str) -> Result<Self, DrillError>;
    pub fn as_str(&self) -> &str;
    pub fn is_builtin(&self) -> bool { self.0 == Self::BUILTIN }
}
```

第10番 `limits` への追加要求: `pub const MAX_PROVIDER_ID_BYTES: usize = 64;`
`validate()` への追加要求（V26 として）: 全 `ExternalShape.provider` が `ProviderId::parse` を通り、
`name` が `MAX_SET_NAME_BYTES` 以内、`params` が 64 件以内、各 `Text` が `MAX_LABEL_BYTES` 以内。

**未知の provider を拒否しない**（前方互換）。`validate()` は文法だけを見る。
未知の provider は「編集できないが開ける」であって「壊れたファイル」ではない。
これも v2 で決めておかないと、後から寛容化するのは（既にエラーで弾いた利用者がいるため）遅い。

##### D3 (β)(γ) `Edit` を ABI に露出しない — プラグイン向けは射影 `PluginEdit`

第10番の `Edit` は **40 バリアント**（`docs/design/10-document-model.md:779-841`）で、
`RouteTable` / `CameraProgram` / `SetCounts` / `IdAllocator` の watermark 規約に密結合し、
`Serialize` を意図的に持たない。これを 1:1 で ABI に出すと、
**内部モデル全体を凍結する**ことになり、3.5.1 の 3 の問題を将来にわたって固定化してしまう。

**決定: プラグインが返すのは `PluginEdit` の列であり、`Edit` ではない。**
`drill-plugin` が `PluginEdit → Edit` を変換し、そのうえで第51番 3.7 の検証パイプライン
（複製に適用 → `validate_untrusted(&Limits::DERIVED)` → 不合格なら全件破棄）へ渡す。
第51番の「プラグインが返した `Vec<Edit>`」という記述は `drill-sandbox` 境界での話であり、
本決定と矛盾しない（3.5.7 の裁定表）。

```rust
/// The projection of `Edit` that plugins may express. Eight variants, chosen
/// to cover the four extension points and nothing more. Adding a variant is a
/// minor API bump; `Edit` may be refactored freely as long as this mapping
/// still compiles.
#[derive(Clone, Debug, PartialEq)]
pub enum PluginEdit {
    MovePoints { set: SetKey, targets: Vec<PerformerKey>, points: Vec<Point> },
    ApplyShape { set: SetKey, shape: ExternalShape, order: Vec<PerformerKey> },
    RelabelPerformers { changes: Vec<(PerformerKey, String)> },
    AssignSection { targets: Vec<PerformerKey>, section: SectionKey },
    RenameSet { set: SetKey, name: String },
    SetSetNote { set: SetKey, note: String },
    InsertSet { after: SetKey, name: String, counts: u16, positions: Vec<Point> },
    SetTempo { events: Vec<(f32, f32)> },
}

/// Opaque per-call handles. **Not** `SetId` / `PerformerId`.
///
/// Minted by the host when it builds the snapshot handed to the plugin, and
/// resolved back on return. Two consequences, both load-bearing:
/// 1. the internal id representation stays free to change (D7);
/// 2. a plugin cannot forge a handle for an object it was never shown —
///    an unknown handle is a hard error, not a dangling reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SetKey(u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PerformerKey(u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SectionKey(u32);

/// The one place where plugin output becomes internal edits.
pub struct KeyMap { /* dense Vec<SetId> / Vec<PerformerId> / Vec<SectionId> */ }

impl KeyMap {
    /// Built alongside the snapshot. Keys are indices, so resolution is O(1)
    /// and an out-of-range key is caught by a bounds check, not a lookup miss.
    pub fn from_document(doc: &Document) -> Self;
    pub fn to_edits(&self, doc: &Document, edits: Vec<PluginEdit>)
        -> Result<Vec<Edit>, DrillError>;
}
```

これは (β) に該当する。`Edit` を直接出す前提で WIT を書いてしまうと、
後から射影へ切り替えるのは非互換変更になる。**最初から射影で始める**ことだけが解。

##### D4 (γ) 能力語彙は第51番の6種で凍結する — 追加しない

第51番 3.7 が定義済みの `Capability`（`ReadDocument` / `ReadPerformerLabels` / `ReadSelection` /
`ProposeEdits` / `ReadAsset` / `WriteExport`）が、本書の4つの拡張点を**過不足なく覆う**ことを検算した。

| 拡張点 | 必要な能力 | 備考 |
|---|---|---|
| 図形生成 | **なし** | `(count, grid, params) -> points` の純関数。文書を読まない |
| 解析 | `ReadDocument`（+ 演者名を出すなら `ReadPerformerLabels`、選択に限定するなら `ReadSelection`） | 返すのは診断のみ。書き込み能力を要さない |
| インポート | `ReadAsset` + `ProposeEdits` | バイト列はホストが開いて不透明ハンドルで渡す。パスは見えない |
| エクスポート | `ReadDocument`（+ `ReadPerformerLabels`）+ `WriteExport` | 出力先はホストが作ったストリーム1本 |
| 帳票テンプレート | `ReadDocument` + `ReadPerformerLabels` + `WriteExport` | エクスポートの特殊形。新しい能力ではない |

**結論: 7 つ目の能力は要らない。** 初版が提案していた `GenerateShape` / `Analyze` /
`ImportBytes` / `ExportBytes` / `ReportTemplate` は**撤回**する。
これらは能力ではなく**インターフェース**であり、能力（＝ホストの資源へのアクセス権）と
インターフェース（＝呼ばれる関数の形）を混同していた。
拡張点の追加は **WIT ワールドへの interface 追加**で行い、`Capability` は増やさない。

`GenerateShape` を撤回しても「大半のプラグインが何の権限も要求しない」という初版の要点は残る
（図形生成の必要能力が**空集合**なので、むしろ強くなった）。

##### D5 (β) 拡張点（seam）を今作り、組み込み実装で使う

「後からフックを差す」は、差す場所が無ければできない。
**4か所に `ProviderId` 鍵のレジストリを置き、組み込み実装も必ずその経路を通す。**
組み込みが同じ経路を通ることが、その経路が本物である唯一の証明である
（通っていなければ、プラグイン導入時に初めて経路を作ることになり、そこで必ず設計が破綻する）。

| # | 拡張点 | 所有文書 | 既にあるもの | 追加要求 |
|---|---|---|---|---|
| S1 | 図形生成 | 第14番 | `trait Shape { sample / arc_length / validate }`（14:320-336）— **既に継ぎ目の形をしている** | `ShapeProvider` レジストリ。組み込み 17 種を `ProviderId::BUILTIN` として登録 |
| S2 | 解析 | 第13番 | `scan_transition` / `scan_formation` / `Severity` / `CollisionEvent`（13:391,588,115,472）— 自由関数 | `AnalysisProvider` トレイトと登録簿。組み込み 2 種を登録 |
| S3 | インポート/エクスポート | 本書 | 無し（本書が新設中） | `FormatProvider`。CSV/MIDI/MusicXML を登録 |
| S4 | 帳票テンプレート | 第17番 | `PageTemplate` / `ReportKind` / `trait PageCanvas`（17:372,696）— データ側は既に分離済み | `ReportProvider`。組み込み帳票を登録 |

トレイトの形（`drill-core` に置けるのは S1/S2、`drill-interop` が S3、`drill-export` が S4）:

```rust
/// One registry per extension point. Deterministic iteration order
/// (`BTreeMap`) so `list()` is stable across runs — 00-conventions.md 不変条件5.
pub struct Registry<P: ?Sized> {
    entries: std::collections::BTreeMap<(ProviderId, String), Box<P>>,
}

impl<P: ?Sized> Registry<P> {
    pub fn new() -> Self;
    /// Later registration of the same key replaces the earlier one and returns
    /// the displaced entry, so the app can warn about a plugin shadowing a
    /// built-in instead of silently letting it win.
    pub fn register(&mut self, provider: ProviderId, name: String, imp: Box<P>) -> Option<Box<P>>;
    pub fn get(&self, provider: &ProviderId, name: &str) -> Option<&P>;
    pub fn list(&self) -> impl Iterator<Item = (&ProviderId, &str)>;
}

/// S1. Implemented by the built-in `ShapeSpec` dispatcher and, later, by
/// `drill_plugin::WasmShapeProvider`. Object-safe on purpose.
pub trait ShapeProvider {
    fn params(&self) -> &[ParamSpec];
    /// Must be pure and deterministic: same (count, grid, params) => same points.
    fn generate(
        &self,
        count: usize,
        grid: &GridConfig,
        params: &std::collections::BTreeMap<String, ParamValue>,
        out: &mut Vec<Point>,
    ) -> Result<(), DrillError>;
}

/// S2.
pub trait AnalysisProvider {
    fn scan(&self, doc: &Document, params: &ScanParams, out: &mut Vec<Finding>)
        -> Result<(), DrillError>;
}

/// S3. `drill-interop`.
pub trait FormatProvider {
    fn extensions(&self) -> &[&str];
    fn can_import(&self) -> bool;
    fn can_export(&self) -> bool;
    fn import(&self, bytes: &[u8], limits: &ImportLimits)
        -> Result<ImportOutcome<Vec<PluginEdit>>, DrillError>;
    fn export(&self, doc: &Document, out: &mut dyn std::io::Write) -> Result<(), DrillError>;
}
```

**この4つのレジストリは P0/P1 で作り、組み込みだけを登録した状態で出荷する。**
wasmtime も `drill-plugin` も要らない。追加コストは実質「関数呼び出しが1段間接になる」だけで、
いずれも毎フレーム経路ではない（5.5）。

##### D6 (β)(γ) 決定論の規約 — プラグイン出力は焼き付け、再実行しない

第51番 3.7 は実行時間の制限に **epoch 割り込み**（実時間 250 ms / 30 s）を採用した。
実時間の締め切りは**非決定的**である。同じ入力でも、負荷次第で成功したり打ち切られたりする。
これは 00-conventions.md 不変条件5（同じ `(Document, config, count)` から常に同じ出力）と
`MEDIA_PIPELINE.md`「同じ project/config から同じフレーム列を生成する」に抵触し得る。

**決定: プラグインは「編集時」にしか動かない。読み込み・描画・書き出しの経路では絶対に呼ばない。**

- プラグイン出力は必ず `baked`（D1）または通常の `Edit`（D3）として文書に固定される。
- 文書を開くとき、DisplayList を組むとき、動画フレームを描くときにプロバイダは参照されない。
- したがって epoch の非決定性は**出力に伝播しない**。打ち切られた呼び出しは結果を捨てるだけで、
  文書は変化しない（不変条件1 と同じ性質）。
- 逆に、この規約が無いと D1 の `baked` は「キャッシュ」に格下げされ、
  プラグインの有無で書き出し結果が変わる。**D1 と D6 は一体の決定**である。

初版が「決定論のために fuel が必須」と書いていたのは、この規約が無い前提での議論だった。
規約を置くほうが正しく、第51番の epoch 採用（wasm 側から回避不能という強い性質を持つ）と両立する。
**fuel は不要。** 第51番の設定をそのまま使う。

##### D7 (γ) `drill-core` 公開 API のうち、ABI に露出するもの／絶対に露出しないもの

WIT の値型は `drill-core` の型の**写像（projection）**であり、再輸出ではない。
Rust の型に `#[repr(C)]` や ABI 都合の serde 属性を付けて寄せることはしない。

| 区分 | 対象 | 理由 |
|---|---|---|
| **露出する**（値のコピーとして WIT record に写す） | `Point` の 2 成分 / `GridConfig` の数値部（`width` `height` `horizontal_steps` `horizontal_units` `vertical_steps` `vertical_units` `major_line_interval` とハッシュ位置の配列） / `SetCounts` の数値 / `CoordinateReading` の数値 / `Severity` / `ParamValue` / `ProviderId` の文字列 | いずれも**意味が幾何・時間として外部に定義されている**ので、内部表現を変えても写像を保てる |
| **絶対に露出しない** | `Document` / `Edit` / `History` / `IdAllocator` / `Revision` `Scopes` / `SetId` `PerformerId` `SectionId` `SubsetId` `CameraId` の生値 / `RouteTable` / `CameraProgram` / `poisoned` / `GridConfig` の `GridLine.kind`（`HashKind::Custom` の文字列） | 内部構造そのもの。露出すると 3.5.1 の 3 が永久化する。ID は `SetKey` 等の不透明ハンドル（D3）で置き換える |
| **写像を書く場所** | `drill-plugin/src/project.rs` 1 ファイルに集約 | 変換が散らばると「どこまで露出したか」が追えなくなる。1 ファイルなら差分レビューで守れる |

`GridLine.kind` を露出しない理由は第16番 不変条件3（`Document` は表示文字列を持たない）と同じで、
`HashKind::Custom { name }` を渡すとロケール非依存の約束が破れるためである。
プラグインにはハッシュの**位置（f32 の配列）**だけを渡す。

##### 「後からで間に合うもの」（今やらない）

対比のため明示する。以下は (α)(β)(γ) のいずれにも該当しないので、着手時にやればよい。

- WIT ワールドへの interface 追加（`analysis` / `format` / `report`）。ワールドは追加に対して非破壊。
- `.drillplug` パッケージ形式、署名鍵の運用、同意ダイアログ、ギャラリー。
- wasmtime のバージョン選択、`Store` の再利用戦略、コンパイル済みモジュールのキャッシュ。
- プラグイン向けの i18n カタログ解決。
- 開発者向けテンプレートリポジトリ、`cargo component` のひな形。

#### 3.5.5 「閉ざさない条件」— 検証可能なチェックリスト

**本変更の主眼。** 現在の設計がプラグイン導入を不可能にしていないことを、
CI で機械的に確認できる条件として明文化する。各条件は 4 章の不変条件に対応する。

| # | 条件 | 検証方法 |
|---|---|---|
| **C1** | 文書に入る外部由来の成果物は `ProviderId` と `baked` を持ち、**プロバイダ不在でも同一に描画される** | `ExternalShape` を含む文書を、レジストリを空にして開き、`DisplayList` がバイト単位で一致すること |
| **C2** | 4 つの拡張点はレジストリを通り、**組み込み実装も同じ経路を通る** | レジストリから組み込みを外すと組み込み機能も引けなくなること（＝迂回路が無いことの証明） |
| **C3** | `Edit` は ABI に露出しない | CI grep: `wit/` 配下に `edit` を名に持つ record/variant が現れないこと。`PluginEdit` のバリアント数の上限（12）をテストで固定 |
| **C4** | 能力語彙は第51番の 6 種 | `Capability` のバリアント数を 6 と assert するテスト（増やすときは第51番の改訂を伴うことを強制する） |
| **C5** | プラグインは読み込み・描画・書き出し経路で呼ばれない | `Registry::get` に呼び出し回数カウンタを入れ、`Document::from_json` / `drill_render::build` / 動画書き出し 1 本の実行後に 0 であることを assert |
| **C6** | `drill-core` は wasmtime にも `drill-plugin` にも依存しない | `cargo tree -p drill-core` の依存が serde / serde_json のみであることを CI で assert（第50番の既存チェックに 1 行追加） |
| **C7** | プラグイン由来の変更は**単一の関門**を通る | `KeyMap::to_edits` → 第51番の検証パイプライン以外に `Document` へ到達する経路が無いこと。`drill-app` から `Edit::apply` を直接呼ぶプラグイン経路が無いことを grep |
| **C8** | スキーマ v2 は未知の `provider` を拒否しない | 架空の `ProviderId` を持つ `ExternalShape` を含むフィクスチャが `validate()` を通り、警告付きで開けること |
| **C9** | WIT の値型は写像であり再輸出でない | 変換関数が `drill-plugin/src/project.rs` にのみ存在すること（grep）。`drill-core` の型に ABI 都合の属性が付いていないこと |
| **C10** | `ShapeSpec` / `ParamValue` / `ProviderId` の serde 表現がスキーマ v2 で凍結されている | ゴールデン JSON（第41番のマイグレーションテストに相乗り） |

**現時点でこれらを破っている箇所は無い。** ただし C1・C2・C8・C10 は
**まだ実装が存在しないので自動的に緑**であり、D1・D2・D5 を実装した時点で初めて意味を持つ。
したがって 8 章では D1/D2/D5 を **Wave 0.5（他の Wave より先）** に置く。

**逆に、これらを破ることになる設計変更の例**（レビュー時のチェック観点）:

- `ShapeSpec` に「全バリアントを網羅する `match`」を `drill-app` に書く（C2 を破る。`External` の追加で壊れる）。
- 解析結果に `&'static str` の日本語メッセージを直接入れる（プラグインが同じ形を返せない。第42番の `Locale` 経由に統一する）。
- CSV エクスポータを `FormatProvider` を経由せず `main.rs` から直接呼ぶ（C2 を破る）。
- `Set.shape` を `Option<ShapeSpec>` から `ShapeSpec` の別表現へ変える（C10 を破る）。

#### 3.5.6 着手判定条件と、最小の第一弾

##### 着手条件（全て判定可能な形）

| # | 条件 | 判定方法 |
|---|---|---|
| G1 | `SCHEMA_VERSION == 2` のビルドが出荷済みで、以後 **6 か月**にわたり `Document` の serde 表現に非互換変更が 0 件 | 第41番のゴールデン JSON が 6 か月間更新されていないこと（git log） |
| G2 | Wasmtime に**採用可能な LTS がある**（版番号が 12 の倍数）。かつ、その LTS の残存サポート期間が **18 か月以上** | Wasmtime のリリースポリシー（毎月 20 日メジャー、LTS は 24 か月）から算出。現行最新は 47.0.3 なので、実質 **48 LTS 以降**を待つ |
| G3 | 3.5.5 の C1〜C10 が全て CI で緑 | CI |
| G4 | 24 か月ごとの LTS 更新工数を確保できる（見積り: 1 回あたり 2〜5 人日） | 体制の判断 |
| G5 | CLI + 書式契約 + マクロ（3.6）では実現できない拡張要望が、**異なる利用者から 3 件以上** | 課題管理 |

##### 段階的な出し方 — G5 を待たずに出せる最小の第一弾

G5（需要の証拠）は時間がかかる。一方 G1〜G4 は我々の側だけで満たせる。
そこで**最小の第一弾**を定義し、G1〜G4 が揃った時点で G5 を待たずに出せるようにする。

> **第一弾 = `shape-generator` インターフェースのみ。能力の付与は空集合。**

理由:

- 必要な `Capability` が**空**（D4 の表）なので、同意ダイアログも権限設計も要らない。
  第51番 3.7 の機構のうち、実行隔離と上限だけを使う。
- 出力は `Vec<Point>` の一種類だけ。`PluginEdit` の変換すら要らない
  （`ApplyShape` 1 バリアントで足りる）。
- 失敗しても被害が小さい。`baked`（D1）があるので、後でこの API をやめても文書は生き残る。
- **拡張点が本当に機能するかを、最小の面積で実証できる。**

第二弾以降（`analysis` → `format` → `report`）は、G5 の実データが出てから、
要望が多い順に interface を足す。WIT ワールドへの interface 追加は非破壊なので、
既存プラグインは何も壊れない（6 章）。

#### 3.5.7 実装時の API 表面と、第51番との裁定

##### 第51番との裁定表（食い違いの解消）

初版の本書は第51番 3.7 を読まずに書かれており、いくつかの点で重複・矛盾していた。
**機構は第51番、表面は本書**という分界（3.0）に従い、次のとおり裁定する。

| 論点 | 初版の本書 | 第51番 3.7 | **裁定** |
|---|---|---|---|
| 実行時間の制限 | fuel（決定論のため必須と主張） | epoch 割り込み 250 ms / 30 s | **第51番を採用。** 決定論は D6（出力を焼き付け、再実行しない）で担保するので fuel は不要。epoch は wasm 側から回避不能という強みがある |
| `Capability` の語彙 | 独自の 7 種 | 6 種 | **第51番を採用。本書の 7 種は撤回**（D4）。本書は「6 種で 4 拡張点を覆える」ことを検算した |
| メモリ上限 | 64 MiB | 256 MiB | **第51番を採用**（`PluginLimits::DEFAULT`） |
| 型の置き場所 | `drill-plugin` | `drill-sandbox/src/plugin.rs`（feature = "plugins"） | **第51番を採用。** `drill-plugin` は表面だけを持ち、`drill_sandbox::plugin::*` を使う |
| 署名 | blake3 ハッシュ + ed25519 | Ed25519 over (wasm ‖ canonical manifest) | **第51番を採用。** blake3 は同一性の再確認用として補助的に併用してよい |
| 返り値 | `Vec<Edit>` を直接 | `Vec<Edit>` を複製に適用 → `validate_untrusted` → 全件破棄 | **検証は第51番を採用。** ただし ABI に出るのは `PluginEdit` であり、`Edit` への変換は `drill-plugin` が行う（D3）。第51番の記述は `drill-sandbox` 境界のものとして整合する |
| 決定論設定 | 言及なし | `wasm_nan_canonicalization(true)` | **第51番を採用。** 本書の D6 と合わせて二重化される |

##### ランタイム選定（第51番の決定の補強。本書は覆さない）

| 候補 | 判断 |
|---|---|
| **wasmtime（LTS 固定）** | **第51番の決定**。コンポーネントモデルの参照実装。`epoch_interruption` / `StoreLimits` / `max_wasm_stack` / `wasm_threads(false)` / `wasm_nan_canonicalization(true)` / `enable_compiler(false)` と必要な制御が揃う。LTS は 12 の倍数で 24 か月サポート、通常版は 2 か月。**LTS へ固定し通常版を追わない**（G2） |
| extism | 不採用。wasmtime を 4 メジャー遅れ（1.30.0 が `wasmtime ^43`、現行 47）で内包する中間層。`allowed_hosts` / `allowed_paths` / `max_pages` は良い設計だが、我々は**ファイルもネットワークも一切与えない**のでその価値を使わない |
| wasmi | 保留の次点。インタプリタなので JIT のコード生成面がゼロ、`no_std` 可。速度は数十分の一だが、第一弾（図形生成、1 回数千点）なら実用範囲。**JIT を持ちたくないという判断に倒れたら第一候補**。第一弾が `shape-generator` だけであることは、この選択肢を生かし続ける効果もある |
| wasmer | 不採用。コンポーネントモデル対応と統治の安定性で wasmtime に劣る |

##### WIT ワールド（第一弾）

```wit
package drillforge:plugin@0.1.0;

interface types {
  record point { x: f32, y: f32 }

  /// Numeric projection of GridConfig. No display strings, no HashKind —
  /// see D7. `hashes` carries only positions, in ascending order.
  record grid-info {
    width: f32,
    height: f32,
    horizontal-steps: u16,
    horizontal-units: f32,
    vertical-steps: u16,
    vertical-units: f32,
    major-line-interval: f32,
    hashes: list<f32>,
  }

  variant param-value { flag(bool), number(f64), text(string) }

  record param-spec {
    key: string,
    kind: param-kind,
    /// i18n key resolved by the host against the plugin's own catalog —
    /// never a display string (00-conventions.md 不変条件7).
    label-key: string,
    default-value: param-value,
    min: option<f64>,
    max: option<f64>,
  }
  enum param-kind { flag, number, text }
}

interface metadata {
  use types.{param-spec};
  record plugin-info {
    /// Must equal the ProviderId in the manifest; the host rejects a mismatch.
    provider-id: string,
    display-name-key: string,
    version: string,
    api-major: u16,
    api-minor: u16,
  }
  describe: func() -> plugin-info;
}

interface shape-generator {
  use types.{point, grid-info, param-value, param-spec};

  record shape-info { name: string, label-key: string, params: list<param-spec> }

  record shape-request {
    count: u32,
    grid: grid-info,
    /// Open-ended bag so new inputs are added without changing this record.
    /// Unknown keys must be ignored by the plugin (forward compatibility).
    params: list<tuple<string, param-value>>,
  }

  list-shapes: func() -> list<shape-info>;

  /// Must be pure: same request => same points, no clock, no randomness.
  /// Returning a count other than `req.count` is a host-side error.
  generate: func(name: string, req: shape-request) -> result<list<point>, string>;
}

world drill-plugin {
  export metadata;
  export shape-generator;
}
```

`label-key` が表示文字列でなく**キー**なのは意図的である。プラグインが日本語/英語の文面を
直接返すとロケール切替が壊れる（00-conventions.md 不変条件7）。プラグインは自分のカタログ
（コンポーネント内の `i18n/*.toml`）を同梱し、ホストが `Locale` に応じて解決する。

##### ホスト側の受け入れ関門（単一経路。C7）

```rust
/// The only path from plugin output to a Document. Atomic: the whole batch is
/// rejected, never partially applied.
pub fn accept(
    doc: &Document,
    keys: &KeyMap,
    edits: Vec<PluginEdit>,
    provider: &ProviderId,
) -> Result<Edit, DrillError> {
    // 1. edits.len() <= MAX_PLUGIN_EDITS, and every Vec inside is capped.
    // 2. keys.to_edits(doc, edits)? — unknown handle => Err (D3).
    // 3. apply to a clone; run clone.validate_untrusted(&Limits::DERIVED)?  (第51番 3.7).
    // 4. On success return one `Edit::Batch` labelled with `provider`
    //    so the history records who made the change (STRIDE の R).
}
```

`drill-core` はプラグインの存在を一切知らない。`Document` へは通常の `Edit` として入る
（00-conventions.md 不変条件1）。

### 3.6 スクリプティング

**判断: 独立したスクリプト言語は P0〜P2 で入れない。** 代わりに2つで代替する。

1. **マクロ（記録・再生）。** 第10番の `Edit` は 40 バリアントのコマンド列だが、
   **`Serialize` を持たない**（`docs/design/10-document-model.md:778`、`#[derive(Clone, Debug, PartialEq)]` のみ）。
   これは意図的な設計であり、マクロのために `Edit` に serde を足してはならない
   — 足した瞬間に `Edit` の内部表現が永続化契約になり、D3 と同じ罠に落ちる。
   マクロは `MacroStep` という**独立した直列化可能な語彙**を持ち、`Edit` へ写像する。
   `PluginEdit`（D3）と同じ構造であり、実装も共有できる（`MacroStep` を `PluginEdit` の
   相対版として定義するのが自然だが、確定は 9 章の未決事項）。
   選択に**相対的**な形（絶対 `PerformerId` ではなく「現在の選択の i 番目」、
   絶対 `SetId` ではなく「現在のセットから +n」）で記録すれば、別の選択・別のセットへ再生できる。

   ```rust
   /// A recorded edit sequence, re-targetable to a different selection/set.
   #[derive(Clone, Debug, Serialize, Deserialize)]
   pub struct Macro {
       pub name: String,
       pub steps: Vec<MacroStep>,
   }

   #[derive(Clone, Debug, Serialize, Deserialize)]
   pub enum MacroTarget {
       SelectionIndex(u32),
       WholeSelection,
       CurrentSetPlus(i32),
   }

   /// Bind a macro to a concrete selection and set, producing real edits.
   pub fn expand_macro(m: &Macro, doc: &Document, sel: &Selection, set: SetId)
       -> Result<Vec<Edit>, DrillError>;
   ```

   これは新しい言語もサンドボックスも要らず、Undo も既存の仕組みで動き、決定論も自動的に守られる。
   「同じ加工を全セットに」「この整列手順を毎回」という実際の要望の大半をここで吸収できる。

2. **CLI**（3.5.3 の (2)）。バッチはシェル・Python・PowerShell から呼べばよい。

**もし将来テキスト言語が必要になったら: `rhai`。** 判断根拠を先に固定しておく。

- `rhai` 1.25.1（MIT/Apache-2.0、直近 328万 DL）。純 Rust で C の FFI が無い。
  `Engine` に `set_max_operations` / `set_max_call_levels` / `set_max_expr_depths` /
  `set_max_string_size` / `set_max_array_size` / `set_max_map_size` / `set_max_variables` /
  `set_max_functions` / `set_max_modules` / `on_progress`（中断コールバック）が揃っており、
  「Don't Panic」保証を掲げている。ホストが関数を登録しない限り
  ファイル・ネットワークに触る手段が無い（＝我々の脅威モデルに合う）。
- `mlua` 0.12 は不採用。C の Lua への FFI であり `unsafe` の量が桁違いで、
  「信頼できない入力をパニック無しで扱う」という 00-conventions.md の要求と噛み合わない。
- ただし **既定は「入れない」**。マクロと CLI で足りなかったという証拠が出るまで着手しない。

### 3.7 乗り換え支援の導線

3.7.1〜3.7.3 は 8章（実装タスク）ではなく設計として先に決めておく。

#### 3.7.1 移行ウィザードの構成

「1画面で全部」ではなく、**取り込み単位ごとに独立して完了できる**構成にする。
途中で失敗しても、そこまでの成果が残る。

```
[1] 座標を入れる      → CSV / Excel / テキスト  （必須。ここだけで最低限使える）
[2] 名簿を整える      → 同じ CSV のセクション列 / 別ファイル / 手動
[3] 音楽の時間軸を入れる → MusicXML / MIDI / 手入力 BPM
[4] 音源を入れる       → WAV/MP3 + 同期アンカー1点
[5] 検算する          → 元の座標表と突き合わせる（3.7.2）
[6] 手作業リストを見る  → 何が入らなかったか（3.7.3）
```

#### 3.7.2 検算画面 — 乗り換えの信頼はここで決まる

インポートが「たぶん合っている」では、指導者は本番で使わない。
取り込んだ `Document` から**もう一度読み上げ文字列を生成し、元ファイルの文字列列と1件ずつ突き合わせる**。

```rust
#[derive(Clone, Debug)]
pub struct DiffRow {
    pub line: u64,
    pub performer: String,
    pub set: String,
    pub source_text: String,
    pub imported_text: String,
}

/// Re-render every imported dot through `coordinates::readable` and compare it
/// with the source file's own text column. Returns only mismatches, capped.
/// This is the single most persuasive artifact in the whole migration flow:
/// "64,000 dots imported, 0 mismatches against your original sheet."
pub fn verify_against_source(
    bytes: &[u8],
    plan: &ImportPlan,
    imported: &Document,
    notation: CoordinateNotation,
    locale: Locale,
    max_rows: usize,
) -> Result<Vec<DiffRow>, DrillError>;
```

元ファイルにテキスト座標列が無い（x/y 数値だけ）場合は、
「数値の往復（x,y → 我々の Point → x,y）の最大誤差」を代わりに提示する。

#### 3.7.3 何が自動で入り、何を手でやるか（そのまま UI とドキュメントに出す表）

| 持ち込むもの | 自動 | 手作業 | 目安 |
|---|---|---|---|
| 全セット・全演者の座標 | CSV/Excel インポータ（列マッピング） | 元製品で CSV 書き出し | 15分 |
| セット名・カウント数 | 同 CSV の列 | — | 0分 |
| 演者名簿・ドリルナンバー | 同 CSV の列 | — | 0分 |
| セクション（パート） | 名前一致 + プリセット照合（3.2.9） | 対応が付かなかった名前だけ確認 | 10分 |
| テンポ・拍子・リハーサル記号 | MusicXML / MIDI | 譜面ソフトか DAW から書き出し | 10分 |
| 音源 | ファイル読込 | 同期アンカー1点を合わせる | 5分 |
| フォーメーションの下敷き画像 | 画像読込 | 位置合わせ | 5分 |
| **経路・ゲート・ホールド** | ✗（全て直線遷移になる） | 必要な箇所だけ指定し直す | 可変 |
| **マーチングスタイル・歩幅設定** | ✗ | ショー単位で1回設定 | 5分 |
| **カメラワーク・演出** | ✗ | 作り直し | 可変 |
| **印刷物の版面意匠** | ✗ | テンプレートを選ぶ | 5分 |
| **演者シンボル・装備・プロップ** | ✗ | 必要なら再設定 | 可変 |

この表を「できないことリスト」として**購入前に見せる**。
乗り換えの失敗は「入ると思っていたものが入らなかった」で起きるのであって、
「入らないと分かっていたものが入らなかった」では起きない。

#### 3.7.4 ロックインしないことを製品の約束にする

- DrillForge は**常に全データを CSV と JSON で書き出せる**。この機能を有料版の制限対象にしない。
- 書式は公開文書として維持する（3.4.2 の凍結列名、`Document` の JSON スキーマ）。
- 「出ていくのが簡単」であることは、入ってくる判断の障壁を下げる。倫理であると同時に営業上の武器である。

#### 3.7.5 元製品からのデータ書き出し案内

アプリ内ヘルプに「他製品から座標 CSV を書き出す一般的な手順」を置く。
その際に守ること（1.1節の境界の運用）:

- 他社の商標を機能名として使わない。ヘルプ本文で「◯◯という製品をお使いの場合」と
  事実として言及するのは可（記述的用法）。
- 他社の画面キャプチャ・アイコン・UI 文言を転載しない。手順は文章で書く。
- 「独自形式は読みません」を**能力の欠如ではなく設計判断として**書く。

### 3.8 API の安定性保証 — 何を約束し、何を約束しないか

全面オープンソース（1.0 節）では**ソースが読める**ため、
「約束していないものにも依存される」危険が閉じた製品より高い。
だからこそ、**約束の範囲を狭く明示する**ことが重要になる。

#### 3.8.1 約束する4つの表面と、それぞれの版管理

| # | 表面 | 版の持ち方 | 互換規則 |
|---|---|---|---|
| A | **ファイル形式** — `Document` の JSON（`schema_version`）、`.drillproj`、座標 CSV 方言 | `schema_version: u16`（第41番が所有）。CSV は**版番号を持たず、列名の追加のみ**（3.4.2） | 読み手は未知のオブジェクトキーを無視する（`deny_unknown_fields` を使わない）。未知の**メジャー版**は拒否。未知の `ProviderId` は拒否しない（不変条件21） |
| B | **CLI** | 実行ファイルの semver。機械可読出力は `{"api": 1, ...}` を先頭に持つ | フラグは追加のみ。`--json` のキーは追加のみ。削除は `api` のメジャー更新 |
| C | **プラグイン WIT** | WIT パッケージの semver（`drillforge:plugin@0.1.0`） | 3.8.2 |
| D | **`.drillmap.toml`**（列マッピング）と将来のマクロ | serde の `#[serde(default)]` による寛容な読み込み | フィールドは追加のみ |

#### 3.8.2 **約束しないもの — Rust のライブラリ API**

**`drill-core` / `drill-interop` / `drill-render` / `drill-export` / `drill-sandbox` / `drill-plugin` の
Rust API は公開契約ではない。** semver を守らず、0.x のまま自由にリファクタする。

これは本書の安定性設計で**最も重要な一項**である。理由:

- 4 つの表面（A〜D）を守るために必要なのは「外から見える振る舞い」だけであり、
  内部の型を凍結する必要は無い。
- `DESIGN_GAPS.md` の Wave 群、第10番の `Edit` 40 バリアント、第15番の `Performer` 拡張、
  第16番の `GridLine` 再設計は、いずれも内部 API の**破壊的変更**である。
  ライブラリ API を約束したら、これらが全部できなくなる。
- OSS だと `crates.io` に publish したくなるが、**publish しない**（あるいは
  `publish = false` を明示する）ことでこの約束の不在を機械的に示す。

`README` と各クレートの `lib.rs` の doc comment に明記する:
「このクレートの Rust API は DrillForge 内部専用であり、予告なく変更されます。
外部から使うなら CLI（B）かファイル形式（A）を使ってください。」

#### 3.8.3 非破壊的拡張の余地を、設計時点で確保しておく

「後から足せる形」を型のレベルで用意しておく。これは 3.5.4 の D1 と同じ思想の一般化である。

| 場所 | 拡張の余地 | 理由 |
|---|---|---|
| CSV | 列名の**追記**（3.4.2）。読み手は列名で写像し位置に依存しない | 位置依存にすると追記が破壊的になる |
| JSON | 全フィールドに `#[serde(default)]`（第15番 3.3 が既に採用）。`deny_unknown_fields` を使わない | 新旧どちらの向きにも読める |
| `ShapeSpec` | `External` バリアント（D1） | 閉じた列挙に後からバリアントを足すと旧読み手が壊れる |
| WIT の入力 | `params: list<tuple<string, param-value>>` という**開いた袋** | WIT の `record` はフィールド追加ができない。袋なら追加が非破壊 |
| WIT のワールド | 拡張点の追加は**新しい interface のエクスポート**として行う | 古いプラグインはそれをエクスポートしないだけで、壊れない |
| `Capability` | **増やさない**（D4） | 増やすと同意 UI の意味が変わり、既存の同意が無効になる |
| エラー型 | `DrillError` は `#[non_exhaustive]`（第42番へ要求） | variant 追加を非破壊にする |

WIT の版の運用規則:

- **パッチ**（`0.1.0` → `0.1.1`）: ドキュメントのみ。
- **マイナー**（`0.1.x` → `0.2.0`）: interface の追加、`params` の新キー、
  `enum` への variant 追加は**行わない**（WIT の enum は追加も破壊的）。
  新しい選択肢が要るなら `param-value::text` で表現し、未知値は無視させる。
- **メジャー**（`0.x` → `1.0` → `2.0`）: record のフィールド変更、関数シグネチャの変更。
  ホストは **N と N−1 のメジャー**を同時に受け入れる。それより古いものはロードを拒否し、
  「このプラグインは DrillForge x.y 以降に対応していません」と表示する。
- プラグインは `metadata.describe()` で `api-major` / `api-minor` を申告し、
  ホストが受理可否を決める。**申告と実際のエクスポートが食い違う場合は instantiation で失敗する**
  （コンポーネントモデルが型で検査する）ので、申告は嘘をつけない。

#### 3.8.4 フィーチャフラグと「落とせるビルド」

3.0 のフィーチャ定義は、機能の取捨だけでなく**約束の範囲を絞る手段**でもある。

- 出荷ビルドの既定は `csv-io` + `musicxml` + `midi`、**`plugins` は無効**。
- 脆弱性が出た形式は、その `feature` を落としたビルドを即日出せる（6.5）。
- `plugins` が無効なビルドでも、Wave 0.5 で作った**レジストリと `ExternalShape` は常に有効**である。
  でなければ「プラグイン無しビルドで文書が開けない」が起きる（不変条件14）。
  すなわち **`plugins` フィーチャは「WASM を実行できるか」だけを切り替え、
  「外部拡張の永続化を理解できるか」は切り替えない。** この分離を守ること。

#### 3.8.5 廃止（deprecation）の手順

- 形式・フラグ・WIT の要素を廃止するときは、まず**警告のみ**の版を出す（読めるが「非推奨」と表示）。
- 廃止告知から**2 マイナー版以上**は受理を続ける。削除はメジャー版でのみ行う。
- ファイル形式の削除は原則行わない。読めなくなった過去のファイルは復旧できないため、
  **読み取りは永続的に維持し、書き出しだけをやめる**（例: CSV の旧列を書かなくなっても読み続ける）。

## 4. 不変条件

テストで検証できる形で書く（対応するテストは7章）。

1. **インポータは `Document` を直接変更しない。** 既存文書へのマージは必ず `Vec<Edit>` を返す
   （00-conventions.md 不変条件1）。新規作成のみ `Document` を返してよい。
2. **どのインポータもパニックしない。** 任意のバイト列に対して `Err` を返すか成功するかのいずれか。
   `unwrap` / `expect` / 添字パニック / 整数オーバーフローを経路上に持たない（fuzz で検証）。
3. **収支が合う。** `report.rows_read == report.rows_accepted + report.rows_skipped`。
   落ちた行は必ず数に含まれる（詳細が `max_reported_rows` で切られても総数は正確）。
4. **上限は消費前に効く。** `max_bytes` / `max_rows` / `max_columns` / `max_field_bytes` は
   該当リソースを確保する**前**に判定される。上限超過は部分的な `Document` を作らない。
5. **自形式の往復が恒等。** `write_coordinates_csv(doc, CsvColumns::Full)` の出力を
   `import_tabular_as_document` に通すと、演者数・セット数・カウント・全座標が
   `RoundedStep` 精度の範囲で元の `doc` と一致する。
6. **座標の算術は core にしかない。** `drill-interop` は
   `(pos - line) / step` 相当の式を持たない。単位変換・読み上げ・パースは全て
   `drill_core::coordinates` の `measure` / `parse_*` / `AxisConvention::to_point` を経由する
   （第16番 不変条件1 のクレート跨ぎ版）。
7. **`drill-core` は `drill-interop` を知らない。** 依存の向きは一方向。
   `drill-core/Cargo.toml` の依存は serde/serde_json のままである。
8. **決定論。** 同じバイト列 + 同じ `ImportPlan` からは常にバイト単位で同じ `Document::to_json()` が出る。
   反復順が実行ごとに変わる `HashMap`/`HashSet` を蓄積器に使わない（`BTreeMap`/`BTreeSet` のみ）。
9. **CSV のヘッダ名は追加のみ。** 凍結列名の改名・削除・並べ替え・意味変更を行わない。
   読む側は列位置に依存しない。
10. **非有限値が `Document` に入らない。** `NaN` / `Inf` を生む行は `SkipReason::NonFinite` で落ちる。
11. **元ファイルを書き換えない。** 全インポータは読み取り専用でファイルを開く。
    インポート失敗が入力を破壊しない。
12. **プラグインは ambient authority を持たない。**（実装した場合）
    ホストは WASI をリンクせず、ファイル・ネットワーク・時刻・乱数・スレッドをエクスポートしない。
    プラグインの返り値は `drill_plugin::accept` を通り、複製への適用と
    `validate_untrusted(&Limits::DERIVED)` に通った場合のみ、原子的に適用される（第51番 3.7）。
13. **`not_imported` は常に非空。** どのインポータも「構造的に運べないもの」を1つ以上必ず報告する。
    「全部入りました」と言わないこと自体を不変条件にする。

### 4.1 「閉ざさない」ための不変条件（3.5.5 の C1〜C10 に対応）

**プラグインを実装するより前に成立させ、以後ずっと維持する。** 検証方法は 3.5.5 の表と 7.7。

14. **プロバイダ不在で描画が変わらない。**（C1）`ExternalShape` を含む文書は、
    レジストリが空でも開け、`DisplayList` が**バイト単位で一致**する。`baked` が幾何の正であり、
    プロバイダはパラメータ編集後の再導出にしか使われない。
15. **迂回路が無い。**（C2）4 つの拡張点（S1〜S4）はレジストリを経由する唯一の経路を持ち、
    組み込み実装も同じ経路を通る。レジストリから組み込みを外せば組み込み機能も引けなくなる。
16. **`Edit` は ABI に現れない。**（C3）プラグイン境界を越える編集語彙は `PluginEdit` のみ。
    `Edit` に `Serialize` を足さない。ID は不透明ハンドル（`SetKey` / `PerformerKey` / `SectionKey`）で渡し、
    未知のハンドルは `Err` であって宙吊り参照ではない。
17. **能力語彙は 6 種で固定。**（C4）`drill_sandbox::plugin::Capability` のバリアント数は 6。
    拡張点の追加は WIT の interface 追加で行い、能力を増やさない。
18. **プラグインは編集時にしか動かない。**（C5・D6）読み込み・`DisplayList` 構築・
    SVG/PDF/動画書き出しの経路からプロバイダを呼ばない。
    したがって epoch 締め切りの非決定性が出力へ伝播しない。
19. **`drill-core` は wasmtime を知らない。**（C6）`cargo tree -p drill-core` の依存は
    serde / serde_json のみ。プラグイン機構は `drill-sandbox`（feature = "plugins"）にのみ存在する。
20. **単一の関門。**（C7）プラグイン由来の変更が `Document` に届く経路は
    `KeyMap::to_edits` → 第51番の検証パイプライン → `Edit::Batch` の 1 本だけである。
21. **未知の provider を拒否しない。**（C8）`validate()` は `ProviderId` の**文法**のみを検査し、
    実在するプロバイダかどうかは問わない。未知は「編集不可・表示可」であって「破損」ではない。
22. **写像であって再輸出でない。**（C9）WIT の値型と `drill-core` の型の変換は
    `drill-plugin/src/project.rs` にのみ存在する。`drill-core` の型に ABI 都合の属性を足さない。
23. **v2 で凍結した外部拡張の表現を変えない。**（C10）`ShapeSpec::External` / `ExternalShape` /
    `ParamValue` / `ProviderId` の serde 表現は、スキーマのメジャー版が上がるまで不変。

## 5. 性能

基準規模: 演者1,000人 / セット64 / 総カウント2,048 → 表形式インポートは **64,000 行**。

### 5.1 16.6ms フレーム予算の取り分

**インポート・エクスポートは 16.6ms 予算のうち 0ms を使う。** これが本設計の性能上の基本方針である。

- `run_import` / `write_*` は `DESIGN_GAPS.md` B-3 の `Job` 上で動く。
  UI スレッドは毎フレーム `poll()` するだけで、その分は B-3 の予算に含まれる。
- 例外は `sniff` のみ。先頭 64 KiB・最大50行に固定してあるので**目標 2ms 未満**。
  ファイル選択直後の1フレームだけ発生し、毎フレームではない。16.6ms 予算の 2ms を借りる。
  ファイル全体をメモリに読む I/O 自体はワーカーで行い、`sniff` には `&[u8]` の先頭だけを渡す。

### 5.2 見積り

| 処理 | 計算量 | 基準規模での見積り |
|---|---|---|
| `sniff` | O(64 KiB) | < 2ms（`chardetng` は 64 KiB を1パス） |
| CSV パース | O(bytes) | 64,000行 × 15列 ≒ 8 MB。`csv` クレートは 100〜200 MB/s → 40〜80ms |
| 文字コード変換 | O(bytes) | `encoding_rs` は SIMD 実装。8 MB で 10〜20ms |
| 数値 x/y 経路 | O(rows) | 128,000 回の `f64` パース ≒ 10ms |
| **テキスト座標経路** | O(rows × 文字数) | 128,000 回の `parse_lateral`/`parse_depth`。1回 1〜2µs 見込み → **130〜260ms**。ここが支配項 |
| 蓄積 `BTreeMap<(u32,u32), Point>` | O(n log n) | 64,000 挿入 ≒ 15ms、常駐 64,000 × 約48B ≒ **3 MB** |
| `Document` 構築 | O(performers × sets) | 64 × `Vec<Point>`(1,000) = 64,000 × 8B = **512 KB** |
| **合計（テキスト座標込み）** | | **300〜400ms**（ワーカー上） |
| **合計（x/y 数値のみ）** | | **80〜130ms** |

進捗は 1,000 行ごとに `progress(n)` を呼ぶ（64回/インポート）。
`egui::Context::request_repaint` の呼び出し頻度としても妥当で、進捗更新のオーバーヘッドは無視できる。

**メモリ:** 入力バイト列（最大 256 MiB 上限だが実際は 8 MB）+ 蓄積 3 MB + 出力 512 KB。
1行ごとの確保はゼロにする — `csv::ByteRecord` と デコード先 `String` を使い回す。
`SkippedRow` は 500 件で打ち止め（500 × 約200B = 100 KB）。

### 5.3 MusicXML / MIDI

- MIDI: 数百 KB。`midly` はゼロコピー借用パース。**< 10ms**。
- MusicXML: 上限 32 MiB。`roxmltree` はツリーを作るので、
  `nodes_limit = 500_000` で最悪 500,000 ノード × 約48B ≒ **24 MB** に抑える。
  典型的な吹奏楽譜（200小節・20パート）で 100,000〜300,000 ノード、**50〜150ms**。
  `.mxl` の展開は 64 MiB 上限。ワーカー上で実行。

### 5.4 エクスポート

- `write_coordinates_csv`（Full、64,000行）: 64,000 回の `measure` + 文字列生成。
  第16番 5章の見積り（64,000 × 約200ns = 13ms）+ フォーマットと I/O で **50〜80ms**。ワーカー上。
- `write_click_midi`: 2,048 カウント × 2イベント = 4,096 イベント。**< 5ms**。UI スレッドで実行してよい。

### 5.5 拡張点レジストリ（P0/P1 で実在する部分）

3.5.5 の C2 は「組み込みもレジストリを通る」を要求する。その追加コストを評価する。

| 拡張点 | 呼び出し頻度 | 追加コスト | 16.6ms 予算への影響 |
|---|---|---|---|
| S1 図形生成 | 利用者がシェイプを適用・パラメータを動かしたとき。連続ドラッグ中で最大 60 回/秒 | `BTreeMap` 検索 1 回（キーは `(ProviderId, String)`、要素 17 個で比較 4〜5 回）+ 仮想呼び出し 1 回。**100ns 未満** | 実質ゼロ。1,000 点の `sample` 自体（数 µs）に埋もれる |
| S2 解析 | `Job` 上。1 回/編集 | 同上、1 回のみ | ゼロ（UI スレッド外） |
| S3 入出力 | ファイル操作時。1 回/操作 | 同上 | ゼロ |
| S4 帳票 | 印刷・書き出し時。1 回/操作 | 同上 | ゼロ |

**レジストリ検索を毎フレーム行わない。** シェイプのドラッグ中は
「ドラッグ開始時に `&dyn ShapeProvider` を 1 回引いて保持し、ドラッグ終了まで使い回す」
（`Registry::get` の返り値のライフタイムは `&self` に縛られるので、レジストリを不変借用したまま保持できる）。
この規約により、C2 を満たしつつ 00-conventions.md 不変条件3（毎フレームのヒープ確保ゼロ）も守れる。

メモリ: 4 レジストリ合計で `Box<dyn _>` が 17 + 2 + 3 + 数個 ≒ 25 エントリ。
キー文字列を含めて **数 KB**。無視できる。

### 5.6 プラグイン実行（P2、実装した場合）

- 第51番 3.7 の `PluginLimits::DEFAULT` に従う: メモリ 256 MiB、
  対話 250 ms / ジョブ 30 s の epoch 締め切り、出力 64 MiB。
- **UI フレームに載せない。** 対話締め切りが 250 ms である以上、
  16.6ms 予算に収まる保証は無い。呼び出しは `Job`（第40番）に載せ、
  ドラッグ中のライブプレビューは「前回の `baked` を表示し、確定時に再導出」にする。
  これは D6（焼き付け）と同じ規約から自然に出てくる。
- インスタンス化コストを避けるため、コンパイル済み `Component` はプロセス内でキャッシュし、
  呼び出しごとに `Store` だけを作り直す（`Store` は軽く、状態の分離になる）。
- epoch ticker は第51番 3.7 のとおりスレッド 1 本・1 ms 周期。第51番 5 章が
  「CPU 使用率は測定限界以下」と見積もっている。

## 6. 失敗モードと安全性

**全てのインポート元は敵性入力として扱う。** 他人から受け取った CSV・譜面・プラグインが
悪意を持って作られている前提で設計する（00-conventions.md「信頼できない入力」）。

### 6.1 表形式

| 壊れ方 | 対処 |
|---|---|
| 巨大ファイル（10 GB） | `max_bytes` で読む前に拒否。ファイルサイズはメタデータで確認し、全読み込みしない |
| 行数爆発（10億行） | `max_rows`。ストリーミングなので到達時点で打ち切り、`Err`（部分文書を作らない） |
| 列数爆発（1行に100万列） | `max_columns`。`csv` の `ByteRecord` は伸びるので、レコードごとに `len()` を検査 |
| 閉じないクォート → 1フィールドがファイル全体 | `max_field_bytes`（4 KiB）。`csv::ReaderBuilder` に上限を設定し、超えたら `SkipReason::FieldTooLong` |
| 不正 UTF-8 / 文字コード誤判定 | U+FFFD 置換で継続 + `ImportWarning`。**プレビューで必ず利用者に見せる**（CP932 の文字化けは実行時に検出不能で、目視だけが最終防衛線） |
| BOM / CRLF / CR 混在 | `sniff` で判定。混在は最初に現れたもので統一し警告 |
| NaN / Inf / `1e400` | `SkipReason::NonFinite`。`Point` に書かない |
| フィールド外の座標 | `SkipReason::OutOfField`。グリッド寸法の ±50% を許容範囲とし、外は落とす |
| 座標系の取り違え（全点が一様にずれる） | `ImportWarning::SuspiciousOrigin`（3.2.8）。落とさず警告し、検算画面（3.7.2）へ誘導 |
| `(performer, set)` の重複 | `DuplicatePolicy`。既定 `Reject` |
| 演者ごとにセット数が違う（ragged） | `RaggedPolicy`。既定 `Reject`。`HoldPrevious` は全件を警告に載せる |
| 上限超の演者数・セット数 | `SkipReason::PerformerLimit` / `SetLimit`。到達時点で `Err` |
| 巨大なラベル文字列 | `max_field_bytes` で切られる。加えて `Performer.label` は 64 バイトで切り詰め + 警告 |
| ゼロ幅文字・双方向制御文字（U+202E 等）をラベルに仕込む | 表示の詐称に使われる。`label` / `Section.name` から Cf カテゴリの制御文字を除去し、警告 |
| CSV インジェクション（`=cmd|...`）を書き出し側で作る | **書き出し側の問題**。`write_coordinates_csv` は `=` `+` `-` `@` `\t` `\r` で始まるフィールドの前に `'` を付ける（Excel の式実行を防ぐ）。`CsvProfile` で無効化できるが既定は有効 |

### 6.2 MusicXML / MIDI / ZIP

| 壊れ方 | 対処 |
|---|---|
| XML entity 爆弾（billion laughs） | `roxmltree::ParsingOptions { allow_dtd: false, .. }`（既定値。明示して意図を残す）。加えて roxmltree 自身が billion laughs 対策を持つ |
| 外部実体参照（XXE、ローカルファイル読み出し） | `entity_resolver: None`。`allow_dtd: false` で DTD ごと拒否 |
| 深いネスト / ノード数爆発 | `nodes_limit = 500_000` |
| zip bomb（1 KB → 10 GB） | エントリ数 ≤ 64、展開後合計 ≤ 64 MiB。宣言サイズを信じず `Read::take` で実読も打ち切る |
| `.mxl` のパス・トラバーサル（`../../.ssh/`） | **そもそもファイルへ展開しない**（メモリ上で1エントリだけ読む）。加えて名前検査で `..`・絶対パス・ドライブレターを拒否 |
| ZIP のシンボリックリンクエントリ | 展開しないので無害。名前検査でも弾く |
| `divisions == 0` / `beat-type == 0` | 直前の値を保持、無ければ 1 として続行 + 警告。ゼロ除算をしない |
| MIDI の SMPTE タイミング | `Err(DrillError::UnsupportedMidiTiming)`。理由と代替手段を提示 |
| MIDI の tempo = 0 μs/quarter | そのイベントをスキップ + 警告（`TempoMap::sanitize_bpm` にも二重の防御がある） |
| MIDI delta の累積オーバーフロー | `u64` + `saturating_add` |
| 小節数・記号数の爆発 | `max_measures` / `max_marks` で打ち切り + `Truncated` 警告（`Err` にはしない。テンポだけでも取れた方が価値がある） |
| 記号テキストの文字コード（Latin-1 / UTF-8 / CP932 混在） | UTF-8 → CP932 → Latin-1 の順に試行。256 バイトで切り詰め |

### 6.3 部分的な結果の正直な提示

これは安全性の問題である。**間違った座標が「取り込めた」ことになる方が、
取り込めなかったことより危険**であり、印刷物と本番の隊形が食い違う。

- `report.rows_skipped > 0` のとき、結果ダイアログは「OK」1つでは閉じられない。
  少なくとも「詳細を見る」か「レポートを保存」を経由させる。
- `report.not_imported` は常に表示する（空にならない。不変条件13）。
- レポートはテキストファイルとして保存でき、行番号・理由・元の抜粋を含む。
- 検算画面（3.7.2）への導線を結果画面に常設する。
- **警告を「後で見る」に押し込まない。** インポートは1回きりの操作であり、
  そこで見なかった警告は永久に見られない。

### 6.4 プラグイン（P2、実装した場合）

機構側の失敗モード（無限ループ・メモリ爆弾・スタック・署名・同意）は
**第51番 3.7 と 6 章（F18 / F19）が正**であり、ここでは重複させない。
本書が担当するのは**表面側**、すなわち「サンドボックスは無傷だが、
API の使われ方として壊れている」ケースである。

| 壊れ方 | 対処 | 担当 |
|---|---|---|
| 無限ループ / メモリ爆弾 / 深い再帰 | epoch 締め切り（250 ms / 30 s）・`StoreLimits`（256 MiB）・`max_wasm_stack` | 第51番 |
| 不正な `Edit` を返す | 複製に適用 → `validate_untrusted(&Limits::DERIVED)` → 不合格なら全件破棄 | 第51番（F19） |
| 署名・改竄・能力の後付け拡大 | Ed25519 署名、同意ダイアログ、自動更新しない | 第51番 |
| **`generate` が `req.count` と違う個数の点を返す** | ホスト側で長さを検査し `Err`。`Set.positions.len() == performers.len()`（V4）を壊させない | 本書 |
| **巨大な返り値**（10 億点） | デコード**中**に `MAX_PLUGIN_POINTS` / `MAX_PLUGIN_EDITS` で打ち切って `Err`。デコード後に検査しない | 本書 |
| **未知の不透明ハンドル**を返す（他人のセットを触ろうとする） | `KeyMap::to_edits` が範囲外を検出して `Err`。ハンドルは呼び出しごとに配られた範囲でしか有効でない（D3） | 本書 |
| **`ProviderId` 詐称**（マニフェストと `describe()` の不一致、他社 ID の名乗り） | ロード時に照合し不一致は拒否。署名済みプラグインは ID を鍵に紐付ける | 本書 |
| **組み込みの影踏み**（`ProviderId::BUILTIN` を名乗る／同名で上書き） | `BUILTIN` は予約語で拒否。同キー登録は `Registry::register` が旧エントリを返すので、UI が「◯◯が組み込みの△△を置き換えます」と警告する | 本書 |
| **`baked` と `params` の不整合**（保存されたシェイプを別プラグインが違う形に再導出する） | `ExternalShape.api_major` が一致しないプロバイダには再導出させない。`baked` を保持し「このシェイプは別バージョンで作られています」と表示 | 本書 |
| **プラグイン非決定性**（同じ入力で違う点を返す） | 時刻・乱数の取得手段が無い（WASI 不供給）+ `wasm_nan_canonicalization(true)`。加えて D6 により**そもそも再実行しない**ので、揺れが出力へ伝播しない | 第51番 + 本書 |
| **プラグイン削除後に文書が開けない** | D1 の `baked` により開ける。編集だけが無効化され、「◯◯を入れると編集できます」と案内する | 本書 |
| ホストのクラッシュ伝播 | trap は `Err` に変換され、`drill-app` は状態を変えずに継続 | 第51番 |
| 悪意ある帳票テンプレート（HTML/JS） | 出力はスクリプト・外部参照を無効化したビューでのみ描画。`<script>` / `on*` 属性 / 外部 URL を除去。**そもそも第一弾には含めない**（3.5.6） | 本書 + 第17番 |

### 6.5 依存クレートそのもののリスク

- 各インポータは Cargo フィーチャで**外せる**（3.0）。脆弱性が出たときに
  「その形式のサポートを落としたビルドを即出す」が選択肢になることを設計に組み込む。
- `cargo-deny` を CI に入れ、`RUSTSEC` の advisory と license を監視する（第50番と協調）。
- `drill-interop` の全パーサ入口を `cargo-fuzz` の対象にする（7章）。
- `unsafe` を `drill-interop` / `drill-plugin` の自前コードで書かない
  （`#![forbid(unsafe_code)]` をクレート属性に置く）。

## 7. テスト計画

### 7.1 単体・ゴールデン

- `sniff`: BOM 4種 / 区切り3種 / CRLF・LF・CR / ヘッダ有無 のフィクスチャで
  `TabularPreview` の全フィールドを assert。
- `guess_mapping`: 日本語ヘッダ・英語ヘッダ・ヘッダ無し・自形式ヘッダの4本で期待する `roles` を assert。
  **自形式ヘッダは辞書を飛ばして自形式プロファイルになる**ことを明示的に検証。
- `AxisConvention::to_point`: 4つの `Origin` × 2つの `DepthDirection` × 4つの `LengthUnit` の
  代表値をテーブル駆動で検証。非有限入力が `None` を返すこと。
- `CountUnit::quarters` と BPM 換算: 6/8 で「四分音符=180」→ カウント 120 になること。
- `import_midi`: 手組みの SMF バイト列（3テンポ変化 + 拍子変化 + マーカー2件）で
  `MusicalTimeline` を完全一致 assert。SMPTE タイミングが `Err` になること。tick=0 が `Err` になること。
- `import_musicxml`: 4/4 → 3/4 の拍子変化、リハーサル記号 A/B/C、弱起小節を含む
  手書き MusicXML（生 XML と `.mxl` の両方）で `MusicalTimeline` を assert。
- `write_click_midi`: 出力バイト列のゴールデン比較（決定論の直接検証）。
  `midly` で読み戻して tick が整数であることを assert。
- 座標 CSV のヘッダ: `lines[0]` の**完全一致**をゴールデンとして固定
  （凍結列名の変更を CI で機械的に止める。不変条件9）。
  既存 `coordinates.rs:302-313` のテストは `starts_with("performer,label,set,counts,x,y")` に緩める。

### 7.2 往復（不変条件5）

```
Document::demo(32, 32)                     // 1,024 performers
  → write_coordinates_csv(Full, Ja)
  → sniff → guess_mapping → plan_import → import_tabular_as_document
  → 演者数・セット数・counts・全座標が RoundedStep 精度で一致
```

同じことを `Locale::En` でも、`CoordinateNotation::front_hash_anchored()` でも回す。
さらに**テキスト列だけを使うマッピング**（`x`/`y` 列を `Ignore` にし、
`side_to_side`/`front_to_back` を `LateralText`/`DepthText` にする）でも往復すること
— これが第16番の `parse_lateral`/`parse_depth` の受け入れテストを兼ねる。

### 7.3 property / テーブル駆動

`drill-core` は依存を serde のみに保つ規約があるが、`drill-interop` は
`[dev-dependencies]` に `proptest` を入れてよい（本体依存には入らない）。

- 任意の `Vec<Point>`（フィールド内、有限）→ CSV → import → 同一。
- 任意のバイト列 → `sniff` がパニックしない。
- 任意の `(u8, Vec<Vec<String>>)` → CSV 生成 → `sniff` の区切り推定が生成時の区切りを当てる
  （フィールドに区切り文字を含まない場合）。

### 7.4 fuzz（`cargo-fuzz`、第50番の CI と協調）

ターゲット: `fuzz_sniff` / `fuzz_import_csv` / `fuzz_import_midi` / `fuzz_import_musicxml` /
`fuzz_import_mxl` / `fuzz_parse_coordinate`（第16番と共有）。
不変条件: **パニックしない**、**指定した上限内のメモリ・時間で終わる**。
`libfuzzer` の `-rss_limit_mb` と `-timeout` を CI で有効にする。
シードコーパスに 7.1 のフィクスチャを入れる。

### 7.5 敵性入力の回帰フィクスチャ

リポジトリに小さな「悪意ある入力」を置き、**拒否されること**をテストする。

- `billion_laughs.musicxml`（DTD 実体展開）→ `Err`、メモリ増加なし。
- `xxe.musicxml`（外部実体で `file:///etc/passwd`）→ `Err`。
- `zipbomb.mxl`（宣言 10 GB、実体 1 KB）→ `Err`、展開しない。
- `traversal.mxl`（エントリ名 `../../evil`）→ `Err`。
- `unterminated_quote.csv`（4 MB の閉じないクォート）→ `FieldTooLong` で全行スキップ、上限内で終わる。
- `million_columns.csv` → `Err`。
- `nan.csv`（`NaN` / `1e400` / `-inf`）→ 全行 `NonFinite` でスキップ、`Document` に入らない。
- `cp932_labels.csv`（日本語ラベル、BOM 無し CP932）→ ラベルが正しく復元される。
- `bidi_label.csv`（U+202E を含むラベル）→ 制御文字が除去される。

### 7.6 ストレス・ベンチ

- 64,000 行 × 15 列（約 8 MB）の生成フィクスチャで `import_tabular_as_document`。
  **1秒未満**、ピーク常駐 **50 MB 未満**、`progress` が単調増加。
- 同じインポートを2回実行し、`to_json()` がバイト単位で一致（不変条件8）。
  さらに別プロセスで実行しても一致すること（`HashMap` の反復順依存を検出）。
- `write_coordinates_csv` の 64,000 行を `cargo bench -p drill-interop`。
  `PRODUCT_QUALITY.md` の再計測手順（`cargo bench -p drill-core --bench core_performance`）に倣い、
  数値をベースラインとして記録する。
- `sniff` を 64 KiB で 10,000 回: 1回あたり 2ms 未満。

### 7.7 「閉ざさない条件」の常設テスト（**プラグイン実装前から回す**）

3.5.5 の C1〜C10 は、緑であり続けることに意味がある。
**プラグインが存在しない P0/P1 の段階から CI に載せる。**

| 条件 | テスト |
|---|---|
| C1 | `ExternalShape`（架空の `ProviderId`、`baked` に 32 点）を含む文書フィクスチャを、レジストリ空で開く。`Document::validate()` が `Ok`。`drill_render::build` の `DisplayList` が、同じ点を `FreePath` で作った文書と**バイト単位で一致** |
| C2 | `Registry::new()`（空）で `Circle` を引くと `None`。組み込み登録後に `Some`。**`drill-app` / `drill-core` に `ShapeSpec` の全バリアント `match` が無いこと**を grep（`External` 追加で壊れる箇所を作らない） |
| C3 | `wit/` 配下に `edit` を名に持つ record / variant が無いことを grep。`PluginEdit` のバリアント数 ≤ 12 を assert。`Edit` が `Serialize` を実装していないことを、実装したらコンパイルエラーになるトレイト境界テストで固定 |
| C4 | `Capability` のバリアント数 == 6 を assert（第51番の型に対するテスト。増やすには第51番の改訂が要る、という圧力をコードで作る） |
| C5 | `Registry` に `AtomicU32` の呼び出しカウンタを置き、`Document::from_json` → `drill_render::build` → SVG 書き出し → 動画 1 本の書き出しを通した後にカウンタが **0** であること |
| C6 | `cargo tree -p drill-core --edges normal` の出力が serde / serde_json のみ（第50番の既存 CI チェックに 1 行追加） |
| C7 | `KeyMap::to_edits` 以外から `PluginEdit` が `Edit` へ変換される経路が無いことを grep |
| C8 | `provider: "com.example.nonexistent"` を含むフィクスチャが `validate()` を通り、`ImportWarning` 相当の「不明なプロバイダ」一覧に現れる |
| C9 | 変換関数が `drill-plugin/src/project.rs` にのみ存在すること（grep）。`drill-core` の型に `#[repr(C)]` が無いこと |
| C10 | `ShapeSpec::External` / `ParamValue` / `ProviderId` のゴールデン JSON（第41番のマイグレーションテストに相乗り） |

さらに `ProviderId::parse` の単体テスト: 妥当（`builtin` / `dev.example.a-b`）、
不当（空 / 大文字 / 先頭ハイフン / `..` / 65 バイト / 非 ASCII / 前後空白）。

### 7.8 プラグイン実行（P2、実装した場合）

機構側（無限ループ・メモリ・署名）のテストは第51番 7 章（`drill-sandbox/tests/`）が持つ。
本書が追加するのは表面側のテストである。

- `generate` が `req.count` と違う個数を返す → `Err`、`Document` は無変更。
- 10 億点を返そうとする → デコード中に `MAX_PLUGIN_POINTS` で `Err`（OOM しない）。
- 範囲外の `SetKey` / `PerformerKey` を返す → `KeyMap::to_edits` が `Err`。
- `describe()` の `provider-id` がマニフェストと違う → ロード拒否。
- `ProviderId::BUILTIN` を名乗る → ロード拒否。
- 組み込みと同キーで登録 → `register` が旧エントリを返し、UI 警告経路が発火する。
- 同じ `shape-request` を 100 回 → 100 回とも同じ点列（決定論）。
- プラグインをアンインストールしてから文書を開く → 開ける、`baked` のまま描画される、
  パラメータ編集 UI だけが無効化される（C1 の実地版）。
- 壊れた WASM バイト列 → `Err`、パニックしない。

### 7.9 CLI

- `drillforge import csv --report r.json` の `r.json` をスナップショット比較。
- `--json` 出力に `"api": 1` が含まれること。
- 終了コード: 成功 0 / 部分成功（スキップ行あり）2 / 失敗 1 を固定し、テストする。

## 8. 実装タスク

1タスク = 1〜3時間相当。`[依存]` は先行タスク。並行可能な塊を Wave で示す。

### Wave 0.5 — 先送りできない決定の実装（**他の全 Wave より先。スキーマ v2 凍結前に必ず終える**）

3.5.4 の D1・D2・D5 と、3.5.5 のチェックリストを実体化する。
**プラグイン機構（wasmtime）は一切含まない。** 含むのは「後から入れられるようにする器」だけである。

- **Z1. `ProviderId` 実装**（2時間）— `drill-core`。文法パーサ、`BUILTIN` 予約、
  serde の `try_from = "String"`、`MAX_PROVIDER_ID_BYTES = 64` の追加、単体テスト（7.7 の一覧）。
  **第10番 `limits` への追加が要るので同文書と調整。**
- **Z2. `ParamValue` / `ParamSpec`**（1時間）[Z1] — `drill-core`。`#[serde(tag="t", content="v")]`。
- **Z3. `ShapeSpec::External` と `ExternalShape`**（3時間）[Z2] — **第14番のタスクとして実施**。
  `impl Shape for ExternalShape`（`baked` 由来の `sample` / `arc_length` / `validate`）。
  再標本化は第14番の `FreePath` 経路を再利用。
  **これがスキーマ v2 凍結前に入らないと、以後の導入は破壊的変更になる（D1）。最優先。**
- **Z4. `validate()` V26 と未知プロバイダの寛容化**（2時間）[Z1, Z3] — **第10番のタスクとして実施**。
  文法のみ検査し実在は問わない。`Document::unknown_providers() -> Vec<ProviderId>` を追加
  （`dangling_section_refs` と同じ流儀、`Err` にしない）。
- **Z5. `Registry<P>` と `ShapeProvider`**（3時間）[Z2] — `drill-core`。
  組み込み 17 シェイプを `ProviderId::BUILTIN` で登録。`register` の置き換え検出。
- **Z6. `AnalysisProvider`（S2）**（2時間）[Z5] — **第13番と調整**。組み込み 2 種を登録。
- **Z7. `FormatProvider`（S3）**（2時間）[Z5, A1] — `drill-interop`。CSV/MIDI/MusicXML を登録。
- **Z8. `ReportProvider`（S4）**（2時間）[Z5] — **第17番と調整**。組み込み帳票を登録。
- **Z9. C1〜C10 の CI テスト**（3時間）[Z3, Z5] — 7.7 の表を実装。
  この時点で全て緑になり、以後**緑であり続けることが要件**になる。
- **Z10. `drill-app` の `ShapeSpec` 全網羅 `match` の排除**（2時間）[Z3, Z5] —
  レジストリ経由に置き換える（C2）。**`External` を足しても壊れないことの実地確認。**

依存: Z1 → Z2 → Z3 → Z4、Z2 → Z5 → (Z6 ∥ Z7 ∥ Z8) → Z9、Z3+Z5 → Z10。
所要は合計 22 時間程度。**プラグインを入れない今、この 22 時間だけが「後で入れられる」を買う代金である。**

### Wave A — 基盤（他文書の完了を待たずに着手できる）

- **A1. `drill-interop` クレート新設**（30分）
  workspace メンバ追加、`Cargo.toml`（3.0 のフィーチャ定義）、`#![forbid(unsafe_code)]`、
  `lib.rs` にモジュール骨格（`tabular` / `music` / `csv_out` / `limits` / `report`）。
- **A2. `ImportLimits` / `ImportReport` / `SkipReason` / `ImportWarning` / `NotImported`**（2時間）[A1]
  型定義とレポート集計ロジック（`max_reported_rows` の打ち止め、収支の検証）。単体テスト。
- **A3. `sniff` / `resniff`**（3時間）[A1]
  BOM 判定、`chardetng` 連携、区切り推定、ヘッダ推定、改行判定。7.1 のフィクスチャ一式。
- **A4. `LengthUnit` / `Origin` / `DepthDirection` / `AxisConvention::to_point`**（2時間）[A1]
  テーブル駆動テスト。非有限入力の `None` 返却。
- **A5. CSV 書き出しの符号化層 `write_coordinates_csv`**（2時間）[A1]
  BOM / CRLF / 区切り / CSV インジェクション対策。`CsvProfile` / `CsvColumns`。
  中身の文字列生成は既存 `drill_core::coordinates::coordinates_csv` を使う。
- **A6. 凍結列名のゴールデンテスト**（1時間）[A5]
  ヘッダ完全一致テスト。`coordinates.rs:302-313` の緩和。

### Wave B — 表形式インポータ本体（A と並行不可）

- **B1. `ColumnRole` / `TableLayout` / `ColumnMapping` / `RaggedPolicy` / `DuplicatePolicy`**（2時間）[A2, A4]
  serde 派生、`.drillmap.toml` の読み書き。
- **B2. `guess_mapping`**（3時間）[B1, A3]
  正規化、多言語辞書、値形状判定、自形式プロファイルの特別扱い。
- **B3. `plan_import`**（2時間）[B1]
  マッピングの整合検査（必須ロール欠落、排他ロール重複、Wide グループの範囲外）。
- **B4. `import_tabular_as_document`（Long、数値 x/y のみ）**（3時間）[B3, A4]
  ストリーミング、`ByteRecord` 再利用、`BTreeMap` 蓄積、進捗、キャンセル。
- **B5. セクション推定**（2時間）[B4]
  第15番 `roster::presets` との照合と `append_sections` 連携。**第15番の実装完了に依存**。
- **B6. テキスト座標経路**（3時間）[B4]
  `parse_lateral` / `parse_depth` 連携、バイリンガルフォールバック。**第16番タスク7に依存**。
- **B7. `import_tabular_into`（`Vec<Edit>` 返却）**（3時間）[B4]
  **第10番 `Edit` の実装完了に依存**。
- **B8. `Wide` レイアウト**（3時間）[B4] — P1。
- **B9. 往復テスト一式**（2時間）[B4, B6, A5]
  7.2 の全パターン。

### Wave C — 音楽ファイル（B と並行可能）

- **C1. `MusicalTimeline` / `CountUnit` / `MusicalImportOptions`**（1時間）[A1]
- **C2. `import_midi`**（3時間）[C1]
  `midly` 連携、tick→count の整数演算、meta 走査、SMPTE 拒否、文字コード試行。
- **C3. `write_click_midi`**（3時間）[C1]
  format-1 SMF 生成、整数 tick、セットマーカー、ゴールデン比較。
- **C4. `.mxl` コンテナ**（2時間）[C1]
  ZIP 上限、`container.xml`、パス検査。zip bomb フィクスチャ。
- **C5. `import_musicxml` の小節グリッド**（3時間）[C4]
  `divisions` / `time` 追跡、`implicit` 小節の音符長推定。
- **C6. `import_musicxml` のテンポとリハーサル記号**（3時間）[C5]
  `<sound tempo>` / `<metronome>` / `<rehearsal>`。
- **C7. `apply_timeline` / `TimelinePolicy`**（2時間）[C1] — **第10番 `Edit` に依存**。

### Wave D — 安全性の検証（B/C の各タスク完了後、随時）

- **D1. fuzz ターゲット6本**（3時間）[B4, C2, C6]
- **D2. 敵性入力フィクスチャ一式**（3時間）[B4, C4, C6]  7.5 の9件。
- **D3. ベンチと決定論テスト**（2時間）[B4, A5]  7.6。

### Wave E — アプリ統合と移行導線

- **E1. インポートを `Job` に載せる**（3時間）[B4] — **B-3 の `Job` 実装に依存**。
- **E2. 列マッピング UI**（3時間 × 2）[B2, E1]
  プレビュー表、列ごとのロール選択、文字コード・区切りの手動上書き、`.drillmap.toml` の保存/読込。
- **E3. インポート結果ダイアログ**（2時間）[E1]
  収支表示、スキップ一覧、`not_imported`、レポート保存。**警告があるとき1クリックで閉じない**制約。
- **E4. `verify_against_source` と検算画面**（3時間）[B6, E3]
- **E5. 移行ウィザードの骨格**（3時間）[E2, E3]  3.7.1 の6ステップ。
- **E6. 「入らないもの」表のヘルプページ**（1時間）  3.7.3 をそのまま。
- **E7. 既存書き出しメニューを新プロファイルに接続**（2時間）[A5]
  `main.rs:1285-1340`。`export_text` を `Job` 経由に変更。

### Wave F — CLI（E と並行可能）

- **F1. `drill-cli` クレート新設と引数パース**（2時間）[A1]
- **F2. `export` / `import` / `validate` サブコマンド**（3時間）[F1, B4, A5]
- **F3. `--json` 出力と終了コードの契約**（2時間）[F2]  7.8。

### Wave G — プラグイン第一弾（**3.5.6 の G1〜G4 を満たしてから着手。G5 は待たなくてよい**）

Wave 0.5 が終わっていることが前提。ここで作るのは 3.5.6 の「最小の第一弾」
（`shape-generator` のみ・能力の付与は空集合）である。

- **G-1. WIT ワールド定義（`types` / `metadata` / `shape-generator`）と `drill-plugin` クレート骨格**（3時間）
- **G-2. `drill-sandbox` の `feature = "plugins"` を有効化し、`PluginHost` を配線**（3時間）[G-1]
  — **型と設定は第51番 3.7 が正。** epoch ticker、`StoreLimits`、`wasm_threads(false)`、
  `wasm_nan_canonicalization(true)`、WASI 不供給。
- **G-3. `project.rs` — `drill-core` 型 ↔ WIT 型の写像（C9 の唯一の置き場所）**（3時間）[G-1]
- **G-4. `WasmShapeProvider`（`ShapeProvider` の WASM 実装）**（3時間）[G-2, G-3, Z5]
  — Wave 0.5 で作ったレジストリに**そのまま挿さる**ことの確認が本タスクの主眼。
- **G-5. `.drillplug` パッケージ読み込み、マニフェスト照合、`ProviderId` 詐称拒否**（3時間）[G-2]
- **G-6. 表面側のテスト一式**（3時間）[G-4, G-5]  7.8。

### Wave H — プラグイン第二弾以降（**3.5.6 の G5 を満たしてから、要望の多い順に**）

- **H-1. `SetKey` / `PerformerKey` / `SectionKey` と `KeyMap`**（3時間）[G-3]
- **H-2. `PluginEdit` と `KeyMap::to_edits`**（3時間）[H-1]
- **H-3. `accept`（単一の関門。第51番の検証パイプラインへの接続）**（2時間）[H-2]
- **H-4. `Capability` の同意 UI（第51番 3.7 の 6 種を日本語で列挙）**（3時間）[H-3]
- **H-5. WIT に `analysis` interface を追加、`WasmAnalysisProvider`**（3時間）[H-3, Z6]
- **H-6. WIT に `format` interface を追加、`WasmFormatProvider`**（3時間）[H-3, Z7]
- **H-7. WIT に `report` interface を追加、`WasmReportProvider`**（3時間）[H-3, Z8]

**並行可能性のまとめ:** **Wave 0.5** → Wave A → (B ∥ C ∥ F) → (D ∥ E) → G → H。
Wave 0.5 のうち Z1〜Z5 はスキーマ v2 の凍結（第41番 / A-7）と同期しなければならないため、
**Wave A より先、かつ第10番・第14番の実装と同じ波で行う**。
Z6〜Z8 は各所有文書（13 / 17）の実装完了に従属するので後追いでよいが、
**Z3・Z4 だけは v2 凍結に間に合わせること**が絶対条件である。
B5/B6/B7/C7/E1 は他文書（第10/15/16番、B-3）の完了待ちなので、
それらをブロックしない B4（数値 x/y のみ）を先に完成させ、**インポータを早期に動く状態にする**のが要点である。

## 9. 未決事項

- **xlsx（`calamine` 0.36）を P1 に入れるか。** 日本の実務では座標表が Excel で管理されている割合が高いと
  想定しているが、**この想定に裏付けがない**。利用者調査で「元データは Excel か CSV か」を確認してから決める。
  入れる場合、結合セル・複数シート・数式セルの扱いを追加設計する必要がある。
- **`.drillmap.toml` を CSV の隣に自動生成するか。** 書き出した CSV の横に、それを読み戻すための
  マッピングを置けば往復が確実になる。一方でファイルが2つになると利用者が片方を失う。
  「CSV 単体で読み戻せる（凍結列名による）」を保証してあるので必須ではない。P1 で判断。
- **`Wide` レイアウトの列グループ推定 UI。** 列名から「セット1のX、セット1のY、セット2のX…」を
  自動でグループ化する規則が決まっていない。手動グループ化から始めるのが安全か。
- **MusicXML の小節途中のテンポ変化。** P1 では小節先頭へ丸める（3.3.4 の 7）。
  丸めが実害になる譜面（accelerando を細かく刻んだもの）が実在するかは未確認。
- **弱起小節の長さ推定。** `<note><duration>` の合計に頼る方法はボイス・`<backup>`・`<forward>` の
  組み合わせで誤る可能性がある。誤った場合に以降の全カウントがずれるので、
  「弱起があった」ことを強い警告として出すか、利用者に長さを聞くかを決めていない。
- **画像下敷きの幾何。** 画像をフィールドのどこに、どのスケールで置くかは
  第22番（3Dスタジアム）・第20番（DisplayList）の座標系と関わる。本書は「画像を読む」までを担当し、
  配置モデルの所属を決めていない。
- **第16番への要求2件（`parse_lateral` / `parse_depth` の分割公開、`CoordinateParse` の位置情報）**が
  受理されるか。却下される場合、テキスト座標インポータは連結文字列を組み立てる必要があり、
  区切り文字のロケール依存を `drill-interop` に持つことになる（第16番 不変条件3 との緊張）。
- **`Document::to_json` の JSON スキーマを公開契約にするか。** 3.7.4 で「ロックインしない」と
  約束する以上、JSON も安定させたいが、A-7 のスキーマ移行が終わるまで凍結できない。
  凍結の時期と、凍結する範囲（全フィールドか、座標と名簿だけか）が未決。
- **`drill-cli` の配布形態。** 単体バイナリか、`drill-app` に同梱か。
  署名・自動更新・ライセンス確認との関係は第53番（製品化）の管轄なので、そちらと調整が要る。
- **他社製品からの CSV 書き出し手順ヘルプの記述範囲。** 1.1節の境界に照らして
  「製品名を事実として挙げる」までは可と判断したが、法務的な最終確認は販売前に行う。

### 9.1 プラグイン関連の未決事項

**先送りできない決定（3.5.4 の D1〜D7）はここに含めない。** それらは確定済みであり、
Wave 0.5 で実装する。ここに残るのは「後で決めてよい」ものだけである。

- **第14番・第10番が D1・D2・D4（V26）を受理するか。** 本書は要求を出す立場であり、
  型の最終形は所有文書が決める。**却下された場合の代替が無い**のがこの項目の重大さで、
  却下＝「将来プラグインを入れると既存ファイルが開けなくなる」を受け入れることを意味する。
  第14番・第10番・第41番（v2 凍結）の三者で早期に合意すること。**本書の未決事項の中で最も急ぐ。**
- **`MacroStep`（3.6）と `PluginEdit`（D3）を同一型にするか。** 構造はほぼ同じだが、
  マクロは選択相対のターゲット指定を要し、プラグインは不透明ハンドルを使う。
  共通の `RelativeTarget` を挟んで統一できそうだが、確定していない。
  統一するなら Wave 0.5 の時点で決めたほうが安い。
- **`ExternalShape.baked` を持つ文書のサイズ。** 1,000 人 × 64 セットの全セットに
  外部シェイプを使うと `baked` が 64,000 点（512 KB）増える。
  組み込みシェイプは `params` から再導出できるので `baked` を持たない設計もあり得るが、
  「組み込みと外部で永続化の形が違う」ことの複雑さと引き合いになる。P2 で実測してから決める。
- **wasmtime LTS の選択。** 現行最新は 47.0.3、LTS は版番号が 12 の倍数。
  3.5.6 の G2 は「48 LTS 以降」を示唆するが、着手時期に依存する。
  `wasmi`（JIT を持たない、コード生成面ゼロ）へ倒す判断もまだ開いており、
  第一弾が `shape-generator` だけであることはこの選択肢を意図的に生かしている（3.5.7）。
- **G5 の閾値「異なる利用者から 3 件以上」に根拠はない。** 利用者が付いてから見直す。
  ただし G5 は第一弾のブロッカーではない（3.5.6）ので、この曖昧さは致命的ではない。
- **プラグイン審査署名鍵の運用**（発行・保管・失効・審査基準）。第53番（製品化）と共同で決める。
  製品リリース署名鍵とは分離する（3.5.2 の 5）ところまでが本書の決定。
- **`Registry` の同キー衝突ポリシー。** 現案は「後勝ち + 旧エントリを返して UI 警告」だが、
  「組み込みは上書き不可」にする案もある。前者は柔軟、後者は予測可能。利用者が付いてから決める。
- **オープンソース化に伴う PR の受け入れ方針との接続。** 3.5.2 は
  「プラグインは本体に入れないと言うための受け皿」と位置づけたが、
  プラグインが出るまでの間に来る機能追加 PR をどう捌くかは本書の管轄外（貢献ガイドラインの領域）。
