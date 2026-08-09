# 42. エラー型・i18n・ローカライズ

## 1. 目的と範囲

現状 `drill-core` は `Result<_, String>` でエラーを返し、しかも日本語の文字列リテラルが
モデルの奥深くに埋め込まれている。これは `ARCHITECTURE.md` が定める「core は UI に依存しない」
という前提に反しないが（`String` 自体はUI非依存）、`PRODUCT_QUALITY.md` が要求する
「日本語・英語UI」を実現する手段が存在しないという意味で実質的な違反である。英語UIを出そうとすると
`drill-core` のロジックまで書き換える必要が生じる。

この文書が確定させるもの:

1. `DrillError`（構造化データを持つ、`std::error::Error` を実装するエラー型）の全バリアント。
2. `Locale` とメッセージカタログの方式（コンパイル時網羅性検査つき）。
3. 翻訳対象の分類（エラー / UIラベル / 座標読み上げ / 帳票見出し / 数値・単位書式）。
4. `drill-core` 内の日本語文字列リテラルを排除する具体的な移行手順（ファイル・行番号つき）。
5. `drill-app` 側でのロケール切替・フォント・レイアウトの扱い。

この文書が扱わないもの:

- 座標読み上げの記譜規約そのもの（[16-coordinates-notation.md](16-coordinates-notation.md) の担当）。
  本書は「`Locale` を受け取ってどう文字列を組み立てるか」の型と関数シグネチャのみを扱う。
- 帳票のレイアウト・ページ割り（[17-printing-drillbook.md](17-printing-drillbook.md) の担当）。
  本書は見出し文字列のカタログ化のみを扱う。
- スキーマ v1→v2 マイグレーションの写像規則そのもの（DESIGN_GAPS.md A-7 の担当）。
  本書は `SchemaError` の型だけを定義し、`migrate()` の実装は踏み込まない。
- egui のウィジェット配置・配色・警告の視覚表現（[43-app-structure-ux.md](43-app-structure-ux.md) の担当）。
  本書は警告文言の語彙（重大度の名前）とテキスト構造のみを扱う。

## 2. 現状

### 2.1 `Result<_, String>` の全箇所（`crates/drill-core/src/*.rs`）

```
crates/drill-core/src/lib.rs:302   pub fn validate(&self) -> Result<(), String>
crates/drill-core/src/lib.rs:399   pub fn from_json(json: &str) -> Result<Self, String>
crates/drill-core/src/audio.rs:65  pub fn validate(&self) -> Result<(), String>
crates/drill-core/src/video.rs:103 pub fn validate(&self) -> Result<(), String>
crates/drill-core/src/video.rs:161 pub fn ffmpeg_args(...) -> Result<Vec<String>, String>
```

`lib.rs:396` の `to_json(&self) -> Result<String, serde_json::Error>` は例外的に型付きエラーを
返しているが、`from_json` はそれを `.map_err(|e| e.to_string())` で握りつぶしている（`lib.rs:400`）。

`crates/drill-app/src/main.rs` はこれらを受けて、さらに日本語のプレフィックスを付けて
`self.status: String` に格納している（例: `main.rs:245` `"保存しました: {}"`、`main.rs:257`
`"書き出しエラー: {error}"`、`main.rs:426` `"保存エラー: {error}"`、`main.rs:455`
`"読込エラー: {error}"`）。ローカライズ不能な文字列が二重（core側の理由文＋app側の接頭辞）に
埋まっている。

`editing.rs` / `camera.rs` / `pathing.rs` / `playback.rs` / `tempo.rs` / `shapes.rs` /
`countsheet.rs` に `Result` を返す公開関数は無く、日本語リテラルも無い（確認済み、変更不要）。

### 2.2 `drill-core` 内の日本語文字列リテラルの全箇所

**`lib.rs`（データに埋まった表示名 — `GridConfig::default()` / `demo()`）**

| 行 | 内容 |
|---|---|
| 75 | `GridLine { label: "フロントハッシュ".into(), .. }`（`GridConfig::default`） |
| 80 | `GridLine { label: "バックハッシュ".into(), .. }`（`GridConfig::default`） |
| 110 | `GridLine { label: "センターライン".into(), .. }`（`GridConfig::soccer`） |
| 282 | `title: "新しいドリル".into()`（`Document::demo`） |
| 289 | `name: "セット 1".into()`（`Document::demo`） |
| 294 | `name: "セット 2".into()`（`Document::demo`） |

**`lib.rs`（`Document::validate` のエラー文言）**

| 行 | 内容 |
|---|---|
| 305 | `"未対応のファイルバージョンです: {}"` |
| 310 | `"セットがありません"` |
| 313 | `"グリッド寸法は正数である必要があります"` |
| 322 | `"セット {} の演者数が一致しません"` |
| 330 | `"演者IDが重複しています"` |

**`audio.rs`（`AudioTrack::validate` のエラー文言、65–79行）**

| 行 | 内容 |
|---|---|
| 67 | `"音源の長さが不正です"` |
| 70 | `"トリム開始位置が音源範囲外です"` |
| 73 | `"トリム終了位置が開始位置より前です"` |
| 76 | `"フェード時間は0以上にしてください"` |

**`video.rs`（`VideoExportConfig::validate` のエラー文言、103–129行）**

| 行 | 内容 |
|---|---|
| 109 | `"解像度の幅と高さは2以上の偶数にしてください"` |
| 112 | `"FPSは1〜240にしてください"` |
| 117 | `"WebMではAV1またはVP9を選択してください"` |
| 120 | `"VP9はWebMコンテナで使用してください"` |
| 123 | `"品質値は0〜51にしてください"` |
| 126 | `"ビットレートを1kbps以上にしてください"` |

**`coordinates.rs`（座標読み上げの語彙、`side_to_side` / `front_to_back` / `readable`）**

行 65/67（`"サイド1"` / `"サイド2"`）、73/75（`"{}ヤードラインちょうど"`）、78（`"外側"` /
`"内側"`）、80（組み立てテンプレート）、98/102/113（`"フロントサイドライン"` /
`"バックサイドライン"`）、117/119/120（`"ちょうど"` / `"後ろ"` / `"前"`）、127
（`readable` の区切り文字 `"、"`）、149（`performer_sheet` の `"{}カウント"`）。

**`continuity.rs`（8方向名と説明文、`direction_name` / `segment` / `continuity_text`）**

