# 41. 永続化・プロジェクトコンテナ・復旧

## 1. 目的と範囲

利用者の制作物を失わないことは、販売製品として最も重要な性質である。本書は次を定義する。

- プロジェクトコンテナ `.drillproj` の形式と内部構造
- アセット（音源・画像）の参照方法と、欠落時の読み込み継続・再リンク
- 原子的保存（Windows の `ReplaceFileW` を前提とした置換手順と失敗時の状態保証）
- 世代管理（上書き前バックアップ・自動保存・編集ジャーナル）とディスク使用量の上限
- クラッシュ復旧（パニック捕捉・異常終了検出・復旧候補の提示）
- スキーマ移行の**実行時のユーザー体験**（確認・バックアップ・中断・旧版非互換の警告）
- 信頼できないプロジェクトファイルの検査手順
- 保存性能と、将来のバイナリ形式への移行余地

**扱わないこと**（他文書の担当）:

| 事項 | 担当文書 |
|---|---|
| `Document` v1→v2 の**写像そのもの**（どのフィールドがどこへ行くか） | 10-document-model.md |
| `Edit` enum の定義とコマンド代数 | 10-document-model.md |
| `Job<T>` / ワーカースレッド基盤の実装 | 40-jobs-threading.md |
| `DrillError` / `Locale` の定義と文言カタログ | 42-errors-i18n.md |
| 復旧ダイアログ・再リンクダイアログの**ウィジェット配置** | 43-app-structure-ux.md |
| 音声・画像デコーダの入力検証 | 30-audio-engine.md / 51-security.md |
| 攻撃者モデル全体・脅威分類 | 51-security.md（本書はその「ファイル読込側」の実施手順を担当） |
| インポート／エクスポート形式（他社形式との相互運用） | 52-interop-plugin.md |
| 拡張子関連付け・インストーラ・更新 | 53-productization.md |

本書は 51 と密結合する。51 が「何を敵性入力とみなすか」を定め、本書 §3.7 / §6 が「プロジェクトファイルに対して具体的に何を検査するか」を定める。

### 1.1 クレート境界の追加提案

`00-conventions.md` のクレート表に **`drill-project`** を1本追加する。理由: コンテナ I/O・zip・ハッシュ・Win32 ファイル置換は `drill-core` の「依存は serde/serde_json のみ / OS を知らない」という制約と両立しない。かといって `drill-app` に置くと、テスト（特にクラッシュ注入テスト）に GUI が必要になる。

```
drill-core     ドキュメントモデル・時間・座標・解析・保存検証。依存は serde/serde_json のみ。
drill-project  ← 追加。コンテナ入出力・原子的保存・セッション/復旧・アセット解決・入力検査。
               依存: drill-core, serde, serde_json, zip, blake3, crc32fast,
                     cfg(windows): windows-sys
               UI・GPU・音声デバイスを知らない。ヘッドレスでテスト可能。
drill-render   drill-core にのみ依存。
drill-audio    drill-core に依存。
drill-export   drill-render に依存。
drill-app      egui/wgpu。drill-core / drill-project / drill-render / drill-audio / drill-export に依存。
```

依存は上から下へ。`drill-project` は `drill-render` を知らない（サムネイル画像はバイト列として受け取るだけ）。

`drill-project` は crate 全体で `#![deny(unsafe_code)]`。例外は `platform::windows` モジュール1つのみで、そこに `#![allow(unsafe_code)]` を置く。unsafe は Win32 呼び出しの薄いラッパ（約80行）に限定し、全関数に単体テストを付ける。

### 1.2 エラー型についての規約からの逸脱

`00-conventions.md` 不変条件8は「公開 API は `Result<_, DrillError>` を返す」と定める。`drill-project` は I/O・OS エラー・コンテナ検査結果という `drill-core` が知り得ない情報を持つため、独立した `ProjectError` を定義する。規約の趣旨（文字列型エラーの禁止・`Locale` による文言解決）は満たす。

- `ProjectError` は構造化 enum であり、`String` を主たるエラー表現に使わない
- `impl ProjectError { pub fn message(&self, locale: Locale) -> String }` を持つ
- `ProjectError::Document(DrillError)` で `drill-core` のエラーを内包する
- `impl From<DrillError> for ProjectError`

42 側でこの2型を1つの表示レイヤに束ねる（`trait Diagnostic { fn message(&self, locale: Locale) -> String; fn detail(&self, locale: Locale) -> String; }`）。

---

## 2. 現状

### 2.1 有るもの

| 箇所 | 内容 |
|---|---|
| [crates/drill-core/src/lib.rs:242](../../crates/drill-core/src/lib.rs) | `Document::schema_version: u16`。デモ生成時は `1`（[lib.rs:281](../../crates/drill-core/src/lib.rs)）。 |
| [lib.rs:302-333](../../crates/drill-core/src/lib.rs) | `Document::validate() -> Result<(), String>`。検査は5項目のみ: スキーマ番号一致 / セット非空 / グリッド寸法が正 / 各セットの点数が演者数と一致 / 演者IDの重複なし。 |
| [lib.rs:303-308](../../crates/drill-core/src/lib.rs) | `schema_version != 1` を即エラー。**移行経路は無い**（DESIGN_GAPS A-7）。 |
| [lib.rs:396-403](../../crates/drill-core/src/lib.rs) | `to_json` は `serde_json::to_string_pretty`。`from_json` は `from_str` → `validate`。 |
| [crates/drill-app/src/main.rs:236-247](../../crates/drill-app/src/main.rs) | `save_to`: 既存ファイルがあれば `fs::copy` でバックアップ、その後 `fs::write` で上書き。 |
| [main.rs:416-428](../../crates/drill-app/src/main.rs) | `save_dialog`: `current_path` があればダイアログ無しで上書き。 |
| [main.rs:430-457](../../crates/drill-app/src/main.rs) | `open_dialog`: `read_to_string` → `Document::from_json`。 |
| [main.rs:477-487](../../crates/drill-app/src/main.rs) | 自動保存: `dirty && 30秒経過` で `path.with_extension("autosave.drill.json")` へ `fs::write`。 |
| [crates/drill-core/src/audio.rs:26](../../crates/drill-core/src/audio.rs) | `AudioTrack::path: String`。doc comment に「interpreted by the app layer」とあり、実質**絶対パス**。 |
| [crates/drill-core/benches/core_performance.rs:16-23](../../crates/drill-core/benches/core_performance.rs) | 1,000人・**2セット**の `to_json` を100回。`PRODUCT_QUALITY.md` の 28.75ms はこの値。 |

### 2.2 無いもの・壊れているもの

1. **原子的保存が無い**。[main.rs:242](../../crates/drill-app/src/main.rs) の `std::fs::write` は対象ファイルを truncate してから書く。書き込み中の電源断・ディスク満杯・プロセス強制終了で、**元ファイルが 0 バイトまたは途中までの内容になる**。`00-conventions.md` の「上書きは原子的置換」に違反している。
2. **`fsync` が無い**。`fs::write` は `sync_all` を呼ばない。write が返った直後に電源断すると、NTFS のメタデータだけ更新されて内容が失われうる。
3. **バックアップ名が壊れている**。[main.rs:239](../../crates/drill-app/src/main.rs) の `path.with_extension("backup.drill.json")` は**最後の拡張子だけ**を置換するので、`show.drill.json` → `show.drill.backup.drill.json` になる。さらに世代は常に1つで、直前の保存内容しか残らない。
4. **バックアップが UI スレッドの `fs::copy`**。OneDrive の Files On-Demand で対象がオンラインのみ状態だと、`fs::copy` がハイドレートを誘発して**数十秒〜数分 UI が固まる**。しかも copy が失敗すると `?` で保存全体が中断し、利用者は保存できない。
5. **未保存の新規ドキュメントが自動保存されない**。[main.rs:478](../../crates/drill-app/src/main.rs) は `if let Some(path) = &self.current_path` で始まる。**最も失われやすい「まだ一度も保存していない作業」が完全に無保護**。
6. **自動保存が元ファイルの隣に書かれる**。OneDrive 配下なら `*.autosave.drill.json` が毎回クラウドへ同期され、別マシンから見ると「本物はどちらか」が分からなくなる。
7. **終了確認が無い**。`main.rs` に `on_exit` / `close_requested` の処理が存在しない（grep 済み）。`dirty` フラグはあるが、ウィンドウの × を押すと**未保存の変更が黙って消える**。
8. **パニックフックが無い**。`std::panic::set_hook` は `main.rs` のどこにも無い。パニックすると何も残らない。
9. **異常終了の検出手段が無い**。ロックファイル・セッションマーカーの概念が無い。
10. **アセット参照が絶対パス**。[audio.rs:26](../../crates/drill-core/src/audio.rs)。別マシンに渡すと必ず切れる。欠落時の状態表現も無い。
11. **入力検査がほぼ無い**。`validate` は要素数上限・再帰深度・**NaN/Inf**・文字列長・座標範囲を一切見ない。`1e400` を書いた JSON を読ませると `f32::INFINITY` が座標に入り、補間・衝突走査・描画へ伝播する。
12. **保存が UI スレッド同期**（DESIGN_GAPS B-3）。基準規模で約100msのフレーム落ち。

---

## 3. 設計

### 3.1 コンテナ形式の選定

| | A: 単一 zip | B: フォルダ（バンドル） | C: 作業フォルダ + 配布 zip |
|---|---|---|---|
| 配布（メール・USB・LINE） | ◎ 1ファイル | × 圧縮を求められる | ○ |
| Windows での取り違え | ◎ 起きない | × 中身をドラッグして壊す事故が起きる（macOS のようなバンドル概念が無い） | △ |
| OneDrive 同期 | ◎ 1オブジェクト、原子的置換が効く | × N個のファイルが独立に同期し、**部分同期状態**が発生する。document.json だけ新しくアセットが古い、が起こる | △ |
| 差分保存（大アセットの再書き込み回避） | × 毎回全体を書き直す | ◎ 変更ファイルのみ | ◎ |
| 原子的置換 | ◎ `ReplaceFileW` が使える | × ディレクトリ全体を原子的に置換する API が Windows に無い | ○ |
| 外部ツールでの中身確認 | ○ 任意の zip ツール | ◎ | ○ |
| 実装量 | 小 | 中 | 大 |

**選定: A（単一 zip）。** 決め手は OneDrive の部分同期と、Windows にフォルダの原子的置換が無いこと。B の唯一の利点（差分保存）は §3.9 の実測（基準規模で全体保存 約100ms、うち zip 書き込み 0.8MB）により、基準規模では不要と判断する。大きな外部音源は §3.4 の外部参照で埋め込みを回避でき、B の利点はさらに薄い。

zip 実装は `zip` クレート（**2.3.0 以上**を必須とする。2.2.x 以下は CVE-2025-29787 のパストラバーサル脆弱性を持つ）。`default-features = false, features = ["deflate"]` とし、aes-crypto / bzip2 / lzma / ppmd / xz / zstd は**有効化しない**（攻撃面を減らす）。`ZipArchive::extract` は**使わない**（後述 §3.7）。

圧縮方式:

| エントリ | 方式 | 理由 |
|---|---|---|
| `mimetype` | Stored | 先頭固定オフセットで識別できるようにする |
| `document.json` | Deflate level 1（既定）/ 6（「小さく保存」）/ Stored（自動保存） | 実測で level 1 と 6 の差は 0.78MB vs 0.68MB、時間は 27ms vs 94ms。既定は速度を取る |
| `manifest.json` | Deflate 1 | 小さい |
| `assets/*.wav` `*.aiff` `*.png`(無圧縮) | Deflate 1 | 効く |
| `assets/*.mp3` `*.aac` `*.m4a` `*.flac` `*.ogg` `*.jpg` | Stored | 既に圧縮済み。再圧縮は時間の無駄 |
| `thumbnails/*.png` | Stored | 既に圧縮済み |

判定は拡張子アロウリスト（`fn compression_for(name: &str) -> Compression`）で行う。内容推定はしない（決定論を保つため）。

ストリーミング: 書き込みは `ZipWriter<W>` に逐次流せる（`Write + Seek` が必要なので `BufWriter<File>` に直接。メモリ上に全体を作らない）。読み込みは中央ディレクトリ（末尾）を先に読む必要があるため `Read + Seek` が要る。ネットワーク越しの逐次読みは想定しない（一度ローカルへ落とす、という運用にはしない。ファイルは常にローカルパスとして与えられる）。

### 3.2 内部構造

```
show.drillproj  (zip, 決定論的バイト列)
├── mimetype                     Stored, 常に最初のエントリ
│                                内容: "application/x-drillforge-project" (改行なし, 33 bytes)
├── manifest.json                コンテナメタデータとアセット索引
├── document.json                Document の serde_json（compact）
├── assets/
│   ├── 0001-show.wav
│   └── 0002-logo.png
└── thumbnails/
    └── preview.png              任意。無くてもよい
```