行 67/69（`"右"` / `"左"`）、75/77（`"前"` / `"後ろ"`）、83（`"静止"`）、128/135
（`"セット{}→{}: {}カウント静止"` / `"セット{}→{}: {}カウントで{}方へ {:.1}歩 ..."`）。

**`svg.rs`（帳票見出し — [17-printing-drillbook.md](17-printing-drillbook.md) と連携）**

行 249/288（`<html lang="ja">` 固定）、252（`"{} — 座標シート"`）、259–260
（`"演者" "セット" "カウント" "座標"`）、291（`"{} — ドットブック"`）、301–302
（`"セット" "カウント" "座標"`）。

### 2.3 依存関係の現状

`crates/drill-core/Cargo.toml` は `serde` と `serde_json` のみに依存する
（`Cargo.toml:7-9`）。ベンチ用に `benches/` があるが本体の依存には影響しない。
`00-conventions.md` のクレート境界表はこれを「依存は serde/serde_json のみ」と明記しており、
本設計はこの制約を破らない（§3.6 で判断根拠を述べる）。

## 3. 設計

### 3.1 `Locale`

```rust
// crates/drill-core/src/locale.rs

/// Display language for all human-readable text produced by drill-core.
/// Never persisted as part of `Document` — see §4 不変条件.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Locale {
    Ja,
    En,
}

impl Locale {
    pub const ALL: [Locale; 2] = [Locale::Ja, Locale::En];
}

impl Default for Locale {
    /// Used only by call sites that cannot thread a locale through (e.g. `Display`
    /// for logs). UI code must always pass an explicit `Locale`, never rely on this.
    fn default() -> Self {
        Locale::En
    }
}
```

`Ja` / `En` の2値から始める。00-conventions.md の言語表に「将来ドイツ語のような長い言語を
足す余地」（要件7）とあるため、`enum` は `#[non_exhaustive]` にはしない（外部クレートではなく
本リポジトリ内で完結するため、新ロケール追加時は全カタログ関数がコンパイルエラーで洗い出される
ほうが安全 — §3.2 参照）。

### 3.2 メッセージカタログの方式（比較と選定）

候補は3つ。

| 方式 | 翻訳網羅性の検査 | 追加コスト | 依存 |
|---|---|---|---|
| A. 文字列キー + 実行時マップ（`HashMap<&str, &str>` や fluent 系） | 実行時（キー欠落は起動時 or 表示時にしか分からない） | キー文字列のタイプミスが型で捕まらない | fluent なら新規クレート数個 |
| B. `enum` バリアント + `match` を **`Locale` ごとに1本ずつ** 用意（`fn ja(&self) -> &str` / `fn en(&self) -> &str`） | 新バリアント追加時、片方の `match` を更新し忘れても**もう片方はコンパイルが通ってしまう** | 低 | なし |
| C. `enum` バリアント + `match` を **`Locale` について1本**、その中で `match locale { Ja => .., En => .. }` を**ネスト**して両方書く | 新バリアント追加時、外側の `match` にワイルドカード `_` を使わない限り**両ロケール分の文面を同時に書かないとコンパイルが通らない** | 低 | なし |

**選定: C。** ワイルドカードアーム (`_ => ...`) を禁止する運用ルール（レビューで機械的に検査可能、
かつ `#[deny(unreachable_patterns)]` 等ではなく人間のコードレビューで担保）のもとでは、
`DrillError` や UI キーの `enum` に新バリアントを足した瞬間、そのバリアントを扱う全ての
`match` 式が「非網羅」でコンパイルエラーになる。これは Rust の網羅性検査がそのまま
「訳し忘れをビルドで落とす」機構になるということであり、要件と合致する。
fluent 等の外部クレートは実行時にファイルをパースしてキーを引くため、キー名のタイプミストが
実行時まで分からず、`drill-core` に新規依存も増える。A/Bはどちらも要件（コンパイル時網羅性）
を満たさないため却下する。

```rust
// crates/drill-core/src/locale.rs (つづき)

/// Implemented by every enum whose variants need locale-resolved text.
/// The `match` inside each impl must switch on `Locale` with no wildcard arm —
/// that is what makes a missing translation a compile error.
pub trait Localized {
    fn text(&self, locale: Locale) -> String;
}
```

`drill-core` 側は `DrillError` とその子エラー、座標読み上げの語彙 (`HashLabel`, `Direction8`,
`YardSide`) がこのトレイトを実装する。`drill-app` 側は UI ラベル用の `enum UiText { .. }` が
同じトレイトを実装する（後述 §3.7）。トレイト自体は `&str`/`String` のみを扱い、追加の依存を
要求しない。

### 3.3 `DrillError`: ドメイン別ネスト + 単一エントリポイント

判断: **1階層だけネストする**。フラットな数十バリアントの単一 `enum` は、
どのモジュールがどのエラーを持つかが `DrillError` 定義を読まないと分からなくなる。
一方で `Document(Box<DocumentError>)` のように何段も潜らせると `?` の変換
（`From<DocumentError> for DrillError`）を書く手間だけが増えて可読性が上がらない。
モジュール1つにつき1つの子 `enum` を対応させ、`DrillError` はそれらを列挙するだけの
薄いラッパーにする。

```rust
// crates/drill-core/src/error.rs

use crate::locale::{Locale, Localized};
use crate::PerformerId;
use crate::video::{VideoCodec, VideoContainer};

/// Every fallible public API in drill-core returns `Result<_, DrillError>`.
/// See 00-conventions.md 不変条件 8.
#[derive(Clone, Debug, PartialEq)]
pub enum DrillError {
    Document(DocumentError),
    Audio(AudioError),
    Video(VideoError),
    Schema(SchemaError),
}

/// `Document::validate` / `Document::from_json` failures.
#[derive(Clone, Debug, PartialEq)]
pub enum DocumentError {
    EmptySets,
    InvalidGrid { width: f32, height: f32 },
    SetSizeMismatch { set_index: usize, expected: usize, found: usize },
    DuplicatePerformerId { id: PerformerId, first_index: usize, duplicate_index: usize },
}

/// `AudioTrack::validate` failures.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioError {
    InvalidDuration { seconds: f32 },
    TrimStartOutOfRange { start: f32, duration: f32 },
    TrimEndBeforeStart { start: f32, end: f32 },
    NegativeFade { fade_in: f32, fade_out: f32 },
}

/// `VideoExportConfig::validate` / `ffmpeg_args` failures.
#[derive(Clone, Debug, PartialEq)]
pub enum VideoError {
    InvalidResolution { width: u32, height: u32 },
    InvalidFps { fps: u32 },
    CodecRequiresContainer { codec: VideoCodec, needs_one_of: &'static [VideoCodec] },
    CodecWrongContainer { codec: VideoCodec, required_container: VideoContainer },
    QualityOutOfRange { quality: u8, max: u8 },
    ZeroBitrate,
}

/// Schema-version and JSON-parse failures (`Document::from_json`, and the
/// future `migrate()` of DESIGN_GAPS.md A-7).
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaError {
    UnsupportedSchema { found: u16, supported: u16 },
    /// `serde_json::Error::to_string()`. Kept as an opaque string rather than
    /// a typed source: `serde_json::Error` implements neither `Clone` nor
    /// `PartialEq`, and DrillError needs both (undo/redo history, tests using
    /// `assert_eq!`). See §3.5 for the source-chain trade-off this implies.
    Malformed(String),
}

impl From<DocumentError> for DrillError {
    fn from(e: DocumentError) -> Self { DrillError::Document(e) }
}
impl From<AudioError> for DrillError {
    fn from(e: AudioError) -> Self { DrillError::Audio(e) }
}
impl From<VideoError> for DrillError {
    fn from(e: VideoError) -> Self { DrillError::Video(e) }
}
impl From<SchemaError> for DrillError {
    fn from(e: SchemaError) -> Self { DrillError::Schema(e) }
}
```

`DuplicatePerformerId` は現行コード（`lib.rs:324-331`）が集合の要素数比較しかしていないため
「どの ID が重複か」を持てない。移行時に検出ロジックを直す（§5 の手順3）。

### 3.4 メッセージ生成とエラー本体の分離: 「原因＋対処」

構造化データ (`DrillError`) と表示文字列 (`describe`) を分離する。1つのエラーに対し、
利用者に伝えるテキストは常に「原因」と「次に何をすればよいか（対処）」の2段で構成する。

```rust
// crates/drill-core/src/error.rs (つづき)

/// Cause + remedy, ready to show in a dialog or status line.
#[derive(Clone, Debug, PartialEq)]
pub struct ErrorText {
    /// What happened, in concrete terms (includes the numbers involved).
    pub cause: String,
    /// What the user should do next. Never empty — if there is truly nothing
    /// actionable, this states the safe fallback (e.g. "元のファイルは変更されていません").
    pub remedy: String,
}

impl ErrorText {
    /// Single-line form for status bars / log lines: "{cause} {remedy}".
    pub fn to_line(&self) -> String {
        format!("{} {}", self.cause, self.remedy)
    }
}

impl DrillError {
    pub fn describe(&self, locale: Locale) -> ErrorText {
        match self {
            DrillError::Document(e) => e.describe(locale),
            DrillError::Audio(e) => e.describe(locale),
            DrillError::Video(e) => e.describe(locale),
            DrillError::Schema(e) => e.describe(locale),
        }
    }

    /// Machine-stable identifier for crash reports / bug tickets. Never
    /// localized, never shown to the user as the primary message.
    pub fn code(&self) -> &'static str {
        match self {
            DrillError::Document(DocumentError::EmptySets) => "document.empty_sets",
            DrillError::Document(DocumentError::InvalidGrid { .. }) => "document.invalid_grid",
            DrillError::Document(DocumentError::SetSizeMismatch { .. }) => "document.set_size_mismatch",
            DrillError::Document(DocumentError::DuplicatePerformerId { .. }) => "document.duplicate_performer_id",
            DrillError::Audio(AudioError::InvalidDuration { .. }) => "audio.invalid_duration",
            DrillError::Audio(AudioError::TrimStartOutOfRange { .. }) => "audio.trim_start_out_of_range",
            DrillError::Audio(AudioError::TrimEndBeforeStart { .. }) => "audio.trim_end_before_start",
            DrillError::Audio(AudioError::NegativeFade { .. }) => "audio.negative_fade",
            DrillError::Video(VideoError::InvalidResolution { .. }) => "video.invalid_resolution",
            DrillError::Video(VideoError::InvalidFps { .. }) => "video.invalid_fps",
            DrillError::Video(VideoError::CodecRequiresContainer { .. }) => "video.codec_requires_container",
            DrillError::Video(VideoError::CodecWrongContainer { .. }) => "video.codec_wrong_container",
            DrillError::Video(VideoError::QualityOutOfRange { .. }) => "video.quality_out_of_range",
            DrillError::Video(VideoError::ZeroBitrate) => "video.zero_bitrate",
            DrillError::Schema(SchemaError::UnsupportedSchema { .. }) => "schema.unsupported",
            DrillError::Schema(SchemaError::Malformed(_)) => "schema.malformed",
        }
    }
}
```

`DocumentError::describe` の実装例（他の子エラーも同じ形。ロケールについてワイルドカードを
使わないのが§3.2の規約）:

```rust
impl DocumentError {
    fn describe(&self, locale: Locale) -> ErrorText {
        match self {
            DocumentError::EmptySets => match locale {
                Locale::Ja => ErrorText {
                    cause: "セットが1つもありません。".into(),
                    remedy: "少なくとも1つセットを追加してから保存してください。".into(),
                },
                Locale::En => ErrorText {
                    cause: "This drill has no sets.".into(),
                    remedy: "Add at least one set before saving.".into(),
                },
            },
            DocumentError::InvalidGrid { width, height } => match locale {
                Locale::Ja => ErrorText {
                    cause: format!("グリッド寸法が不正です（幅 {width}、高さ {height}）。"),
                    remedy: "グリッド設定で幅と高さを正の数にしてください。".into(),
                },
                Locale::En => ErrorText {
                    cause: format!("Invalid field size (width {width}, height {height})."),
                    remedy: "Set both width and height to a positive number in Grid Settings.".into(),
                },
            },
            DocumentError::SetSizeMismatch { set_index, expected, found } => match locale {
                Locale::Ja => ErrorText {
                    cause: format!(
                        "セット{}の演者数が一致しません（期待値 {expected} 名、実際 {found} 名）。",
                        set_index + 1
                    ),
                    remedy: "そのセットに演者を追加または削除して人数を揃えてください。".into(),
                },
                Locale::En => ErrorText {
                    cause: format!(
                        "Set {} has the wrong number of performers (expected {expected}, found {found}).",
                        set_index + 1
                    ),
                    remedy: "Add or remove performers in that set so the counts match.".into(),
                },
            },
            DocumentError::DuplicatePerformerId { id, first_index, duplicate_index } => match locale {
                Locale::Ja => ErrorText {
                    cause: format!(
                        "演者ID {id} が重複しています（{}番目と{}番目）。",
                        first_index + 1, duplicate_index + 1
                    ),
                    remedy: "重複した演者のどちらかのIDを変更してください。".into(),
                },
                Locale::En => ErrorText {
                    cause: format!(
                        "Performer ID {id} is used twice (performer #{} and #{}).",
                        first_index + 1, duplicate_index + 1
                    ),
                    remedy: "Change the ID on one of the two performers.".into(),
                },
            },
        }
    }
}
```