エントリ順は固定（上記の順、`assets/` は `AssetId` 昇順、`thumbnails/` は名前昇順）。全エントリの DOS タイムスタンプを **1980-01-01 00:00:00** に固定し、拡張フィールド・Unix パーミッション・コメントを書かない。これにより「同じ `Document` + 同じアセット → 同じバイト列」が成立し、ゴールデンテストが単純なバイト比較で書ける。

```rust
pub const MIMETYPE: &str = "application/x-drillforge-project";
pub const CONTAINER_VERSION: u16 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    /// Container layout version. Independent from `Document::schema_version`.
    pub container_version: u16,
    /// Duplicated from document.json so `probe` can answer without inflating it.
    pub document_schema_version: u16,
    /// Which entry holds the document, and in what encoding.
    pub document_entry: String,
    pub document_format: DocumentFormat,
    /// Informational only. Never used for behaviour decisions.
    pub app_version: String,
    pub created_utc: String,   // RFC 3339, UTC
    pub modified_utc: String,
    /// Cheap counts so the recovery/open UI can describe the file without loading it.
    pub summary: DocumentSummary,
    pub assets: Vec<AssetEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentFormat {
    /// serde_json, compact. The only format written by 0.x.
    Json,
    /// Reserved: positions moved to a side entry of little-endian f32.
    /// Readers that do not understand it must refuse the file, not guess.
    PackedV1,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DocumentSummary {
    pub title: String,
    pub performers: u32,
    pub sets: u32,
    pub total_counts: u32,
}
```

**バージョン番号は2本立て**にする。`container_version` は zip 内のレイアウト、`document_schema_version` は `Document` の形。片方だけ上げられる。読み手の規則:

- `container_version > CONTAINER_VERSION` → 拒否（推測して開かない）
- `container_version <= CONTAINER_VERSION` → レイアウトは既知。`document_schema_version` を §3.6 に従って処理
- `document_format` が未知 → 拒否

### 3.3 アセット参照

`Document` 側は `AssetId` だけを持つ。パス文字列を持たない。

```rust
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct AssetId(pub u32);

/// blake3-256. Serialized as a 64-char lowercase hex string.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Hash32(pub [u8; 32]);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum AssetKind { Audio, Image }

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AssetLocation {
    /// Bytes live inside the container at `entry`.
    Embedded { entry: String },
    /// Bytes live outside. Both hints are advisory; the hash is authoritative.
    External {
        /// Path relative to the project file's directory, forward slashes,
        /// never starting with `/` and never containing a `..` component.
        relative: Option<String>,
        /// Last known absolute path. Used only as a search hint, never trusted.
        absolute_hint: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetEntry {
    pub id: AssetId,
    pub kind: AssetKind,
    pub location: AssetLocation,
    /// Name shown in the UI and used to build the relink dialog's filter.
    pub original_name: String,
    pub byte_len: u64,
    pub hash: Hash32,
}
```

**外部参照を許すか: 許す。ただし既定は埋め込み。** 5分の 48kHz/24bit ステレオ WAV は約86MB あり、これを埋め込むと**毎回の保存で86MBを書き直す**（そして OneDrive が毎回86MBをアップロードする）。`SaveOptions::embed_threshold_bytes`（既定 64 MiB）を超えるアセットは、追加時に埋め込みか外部参照かを利用者に選ばせる。外部参照を選んだ場合、保存時に「このプロジェクトは外部ファイル1件を参照しています。別のPCへ渡すときは同梱してください」を状態行へ出し、書き出し時に「アセットを同梱して書き出す」を提供する。

**解決順序**（`AssetResolver::resolve`）:

1. `Embedded { entry }` → コンテナ内から取り出し、セッションキャッシュへ展開して `Ready`
2. `External { relative }` → `project_dir.join(relative)`（正規化して `project_dir` の外へ出ないことを確認）
3. `External { absolute_hint }` → そのパスをそのまま
4. `project_dir/<stem>_assets/<original_name>`（書き出し時の同梱先の慣習位置）
5. 「最近使ったアセットフォルダ」（アプリ設定に最大16件保持）を `original_name` で検索
6. 5 のフォルダ群を**内容ハッシュ**で検索（ファイル名が変わっていても当たる。1フォルダあたり上限 2,000 ファイル・合計 4GB まで、ワーカーで実行）
7. どれも当たらなければ `Missing`

2〜6 で見つかったファイルは必ず blake3 を計算して `hash` と照合する。不一致なら `Mismatch` とし、**使わない**。

```rust
pub enum AssetState {
    Ready    { path: PathBuf, source: AssetSource },
    Missing  { searched: Vec<PathBuf> },
    Mismatch { path: PathBuf, found: Hash32 },
    /// Declared size exceeds the limit; refused without reading.
    TooLarge { declared: u64, limit: u64 },
}

pub enum AssetSource { Embedded, ProjectRelative, AbsoluteHint, Relinked, FoundByHash }

pub struct AssetTable { /* BTreeMap<AssetId, (AssetEntry, AssetState)> */ }

impl AssetTable {
    pub fn entry(&self, id: AssetId) -> Option<&AssetEntry>;
    pub fn state(&self, id: AssetId) -> Option<&AssetState>;
    pub fn unresolved(&self) -> impl Iterator<Item = (AssetId, &AssetEntry, &AssetState)> + '_;
    /// Explicit relink from the UI. Verifies the hash; a mismatch is allowed
    /// only when `force` is set (the user said "yes, this is a different take").
    pub fn relink(&mut self, id: AssetId, path: &Path, force: bool) -> Result<(), ProjectError>;
    /// Scan a folder and relink every unresolved asset whose content hash matches.
    /// Returns how many were fixed. Bounded by `Limits::relink_scan_files`.
    pub fn relink_folder(&mut self, dir: &Path, limits: &Limits, cancel: &AtomicBool) -> usize;
}
```

**欠落しても必ず開ける。** `load` は `AssetState::Missing` を `LoadWarning` として返すだけで、`Err` にしない（`PRODUCT_QUALITY.md`「音声・画像・外部ファイルが欠落してもドリル本体を開ける」）。

再リンク UI の要件（配置は 43）:

- 開いた直後に非モーダルの通知バー「音源1件が見つかりません [再リンク] [フォルダを指定] [無視]」
- 再リンクダイアログは表形式: 元のファイル名 / 種別 / 長さ・サイズ / 探した場所（複数行、コピー可）/ 状態
- 「フォルダを指定」はハッシュ検索（解決順序6）を走らせ、進捗とキャンセルを出す
- ハッシュ不一致の再リンクは「内容が異なります。それでも使いますか？」の確認を挟む（同期位置がずれるため）
- 未解決のまま保存した場合、`AssetEntry` は**そのまま保持する**。欠落を理由にプロジェクトから参照を消してはならない。後日ファイルが見つかれば復旧できる

### 3.4 原子的保存

#### 3.4.1 手順

```
0. Document::validate() と audit_numbers() を通す          ← 壊れた文書を保存しない
1. ドライブ種別と placeholder 状態を調べ、RetryPolicy を選ぶ
2. 必要容量を見積り、空きを確認（見積り × 3）
3. temp = <dir>/.drillforge-tmp/<stem>.<pid>.<nonce>.tmp    ← 同一ボリューム必須
   属性に FILE_ATTRIBUTE_TEMPORARY | FILE_ATTRIBUTE_HIDDEN を付ける
4. BufWriter<File> へコンテナ全体を書く。同時に blake3 を計算
5. File::sync_all()                                        ← FlushFileBuffers
6. File を閉じる                                            ← ReplaceFileW 前に必須
7. target が存在する → ReplaceFileW(target, temp, backup, IGNORE_MERGE_ERRORS|IGNORE_ACL_ERRORS)
   target が存在しない → MoveFileExW(temp, target, MOVEFILE_REPLACE_EXISTING|MOVEFILE_WRITE_THROUGH)
8. 書き込み後検証: target を読み直して長さと blake3 を照合（既定 on）
9. 世代の刈り取り（§3.5）
10. .drillforge-tmp が空なら削除
```

#### 3.4.2 なぜ `ReplaceFileW` か

`MoveFileEx(MOVEFILE_REPLACE_EXISTING)` の原子性は Microsoft が保証していない。`ReplaceFileW` は同一ボリューム上での置換を1関数にまとめ、置換対象の**作成日時・短縮名・オブジェクトID・DACL・セキュリティリソース属性・暗号化・圧縮属性・代替データストリーム**を引き継ぐ。OneDrive/Dropbox は代替データストリームや属性で同期状態を追跡することがあり、これらを失うと「新規ファイル扱いで再アップロード」「バージョン履歴の断絶」が起きる。したがって既存ファイルの上書きは `ReplaceFileW` を使う。

注意点:

- `REPLACEFILE_WRITE_THROUGH` (0x1) は公式に「**This value is not supported**」と記載されている。**渡さない。** 耐久性は手順5の `sync_all()` で確保する。
- バックアップ・置換対象・置換ファイルは**同一ボリューム**にある必要がある。したがって §3.5 の上書き前バックアップの既定置き場はプロジェクトと同じボリュームでなければならない。
- 結果ファイルのファイルIDは**置換ファイル側**のIDになる（作成日時などは引き継がれるがファイルIDは引き継がれない）。ファイルIDに依存する外部連携を作らない。
- ディレクトリには使えない（フォルダ形式を採らないもう1つの理由）。

#### 3.4.3 `lpBackupFileName` を必ず渡す（不変条件）

`ReplaceFileW` の失敗コードと、その時のディスク上の状態:

| コード | 値 | バックアップ名**あり**の場合の状態 | バックアップ名 NULL の場合 |
|---|---|---|---|
| `ERROR_UNABLE_TO_REMOVE_REPLACED` | 1175 | 置換対象・置換ファイルとも元の名前のまま。**何も変わっていない** | 同左 |
| `ERROR_UNABLE_TO_MOVE_REPLACEMENT` | 1176 | 置換対象・置換ファイルとも元の名前のまま。**何も変わっていない** | **置換対象が消滅**し、置換ファイルが元の名前で残る |
| `ERROR_UNABLE_TO_MOVE_REPLACEMENT_2` | 1177 | 置換ファイルは元の名前のままだが属性・ストリームは引き継ぎ済み。置換対象は**バックアップ名**に移動済み | 置換対象は別の名前になっているが、その名前が分からない |
| その他（`ERROR_INVALID_PARAMETER` 等） | — | 両者とも元の名前のまま | 同左 |

1176 の欄が決定的である。**バックアップ名を NULL で呼ぶと、この1つのエラーで利用者のファイルが消える。** よって:

> **不変条件 I8**: `ReplaceFileW` は常に `lpBackupFileName` 非 NULL で呼ぶ。`BackupPolicy::None` が設定されている場合でも、`.drillforge-tmp/<stem>.<nonce>.old` を渡し、成功後に削除する。

各コードからの復旧:

- 1175 / 1176 / その他 → 元ファイルは無傷。`is_transient` なら再試行、そうでなければ temp を残して失敗を返す
- 1177 → 実質的に置換は完了寸前。`MoveFileExW(backup, target, MOVEFILE_REPLACE_EXISTING)` で元に戻すか、`MoveFileExW(temp, target, MOVEFILE_REPLACE_EXISTING)` で前に進めるかを選ぶ。**前に進める**（temp は検証済みの新しい内容であり、backup はそのまま世代として残る）。この2次操作も失敗したら、`target` が存在しない状態で `backup` と `temp` が残る。この場合はエラー本文に両方のパスを出し、「復元」ボタンを提供する

#### 3.4.4 再試行

一過性の失敗（ウイルス対策のスキャン、Windows Search インデクサ、OneDrive の同期ハンドル、エクスプローラのプレビュー）は日常的に起きる。

```rust
#[derive(Clone, Copy, Debug)]
pub struct RetryPolicy { pub attempts: u8, pub base: Duration, pub max: Duration }

impl RetryPolicy {
    pub const LOCAL:  Self = Self { attempts: 5,  base: Duration::from_millis(20),  max: Duration::from_millis(700) };
    pub const CLOUD:  Self = Self { attempts: 8,  base: Duration::from_millis(50),  max: Duration::from_millis(3_000) };
    pub const REMOTE: Self = Self { attempts: 10, base: Duration::from_millis(100), max: Duration::from_millis(5_000) };

    pub fn for_target(drive: DriveKind, placeholder: PlaceholderState) -> Self;
    /// Exponential with full jitter: `min(max, base * 2^n)` scaled by a random 0.5..1.5.
    pub fn delay(&self, attempt: u8) -> Duration;
}

/// Windows error codes that justify another attempt.
/// Numeric literals are only for the well-known ones; the ERROR_CLOUD_FILE_*
/// family is referenced through `windows_sys::Win32::Foundation` constants by
/// name, never by hard-coded number.
pub fn is_transient(os_error: i32) -> bool;
```