`SchemaError::Malformed(detail)` の `detail`（`serde_json::Error` のメッセージ）は英語の
技術的文言であり翻訳しない。「対処」文だけをロケール化し、`cause` に生の `detail` を埋め込む:

```rust
SchemaError::Malformed(detail) => match locale {
    Locale::Ja => ErrorText {
        cause: format!("ファイルを読み込めませんでした（{detail}）。"),
        remedy: "ファイルが破損している可能性があります。バックアップから復元してください。".into(),
    },
    Locale::En => ErrorText {
        cause: format!("Could not read the file ({detail})."),
        remedy: "The file may be corrupted. Restore from a backup.".into(),
    },
},
```

### 3.5 `std::error::Error` 実装と `thiserror` を使わない判断

```rust
impl std::fmt::Display for DrillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Display / std::error::Error is for logs and panic messages, which are
        // developer-facing, not user-facing — always English, never localized.
        // UI code must call `.describe(locale)` and never `.to_string()`.
        write!(f, "{}", self.describe(Locale::En).to_line())
    }
}

impl std::error::Error for DrillError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // All current variants are leaves holding plain data (numbers, IDs,
        // pre-stringified serde_json messages) — there is no typed inner
        // `std::error::Error` to return. If a future variant wraps another
        // error type that must keep its own `Error` impl reachable, hold it as
        // `Arc<dyn std::error::Error + Send + Sync>` (Clone-able, unlike a
        // bare `Box`) rather than adding thiserror.
        None
    }
}
```

**`thiserror` を追加しない。** 00-conventions.md のクレート境界表は
`drill-core` の依存を「serde/serde_json のみ」と明記しており、これは設計文書が覆せない
既定の境界である（同ファイル冒頭）。`thiserror` はコンパイル時間を増やす手続き型マクロであり、
バリアント数が現時点で15個程度（§3.3）と少ないため、`Display`/`Error` を手書きするコストは
低い。将来バリアントが増えて手書きが本当に負担になった時点で、`drill-core` の依存を増やす
判断を別途行う（本書はその判断を先取りしない）。

### 3.6 依存を増やさずに実現できるか

**できる。** `Locale` / `DrillError` / メッセージカタログ / `Localized` トレイトは
すべて `enum` と `match` と `String` のみで書け、新規クレートを要求しない。
`drill-core` の `Cargo.toml` は `serde` / `serde_json` のまま変更しない
（`DrillError` 自体は `Document` の一部として永続化されないため `Serialize`/`Deserialize`
も不要 — §4 不変条件2）。

### 3.7 翻訳対象の分類

| 分類 | 例 | 解決タイミング | 保存されるか |
|---|---|---|---|
| エラーメッセージ | `DrillError::describe(locale)` | 表示時 | されない（構造化データのみ保存） |
| UIラベル（ボタン・メニュー・ダイアログ） | `drill-app` の `UiText` カタログ（§3.9） | 表示時（毎フレーム） | されない |
| 座標読み上げ | `coordinates::readable(point, grid, locale)` | 表示・帳票生成時 | されない（`Point` の数値のみ保存） |
| 帳票見出し | `svg::coordinate_sheet_html(doc, locale)` の `<title>`/`<th>` | 帳票生成時 | されない（HTML/SVGは生成物） |
| 数値・単位の書式 | 歩数の小数表示、ヤード/メートルの単位語 | 表示・帳票生成時 | されない（`f32` の生値のみ保存） |
| ユーザー入力の自由記述 | `Document.title`、`Set.name`、`Performer.label`、`HashLabel::Custom` | — | **される**。翻訳対象ではない（ユーザーの母語のまま） |

最後の行が重要な区別: `Set.name` のような「利用者が打った文字列」は翻訳の対象にしない
（ユーザーが日本語で名付けたセット名を英語UIに切り替えても英訳しない、翻訳エンジンではない）。
翻訳の対象は「アプリが生成する定型文」だけである。

### 3.8 「データに埋まった表示名」の解消: `HashLabel`

`GridConfig::default()` の `label: "フロントハッシュ".into()`（`lib.rs:75/80/110`）は
組み込みの定型語がそのまま `Document`（保存対象）に書き込まれてしまっている。ロケールを
切り替えても保存済みファイルの表示名は日本語のまま、という不変条件違反（§4-3）を起こす。

```rust
// crates/drill-core/src/lib.rs

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum HashLabel {
    /// Built-in role — display text is resolved from `Locale` at read time,
    /// never stored as text.
    Front,
    Back,
    Center,
    /// User-typed name. Stored and shown verbatim in every locale (§3.7).
    Custom(String),
}

impl HashLabel {
    pub fn text(&self, locale: Locale) -> String {
        match self {
            HashLabel::Front => match locale {
                Locale::Ja => "フロントハッシュ".into(),
                Locale::En => "Front hash".into(),
            },
            HashLabel::Back => match locale {
                Locale::Ja => "バックハッシュ".into(),
                Locale::En => "Back hash".into(),
            },
            HashLabel::Center => match locale {
                Locale::Ja => "センターライン".into(),
                Locale::En => "Center line".into(),
            },
            HashLabel::Custom(name) => name.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridLine {
    pub position: f32,
    pub label: HashLabel, // was: `pub label: String`
    pub weight: f32,
}
```

保存フォーマット（JSON）は `label` フィールドの型が変わるため、これはスキーマ変更であり
DESIGN_GAPS.md A-7 のマイグレーション（v1→v2）に一枚追加する: 旧 `label: String` が
`"フロントハッシュ"` / `"バックハッシュ"` / `"センターライン"` と完全一致すれば対応する
組み込みバリアントへ、それ以外は `HashLabel::Custom(旧文字列)` へ写す。

`Document::demo()` の `title: "新しいドリル"` / `name: "セット 1"` も同種の問題だが、
こちらは「ユーザーが最初に見る初期値であって、以後は自由記述として保存される」性質のものなので
`HashLabel` のように型を変える必要はない。代わりに `demo` 自体が `Locale` を受け取り、
呼び出し時のロケールで初期値を生成する（§5 手順1）。

### 3.9 座標読み上げ・帳票見出し・8方向名の `Locale` 化

`coordinates.rs` と `continuity.rs` は、語彙を `Localized` な小さい列挙型に置き換え、
組み立て関数は `locale: Locale` を最後の引数として受け取る。

```rust
// crates/drill-core/src/coordinates.rs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YardSide { Side1, Side2 }

impl YardSide {
    fn text(self, locale: Locale) -> &'static str {
        match self {
            YardSide::Side1 => match locale { Locale::Ja => "サイド1", Locale::En => "side 1" },
            YardSide::Side2 => match locale { Locale::Ja => "サイド2", Locale::En => "side 2" },
        }
    }
}

pub fn side_to_side(point: Point, grid: &GridConfig, locale: Locale) -> String { /* ... */ }
pub fn front_to_back(point: Point, grid: &GridConfig, locale: Locale) -> String { /* ... */ }
pub fn readable(point: Point, grid: &GridConfig, locale: Locale) -> String { /* ... */ }
pub fn performer_sheet(doc: &Document, performer_index: usize, locale: Locale) -> String { /* ... */ }
pub fn coordinates_csv(doc: &Document, locale: Locale) -> String { /* ... */ }
```

`front_to_back` は `grid.hashes[..].label` を `HashLabel::text(locale)` で解決するため、
`GridConfig` への依存とあわせて自然に `Locale` を通す形になる。

`continuity.rs` は「安定データ」と「表示文字列」を分離する。現行 `ContinuitySegment.direction:
String` は日本語決め打ちのため、テストが `"右"` のような文字列に依存していた
（`continuity.rs:208,226,237,244,254`）。これを安定な `enum` に変える:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction8 { N, NE, E, SE, S, SW, W, NW, Hold }

impl Direction8 {
    fn text(self, locale: Locale) -> &'static str {
        match self {
            // Field convention: -y = toward audience (前/forward), +x = 右/right.
            Direction8::N  => match locale { Locale::Ja => "前",   Locale::En => "upstage" },
            Direction8::S  => match locale { Locale::Ja => "後ろ", Locale::En => "downstage" },
            Direction8::E  => match locale { Locale::Ja => "右",   Locale::En => "right" },
            Direction8::W  => match locale { Locale::Ja => "左",   Locale::En => "left" },
            Direction8::NE => match locale { Locale::Ja => "右前", Locale::En => "upstage right" },
            Direction8::SE => match locale { Locale::Ja => "右後ろ", Locale::En => "downstage right" },
            Direction8::NW => match locale { Locale::Ja => "左前", Locale::En => "upstage left" },
            Direction8::SW => match locale { Locale::Ja => "左後ろ", Locale::En => "downstage left" },
            Direction8::Hold => match locale { Locale::Ja => "静止", Locale::En => "hold" },
        }
    }
}

pub struct ContinuitySegment {
    pub from_set: usize,
    pub to_set: usize,
    pub counts: u16,
    pub steps_x: f32,
    pub steps_y: f32,
    pub distance_steps: f32,
    pub direction: Direction8, // was: `pub direction: String`
    pub step_size_per_count: f32,
    // `description: String` field removed — text is generated on demand (below),
    // never stored, so it cannot go stale relative to `direction`/distances.
}

pub fn describe_segment(segment: &ContinuitySegment, locale: Locale) -> String {
    match locale {
        Locale::Ja if segment.direction == Direction8::Hold => format!(
            "セット{}→{}: {}カウント静止",
            segment.from_set + 1, segment.to_set + 1, segment.counts
        ),
        Locale::Ja => format!(
            "セット{}→{}: {}カウントで{}方へ {:.1}歩 ({:.2} steps/count)",
            segment.from_set + 1, segment.to_set + 1, segment.counts,
            segment.direction.text(locale), segment.distance_steps, segment.step_size_per_count
        ),
        Locale::En if segment.direction == Direction8::Hold => format!(
            "Set {}→{}: hold for {} counts",
            segment.from_set + 1, segment.to_set + 1, segment.counts
        ),
        Locale::En => format!(
            "Set {}→{}: {} counts {} for {:.1} steps ({:.2} steps/count)",
            segment.from_set + 1, segment.to_set + 1, segment.counts,
            segment.direction.text(locale), segment.distance_steps, segment.step_size_per_count
        ),
    }
}

pub fn continuity_text(doc: &Document, performer_index: usize, locale: Locale) -> String { /* ... */ }
```

`description` フィールドを構造体から削除したのは要件「メッセージ生成と分離する」の直接の帰結:
以前は `segment()` の中で `direction: String` と `description: String` を**同時に**組み立てて
おり、片方だけ更新すると矛盾したデータが保存されうる形をしていた（実際には保存されないが、
`Clone` して使い回すコードがあれば矛盾しうる）。安定データと表示文字列を分離すれば
矛盾自体が構造的に起こらない。

`svg.rs` の帳票見出しは同様に `locale: Locale` を引数に足す:

```rust
pub fn coordinate_sheet_html(doc: &Document, locale: Locale) -> String { /* ... */ }
pub fn drill_book_html(doc: &Document, locale: Locale) -> String { /* ... */ }
```

`<html lang="ja">` は `locale` に応じて `"ja"` / `"en"` を出す（`lang` 属性はブラウザの
読み上げ・フォント選択に影響するため、翻訳漏れとは別に必ず合わせる）。

### 3.10 数値・単位の書式化の集約

小数・単位の書式規則を1箇所（`crates/drill-core/src/format.rs`、新規）にまとめる。
既存の `coordinates.rs::fmt_num`（15–30行）と `continuity.rs` の `{:.1}`/`{:.2}` 直書きを
ここへ移す。

```rust
// crates/drill-core/src/format.rs

use crate::{Locale, Unit};

/// Drop a redundant fractional part: `2.0 -> "2"`, `1.5 -> "1.5"`.
/// Locale-independent: both ja/en drill notation use `.` as the decimal mark
/// and neither uses a thousands separator at these magnitudes (max ~4000
/// performers / few hundred yards — see PRODUCT_QUALITY.md upper bound).
pub fn fmt_steps(v: f32) -> String { /* moved from coordinates::fmt_num, unchanged */ }