一過性とみなすもの: `ERROR_ACCESS_DENIED` (5), `ERROR_NOT_READY` (21), `ERROR_SHARING_VIOLATION` (32), `ERROR_LOCK_VIOLATION` (33), `ERROR_USER_MAPPED_FILE` (1224), `ERROR_UNABLE_TO_REMOVE_REPLACED` (1175), `ERROR_UNABLE_TO_MOVE_REPLACEMENT` (1176), および `ERROR_CLOUD_FILE_*` のうち `..._PROVIDER_NOT_RUNNING` / `..._NETWORK_UNAVAILABLE` / `..._REQUEST_TIMEOUT` / `..._IN_USE` / `..._PINNED` 系。

一過性でないもの: `ERROR_DISK_FULL` (112), `ERROR_FILE_READ_ONLY`, `ERROR_WRITE_PROTECT`, `ERROR_INVALID_PARAMETER`, `ERROR_NOT_SAME_DEVICE`。これらは即座に、原因が分かる文言で返す。

再試行を使い切ったら:

1. **temp を消さない**（`Stranded`）
2. セッションディレクトリ（ローカル、必ず書ける）へ**緊急世代**を書く
3. `SaveFailure { target_intact: true, emergency_copy: Some(...), stranded: Some(...) }` を返す
4. UI は「保存できませんでした。作業内容は次の場所に保存済みです: `<path>`」と**必ずパスを出す**。「再試行」「別名で保存」「フォルダを開く」を提供する

#### 3.4.5 OneDrive / Dropbox / ネットワークドライブ

このリポジトリ自体が `C:\Users\...\OneDrive\Documents\` 配下にあり、利用者の大半も同様と想定する。同期フォルダは既定の動作環境である。

**(a) Files On-Demand のプレースホルダ**

オンラインのみ状態のファイルは `FILE_ATTRIBUTE_RECALL_ON_OPEN` / `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS` / `FILE_ATTRIBUTE_OFFLINE` を持つ。通常の `CreateFile`/`ReadFile` はこれを**同期的にハイドレート**するため、回線次第で数十秒〜数分ブロックする。

- 開く前に `GetFileAttributesW` で状態を判定する（この呼び出し自体はハイドレートしない）
- プレースホルダなら、UI に「OneDrive から取得しています…」を出し、**ワーカー**で開く。キャンセル可能にする
- 属性だけ調べたい場面では `FILE_FLAG_OPEN_NO_RECALL` を付けて開く
- 現状 [main.rs:239-240](../../crates/drill-app/src/main.rs) の `fs::copy` は UI スレッドでこれを踏む。§3.4.1 の手順は `ReplaceFileW` のバックアップスロットに委譲するので**保存パスから読み取りが消える**（＝ハイドレートを誘発しない）。これは本設計の重要な副産物である

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaceholderState { NotPlaceholder, Hydrated, Partial, Dehydrated, Unknown }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriveKind { Fixed, Removable, Remote, Ram, CdRom, Unknown }
```

**(b) 同期クライアントが temp を拾う**

同期クライアントは新規ファイル作成を即座に検出してアップロードを始め、その直後に消えると「削除」として扱う。無駄な往復と、稀に `-safeBackup` のような副産物が生まれる。緩和:

- temp を `.drillforge-tmp` サブディレクトリに置く（同一ボリューム条件は満たす）
- `FILE_ATTRIBUTE_TEMPORARY`（同期クライアントの多くが尊重する）と `FILE_ATTRIBUTE_HIDDEN` を付ける
- 成功後すぐ削除して露出時間を最小化する
- **自動保存・編集ジャーナル・セッションデータは同期ツリーの外**（`%LOCALAPPDATA%`）に置く。これが最大の効果を持つ。同期対象は「利用者が明示的に保存したファイル」と「上書き前バックアップ」だけにする

同期クライアントの除外設定をアプリから強制する手段は無い。ヘルプに「`.drillforge-tmp` と `.drillforge` を同期対象から外すと快適です」と記載するに留める。

**(c) 競合コピー**

2台で同じプロジェクトを編集すると OneDrive は `show-DESKTOP-XXXX の競合コピー.drillproj` を作る。開くときに兄弟ファイルを走査して検出し、`Probe::conflict_copies` として返す。UI は「同じ場所に競合コピーが2件あります」を出し、それぞれの更新時刻・セット数・演者数を並べて比較させる（中身の差分表示は 10 の `Document` 比較 API を使う。本書はパス検出までを担当）。

検出パターン（言語依存なので複数）: `<stem>-<hostname> の競合コピー`, `<stem>-<hostname>'s conflicted copy`, `<stem> (<hostname> の競合コピー <date>)`, `<stem> (2)`。誤検出しても害は無い（提示するだけ）。

**(d) ネットワークドライブ / UNC**

`GetDriveTypeW == DRIVE_REMOTE` のとき:

- SMB の書き込みは耐久性保証が弱い。`sync_all` は投げるが信用しきらない
- **保存の前に**必ずセッションディレクトリ（ローカル）へ同じバイト列を書く。ネットワークが落ちても作業は残る
- `RetryPolicy::REMOTE` を使う
- 同一ボリューム条件: UNC 共有は1ボリュームなので `ReplaceFileW` は使える。ただし共有をまたぐ（`\\srv\a` と `\\srv\b`）と `ERROR_NOT_SAME_DEVICE`。temp を必ず同一ディレクトリに作れば起こらない

**(e) 長いパス**

`C:\Users\<name>\OneDrive\Documents\...` は深くなりやすい。Win32 呼び出しに渡すパスは**常に** `\\?\` プレフィックス付き UTF-16 に変換する（`to_extended`）。ただし `\\?\` は `.` や `..` を正規化しないので、変換前に自前で絶対化・正規化する。UNC は `\\?\UNC\server\share\...` に変換する。

**(f) ディレクトリの耐久性**

Windows には POSIX の「ディレクトリを fsync する」に相当する手段が無い（`FlushFileBuffers` はディレクトリハンドルを受け付けない）。したがって「rename のディレクトリエントリが物理的に書かれた」ことは保証できない。**残存リスクとして受け入れ**、代わりに世代（バックアップ・自動保存）が別ファイルとして存在することで補償する。ボリューム全体のフラッシュには管理者権限が要るため使わない。

#### 3.4.6 型

```rust
/// A file being written for later atomic installation over `target`.
pub struct AtomicFile { /* target, temp, file, written, hasher */ }

pub struct Committed { pub target: PathBuf, pub backup: Option<PathBuf>, pub bytes: u64, pub hash: Hash32 }

/// Returned when commit fails. `target_intact == false` means the caller must
/// show the manual-recovery UI; it is only possible on the 1177 double-fault path.
pub struct Stranded { pub temp: PathBuf, pub backup: Option<PathBuf>, pub target_intact: bool }

impl AtomicFile {
    /// Creates `<dir>/.drillforge-tmp/<stem>.<pid>.<nonce>.tmp` on the same volume.
    pub fn create(target: &Path) -> Result<Self, ProjectError>;
    pub fn writer(&mut self) -> &mut BufWriter<File>;
    /// fsync, close, then ReplaceFileW (or MoveFileExW when `target` is absent),
    /// with the retry ladder. `backup` must be `Some` whenever `target` exists.
    pub fn commit(self, backup: Option<&Path>, retry: RetryPolicy)
        -> Result<Committed, (ProjectError, Stranded)>;
    /// Deletes the temp file. Only for a deliberate, successful-so-far abort.
    pub fn abort(self) -> Result<(), ProjectError>;
}

/// Drop deletes the temp file only if neither `commit` nor `abort` ran
/// (the `?`-propagation case). Commit failures return `Stranded` instead.
impl Drop for AtomicFile { /* ... */ }
```

```rust
pub struct SaveRequest {
    pub target: ProjectPath,
    /// Immutable snapshot. Editing may continue while the job runs.
    pub document: Arc<Document>,
    pub assets: Arc<AssetTable>,
    pub options: SaveOptions,
}

#[derive(Clone, Debug)]
pub struct SaveOptions {
    pub compression: CompressionLevel,   // Fast | Small | None
    pub backup: BackupPolicy,
    /// Re-read and hash the installed file. Default true. ~2 ms at 0.8 MB.
    pub verify_after_write: bool,
    pub embed_threshold_bytes: u64,      // default 64 MiB
    pub pretty_document: bool,           // debug aid, default false
}

pub struct SaveReport {
    pub bytes_written: u64,
    pub hash: Hash32,
    pub backup: Option<PathBuf>,
    pub replaced_existing: bool,
    pub elapsed: Duration,
    pub warnings: Vec<SaveWarning>,      // e.g. ExternalAssetNotBundled
}

pub struct SaveFailure {
    pub error: ProjectError,
    pub target_intact: bool,
    pub emergency_copy: Option<PathBuf>,
    pub stranded: Option<PathBuf>,
}

/// Runs on a worker (40-jobs-threading.md). `progress` is 0..=10_000.
pub fn save(
    request: &SaveRequest,
    session: &mut Session,
    progress: &dyn Fn(u32),
    cancel: &AtomicBool,
) -> Result<SaveReport, SaveFailure>;
```

同時実行制御: 同一 `target` に対する `save` は直列化する。保存中に新しい保存要求が来たら**キューの末尾1件だけ**を残して合体させる（`SaveQueue`）。保存中の編集は許可する（スナップショットが不変なので安全）が、保存完了時に `dirty` を落とすのは「スナップショットを取った時点のリビジョン == 現在のリビジョン」のときだけ。

```rust
pub struct SaveQueue { running: Option<JobHandle>, pending: Option<SaveRequest> }
impl SaveQueue {
    pub fn request(&mut self, r: SaveRequest);
    pub fn poll(&mut self) -> Option<Result<SaveReport, SaveFailure>>;
    pub fn is_busy(&self) -> bool;
}
```

### 3.5 世代管理

3種類の世代を明確に分ける。**用途・置き場・寿命が全部違う。**

| | 上書き前バックアップ | 自動保存 | 編集ジャーナル |
|---|---|---|---|
| いつ | 明示保存のたび | 30秒 / 200編集 / フォーカス喪失 / 危険操作の直前 | 編集のたび（100ms バッチ） |
| どこ | `<dir>/.drillforge/backups/<stem>/`（既定）または `%LOCALAPPDATA%\DrillForge\backups\` | `%LOCALAPPDATA%\DrillForge\sessions\<id>\` | 同左 |
| 同期される | される（既定の場合） | されない | されない |
| 世代数 | 3（0〜10 設定可） | 2（リング） | 1（自動保存ごとに truncate） |
| 目的 | 「昨日の状態に戻したい」 | クラッシュ復旧 | クラッシュ時の損失を30秒→0.1秒にする |
| 生成コスト | 0（`ReplaceFileW` のバックアップスロット） | 約5ms（Stored） | 約0.05ms |

```rust
#[derive(Clone, Debug)]
pub enum BackupPolicy {
    /// Still passes a temp backup name to ReplaceFileW (invariant I8) and
    /// deletes it on success.
    None,
    BesideProject { keep: u8, max_total_bytes: u64, max_age_days: u16 },
    /// Only usable when the app-data volume equals the project's volume;
    /// otherwise the ReplaceFileW backup lands beside the project and is then
    /// *moved*, which is not atomic but is not on the data-loss path.
    AppData { keep: u8, max_total_bytes: u64, max_age_days: u16 },
}

impl Default for BackupPolicy {
    fn default() -> Self {
        Self::BesideProject { keep: 3, max_total_bytes: 512 * 1024 * 1024, max_age_days: 30 }
    }
}
```

バックアップ名: `<stem>-<YYYYMMDD-HHMMSS>.drillproj`。刈り取りは保存成功後にワーカーで行い、**古い順**に、`keep` 件・`max_total_bytes`・`max_age_days` のいずれかを超えたものを消す。**最新の1件は絶対に消さない。**

`BesideProject` を既定にする理由: PC ごと壊れた／盗まれた場合でも OneDrive にバックアップが残る。代償はクラウド容量が約4倍（本体＋3世代）になることで、基準規模では 0.8MB × 4 = 3.2MB なので許容できる。設定画面に現在のバックアップ合計サイズを表示し、`AppData` へ切り替えられるようにする。

自動保存とジャーナル:

```rust
pub struct AutosavePolicy {
    pub min_interval: Duration,     // 30 s
    pub edits_trigger: u32,         // 200
    pub on_focus_lost: bool,        // true
    pub before_risky_ops: bool,     // true (migration, import, video export, relink-scan)
    pub ring: u8,                   // 2
}
```

リングを2にする理由: 自動保存の**書き込み中**にクラッシュしても、もう1本が完全な状態で残る。`autosave-000` / `autosave-001` を交互に使い、書き終わってから `session.json` の `current_autosave` を更新する（この更新自体も `AtomicFile` で行う）。

ジャーナル（要 `Edit`: 10-document-model.md の A-1）:

```rust
#[derive(Serialize, Deserialize)]
pub struct JournalRecord { pub seq: u64, pub base_revision: u64, pub edit: Edit }

/// Frame: [u32 payload_len][u32 crc32(payload)][payload: serde_json of JournalRecord]
/// A torn tail after a crash is expected; the reader stops at the first bad
/// frame and returns what it read. It never errors and never panics.
pub struct Journal { /* file, buffer, seq */ }