/// The unit word for a distance, e.g. "5 yards" / "5メートル".
pub fn fmt_distance(value: f32, unit: Unit, locale: Locale) -> String {
    let number = fmt_steps(value);
    match (unit, locale) {
        (Unit::Yards, Locale::Ja) => format!("{number}ヤード"),
        (Unit::Yards, Locale::En) => format!("{number} yd"),
        (Unit::Meters, Locale::Ja) => format!("{number}メートル"),
        (Unit::Meters, Locale::En) => format!("{number} m"),
    }
}

/// "16 counts" / "16カウント".
pub fn fmt_counts(counts: u16, locale: Locale) -> String {
    match locale {
        Locale::Ja => format!("{counts}カウント"),
        Locale::En => format!("{counts} counts"),
    }
}
```

日付書式（自動保存タイムスタンプ、クラッシュレポート — [41-persistence.md](41-persistence.md)
と連携）は `drill-core` が時刻を扱わない（クレート境界表に `std::time`/OS 依存が無い）ため
本書の範囲外だが、同じ理由で「1箇所に集約する」原則は `drill-app` 側でも踏襲すべきと
申し送る（§9 未決事項）。

### 3.11 `drill-app` 側の適用

**UIラベルカタログ**（新規 `crates/drill-app/src/i18n.rs`）は §3.2 方式Cをそのまま使う:

```rust
use drill_core::locale::Locale;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiText {
    MenuFileOpen,
    MenuFileSave,
    StatusSaved { path_shown_separately: () }, // see note below
    StatusOpenFailed,
    // ... one variant per UI-owned string
}

impl UiText {
    pub fn text(self, locale: Locale) -> &'static str {
        match self {
            UiText::MenuFileOpen => match locale { Locale::Ja => "開く", Locale::En => "Open" },
            UiText::MenuFileSave => match locale { Locale::Ja => "保存", Locale::En => "Save" },
            // ...
        }
    }
}
```

可変部分（ファイルパス、エラー詳細）を `enum` バリアントの `&'static str` に埋め込むことは
できない。`main.rs:245` の `"保存しました: {}"` のような組み立ては、定型句を `UiText` から取り、
可変部分は呼び出し側で `format!` する:

```rust
self.status = format!("{} {}", UiText::StatusSaved.text(self.locale), path.display());
```

`self.status: String` フィールド自体はロケール混在を防ぐため、**保存せず毎回その場で
組み立てる**（既存コードは既にその場組み立てなので変更不要、ロケール切替時に再構築されるのは
次の操作時 — 直前の状態メッセージが旧ロケールのまま残る点は許容する。常時再翻訳したければ
`status` を `Option<StatusKey>` に変えて描画時に解決する設計へ拡張できるが、現状の
ステータスバーの使われ方（一時的な通知）では過剰）。

**フォント**: `assets/NotoSansJP.ttf`（`main.rs:34-49` で `Proportional`/`Monospace` 双方の
先頭フォントとして登録済み）は日本語・ラテン文字の両方のグリフを含む OpenType フォントであり、
英語UIでも同じフォントで正しく表示できる。ロケール切替時にフォントを差し替える必要は無い。
将来ハングル・キリル文字等、Noto Sans JP がカバーしない文字集合の言語を追加する場合のみ、
`install_fonts` にフォールバックフォントを追加する（`fonts.families` の同じ `Vec` に
追記するだけで、egui は先頭から探して見つかった最初のフォントで描画するため既存コードの
構造を変えずに追加できる）。

**レイアウト崩れ**: 現状 `main.rs` のボタン・ラベルは固定ピクセル幅を指定していない箇所が
大半（egui のデフォルトは内容に合わせて自動サイズ）。ドイツ語のような複合語で長くなる言語を
将来足す際に備え、次を新規UIコードのレビュー基準にする（既存コードへの変更はこの文書の
スコープ外 — 実装は担当43 or 実装タスクで行う）:

- ボタン・タブに `min_width` は付けてよいが `max_width` で文字を切り詰めない。
- 固定幅の `egui::Grid` 列は使わず、`Ui::available_width()` に対する比率で確保する。
- 長い文字列が入るテスト用ロケール（例: 全ラベルの前後に `"⟦...⟧"` を付ける疑似ロケール）を
  `#[cfg(test)]` の `Locale` バリアントとして用意する余地を残す（本書では追加しない — 要件7
  への布石として型だけ `#[non_exhaustive]` にしない判断を確認済み、§3.1）。

### 3.12 言語切替は再起動不要・保存内容に影響しない

不変条件として設計する（§4 でテスト可能な形に落とす）:

1. `Locale` は `DrillApp` の実行時状態（例: `self.locale: Locale`）としてのみ存在し、
   `Document` のどのフィールドにも書き込まれない。
2. `Document` に保存される文字列は「ユーザーの自由記述」（`title`/`Set.name`/
   `Performer.label`/`HashLabel::Custom`）と「翻訳不要の識別子・数値」のみ。
   組み込み定型語（ハッシュ名、方向名、エラー文言）は列挙型として保存され、
   表示文字列は**保存されない**（§3.8, §3.9）。
3. egui は毎フレーム全体を再構築する即時モードGUIのため、`self.locale` を書き換えた
   次のフレームから全ラベルが新ロケールで描画される。追加の「再描画」処理は不要。

### 3.13 アクセシビリティ関連: 重大度の語彙統一

`PRODUCT_QUALITY.md`「UX and accessibility」節（37–42行目）の「色だけに依存しない」要求のうち、
テキスト側を本書が担当する（形・アイコン側は
[43-app-structure-ux.md](43-app-structure-ux.md)）。

```rust
// crates/drill-core/src/error.rs (つづき)

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity { Info, Warning, Error }

impl Severity {
    pub fn label(self, locale: Locale) -> &'static str {
        match self {
            Severity::Info => match locale { Locale::Ja => "情報", Locale::En => "Info" },
            Severity::Warning => match locale { Locale::Ja => "警告", Locale::En => "Warning" },
            Severity::Error => match locale { Locale::Ja => "エラー", Locale::En => "Error" },
        }
    }
}

impl DrillError {
    /// All current variants are hard failures — validation/save/load errors
    /// that block the operation. `Warning` is reserved for future analysis
    /// results (e.g. collision/stride warnings from `analyze_transition`,
    /// see 13-collision-analysis.md) that do not block anything.
    pub fn severity(&self) -> Severity {
        Severity::Error
    }
}
```

`Severity::label` を常に `Severity` の名前と併記する運用にすることで、色分けが認識できない
利用者にも「これは警告なのかエラーなのか」がテキストで伝わる。ツールバー等での実際の
アイコン割り当ては43番に委ねる。