impl Journal {
    pub fn append(&mut self, record: &JournalRecord) -> Result<(), ProjectError>;
    /// Called every 100 ms or every 64 records, whichever comes first.
    pub fn flush(&mut self) -> Result<(), ProjectError>;
    pub fn truncate(&mut self) -> Result<(), ProjectError>;
    pub fn replay(path: &Path, base: &mut Document, limits: &Limits) -> ReplayReport;
}

pub struct ReplayReport { pub applied: u32, pub skipped: u32, pub stopped_at: Option<u64> }
```

`replay` は `base_revision` が合わないレコードを飛ばし、`Edit::apply` が `Err` を返したら**そこで止める**（それ以降は適用しない）。適用件数を復旧ダイアログに出す。

ディスク使用量の総枠:

- `sessions/` 全体の上限 **2 GiB / 30日**。起動時にバックグラウンドで刈り取る（生存中のセッションは除外）
- 単一セッションの上限 **256 MiB**（自動保存2本＋ジャーナル＋キャッシュ）。超えたらジャーナルを truncate して自動保存を先に走らせる
- アセットキャッシュ（埋め込みアセットの展開先）はセッション終了時に削除。異常終了で残ったものは起動時の刈り取りで消える

### 3.6 セッションと復旧

#### 3.6.1 セッションディレクトリ

```
%LOCALAPPDATA%\DrillForge\
├── sessions\
│   └── 7f3a1c9e5b2d40118a6c\           SessionId (128bit hex)
│       ├── session.lock                プロセス生存中は排他保持
│       ├── session.json                SessionMeta
│       ├── autosave-000.drillproj
│       ├── autosave-001.drillproj
│       ├── journal.bin
│       ├── crash-20260809-142233.txt   パニック時のみ
│       ├── emergency-20260809-142233.drillproj  保存失敗時のみ
│       └── cache\                      埋め込みアセットの展開先
├── backups\                            BackupPolicy::AppData のとき
└── discarded\                          「破棄」された復旧候補を7日保持
```

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub pid: u32,
    pub app_version: String,
    pub started_utc: String,
    pub last_heartbeat_utc: String,
    /// The user's file, if the document came from or was saved to one.
    /// Recovery code reads this to *describe* the candidate. It is never
    /// used as a write target. See invariant I4.
    pub origin: Option<PathBuf>,
    pub summary: DocumentSummary,
    pub current_autosave: Option<String>,   // "autosave-000.drillproj"
    pub clean_exit: bool,
    pub end: Option<SessionEnd>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SessionEnd { Normal, Panicked { report_file: String }, MigrationAborted }

pub struct Session { /* root, _lock: File, meta, journal, ring */ }

impl Session {
    pub fn open_new(app_data: &Path, app_version: &str) -> Result<Self, ProjectError>;
    pub fn root(&self) -> &Path;
    pub fn heartbeat(&mut self) -> Result<(), ProjectError>;                     // every 5 s
    pub fn set_origin(&mut self, origin: Option<ProjectPath>) -> Result<(), ProjectError>;
    pub fn write_autosave(&mut self, doc: &Document, assets: &AssetTable)
        -> Result<PathBuf, ProjectError>;
    pub fn write_emergency(&mut self, bytes: &[u8]) -> Result<PathBuf, ProjectError>;
    pub fn journal(&mut self) -> &mut Journal;
    pub fn close_clean(self) -> Result<(), ProjectError>;
}
```

`session.lock` は `CreateFileW` を `dwShareMode = FILE_SHARE_READ`（write/delete を共有しない）で開き、プロセスの生存中ずっと保持する。Windows はプロセス終了時に必ずハンドルを閉じるので、**強制終了・電源断（再起動後）を含めてロックは自動的に解放される**。

#### 3.6.2 異常終了の検出

起動時、`sessions/` 配下の各ディレクトリについて:

```rust
pub enum SessionLiveness { Alive, Dead }

fn probe_liveness(dir: &Path) -> SessionLiveness {
    // Try to open session.lock with write access and no sharing.
    // ERROR_SHARING_VIOLATION => another instance owns it => Alive.
    // Success => the owning process is gone => Dead (close immediately).
}
```

`Dead` かつ `session.json`:

| `clean_exit` | `crash-*.txt` | 判定 | 提示文言（日本語） |
|---|---|---|---|
| true | — | 正常終了 | 提示しない。ディレクトリを削除 |
| false | あり | パニック | 「前回はエラーで終了しました」＋レポート表示ボタン |
| false | なし | 電源断・強制終了・OS再起動 | 「前回は予期せず終了しました」 |
| 読めない | — | 破損 | 自動保存ファイルが読めれば候補として出す。読めなければ `discarded/` へ |

`last_heartbeat_utc` と自動保存の更新時刻から「およそ何分前までの作業が残っているか」を算出して提示する。

#### 3.6.3 パニックフック

```rust
pub struct RecoveryHandle {
    root: PathBuf,                 // == session.root(). The only writable location.
    journal: Weak<Mutex<Journal>>,
    panicking: AtomicBool,
}

/// Chains to the previously installed hook so RUST_BACKTRACE output survives.
pub fn install_panic_hook(handle: Arc<RecoveryHandle>);
```

フックの動作:

1. `panicking.swap(true)` が既に `true` なら**即座に return**（二重パニックで無限ループしない）
2. `crash-<utc>.txt` を書く: パニックメッセージ、`PanicHookInfo::location()`、`std::backtrace::Backtrace::force_capture()`、アプリ版・OS版・スレッド名
3. ジャーナルが取れたら `flush()` を最大 500ms 待って呼ぶ（`try_lock` が失敗したら諦める。デッドロックしない）
4. `session.json` の `end = Panicked { .. }`、`clean_exit = false` を書く
5. 直前のフックへ委譲する

**フックはドキュメントを直列化しない。** パニック中の `Document` は不整合な可能性があり、直列化中の再パニックで復旧そのものを失う。文書データは自動保存＋ジャーナルで既に耐久化されており、フックがやるべきなのは「クラッシュの事実と原因を記録し、ジャーナルの末尾を確定させる」ことだけである。

**元ファイルを絶対に書き換えない保証**:

- `RecoveryHandle` は `origin`（利用者のファイルパス）を**フィールドとして持たない**。持てないので書けない
- `RecoveryHandle::write` は全パスについて `path.starts_with(&self.root)` を実行時に検査し、偽なら書き込みを放棄する（フック内でパニックしないよう `Result` を捨てる）
- `Session::root()` は必ず `%LOCALAPPDATA%\DrillForge\sessions\` 配下。`open_new` がこれを検証する
- テストで検証する（不変条件 I4）

`Cargo.toml` の `[profile.release]` に `panic = "abort"` を**設定しない**（設定するとフックは動くが 4 の後の巻き戻しが無くなり、`Drop` による `AtomicFile` の後始末が走らない）。現状の workspace 設定は unwind のままなので変更不要。

#### 3.6.4 復旧候補の提示

```rust
pub struct RecoveryCandidate {
    pub session_dir: PathBuf,
    pub meta: SessionMeta,
    pub autosave: PathBuf,
    pub autosave_utc: SystemTime,
    pub journal_records: u32,
    /// Modification time of `meta.origin`, if it still exists. Lets the UI say
    /// "the saved file is newer than the recovery data".
    pub origin_modified_utc: Option<SystemTime>,
    pub reason: SessionEnd,
}

/// Never fails. Unreadable session directories are skipped and logged.
pub fn scan_recoverable(app_data: &Path) -> Vec<RecoveryCandidate>;

pub struct RecoveredDocument {
    pub document: Document,
    pub assets: AssetTable,
    /// Shown to the user. NOT a save target. The first save is Save-As.
    pub origin: Option<ProjectPath>,
    pub replayed_edits: u32,
    pub warnings: Vec<LoadWarning>,
}

pub fn open_candidate(c: &RecoveryCandidate, limits: &Limits) -> Result<RecoveredDocument, ProjectError>;
/// Soft delete: moves the directory into `discarded/`, kept for 7 days.
pub fn discard_candidate(c: &RecoveryCandidate) -> Result<(), ProjectError>;
```

UI 要件（配置は 43）:

- 起動時、**空のドキュメントを出す前**に、候補が1件以上あるときだけ表示する
- 表: 元ファイル名（無ければ「無題」）/ 復旧データの時刻 / 保存済みファイルの時刻 / 演者数・セット数 / 終了理由
- 「保存済みファイルのほうが新しい」場合はその旨を明示する（利用者が古い復旧データで上書きするのを防ぐ）
- 行ごとの操作: 「復旧して開く」「破棄」「保存先フォルダを開く」
- 「復旧して開く」の結果:
  - `current_path = None`、`dirty = true`、タイトルバーは「復旧: <元の名前>」
  - 最初の「保存」は**必ず名前を付けて保存**になり、既定名は `<元のstem>-復旧-<YYYYMMDD-HHMMSS>.drillproj`
  - 元ファイルへ上書きするには、ダイアログで元のパスを選び直したうえで上書き確認に答える必要がある（**不変条件 I10**）
- ダイアログはスキップ可能。後から「ファイル ▸ 復旧データ…」で開ける
- 復旧データは復旧完了後も即座には消さない。利用者が保存に成功した時点で `discarded/` へ移す

### 3.7 読み込みと信頼できない入力

#### 3.7.1 二段階の読み込み

```rust
pub struct Probe {
    pub source: SourceFormat,
    pub container_version: u16,
    pub document_schema_version: u16,
    pub summary: DocumentSummary,
    pub asset_count: u32,
    pub bytes: u64,
    pub placeholder: PlaceholderState,
    pub drive: DriveKind,
    pub conflict_copies: Vec<PathBuf>,
    pub needs_migration: bool,
}

pub enum SourceFormat { Container, LegacyJson }

/// Cheap: reads the central directory, `mimetype` and `manifest.json` only.
/// ~1 ms for a 0.8 MB container. Still runs on a worker because the file may
/// be a OneDrive placeholder.
pub fn probe(path: &ProjectPath, limits: &Limits) -> Result<Probe, ProjectError>;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigrationDecision { NotNeeded, Approved, Rejected }

pub struct LoadOutcome {
    pub document: Document,
    pub assets: AssetTable,
    pub manifest: Manifest,
    pub migrated_from: Option<u16>,
    pub warnings: Vec<LoadWarning>,
}

pub fn load(
    path: &ProjectPath,
    cache_dir: &Path,
    decision: MigrationDecision,
    limits: &Limits,
    progress: &dyn Fn(u32),
    cancel: &AtomicBool,
) -> Result<LoadOutcome, ProjectError>;

pub enum LoadWarning {
    AssetMissing { id: AssetId, name: String },
    AssetMismatch { id: AssetId, name: String },
    AssetTooLarge { id: AssetId, name: String, declared: u64 },
    UnknownEntry { name: String },          // ignored, forward compatibility
    Migrated { from: u16, to: u16 },
    ThumbnailUnreadable,
}
```

`probe` → UI が移行確認や競合コピー提示を出す → `load(decision)`。この2段構えにより、ワーカーが UI をブロックして待つ構造を作らずに済む（40 のジョブモデルと整合する）。

#### 3.7.2 検査手順（`load` の実施順）

**Phase 0 — ファイルを開く前**

| # | 検査 | 失敗時 |
|---|---|---|
| 0.1 | `metadata().len() > limits.container_bytes`（既定 2 GiB） | `TooLarge`。1バイトも読まない |
| 0.2 | `metadata().len() < 64`（zip の最小サイズ未満） | `NotADrillForgeProject` |
| 0.3 | placeholder 状態を確認 | `Dehydrated` なら UI へ通知してから続行（拒否ではない） |

**Phase 1 — zip 中央ディレクトリ**

`zip::ZipArchive::new` で開く。**`ZipArchive::extract` は使わない。** エントリは名前で個別に取り出し、展開先のパスは**こちらが生成する**（`cache/<asset_id>.<ext>`）。アーカイブ内の名前をファイルシステムのパスとして一切使わないので、zip-slip の攻撃面が構造的に消える。

| # | 検査 | 上限（既定） | 失敗時 |
|---|---|---|---|
| 1.1 | `archive.len()` | 10,000 | `TooManyEntries` |
| 1.2 | 各エントリ名が有効な UTF-8 | — | `BadEntryName{NotUtf8}` |
| 1.3 | エントリ名のバイト長 | 255 | `BadEntryName{TooLong}` |
| 1.4 | エントリ名の文字集合 `^[A-Za-z0-9._/-]+$` | — | 下表の `NameProblem` |
| 1.5 | エントリ名の重複 | — | `DuplicateEntry` |
| 1.6 | 宣言済み非圧縮サイズの総和 | 4 GiB | `TooManyElements` |
| 1.7 | エントリごとの圧縮率 `uncompressed / compressed` | 200 | `SuspiciousRatio` |
| 1.8 | エントリごとの宣言済みサイズ | 種別ごと（下記） | `TooLarge` |
| 1.9 | 先頭エントリが `mimetype` で内容が一致 | — | `NotADrillForgeProject` |
| 1.10 | `manifest.json` と `document.json` の存在 | — | `MissingRequiredEntry` |

1.4 が弾く `NameProblem`:

```rust
pub enum NameProblem {
    NotUtf8, TooLong,
    Absolute,           // "/x" or "C:/x"
    DriveLetter,        // "C:x"
    Backslash,          // "a\b" — Windows treats it as a separator
    ParentComponent,    // any component exactly ".."
    CurrentComponent,   // any component exactly "."
    EmptyComponent,     // "a//b"
    ControlChar,        // U+0000..U+001F, U+007F
    BidiControl,        // U+202A..U+202E, U+2066..U+2069 — display spoofing
    AlternateStream,    // contains ':'
    ReservedDosName,    // CON PRN AUX NUL COM1..9 LPT1..9 (stem, case-insensitive)
    TrailingDotOrSpace, // "a." / "a " — Windows silently strips these
    NotAllowedPrefix,   // outside assets/ thumbnails/ and the fixed top-level set
}
```

これらは「パスとして使わない」設計でも検査する。理由は二重防御と、**利用者にエントリ一覧を表示する場面（アセット管理画面）での表示なりすまし防止**（bidi 制御文字）。

1.7 の補足: 宣言済みサイズは攻撃者が自由に書けるので、比率検査だけでは足りない。**読み出し時にも実測で止める**:

```rust
/// Reads at most `declared` bytes; if the decompressor keeps producing data
/// past that, the archive lied and we abort. Also caps by `hard_limit`.
fn read_entry_bounded(
    entry: &mut zip::read::ZipFile<'_>,
    declared: u64,
    hard_limit: u64,
    out: &mut Vec<u8>,
) -> Result<(), RejectReason>;
```

`out` の伸長は必ず `try_reserve` を使い、確保失敗を `ProjectError::OutOfMemory` として返す（`abort` しない）。

種別ごとのサイズ上限:

| 対象 | 既定上限 |
|---|---|
| `mimetype` | 64 B |
| `manifest.json` | 8 MiB |
| `document.json` | 256 MiB |
| `thumbnails/*` | 16 MiB |
| `assets/*`（1件） | 512 MiB |
| `assets/*`（合計） | 2 GiB |

**Phase 2 — `manifest.json`**

- `container_version > CONTAINER_VERSION` → `FutureContainer`。「このファイルは新しいバージョンの DrillForge で作られています」
- `document_format` が未知 → `FutureContainer`
- `assets` の件数 ≤ 1,024。`AssetId` の重複禁止。`Embedded { entry }` が実在するエントリを指すこと。`External { relative }` が Phase 1 と同じ名前規則を満たすこと（`..` を含む相対パスは拒否）
- `byte_len` がエントリの宣言サイズと一致すること

**Phase 3 — `document.json`**

- `read_entry_bounded` でメモリへ
- `serde_json::from_slice` で読む。**再帰深度は serde_json の既定上限（128）に委ねる。`unbounded_depth` フィーチャを有効化してはならない**（`Cargo.toml` に明記し、CI の `cargo tree -e features` で検査する）
- デシリアライズ後に構造検査:

```rust
pub struct Limits {
    pub container_bytes: u64,      // 2 GiB
    pub entries: u32,              // 10_000
    pub entry_name_bytes: u16,     // 255
    pub manifest_bytes: u64,       // 8 MiB
    pub document_bytes: u64,       // 256 MiB
    pub total_uncompressed: u64,   // 4 GiB
    pub max_ratio: u32,            // 200
    pub performers: u32,           // 10_000
    pub sets: u32,                 // 1_000
    pub points: u64,               // 4_000_000
    pub tempo_segments: u32,       // 4_096
    pub route_overrides: u32,      // 10_000
    pub sections: u32,             // 256
    pub set_counts_max: u16,       // 1_024
    pub title_bytes: u16,          // 512
    pub label_bytes: u16,          // 64
    pub set_name_bytes: u16,       // 128
    pub asset_bytes: u64,          // 512 MiB
    pub assets_total_bytes: u64,   // 2 GiB
    pub assets: u32,               // 1_024
    pub coordinate_abs_max: f32,   // 1.0e6
    pub relink_scan_files: u32,    // 2_000
    pub load_timeout: Duration,    // 10 s
}
impl Default for Limits { /* the table above */ }
```

| # | 検査 |
|---|---|
| 3.1 | `performers.len() <= limits.performers` |
| 3.2 | `sets.len() <= limits.sets` かつ `>= 1` |
| 3.3 | `performers.len() * sets.len() <= limits.points`（10,000 × 1,000 = 10M の膨張を止める） |
| 3.4 | 各 `set.positions.len() == performers.len()`（既存 `validate`） |
| 3.5 | `PerformerId` / `SetId` の重複なし（既存 `validate` は `PerformerId` のみ） |
| 3.6 | `1 <= set.counts <= limits.set_counts_max`（**現状 0 を許してしまい `locate_count` が壊れる**） |
| 3.7 | 文字列長: `title` / `label` / `set.name` / セクション名 |
| 3.8 | `tempo` の区間数 ≤ 4,096、各 BPM が `1.0..=400.0` の有限値、区間開始カウントが単調増加 |
| 3.9 | `RouteTable::overrides` の件数、参照する `PerformerId` の実在（11 の型に依存） |
| 3.10 | **数値監査**（下記） |

数値監査:

```rust
#[inline]
fn finite_in(v: f32, limit: f32, pointer: &str) -> Result<(), RejectReason> {
    if v.is_finite() && v.abs() <= limit { Ok(()) }
    else { Err(RejectReason::Numeric { pointer: pointer.to_owned(), value: v as f64 }) }
}

/// O(performers * sets). ~2 ms at 64,000 points.
pub fn audit_numbers(doc: &Document, limits: &Limits) -> Result<(), RejectReason>;
```

対象: 全 `Point.x/y`、`GridConfig::{width,height}` とグリッド線位置、`TempoMap` の全 BPM と区間境界、`AudioTrack::{duration_seconds, offset_seconds, gain_db, trim_*, fade_*}`、`Route` の制御点、`Performer::height_m`、カメラのキーフレーム値。

**なぜ必須か**: `serde_json` はリテラル `NaN` / `Infinity` を JSON 数値として受け付けないが、`1e400` は `f64::INFINITY` として読み、`f32` フィールドへは `inf` が入る。`-1e400` も同様。`inf` が座標に入ると、補間で `NaN` を生み、衝突走査の空間ハッシュがセル索引計算で破綻し、GPU へ `NaN` 頂点が渡って描画がハングまたは真っ黒になる。ファイルを開いた瞬間ではなく**再生を始めた瞬間**に壊れるので、原因追跡が極めて困難になる。読み込み時に必ず弾く。

`RejectReason::Numeric` の `pointer` は JSON Pointer（`/sets/12/positions/300/x`）にし、エラーダイアログに「詳細をコピー」で出す。

**Phase 4 — アセット**

- 埋め込みアセットは `cache/<asset_id>.<ext>` へ書き出す。`ext` はアロウリスト（`wav aiff aif flac mp3 m4a aac ogg opus png jpg jpeg webp`）から選び、外れたら `bin`。**アーカイブ内の名前は使わない**
- 書き出しながら blake3 を計算し、`manifest` の `hash` と照合。不一致は `Mismatch`（`Err` にしない）
- **この段階でデコードは一切しない**。音声・画像デコーダに渡すのは利用者が再生・表示を要求した時点で、その入力検証は 30 / 51 の担当
- 外部参照は §3.3 の解決順序で探す。ファイルシステムの走査は `limits.relink_scan_files` で打ち切る

**Phase 5 — 全体**

- `load` 全体に `limits.load_timeout`（10秒）の壁時計予算を置く。超えたら `RejectReason::Timeout`。想定外のアルゴリズム的膨張に対する最後の網
- `cancel` を各フェーズの境界と、Phase 1/4 のエントリごとに確認する
- 途中で失敗したら `cache/` に書いた分を消す。`Document` は返さない。**元ファイルには一切触れていない**（読み取り専用で開いている）

**パニック禁止**: `drill-project` の読み込み経路には `unwrap` / `expect` / スライス添字 / `as` による数値切り詰めを置かない。CI で `clippy::indexing_slicing` / `clippy::unwrap_used` / `clippy::expect_used` / `clippy::cast_possible_truncation` を `deny` にする（`#[cfg(test)]` は除外）。

#### 3.7.3 レガシー `.drill.json`

現行の単一 JSON も開ける。`probe` が `SourceFormat::LegacyJson` を返し、Phase 3・Phase 5 の検査を同じ `Limits` で適用する（Phase 0 のサイズ上限は `document_bytes` を使う）。アセットは `AudioTrack::path` を `External { absolute_hint }` に変換する。保存時の既定は `.drillproj` で、`.drill.json` への上書きは「旧形式で保存します（音源は同梱されません）」の確認を挟む。

### 3.8 スキーマ移行の実行時体験

写像そのものは 10-document-model.md が定義する。本書はその**周辺の保証**を定める。

```rust
pub struct MigrationPlan {
    pub from: u16,
    pub to: u16,
    /// Human-readable, locale-resolved bullet points of what will change.
    pub changes: Vec<MigrationNote>,
    /// Whether saving in the new format makes the file unopenable by older apps.
    pub breaks_backward_compatibility: bool,
    pub last_compatible_app_version: String,
}

pub enum MigrationNote { SetIdsAssigned, CountsSplit, DefaultSectionCreated, RoutesDefaulted, Other(String) }

pub fn plan_migration(from: u16) -> Option<MigrationPlan>;

pub struct MigrationError {
    pub from: u16,
    pub to: u16,
    /// JSON Pointer into the source document, e.g. "/sets/12/positions/300".
    pub pointer: String,
    pub reason: DrillError,
}
```

**規則:**

1. **移行はメモリ上でのみ行う。** `load` は移行後の `Document` を返すが、ディスク上のファイルは v1 のまま。利用者が保存するまで一切書き換えない
2. **開く前に確認する。** `probe` で `needs_migration` を得たら、モーダルで:
   > 「このファイルは旧形式（v1）です。新形式（v2）に変換して開きます。
   > 　・元のファイルは変更されません
   > 　・変換内容: セットIDの採番／カウントの分離／既定セクションの作成／既定ルートの設定
   > 　［変換して開く］［キャンセル］」
3. **`dirty` は立てない。** 開いただけで「変更あり」にすると、閉じるたびに保存を促されて鬱陶しい。代わりに `migrated_from: Some(1)` を保持し、タイトルバーに「（旧形式から変換）」を出す
4. **初回の上書き保存で2度目の確認。**
   > 「保存すると v2 形式になり、DrillForge 0.4 以前では開けなくなります。
   > 　［v1 のコピーを残して保存］（既定）［そのまま保存］［キャンセル］」
   >
   > 「v1 のコピーを残して保存」は、原子的保存を始める**前に**元ファイルのバイト単位コピーを
   > `<stem>-v1-<YYYYMMDD-HHMMSS>.drillproj`（または `.drill.json`）として作る。
   > このコピーは §3.5 の刈り取り対象に**しない**（世代数上限で消えては意味が無い）
5. **移行失敗は開くこと自体を中止する。** 部分的に移行した `Document` を絶対に返さない。エラーには失敗した JSON Pointer と理由を出し、「詳細をコピー」を提供する。`session.json` に `SessionEnd::MigrationAborted` は記録するが、これは復旧候補にしない（元データはディスク上に無傷で残っている）
6. **移行の直前に自動保存を1本取る**（`AutosavePolicy::before_risky_ops`）。移行中にパニックしても、直前の状態（＝移行前の別ドキュメント）が残る
7. **将来形式は絶対に推測しない。** `container_version` または `document_schema_version` が自分より大きいファイルは、部分的にも読まない。「DrillForge を更新してください」＋現在版と必要版を表示する
8. 移行結果は**その場で検証する**。`migrate` の直後に `Document::validate()` と `audit_numbers()` を通し、落ちたら 5 の扱いにする

### 3.9 保存性能とバイナリ形式への道

基準規模（演者1,000人 × セット64 = 64,000点）における実測値:

| 項目 | 値 | 出典 |
|---|---|---|
| `to_string_pretty` の出力 | **4.70 MB** | 等価な JSON をオフラインで生成して実測 |
| compact（`to_string`）の出力 | **1.94 MB** | 同上 |
| compact + Deflate level 1 | **0.78 MB** / 27 ms | 同上（zlib） |
| compact + Deflate level 6 | **0.68 MB** / 94 ms | 同上（zlib） |
| pretty + Deflate level 6 | 0.77 MB / 71 ms | 同上 |
| `to_string_pretty` 1回（1,000人 × **2**セット） | 0.2875 ms | [benches/core_performance.rs:16-23](../../crates/drill-core/benches/core_performance.rs)、28.75ms / 100回 |
| → 64セットへ外挿（点数32倍） | 約 **9.2 ms** | 上記 × 32 |
| → compact へ換算（出力バイト比 1.94/4.70） | 約 **3.8 ms** | |