## 4. 不変条件

1. `drill-core` の公開関数で人間可読テキストを返すものは、最後の引数として `Locale` を取る
   （00-conventions.md 不変条件7）。例外は無い。テストで全公開関数のシグネチャを目視確認する
   （コンパイラは「取り忘れ」を検出できないため、実装タスクのレビューチェックリストに入れる）。
2. `Document` の `Serialize`/`Deserialize` 経路に `Locale` も `DrillError` も現れない。
   `cargo doc` で `Document` の全フィールド（再帰的に）を目視し、`Locale` 型のフィールドが
   無いことをテストで担保する（`Document` の JSON スキーマに `"locale"` キーが出ないことを
   `serde_json::to_value` の結果で assert する）。
3. 同じ `Document` を `Locale::Ja` で開いても `Locale::En` で開いても、`positions` / `counts`
   / `id` など数値データは完全一致する（テキスト表示のみが変わる）。
4. `DrillError` の `match locale { .. }` はワイルドカードアーム (`_`) を持たない
   （レビュー基準。`cargo clippy` の `wildcard_enum_match_arm` を該当ファイルに限定して
   `#[warn]` することで機械チェックに格上げできる — §8 実装タスク）。
5. 全ての `DrillError::describe` は `cause` と `remedy` の両方を非空文字列で返す。
6. `HashLabel::Custom(name)` の `name` はロケールに関わらず不変（§3.7 の「自由記述は
   翻訳しない」の直接のテスト対象）。

## 5. 性能

`describe(locale)` は `format!` を数回呼ぶだけの O(1) 処理であり、エラーパス（ホットパスでは
ない: 保存失敗・バリデーション失敗は毎フレーム発生しない）でのみ呼ばれる。
`readable`/`continuity_text`/帳票生成は既存の非エラーパスの文字列組み立てと同じ計算量
（O(1) per point, O(sets) per performer, O(performers × sets) for CSV/HTML）であり、
`Locale` の追加は分岐が1段増えるだけで漸近計算量に影響しない。16.6ms 予算への影響は
測定不能な水準（既存 `coordinates.rs`/`continuity.rs` はいずれも再生ループの外、
帳票・CSV生成時にのみ呼ばれる）。

`UiText::text` は `&'static str` を返す `match` のみで、egui の毎フレーム呼び出しに対しても
ヒープ確保ゼロ（00-conventions.md 性能予算「再生中のフレーム内ヒープ確保ゼロ」に抵触しない）。

## 6. 失敗モードと安全性

- **翻訳漏れ**: §3.2 方式Cの `match` 網羅性検査により、新バリアント追加時にビルドが落ちる。
  レビューでワイルドカードアームの追加を禁止する（§4-4）。
- **ロケールと保存データの混線**: `HashLabel` を `String` から `enum` へ変えたことで、
  組み込み定型語が保存データに紛れ込む経路を型レベルで塞ぐ（§3.8）。新しい組み込み定型語を
  追加する開発者が誤って `label: "...".into()` を書けないよう、`GridLine::label` の型を
  `HashLabel` に固定する。
- **信頼できない入力とエラー文言**: `SchemaError::Malformed(detail)` の `detail` は
  外部ファイル（他人から受け取ったプロジェクトファイル）由来の `serde_json::Error` メッセージを
  含む。これは経路情報やバイト列を含みうるが機密情報ではない（00-conventions.md の
  「信頼できない入力」節は主にパニック・サイズ上限の話であり、本書はエラー文言に
  ファイル内容の値そのものを埋め込まない — 埋め込むのは行番号・列番号程度の `serde_json`
  標準メッセージのみ）。
- **`Display`/`to_string()` の誤用**: `DrillError` に `Display` を実装する（`std::error::Error`
  境界のため必須）が、これは英語固定でありUIに直接出してはならない。`drill-app` 側の
  コードレビュー基準に「`DrillError` に対して `.to_string()` を呼ばない、必ず
  `.describe(locale)` を使う」を追加する（§8 実装タスクでlintまたはコメントとして明文化）。
- **パニック**: `describe`/`text` はいずれも全域関数（`match` は全バリアントを網羅、
  数値のフォーマットは `f32`/`u16` に対する `format!` で失敗しない）であり、`unwrap`/`expect`
  を使わない（00-conventions.md 安全性節のパニック禁止経路に合致）。

## 7. テスト計画

**単体テスト（`crates/drill-core/src/error.rs`）**

- `describe` が全 `DrillError` バリアント × 全 `Locale` について非空の `cause`/`remedy` を返す。
  代表インスタンスを1つずつ手で構築し、`Locale::ALL` でループする
  （enum のバリアント一覧を実行時に反射できないため、リストは手書きし、
  「新バリアントを追加したらこのリストにも足す」ことをコードコメントで明示する。
  これはビルドでは検出できない箇所であり、レビューチェックリストの項目として明文化する）。
- 同一バリアントについて `describe(Ja).cause != describe(En).cause` であることを assert する
  （コピペしてロケール分岐だけ忘れるミスを検出する）。
- `code()` が全バリアントに対しユニークな文字列を返す（`HashSet` に集めて長さ比較）。

**単体テスト（`coordinates.rs` / `continuity.rs`）**

- 既存テスト（`coordinates.rs:192-321`、`continuity.rs:200-299`）を `locale: Locale::Ja`
  引数追加のうえ維持し、期待文字列は変えない（振る舞いの回帰が無いことを保証）。
- 同じ入力に対する `Locale::En` 版を追加し、英語の期待文字列を新規に書く。
- `Direction8` を返す `continuity::segment` の内部関数について、8方向 + hold の全パターンを
  入力し、`text(Ja)`/`text(En)` が両方非空であることを確認する。

**プロパティテスト**

- `readable`/`continuity_text`/`coordinate_sheet_html` について、任意の `Locale` で
  呼び出しても `Document` 自体（`to_json()` の出力）が変化しないことをプロパティとして書く
  （「保存内容に影響しない」の直接的な回帰防止、§4-3）。

**ゴールデンテスト**

- `coordinate_sheet_html(doc, Locale::Ja)` / `Locale::En` それぞれの出力をスナップショットとして
  固定し、17番（帳票）の実装時に見出し文言が意図せず変わっていないか検出する。

**ストレス/網羅性テスト**

- ワークスペース全体を `grep`（CI ステップ）して `crates/drill-core/src/**/*.rs` に
  日本語の三点セット（ひらがな/カタカナ/漢字の Unicode 範囲）を含む行が `#[cfg(test)]`
  ブロックの外に存在しないことを検査するテスト or CI ジョブを追加する
  （§2.2 の再発防止。テストのコメント・docコメントに残す日本語例示は許容する必要があるため、
  完全な機械検査ではなく「新規追加分に対する差分検査」として運用する — §9 未決事項）。

**drill-app 側**

- `UiText::text` も同じ「全バリアント × 全ロケールで非空・ロケール間で異なる」テストを持つ。
- スナップショットテスト: `Locale::Ja` と `Locale::En` それぞれでウィンドウを1フレーム描画し、
  ステータスバーやメニューに想定外の言語文字列が混ざらないことを目視確認する
  （自動化は43番のUIテスト基盤に依存するため、本書は「テストがあるべき」ことのみ書く）。

## 8. 実装タスク

Codex に渡せる粒度（1タスク=1〜3時間）に分解する。依存は上から下へ（並行可能なものは
「並行可」と明記）。

1. **`crates/drill-core/src/locale.rs` を新規作成**: `Locale` enum、`Localized` トレイト。
   依存: 無し。
2. **`crates/drill-core/src/error.rs` を新規作成**: `DrillError`/`DocumentError`/`AudioError`/
   `VideoError`/`SchemaError`、`ErrorText`、`Severity`、`Display`/`Error` 実装、
   `From` 実装一式。依存: タスク1。
3. **`lib.rs` の `HashLabel` 導入**: `GridLine::label` の型変更、`GridConfig::default`/`soccer`
   の初期値を `HashLabel::Front`/`Back`/`Center` に、`Document::validate`/`from_json` を
   `Result<_, DrillError>` へ、重複ID検出ロジックを「最初のIDとインデックス」を返すよう修正。
   依存: タスク2。並行可: タスク4, 5, 6と。
4. **`audio.rs` の `validate` を `Result<(), AudioError>` へ**。依存: タスク2。並行可: 3, 5, 6。
5. **`video.rs` の `validate`/`ffmpeg_args` を `DrillError` へ**（`CodecRequiresContainer`/
   `CodecWrongContainer` の作り分け）。依存: タスク2。並行可: 3, 4, 6。
6. **`crates/drill-core/src/format.rs` を新規作成**: `fmt_steps`/`fmt_distance`/`fmt_counts`
   を `coordinates.rs::fmt_num` から移設。依存: タスク1。並行可: 3, 4, 5。
7. **`coordinates.rs` の `Locale` 化**: `YardSide`、`side_to_side`/`front_to_back`/`readable`/
   `performer_sheet`/`coordinates_csv` へ `locale` 引数追加、既存テストの更新 + 英語版テスト追加。
   依存: タスク3（`HashLabel::text`）, 6。
8. **`continuity.rs` の `Locale` 化**: `Direction8` 導入、`ContinuitySegment.direction` の型変更、
   `description` フィールド削除と `describe_segment` 関数への切り出し、既存テスト更新 + 英語版
   追加。依存: タスク1, 6。並行可: タスク7と。
9. **`svg.rs` の `Locale` 化**: `coordinate_sheet_html`/`drill_book_html` への引数追加、
   `lang` 属性の切替、見出し文言のカタログ化。依存: タスク7, 8（両方の出力を埋め込むため）。
10. **`crates/drill-app/src/i18n.rs` を新規作成**: `UiText` カタログの型と、
    `main.rs` 内で使われている全日本語文言（ステータスメッセージ・ダイアログのフィルタ名等）の
    洗い出しと変換。依存: タスク2（`DrillError::describe` を呼ぶため）。
11. **`main.rs` の `self.locale: Locale` フィールド追加とメニューからの切替UI**（再起動不要の
    確認込み）。依存: タスク10。
12. **日本語リテラル検査テスト/CIジョブの追加**（§7 ストレス/網羅性テスト）。依存: タスク3–9
    完了後（既存の意図的な日本語がすべてカタログ経由になった状態を基準線にするため）。
13. **DESIGN_GAPS.md A-7（マイグレーション）との接続点の申し送り**: `HashLabel` のスキーマ
    変更を v1→v2 マイグレーション写像に追記する変更依頼を A-7 の担当へ渡す
    （本書はA-7の実装そのものを書かない — 冒頭「扱わないもの」参照）。依存: タスク3。

## 9. 未決事項

- **`self.status: String` の完全な遅延評価化**: §3.11 で述べた通り、現状の一時的ステータス
  メッセージはロケール切替の瞬間に旧ロケールのまま残りうる。実害は小さい（次の操作で上書き
  される）と判断したが、43番（UX）がステータス表示の設計を変える場合はこの判断を再考する
  余地がある。
- **日付・タイムスタンプ書式の集約先**: `drill-core` は時刻を扱わないため、自動保存/クラッシュ
  レポートの日時表示書式（41番）をどのクレートの `format` 相当モジュールに置くかは未定。
  `drill-app` に `crates/drill-app/src/format.rs` を新設するか、`drill-core::format` の
  ロケール定数だけを再利用するかは41番の担当と合意する必要がある。
- **座標読み上げにおける「ヤードライン」という語の単位依存性**: `Unit::Meters`（サッカー場
  プリセット等）でも読み上げは "ヤードライン" 相当の語（マーチング業界の慣習語）を使うべきか、
  それとも `Unit` に応じて「メートルライン」のような語に変えるべきかは、16番（座標系と読み上げ・
  記譜規約）が決めるドメイン知識であり、本書は型（`Locale` を受け取ること）だけを用意した。
- **日本語リテラル残存検査の粒度**: §7 で「テストコード内の日本語は許容」と書いたが、
  doc コメント中の説明用日本語（今回は無いが将来混入しうる）を検査対象に含めるかは、
  50番（テスト・CI戦略）が全体のCI構成を決める際に確定させる。
- **`Locale::ALL` を使った反射的な網羅性テストの限界**: 新しい `DrillError` バリアントを
  追加した開発者が §7 の「代表インスタンスのリスト」を更新し忘れるとテストは静かに
  そのバリアントを検査しない。`enum` の全バリアントを列挙する仕組み（例: 手書きの
  `const ALL_FOR_TESTS: &[DrillError]`）をどこまで自動化するかは、外部クレート
  （`strum` 等）を導入するかどうかの判断を伴うため、drill-core の依存方針（§3.6）と
  合わせて別途判断する。