**決定: `document.json` は compact（`to_writer`）で書き、Deflate level 1 で圧縮する。**
pretty は 2.4 倍のバイト数を生むだけで、zip の中に入るので可読性の利点も無い。デバッグ用に `SaveOptions::pretty_document` を残す。
Rust の `flate2`（既定バックエンド `miniz_oxide`）は zlib の約1.5〜2倍の時間がかかるので、level 1 で **40〜60 ms** と見積もる。`zlib-rs` バックエンドを使えば zlib 同等になる（採否は §9）。

保存1回の内訳（基準規模、ローカル SSD、アセットなし）:

| 段階 | 想定 |
|---|---|
| `validate` + `audit_numbers` | 2 ms |
| 直列化（compact） | 4 ms |
| Deflate level 1 | 40–60 ms |
| ファイル書き込み 0.78 MB | 2 ms |
| `sync_all` | 3–30 ms（デバイス依存） |
| `ReplaceFileW` | 1–20 ms（同期フォルダ・AV 環境で上振れ） |
| 書き込み後検証（読み直し + blake3） | 2 ms |
| **合計** | **約 55–120 ms** |

**16.6ms 予算のうちの取り分: 0 ms。** 保存は 40 の `Job` としてワーカーで走り、UI スレッドが毎フレーム行うのは `SaveQueue::poll()`（`AtomicU32` の load と `try_recv`）だけで 1 µs 未満。

UI スレッドに残る唯一のコストは**スナップショットの取得**である。

- 案 A（当面）: `Document::clone()`。64,000 点 × 8 B = 512 KB ＋ `Vec` 64本のヘッダで **0.3–0.6 ms**。1フレームだけの一過性コストとして許容する。上限規模（4,000人 × 256セット = 1,024,000 点、8 MB）では 5–10 ms かかり、1フレーム落ちる
- 案 B（推奨・10 と調整）: `Document` を `Arc` 越しに保持し、`Edit::apply` を copy-on-write にする。スナップショットは `Arc::clone` の1命令になる

要件として置く: **「保存のためのスナップショット取得は UI スレッドで 2 ms 未満」**。上限規模で案 A が破るので、案 B は上限規模対応の前提条件になる。

読み込み1回の内訳: Deflate 展開 0.78→1.94 MB で 8 ms、`from_slice` で 15–25 ms、構造検査＋数値監査 3 ms、合計 **約 40 ms**。同じくワーカー。

自動保存: Stored（圧縮なし）で `document.json` を書く。直列化 4 ms ＋ 1.94 MB 書き込み 3 ms ＝ **約 8 ms**。30 秒ごとにワーカーで走り、1時間あたり約 230 MB の書き込みになるが、リングバッファなので占有は 4 MB。SSD 寿命への影響は無視できる。

**バイナリ形式への移行余地**: 座標だけが嵩む（64,000点 = 512 KB の生データが JSON で 1.7 MB になっている）。`Manifest::document_format = PackedV1` を将来値として予約済みで、その形は:

```
document.json      Document から positions を抜いたもの（数十 KB）
positions.bin      little-endian f32, sets × performers × 2, Stored
```

これで直列化 4 ms → 0.5 ms、読み込み 25 ms → 2 ms になる。`document_format` を manifest に持たせてあるので、旧版アプリは未知の値を見て**推測せず拒否**でき、移行は破壊的にならない。0.x では実装しない。

### 3.10 「保存したはずのデータが消えた」の経路と遮断

| # | 経路 | 現状 | 遮断 |
|---|---|---|---|
| 1 | 保存中にクラッシュ／電源断 → 元ファイルが途中まで／0バイト | [main.rs:242](../../crates/drill-app/src/main.rs) の `fs::write` で発生する | temp へ書いて `sync_all` → `ReplaceFileW`。元ファイルは置換の瞬間まで完全 |
| 2 | ディスク満杯で書き込み途中失敗 | 同上。truncate 済みの元ファイルが残る | 同上＋事前に空き容量（見積り×3）を確認 |
| 3 | `fsync` 省略で rename 後に電源断 → 0バイトファイル | `fs::write` は `sync_all` しない | 置換前に必ず `sync_all()` |
| 4 | `ReplaceFileW` が 1176 を返し元ファイルが消える | — | **バックアップ名を必ず渡す**（不変条件 I8） |
| 5 | 上書き前バックアップの `fs::copy` が UI をブロック／失敗して保存全体が中断 | [main.rs:239-240](../../crates/drill-app/src/main.rs)。OneDrive placeholder で数分固まる | `ReplaceFileW` のバックアップスロットに委譲。保存パスから読み取りが消える |
| 6 | バックアップ名が壊れて上書きされる | `with_extension` が `show.drill.json` → `show.drill.backup.drill.json` を生成。世代は常に1つ | タイムスタンプ付きの名前で3世代 |
| 7 | 未保存の新規ドキュメントが自動保存されない | [main.rs:478](../../crates/drill-app/src/main.rs) が `current_path` 必須 | セッションディレクトリへ保存。**無題でも必ず対象** |
| 8 | ウィンドウを閉じて未保存の変更が消える | 終了確認が存在しない | `close_requested` を捕捉して「保存 / 保存しない / キャンセル」。「保存しない」でもセッションの自動保存は残す |
| 9 | 保存中にアプリが落ちる | 何も残らない | 自動保存リング2本＋ジャーナル。損失は最大 100 ms 分 |
| 10 | ネットワークドライブ切断中の保存失敗で作業が消える | 失敗して終わり | 失敗前にローカルへ緊急世代。エラーにパスを明記 |
| 11 | 保存成功と表示したが実際は書けていない（AV による差し替え・同期の巻き戻し） | 検出しない | 書き込み後にファイルを読み直して長さと blake3 を照合。不一致は保存失敗扱いで temp を残す |
| 12 | ウイルス対策が temp を隔離 | — | 置換前に temp の存在と長さを再確認。失敗時は再試行→別名保存の導線 |
| 13 | 壊れた `Document` を保存して次回開けない | `validate` を保存前に通していない | 保存パイプラインの先頭で `validate` + `audit_numbers`。失敗したら保存を中止し、緊急世代を書く |
| 14 | 移行後の保存で旧形式に戻れない | 移行機構自体が無い | 初回上書き時に v1 コピーを既定で残す（刈り取り対象外） |
| 15 | 二重起動で同じファイルを両方が保存し、後勝ちで消える | 検出しない | セッションの `origin` を走査して「別のウィンドウで開いています」を検出。読み取り専用または別名保存を促す |
| 16 | OneDrive の競合コピーで古いほうを開き続ける | 検出しない | `probe` が兄弟の競合コピーを検出して提示 |
| 17 | 音源の絶対パスが切れて音が消える | [audio.rs:26](../../crates/drill-core/src/audio.rs) | 埋め込み／相対＋内容ハッシュ。欠落しても開け、再リンクできる |
| 18 | 自動保存ファイルを本物と取り違えて上書きする | 元ファイルの隣に置かれるので起きうる | 自動保存は同期外のセッションディレクトリ。復旧して開いた文書の初回保存は必ず別名（不変条件 I10） |
| 19 | 世代がディスクを食い潰して以後の保存が失敗する | — | `sessions/` 2 GiB / 30日、バックアップ 512 MiB / 3世代 / 30日で刈り取り |
| 20 | Undo 上限を超えて戻せない | `History::with_limit(500)` | 保存とは別問題だが、世代（バックアップ・自動保存）が実質的な救済路になる |

---

## 4. 不変条件

テストで検証できる形で書く。括弧内は §7 の対応する試験。

- **I1（原子性）** 保存中の任意の時点でプロセスを終了させても、`target` の内容は「保存前の完全なバイト列」か「保存後の完全なバイト列」のいずれかである。中間状態は観測されない。（T-CRASH）
- **I2（無害な失敗）** `save` が `SaveFailure { target_intact: true }` を返したとき、`target` の内容・長さ・更新時刻は呼び出し前と一致する。（T-FAULT）
- **I3（失敗時の保全）** `save` が失敗したとき、`emergency_copy` に完全なコンテナが存在する。ただし `emergency_copy` の書き込み自体が失敗した場合を除き、その場合は `SaveFailure::error` がそれを明示する。（T-FAULT）
- **I4（復旧の書き込み範囲）** パニックフックと復旧コードが書き込むパスは、すべて `Session::root()` を先頭に持つ。`SessionMeta::origin` が指すパスへは、いかなる経路でも書き込まない。（T-HOOK）
- **I5（読み込みの安全性）** `probe` と `load` は、任意のバイト列に対して (a) パニックしない (b) `Limits` を超えるメモリを確保しない (c) `cache_dir` の外へ書き込まない (d) `limits.load_timeout` 以内に終わる。（T-FUZZ）
- **I6（アセット欠落耐性）** 埋め込みアセットが欠落、ハッシュ不一致、またはサイズ超過でも、`load` は `Ok` を返し、`Document` は完全である。（T-ASSET）
- **I7（往復同値）** 任意の妥当な `Document` について `load(save(doc)).document == doc`。`Point` は**ビット単位**で一致する（`f32` の丸めが起きない）。`SetId` / `PerformerId` / セット順序も一致する。（T-ROUNDTRIP）
- **I8（バックアップ必須）** `ReplaceFileW` は常に `lpBackupFileName` 非 NULL で呼ばれる。（T-UNIT、型で強制: `replace_file(replaced, replacement, backup: &Path)` は `Option` を取らない）
- **I9（決定論）** 同じ `Document` と同じアセット集合からは、同じコンテナのバイト列が得られる。エントリ順・タイムスタンプ・圧縮設定が固定されている。（T-GOLDEN）
- **I10（復旧は上書きしない）** 復旧して開いたドキュメントの最初の保存は、利用者が保存先を明示的に選び直さない限り、`SessionMeta::origin` を上書きしない。（T-UNIT）
- **I11（移行の非破壊）** 移行を伴う `load` は、ディスク上のファイルを一切変更しない。移行が失敗したとき `load` は `Err` を返し、部分的に移行された `Document` を返さない。（T-MIGRATE）
- **I12（将来形式）** `container_version` または `document_schema_version` が現在の対応値を超えるファイルは、部分的にも読まれず、専用のエラーで拒否される。（T-UNIT）
- **I13（一時ファイルの同一ボリューム）** `AtomicFile::create` が作る temp は、常に `target` と同じディレクトリ配下（`.drillforge-tmp`）にある。（T-UNIT）
- **I14（世代の下限）** 刈り取りは、どの条件が成立しても、各系列の**最新1件**を削除しない。（T-UNIT）

---

## 5. 性能

`00-conventions.md` の 16.6 ms 予算に対する本設計の取り分:

| 操作 | UI スレッドの消費 | ワーカーでの実時間（基準規模） |
|---|---|---|
| 明示保存 | **0 ms**（+ スナップショット取得 0.3–0.6 ms、案 B で 0） | 55–120 ms |
| 自動保存（30秒ごと） | **0 ms**（同上） | 8 ms |
| ジャーナル追記（編集ごと） | **0.05 ms**（`Edit` を直列化してチャネルへ送るのみ） | 100 ms ごとに 0.2 ms |
| ハートビート（5秒ごと） | 0 ms | 0.3 ms |
| 読み込み | **0 ms** | 40 ms |
| `probe` | 0 ms | 1 ms |
| 復旧走査（起動時） | 0 ms | セッション10件で 5 ms |
| 世代刈り取り | 0 ms | 保存後に 3 ms |

**毎フレーム走るコードは `SaveQueue::poll()` と `Job::poll()` だけで、いずれもヒープ確保をしない。**

メモリ:

| 対象 | 基準規模 | 上限規模（4,000人×256セット） |
|---|---|---|
| `Document` 実体 | 約 0.9 MB | 約 17 MB |
| 保存スナップショット（案 A の clone） | +0.9 MB | +17 MB |
| 直列化バッファ（compact） | 1.94 MB | 31 MB |
| Deflate 作業領域 | 約 0.3 MB | 約 0.3 MB |
| 読み込み時の一時バッファ | 1.94 MB | 31 MB |
| ジャーナルバッファ | 64 KB 固定 | 64 KB 固定 |
| **保存中のピーク増分** | **約 3.2 MB** | **約 48 MB** |

直列化バッファは `Vec<u8>` を `Session` が保持して**再利用**する（`clear()` して使い回す）。保存のたびに 2 MB を確保・解放しない。

計算量:

- 保存: 直列化 O(P·S)、圧縮 O(出力バイト)、置換 O(1)、検証 O(出力バイト)、刈り取り O(世代数)
- 読み込み: 展開 O(バイト)、パース O(バイト)、構造検査 O(P·S)、数値監査 O(P·S)
- アセットのハッシュ検索: O(走査ファイル数 × ファイルサイズ)。`relink_scan_files` と合計バイトで打ち切る
- 復旧走査: O(セッション数)。各セッションは `session.json`（数百バイト）のみ読む

ディスク I/O:

- 明示保存: 0.78 MB 書き込み ＋ 0.78 MB 読み直し（検証）
- 自動保存: 1.94 MB 書き込み × 120回/時 = 233 MB/時。占有は 3.9 MB（リング2本）
- ジャーナル: 1編集あたり数百バイト。10,000編集で約 3 MB。自動保存ごとに truncate

---

## 6. 失敗モードと安全性

### 6.1 保存

| 失敗 | 検出 | 対処 | 元ファイル |
|---|---|---|---|
| `validate` / `audit_numbers` 失敗 | 保存開始時 | 保存を中止、緊急世代を書く、原因を JSON Pointer で提示 | 無傷 |
| 空き容量不足 | 事前確認 / `ERROR_DISK_FULL` | 必要量と空き量を出す。別の場所へ保存を提案 | 無傷 |
| temp 作成失敗（権限・読み取り専用） | `AtomicFile::create` | 「このフォルダに書き込めません」＋別名保存 | 無傷 |
| 書き込み中の I/O エラー | `Write` の `Err` | temp を残す、緊急世代、エラー提示 | 無傷 |
| `sync_all` 失敗 | `Err` | 同上。置換に**進まない** | 無傷 |
| `ReplaceFileW` 1175 / 1176 | 戻り値 | 再試行ラダー → 失敗なら temp を残す | 無傷 |
| `ReplaceFileW` 1177 | 戻り値 | `MoveFileExW(temp → target)` で前進。それも失敗なら backup と temp のパスを提示して手動復元 | backup 名に存在 |
| 共有違反（AV・インデクサ・同期） | `ERROR_SHARING_VIOLATION` 等 | ジッタ付き指数バックオフで最大10回 | 無傷 |
| 書き込み後検証の不一致 | blake3 照合 | 保存失敗として扱う。temp を残す。「保存内容を確認できませんでした」 | 置換済み（内容は不明）→ backup から復元を提案 |
| プロセス強制終了 | — | 次回起動時に復旧候補 | 置換前なら無傷、置換後なら新しい完全な内容 |
| 電源断 | — | 同上（ディレクトリエントリの耐久性は保証外、§3.4.5(f)） | 同上 |

### 6.2 読み込み

| 失敗 | 対処 |
|---|---|
| ファイルが zip でない | 「DrillForge のプロジェクトファイルではありません」。先頭バイトから推測して「これは JSON のようです。旧形式として開きますか？」を提案 |
| 中央ディレクトリ破損 | 拒否。「ファイルが破損しています」＋バックアップ世代の一覧を提示（同じフォルダの `.drillforge/backups/<stem>/` を走査） |
| `document.json` 欠落 | 拒否。同上 |
| 将来のコンテナ／スキーマ | 拒否。「DrillForge <必要版> 以降が必要です」 |
| 移行失敗 | 拒否。JSON Pointer と理由。元ファイルは無傷 |
| 上限超過（要素数・サイズ・比率） | 拒否。どの上限をどれだけ超えたかを数値で出す |
| NaN / Inf / 範囲外座標 | 拒否。JSON Pointer で位置を出す |
| アセット欠落・不一致 | **警告のみ。開く** |
| サムネイル破損 | 警告のみ。開く |
| 未知のエントリ（将来版が追加したもの） | 無視して警告。開く（前方互換） |
| タイムアウト | 拒否。「ファイルの解析に時間がかかりすぎました」 |
| メモリ確保失敗 | `try_reserve` で捕捉して拒否。プロセスは死なない |

### 6.3 信頼できない入力の脅威と遮断（51 と共有）

| 脅威 | 遮断 |
|---|---|
| zip 爆弾（高圧縮率） | 比率上限 200、宣言サイズの総和上限 4 GiB、**読み出し時の実測打ち切り** |
| zip 爆弾（多数エントリ） | エントリ数上限 10,000 |
| 入れ子アーカイブ爆弾 | アセットを再帰的にアーカイブとして開かない |
| パストラバーサル（`../`） | 展開先パスを**こちらが生成**する（アーカイブ内名を使わない）＋名前検査で `..` を拒否＋`zip` 2.3.0 以上 |
| ADS 注入（`a.txt:evil.exe`） | 名前に `:` を含むエントリを拒否 |
| DOS 予約名（`CON` 等） | 名前検査で拒否 |
| 末尾のドット・空白（Windows が黙って削る） | 名前検査で拒否 |
| bidi 制御文字による表示なりすまし | 名前検査で拒否 |
| エントリ名の重複（読み手により異なる内容が選ばれる） | 拒否 |
| 巨大要素数（10,000人 × 1,000セット = 10M 点） | `points` 上限 4M ＋個別上限 |
| 深い再帰 | `serde_json` の既定深度上限 128。`unbounded_depth` を有効化しない（CI で検査） |
| NaN / Inf（`1e400` 経由） | `audit_numbers` で全数値を検査 |
| 整数オーバーフロー | `checked_*` / `saturating_*` を使う。`clippy::arithmetic_side_effects` を読み込み経路で `deny` |
| シンボリックリンク経由の外部参照 | `External { relative }` を `project_dir` 基準で正規化し、外へ出るものを拒否。`absolute_hint` はハッシュ照合を必須にする |
| 巨大アセットによるディスク枯渇 | 単体 512 MiB、合計 2 GiB、展開前に空き容量を確認 |
| 実行ファイルの同梱 | 拡張子アロウリスト外は `.bin` として展開し、いかなる場合も**実行しない・関連付けで開かない** |

### 6.4 残存リスク（受け入れる）

1. **ディレクトリエントリの耐久性**: Windows にディレクトリの fsync が無いため、置換直後の電源断で置換が巻き戻る可能性がある。世代（バックアップ・自動保存）で補償する
2. **同期クライアントの巻き戻し**: OneDrive がサーバ側の古い版で上書きする事象は、アプリからは防げない。書き込み後検証で「保存直後の不一致」は検出できるが、数分後の巻き戻しは検出できない。ヘルプで「重要な作業の前後はバージョン履歴を確認してください」と案内する
3. **ネットワークドライブの耐久性**: SMB サーバの実装依存。ローカル緊急世代で補償する
4. **blake3 の衝突**: 現実的に起こらないが、ハッシュ照合は改竄検出ではなく取り違え検出が目的であり、署名の代替ではない

---

## 7. テスト計画

### T-UNIT（単体）

| 対象 | 項目 |
|---|---|
| エントリ名検査 | `NameProblem` の全12種を1件ずつ。`../x`, `/x`, `C:x`, `a\b`, `a//b`, `a\u{0}b`, `a\u{202E}b`, `a:b`, `CON.png`, `con`, `a.`, `a `, 256バイト名、非UTF-8 |
| 圧縮率検査 | 比率 199 は通る / 201 は落ちる / `compressed == 0` で除算しない |
| 宣言サイズ詐称 | 宣言 100 B、実体 10 MB のエントリで `DeclaredSizeExceeded` |
| 数値監査 | `1e400` / `-1e400` / `1e-400`（0 になるので通る）/ `1e7`（範囲外）/ 正常値。`Point.x`, グリッド, BPM, `AudioTrack` の各フィールド |
| 要素数上限 | 演者 10,001、セット 1,001、点 4,000,001、tempo 区間 4,097、文字列長 |
| `set.counts == 0` | 拒否される |
| `RetryPolicy::delay` | 単調非減少、`max` を超えない、ジッタが範囲内 |
| `is_transient` | 5/21/32/33/1175/1176/1224 は真、112/1177/`ERROR_NOT_SAME_DEVICE` は偽 |
| `AtomicFile::create` | temp が `target` と同ディレクトリ配下（I13） |
| `Drop for AtomicFile` | commit も abort もせず落とすと temp が消える。commit 失敗後の `Stranded` は消えない |
| 刈り取り | 最新1件は `keep=0` でも残る（I14）。`max_age_days` と `max_total_bytes` の相互作用 |
| I10 | `RecoveredDocument::origin` が `Some` でも `save` の既定 target にならない |
| I12 | `container_version = CONTAINER_VERSION + 1` / `document_schema_version = SCHEMA_VERSION + 1` を拒否 |
| `to_extended` | 通常パス / UNC / 既に `\\?\` 付き / 相対パス（エラー） |

### T-GOLDEN（ゴールデン）

`crates/drill-project/tests/fixtures/` にコミットする。

| フィクスチャ | 期待 |
|---|---|
| `v1-plain.drill.json` | 開ける。移行される。演者数・座標・カウントが一致 |
| `v1-in-container.drillproj` | 同上 |
| `v2-minimal.drillproj` | バイト単位で再現できる（I9） |
| `v2-reference-scale.drillproj` | 1,000人×64セット。ベンチと共用 |
| `missing-asset.drillproj` | `Ok` + `LoadWarning::AssetMissing` |
| `hash-mismatch.drillproj` | `Ok` + `LoadWarning::AssetMismatch` |
| `future-container.drillproj` | `FutureContainer` |
| `zip-bomb-ratio.drillproj` | `SuspiciousRatio` |
| `zip-bomb-entries.drillproj` | `TooManyEntries` |
| `traversal.drillproj` | `BadEntryName{ParentComponent}` |
| `dup-entry.drillproj` | `DuplicateEntry` |
| `nan-coordinate.drillproj` | `Numeric{pointer:"/sets/0/positions/3/x"}` |
| `truncated.drillproj` | 拒否。パニックしない |
| `empty.drillproj`（0 B）/ `one-byte.drillproj` | 拒否 |
| `not-a-zip.drillproj`（ただの JSON） | 「旧形式として開きますか？」の判定に到達 |

I9 は「同じ `Document` を2回保存してバイト比較」＋「`v2-minimal.drillproj` と再生成物のバイト比較」で検証する。

### T-ROUNDTRIP（property）

- 有界ジェネレータ（演者 0..200、セット 1..20、座標 `-1e4..1e4` の有限 `f32`、Unicode 文字列）で `Document` を生成
- `load(save(doc)).document == doc`（`Point` はビット比較）
- `load(save(load(save(doc)))) == load(save(doc))`（冪等）
- 追加依存を避けるため、`proptest` を使わず自前の xorshift ジェネレータ + 1,000 ケースで実装する（既存の `.expected` 方式と同じ方針）

### T-CRASH（クラッシュ注入・I1）

`drill-project` にテスト専用バイナリ `crash_harness` を置く。

```
DRILLFORGE_KILL_AFTER_BYTES=<n> crash_harness <target> <fixture>
```

親テストが 200 回、`n` を 0..(ファイルサイズ+α) からランダムに選んで子を起動し、子は指定バイト書いた時点で `TerminateProcess(GetCurrentProcess(), 1)` する。親は毎回 `target` を読み、**旧内容と完全一致するか、新内容と完全一致するか**を検査する（I1）。中間状態が1回でも観測されたら失敗。

さらに、`AtomicFile::commit` の直前・`ReplaceFileW` の直後に強制終了する専用ポイントも試す。

### T-FAULT（障害注入・I2/I3）

ファイル操作を trait 越しにする。

```rust
pub trait FileOps: Send + Sync {
    fn create_temp(&self, dir: &Path, name: &str) -> io::Result<File>;
    fn sync_all(&self, f: &File) -> io::Result<()>;
    fn replace(&self, replaced: &Path, replacement: &Path, backup: &Path) -> Result<(), ReplaceError>;
    fn move_replace(&self, from: &Path, to: &Path) -> Result<(), ReplaceError>;
    fn free_space(&self, dir: &Path) -> io::Result<u64>;
}
pub struct RealFileOps;
#[cfg(test)] pub struct FaultyFileOps { /* step -> os_error */ }
```

各ステップ × 各エラーコード（5, 32, 33, 112, 1175, 1176, 1177, 1224）の直積で `save` を回し、毎回 I2 と I3 を検査する。1177 は「2次操作も失敗」のケースを含める。

### T-HOOK（パニックフック・I4）

- フックを一時ディレクトリのセッションで install し、`panic!` を起こす子スレッドを走らせる
- 検査: `crash-*.txt` が生成される / `session.json` の `end == Panicked` / **セッションルート外のファイルが1つも作成・変更されていない**（テスト開始前後でツリー全体のスナップショットを取って比較）
- 二重パニック（フック内で `Drop` がパニックする状況を人工的に作る）で無限ループしない
- `RecoveryHandle::write` にセッションルート外のパスを渡すと書き込まれない

### T-FUZZ（I5）

- `cargo-fuzz` ターゲット `fuzz_load`: 任意バイト列 → `probe` + `load`。パニック・OOM・タイムアウトを検出
- 構造化ターゲット `fuzz_container`: 妥当なコンテナを起点に、エントリ名・宣言サイズ・JSON の数値・要素数を変異させる
- コーパスを `fuzz/corpus/` にコミット。CI では 1 ターゲットあたり 5 分（nightly ジョブで 1 時間）
- `load` 実行中に `cache_dir` 外へファイルが作られていないことを、テストハーネスで毎回検査

### T-MIGRATE（I11）

- `v1-plain.drill.json` を読み込み → ファイルの更新時刻とバイト列が変わっていないこと
- `migrate` 内部で人工的にエラーを起こし、`Err` が返り `Document` が返らないこと
- 移行後 `validate` + `audit_numbers` が走ることを、意図的に不正な写像を注入して確認
- 「v1 のコピーを残して保存」でコピーが作られ、刈り取り対象外であること

### T-ASSET（I6）

- 埋め込みアセットのエントリを削った容器 → `Ok` + `AssetMissing`
- ハッシュだけ書き換えた容器 → `Ok` + `AssetMismatch`、`AssetState` が `Ready` にならない
- 512 MiB 超を宣言したアセット → `Ok` + `AssetTooLarge`、実データを読まない
- `relink_folder` が名前変更済みファイルをハッシュで見つける
- 外部参照の `relative` に `../../x` を入れた容器 → 解決を拒否し `Missing`

### T-STRESS

- 10,000 回のランダム `Edit` をジャーナルに記録 → プロセスを強制終了 → 自動保存 + ジャーナル再生 → 期待ドキュメントと一致
- 2時間の自動保存ループ（加速版: 30秒間隔を50msに縮めて 8,640 回）で、`sessions/` の占有が上限内に収まりファイルハンドルが漏れないこと
- 上限規模（4,000人 × 256セット）の保存・読み込みが完走し、ピークメモリが §5 の見積り内に収まること

### T-BENCH

`cargo bench -p drill-project --bench container` に追加。

| 項目 | 目標 |
|---|---|
| 保存（1,000×64、Deflate 1、アセットなし） | < 150 ms |
| 保存（同、Stored） | < 30 ms |
| 読み込み（同） | < 80 ms |
| `probe` | < 5 ms |
| `audit_numbers`（64,000点） | < 3 ms |
| スナップショット取得（`Document::clone`） | < 2 ms |
| 保存（4,000×256） | < 1,500 ms |

`PRODUCT_QUALITY.md` のベースライン表に上記を追記する。

### T-MANUAL（自動化できない実環境マトリクス）

`docs/qa/onedrive-matrix.md` に手順と結果表を置き、リリースごとに実施する。

| 環境 | 検査 |
|---|---|
| OneDrive 通常同期フォルダ | 保存・上書き・自動保存が成功する。競合コピーが生成されない |
| OneDrive「オンラインのみ」のプロジェクトを開く | 進捗が出る。UI が固まらない。キャンセルできる |
| OneDrive 同期一時停止中 | 保存が成功する |
| OneDrive で意図的に競合コピーを作る | `probe` が検出して提示する |
| Dropbox 同期フォルダ | 同上 |
| ネットワークドライブ（SMB） | 保存が成功する。切断中はローカル緊急世代が作られる |
| USB メモリ（FAT32 / exFAT） | 保存が成功する（`ReplaceFileW` は FAT でも動くが属性引き継ぎは限定的） |
| Windows Defender 実時間保護 ON | 再試行で吸収され、利用者にエラーが出ない |
| 250文字超のパス | 保存・読み込みが成功する |
| 100% / 150% / 200% スケーリング | 復旧ダイアログ・再リンクダイアログが読める |

---

## 8. 実装タスク

1タスク = 1〜3時間相当。`[dep: n]` は先行タスク。同じ Wave 内で依存の無いものは並行可能。

### Wave A — 基盤（他の全てが乗る）

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| A1 | `drill-project` クレート新設。`Cargo.toml`（`zip 2.3+` の `default-features=false, features=["deflate"]`、`blake3`、`crc32fast`、cfg(windows) `windows-sys`）、`#![deny(unsafe_code)]`、lint 設定、CI に `cargo tree -e features` の `unbounded_depth` 検査 | — | ○ |
| A2 | `ProjectError` / `RejectReason` / `NameProblem` / `LoadWarning` の定義と `message(Locale)`。42 と文言キーを合わせる | A1 | ○ |
| A3 | `platform::windows`: `to_extended` / `replace_file` / `move_replace` / `drive_kind` / `cloud_placeholder` / `free_space_bytes` / `set_temp_hidden`。unsafe はここだけ。単体テスト付き | A1 | ○ |
| A4 | `platform::posix`（テストと将来の移植用。`rename` + `File::sync_all` で同等の意味論を出す） | A1 | ○ |
| A5 | `Limits` と `Default` 実装、`finite_in` / `audit_numbers` | A1, A2 | ○ |
| A6 | `Hash32`（blake3 ラッパ、hex の serde 実装）、`HashingWriter<W>` | A1 | ○ |

### Wave B — 原子的保存

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| B1 | `FileOps` trait と `RealFileOps` | A3, A4 | |
| B2 | `RetryPolicy` / `is_transient` / `delay`（ジッタ） | A2 | ○ |
| B3 | `AtomicFile`（create / writer / commit / abort / Drop）と 1175/1176/1177 の分岐処理 | B1, B2 | |
| B4 | `FaultyFileOps` と T-FAULT 一式 | B3 | |
| B5 | `crash_harness` バイナリと T-CRASH | B3 | ○（B4と） |
| B6 | 空き容量確認、placeholder 判定の保存パスへの組み込み | B3, A3 | |

### Wave C — コンテナ

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| C1 | `Manifest` / `AssetEntry` / `AssetLocation` / `DocumentSummary` の型と serde | A1, A6 | ○ |
| C2 | `entry_name::validate`（`NameProblem` 全種）と単体テスト | A2 | ○ |
| C3 | `write_container`（決定論的：順序固定・タイムスタンプ固定・`compression_for`） | C1, A6 | |
| C4 | `read_entry_bounded` と Phase 1 検査（`inspect`） | C1, C2, A5 | |
| C5 | `probe`（`mimetype` + `manifest.json` のみ。競合コピー検出を含む） | C4 | |
| C6 | `load` Phase 2〜5（アセット展開・ハッシュ照合を含む） | C4, A5 | |
| C7 | `save`（validate → 直列化 → `write_container` → `AtomicFile::commit` → 検証 → 刈り取り） | C3, B3 | |
| C8 | T-GOLDEN のフィクスチャ生成スクリプトと期待値、T-ROUNDTRIP | C6, C7 | |
| C9 | `fuzz_load` / `fuzz_container` ターゲットとコーパス | C6 | ○ |

### Wave D — アセット

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| D1 | `AssetTable` / `AssetState` / `AssetSource` と解決順序 1〜4 | C1 | |
| D2 | 解決順序 5〜6（最近のフォルダ、ハッシュ検索）とキャンセル対応 | D1 | |
| D3 | `relink` / `relink_folder` | D1 | |
| D4 | `AudioTrack::path` → `AssetId` への移行（10 と調整。`drill-core` 側の変更は 10 が行い、本タスクは `drill-project` 側の対応と v1 写像の検証） | D1, 10-A7 | |
| D5 | T-ASSET 一式 | D3 | |

### Wave E — セッション・復旧

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| E1 | `SessionId` / `SessionMeta` / `Session::open_new` / `session.lock`（共有モード指定）/ `heartbeat` / `close_clean` | A3, C1 | |
| E2 | 自動保存リング（2本）と `AutosavePolicy` | E1, C7 | |
| E3 | `Journal`（フレーム形式・CRC・`replay` の破断耐性）。`Edit` に依存 | E1, 10-A1 | |
| E4 | `RecoveryHandle` / `install_panic_hook`（二重パニック防止・ルート外書き込み禁止） | E1 | |
| E5 | `probe_liveness` / `scan_recoverable` / `open_candidate` / `discard_candidate` | E1, E2, E3, C6 | |
| E6 | 世代刈り取り（`sessions/` と `backups/` の両方、上限3種の相互作用） | E1, C7 | ○ |
| E7 | T-HOOK / T-STRESS | E3, E4 | |

### Wave F — アプリ統合（43 / 40 と共同）

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| F1 | `SaveQueue` と `Job<SaveReport>` / `Job<LoadOutcome>` の配線。`main.rs` の `save_to` / `save_dialog` / `open_dialog` / 自動保存を置き換え | C7, C6, 40-B3 | |
| F2 | 終了確認（`close_requested` の捕捉、「保存 / 保存しない / キャンセル」） | F1 | ○ |
| F3 | 復旧ダイアログ（起動時、および「ファイル ▸ 復旧データ…」） | E5, F1 | |
| F4 | 移行確認ダイアログ + 初回上書き時の v1 コピー確認 | C5, C7 | ○ |
| F5 | 再リンク UI（通知バー・一覧・フォルダ指定・ハッシュ検索の進捗） | D3, F1 | ○ |
| F6 | 保存失敗ダイアログ（緊急世代のパス提示・再試行・別名保存・フォルダを開く） | F1 | ○ |
| F7 | 設定画面: バックアップ位置・世代数・自動保存間隔・現在のディスク使用量表示 | E6, F1 | ○ |
| F8 | 競合コピー提示 UI | C5, F1 | ○ |
| F9 | T-BENCH の追加と `PRODUCT_QUALITY.md` のベースライン更新 | F1 | |
| F10 | T-MANUAL の手順書 `docs/qa/onedrive-matrix.md` 作成と初回実施 | F1..F8 | |

### 他文書への依存（ブロッカー）

| 依存先 | 必要なもの | 影響するタスク |
|---|---|---|
| 10-document-model.md | `Edit` が `Serialize + Deserialize` であること | E3（ジャーナル） |
| 10-document-model.md | `migrate_v1_to_v2` と `SCHEMA_VERSION = 2` | C6, F4 |
| 10-document-model.md | `Document` のスナップショット取得コスト < 2 ms（案 B の `Arc` CoW） | 上限規模での F1 |
| 10-document-model.md | `AudioTrack` のパス欄を `AssetId` へ | D4 |
| 40-jobs-threading.md | `Job<T>` / 進捗 / キャンセル | F1 |
| 42-errors-i18n.md | `Locale` と文言カタログ | A2 |
| 43-app-structure-ux.md | ダイアログの配置とキーボード操作 | F2..F8 |
| 51-security.md | `Limits` の既定値の最終合意 | A5 |

---

## 9. 未決事項

| # | 論点 | 選択肢 | 決めるために必要なもの |
|---|---|---|---|
| 1 | 上書き前バックアップの既定位置 | `BesideProject`（クラウドにも残るが容量4倍）/ `AppData`（クラウドを汚さないが PC 故障で消える） | 実利用者3〜5名に「OneDrive の容量を気にするか」を確認。暫定は `BesideProject { keep: 3 }` |
| 2 | Deflate バックエンド | `flate2` 既定の `miniz_oxide`（純 Rust、遅い）/ `zlib-rs`（純 Rust、速い）/ `zlib-ng`（C 依存） | 基準規模で実測。純 Rust を維持したいので `zlib-rs` が第一候補。40〜60 ms が 25 ms になるなら採用 |
| 3 | 圧縮アルゴリズム | Deflate（どの zip ツールでも開ける）/ Zstd（速くて小さいが zip の互換性が落ちる） | 「利用者が中身を zip ツールで覗ける」ことをどこまで価値とみなすか。暫定は Deflate |
| 4 | 編集ジャーナルを 0.x で既定 ON にするか | ON（損失 100 ms）/ OFF（損失 30 s、実装とテストが軽い） | E3 の実装コストと、`Edit` の直列化サイズの実測。P0 では OFF、P1 で ON が現実的か |
| 5 | 外部参照の既定しきい値 64 MiB | 32 / 64 / 128 MiB | 典型的な参考音源のサイズ分布（3〜5分 MP3 で 5〜10 MB、WAV で 30〜90 MB）。64 MiB は「MP3 は埋め込み、WAV は選ばせる」になる |
| 6 | プロジェクトファイルを開いている間、ハンドルを保持するか | 保持（外部からの改変を防げる）/ 保持しない（同期クライアントと衝突しない） | OneDrive での実測。暫定は**保持しない**（サイドカーのロックで代替） |
| 7 | 同一ファイルの二重オープンの扱い | 2つ目は読み取り専用 / 警告のみで両方編集可 / 2つ目を拒否 | 43 の複数ウィンドウ方針次第 |
| 8 | 書き込み後検証の既定 ON/OFF | ON（+2 ms、AV・同期環境で実際に効く）/ OFF | T-MANUAL で「検証が実際に不一致を捕まえるか」を観測してから確定。暫定 ON |
| 9 | `.drill.json` の書き出しを恒久サポートするか | 相互運用の入口として残す / `.drillproj` へ一本化して 52 のエクスポータに寄せる | 52 の方針決定待ち |
| 10 | サムネイル（`thumbnails/preview.png`）を 0.x で生成するか | 生成する（エクスプローラのサムネイルハンドラ 53 と、復旧ダイアログのプレビューに使える）/ 後回し | `drill-render` のヘッドレスラスタライズ（B-1）の完成時期 |
| 11 | 復旧候補の保持期間 7 日 / 30 日 | — | ディスク使用量の実測。`sessions/` 2 GiB の枠内で決める |
| 12 | `ERROR_CLOUD_FILE_*` のうちどれを一過性とみなすか | — | `windows-sys` の定数一覧を精査し、OneDrive で実際に返る値を T-MANUAL で採取する。数値をハードコードせず定数名で参照する方針は確定 |
