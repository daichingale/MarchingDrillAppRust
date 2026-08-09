# 51. セキュリティと信頼できない入力

## 1. 目的と範囲

マーチング業界では、ドリル制作物（プロジェクトファイル・音源・座標表）を人から人へ渡すことが日常的に起きる。
デザイナーは指導者から、指導者は他校から、生徒は先輩から、実名の入ったファイルを受け取って開く。
したがって DrillForge にとって「他人が作ったファイルを開く」は例外操作ではなく**主要ユースケース**である。

本書は次を保証する設計を定める。

- **悪意あるファイルを開いても、任意コード実行・任意ファイル読み書き・情報漏えい・クラッシュが起きない。**
- **上限で弾いたとき、利用者は「何が起きたか」「自分に落ち度はないか」を理解できる。**
- **既定でネットワークへ出ない。実名を含む制作物が本人の意図なく端末外へ出ない。**

`00-conventions.md` の「安全性」節を全設計文書に展開する母体が本書であり、各文書はここで定義した
`Limits` / `SafeName` / `ExternalTool` / `atomic_write` を利用する側として記述する。

### 扱わないこと

| 範囲 | 担当 |
|---|---|
| `DrillError` のメッセージ本文と i18n テーブル | 42-errors-i18n.md（本書はエラー**種別**のみ定義） |
| インポーター個別の外部形式パーサ | 52-interop-plugins.md（本書はプラグイン**サンドボックス方式**のみ決定） |
| コード署名証明書の調達・価格・更新配信 UI | 53-productization.md（本書は検証手順のみ定義） |
| 保存フォーマットとマイグレーション | 41-persistence.md（本書は「読む前の検査」のみ） |
| 非同期ジョブの実行機構 | 40-jobs.md（本書は「検査はワーカーで走る」ことだけ要求） |

多人数同時編集・サーバ・アカウントは製品範囲外。したがって認証・認可・セッション管理は本書の対象外である。

---

## 2. 現状

リポジトリの現在のコードにおける実態。

### 2.1 入力検証

| 箇所 | 実態 |
|---|---|
| [lib.rs:399-403](../../crates/drill-core/src/lib.rs) `Document::from_json` | `serde_json::from_str` → `validate()` の 2 行のみ。**サイズ上限・要素数上限・ネスト深度上限・数値の有限性検査がいずれも無い。** |
| [lib.rs:302-333](../../crates/drill-core/src/lib.rs) `Document::validate` | 検査するのは 5 項目のみ: `schema_version == 1` / `sets` 非空 / `grid.width, height > 0.0` / 各セットの `positions.len()` 一致 / `performer.id` の一意性。 |
| 同上 | **NaN / Inf を一切拒否していない。** `grid.width = NaN` は `NaN <= 0.0 == false` なので `validate` を通過する。座標 `Point { x: NaN }` も通過する。 |
| 同上 | 演者数・セット数・カウント値・文字列長・`grid.hashes` の要素数に上限が無い。 |
| [lib.rs:399](../../crates/drill-core/src/lib.rs) | エラーが `Result<_, String>`。`serde_json` のメッセージをそのまま返すため、**攻撃者が制御した文字列断片が UI に出る**。 |
| [audio.rs:26](../../crates/drill-core/src/audio.rs) `AudioTrack::path: String` | **送り手の絶対パスがそのまま文書に埋まる。** 受け取り側でそのパスを開く経路ができれば任意ファイル読み取り／UNC 経由の資格情報漏えいになる。 |
| [audio.rs:65-79](../../crates/drill-core/src/audio.rs) `AudioTrack::validate` | `duration_seconds` の有限性は見ているが、`offset_seconds` / `gain_db` / `fade_*` の有限性は未検査（`gain_linear` は `clamp` で守られているが `offset_seconds` は素通り）。 |
| [tempo.rs:195](../../crates/drill-core/src/tempo.rs) | `bpm.is_finite() && bpm > 0.0` の検査はある（良い前例）。ただし `TempoMap` 全体の要素数上限は無い。 |

### 2.2 パニック経路

`unwrap` / `expect` / 添字が実コード（テスト外）に残っている箇所。

| 箇所 | 内容 |
|---|---|
| [lib.rs:385](../../crates/drill-core/src/lib.rs) | `self.sets[set_index.min(self.sets.len() - 1)]` — `sets` が空だと `0usize - 1` で**整数アンダーフロー**（debug: panic / release: 巨大値 → 添字 panic）。`from_json` 経由なら `validate` が空を弾くが、`Document` は pub フィールドなので他経路で空にできる。 |
| [editing.rs:65-67](../../crates/drill-core/src/editing.rs) / [editing.rs:83-85](../../crates/drill-core/src/editing.rs) | `points[order[0]]` / `order.last().unwrap()` / `(points.len() - 1)`。冒頭の `if points.len() < 3` ガードで現状は到達不能だが、ガードとパニック源が離れており脆い。 |
| [countsheet.rs:29](../../crates/drill-core/src/countsheet.rs) | `String::from_utf8(letters).expect("ASCII letters are valid UTF-8")` — 論理的に成立するが `expect` が残る。 |
| [main.rs:1351](../../crates/drill-app/src/main.rs) | `self.document.audio.as_mut().unwrap()`。直前に `is_some()` があるが `unwrap` は残っている。 |
| ワークスペース全体 | `#![forbid(unsafe_code)]` も `[workspace.lints]` も**無い**。clippy の制限 lint 設定も無い。 |
| [Cargo.toml](../../Cargo.toml) `[profile.release]` | `lto` / `codegen-units` / `strip` のみ。**`overflow-checks` が無い** → release で整数オーバーフローが静かに折り返す。 |

### 2.3 外部プロセス

| 箇所 | 実態 |
|---|---|
| [main.rs:1268](../../crates/drill-app/src/main.rs) | `std::process::Command::new("ffmpeg")` — **相対名での起動**。Windows の探索順（後述 3.5）では「親プロセスのカレントディレクトリ」が `PATH` より先に見られる。プロジェクトファイルと同じフォルダに `ffmpeg.exe` を同梱した zip を渡されると、それが実行され得る。 |
| 同上 | 環境変数を親から丸ごと継承する（`env_clear` なし）。FFmpeg は `FFREPORT` などを解釈するため、汚染された環境は任意ファイル書き込みになり得る。 |
| [video.rs:157-215](../../crates/drill-core/src/video.rs) `ffmpeg_args` | 引数配列で組み立てている点は方針どおり（良い）。しかし `audio_path: &str` / `output_path: &str` を**無検証でそのまま `-i` と出力に渡す**。`-protocol_whitelist` も `file:` 接頭辞も `-f`（マルチプレクサ明示）も無い。`-y` で無条件上書きする。 |
| [video.rs:212](../../crates/drill-core/src/video.rs) | 出力パスが引数列の末尾。`-` で始まるパスならフラグとして解釈される。 |
| 実行部 | 実際にプロセスを起動して stdin へ書き込むコードはまだ存在しない（`MEDIA_PIPELINE.md` P0 が未実装）。**つまり今なら設計をやり直せる。** |

### 2.4 ファイルシステム

| 箇所 | 実態 |
|---|---|
| [main.rs:236-247](../../crates/drill-app/src/main.rs) `save_to` | `std::fs::write(path, json)` — **その場切り詰め書き込み**。書き込み中の失敗・電源断で原本を失う。`00-conventions.md` の「上書きは原子的置換」に違反。 |
| [main.rs:238-241](../../crates/drill-app/src/main.rs) | バックアップは `fs::copy`。`with_extension("backup.drill.json")` は `show.drill.json` → `show.drill.backup.drill.json` になり、命名が破綻している。 |
| [main.rs:482](../../crates/drill-app/src/main.rs) | 自動保存も `fs::write`。非原子的。 |
| [main.rs:437](../../crates/drill-app/src/main.rs) | `std::fs::read_to_string(&path)` — **サイズ上限なしで全体をメモリへ**。10 GB のファイルを掴まされると OOM。 |
| 全体 | シンボリックリンク／ジャンクション判定なし。一時ファイル方針なし。パニックフックなし。`.drillproj` コンテナ（zip）は未実装。 |

### 2.5 供給網・配布

- 依存: `drill-core` は `serde` / `serde_json` のみ。`drill-app` は `drill-core` / `eframe`(wgpu) / `rfd`。
- `deny.toml` / `.cargo/config.toml` / `rust-toolchain.toml` / `.github/workflows` / `fuzz/` は**いずれも存在しない**。
- `Cargo.lock` はコミット済み（良い）。
- コード署名・チェックサム公開・更新機構は未着手。

### 2.6 出力側の穴（受け取ったデータが出ていく経路）

- [svg.rs:18-36](../../crates/drill-core/src/svg.rs) `xml_escape` は `& < > " '` を処理する（良い）。ただし **C0 制御文字を除去しない**。XML 1.0 で不正な `U+0000`–`U+0008` を含むセット名は、生成された SVG をブラウザ・Illustrator が読めなくする。
- [svg.rs:44-49](../../crates/drill-core/src/svg.rs) `px(v)` は `format!("{v:.2}")`。座標が NaN なら `"NaN"` が属性値として出力され、SVG 全体が壊れる。**2.1 の NaN 未検査と直結している。**

---

## 3. 設計

### 3.0 クレート配置

`00-conventions.md` のクレート表に 1 本追加する。依存方向は上から下を守る。

```
drill-core     ドキュメントモデル + 上限値定義 + JSON 事前スキャン + 検証。依存は serde/serde_json のみ。
drill-sandbox  【新規】信頼できない入力の外殻。zip コンテナ・パス検証・画像/音声の入口ゲート・
               外部プロセス起動・原子的ファイル書き込み・（feature）WASM プラグイン。
               依存: drill-core, zip, image, sha2, rand（+ cfg(windows): windows-sys）
drill-render   drill-core にのみ依存（変更なし）
drill-audio    drill-core, drill-sandbox に依存
drill-export   drill-render, drill-sandbox に依存
drill-app      上記すべての利用者。OS ダイアログ由来のパスだけが書き込み権限の起点。
```

**規則: `drill-audio` / `drill-export` / `drill-app` は `std::fs` / `std::process` を直接呼ばない。**
必ず `drill-sandbox` を経由する。この規則は clippy の `disallowed-methods` で機械的に強制する（3.4）。

`drill-core` に上限値と検証を置くのは、`00-conventions.md` が要求する「保存検証は純粋ロジックとして単体テスト可能」を守るため。
デコーダを持つ層（zip/画像/音声）だけが `drill-sandbox` に降りる。

---

### 3.1 脅威モデル（STRIDE）

#### 資産

| # | 資産 | 失われたときの被害 |
|---|---|---|
| A1 | 利用者の制作物（`.drillproj` / `.drill.json`） | 数百時間の作業の消失。ショー本番に間に合わない。 |
| A2 | 演者の実名・所属（`Performer.label`, `Section.name`） | 未成年を含む個人情報の漏えい。学校・団体の信用失墜。 |
| A3 | 端末上の他ファイル | 任意ファイル読み取り／上書き。 |
| A4 | 利用者アカウントの実行権限 | 任意コード実行 → ランサムウェア等への踏み台。 |
| A5 | Windows の資格情報（NTLM ハッシュ） | UNC パス誘導による認証情報の外部送出。 |
| A6 | ライセンス鍵・アクティベーションファイル | 不正利用・なりすまし。 |
| A7 | リリース署名鍵 | 全利用者へのマルウェア配布。最悪の資産。 |

#### 攻撃者

| # | 攻撃者 | 能力 | 現実味 |
|---|---|---|---|
| T1 | 悪意あるファイルの送り手 | 任意のバイト列を「ドリルファイル」「音源」として渡す。利用者は自発的に開く。 | **高**。業界の日常動線そのもの。 |
| T2 | 侵害された／悪意あるプラグイン作者 | 配布物に任意のプラグインコードを含める。 | 中（P2 でプラグイン公開後）。 |
| T3 | 中間者（公衆 Wi-Fi、企業 TLS 検査、DNS 汚染） | 更新配信・ライセンス通信を改竄する。 | 中。 |
| T4 | 同一端末の別ユーザ／マルウェア | `PATH` 汚染、カレントディレクトリへのファイル設置、一時ファイル差し替え。 | 中（共用 PC の音楽室・部室は実在する）。 |
| T5 | 供給網（依存クレート・ビルド環境） | 依存の乗っ取り、build.rs での任意実行。 | 低〜中。 |

**非対象**: 端末を物理的に掌握した攻撃者、管理者権限を既に持つ攻撃者、利用者自身が意図して行う操作。
これらに対する「防御」は実効がないので設計しない。

#### 信頼境界と STRIDE

```
        ┌──────────────────────────────────────────────┐
        │            DrillForge プロセス                │
        │  ┌────────────────────────────────────────┐  │
   ①───▶│  │ drill-sandbox（検査の外殻）             │  │
 ファイル │  └──────────────┬─────────────────────────┘  │
        │                 │ 検証済みの型のみ通過        │
        │  ┌──────────────▼─────────────────────────┐  │
        │  │ drill-core / render / audio / export    │  │
        │  └──────────────┬─────────────────────────┘  │
        └─────────────────┼────────────────────────────┘
              ②           │            ③            ④
        外部プロセス   ファイルシステム   ネットワーク   プラグイン
        (FFmpeg)      (保存/一時)      (更新/認証)    (WASM)
```

| 境界 | S 偽装 | T 改竄 | R 否認 | I 漏えい | D サービス妨害 | E 権限昇格 |
|---|---|---|---|---|---|---|
| ① ファイル読込 | 拡張子偽装（zip を `.mp3` と称する） | 破損した文書で不変条件を壊す | — | 文書内の絶対/UNC パスで他ファイルを読ませる | JSON 爆弾・zip 爆弾・デコード爆弾 | — |
| ② 外部プロセス | CWD/PATH 上の偽 `ffmpeg.exe` | 出力先パスの操作で任意ファイル上書き | — | `-i` に任意パスを渡して動画へ埋め込む | パイプデッドロック、孤児プロセス | **偽 exe / .bat 引数注入で任意コード実行** |
| ③ ファイルシステム | — | 非原子的書き込みでの原本破壊 | — | 一時ファイルの読み取り | ディスク枯渇 | シンボリックリンク経由の特権パス上書き |
| ④ ネットワーク | 偽更新サーバ | 更新バイナリの差し替え | — | テレメトリでの実名送出 | オフラインで起動不能 | **改竄バイナリの実行** |
| ⑤ プラグイン | 署名なしプラグインの偽装 | 文書の不正改変 | 誰の編集か不明 | 文書内容の外部送出 | 無限ループで UI 凍結 | **ネイティブ DLL なら即座に完全掌握** |

太字が「起きたら製品として終わり」の項目。設計はこの 4 つを構造的に不可能にすることを最優先する。

---

### 3.2 上限値

上限は「**実用を妨げない最小**」に置く。根拠となる実測値を先に示す。

#### 実測: 1,000 人 × 256 セットの JSON サイズ

現行スキーマと同型の文書を生成して計測した（座標は小数第 4 位、セット名は日本語）。

| 形式 | サイズ |
|---|---|
| `to_string_pretty`（現行 `Document::to_json` と同じ） | **16.6 MB** |
| `to_string`（compact） | 6.3 MB |
| pretty を deflate 圧縮 | 2.3 MB |
| JSON 値ノード数（オブジェクト/配列/スカラの総数） | **約 773,000** |

上限規模（`00-conventions.md`: 演者 4,000 人 / セット 256）は約 4 倍で **66 MB / 約 3,100,000 ノード**。

#### `Limits`（drill-core）

```rust
// crates/drill-core/src/limits.rs

/// Ingest-time resource ceilings. Every field is an inclusive maximum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Total bytes of the JSON payload before parsing.
    pub json_bytes: u64,
    /// Maximum nesting depth of the JSON value tree.
    pub json_depth: u16,
    /// Maximum number of JSON value nodes (objects, arrays, scalars).
    pub json_nodes: u64,
    /// Maximum UTF-8 byte length of any single JSON string.
    pub string_bytes: u32,
    pub title_bytes: u32,
    pub set_name_bytes: u32,
    pub performer_label_bytes: u32,
    pub asset_path_bytes: u32,

    pub performers: u32,
    pub sets: u32,
    pub counts_per_set: u16,
    pub tempo_events: u32,
    pub grid_hashes: u32,
    pub sections: u32,
}

impl Limits {
    /// Default ceiling for files opened from the UI.
    pub const INTERACTIVE: Self = Self {
        json_bytes: 128 * 1024 * 1024,
        json_depth: 32,
        json_nodes: 16_000_000,
        string_bytes: 4_096,
        title_bytes: 256,
        set_name_bytes: 128,
        performer_label_bytes: 64,
        asset_path_bytes: 1_024,
        performers: 10_000,
        sets: 1_024,
        counts_per_set: 4_096,
        tempo_events: 8_192,
        grid_hashes: 64,
        sections: 256,
    };

    /// Ceiling for documents produced by importers and plugins. Same shape,
    /// tighter numbers, because those sources have no human in the loop.
    pub const DERIVED: Self = Self {
        json_bytes: 96 * 1024 * 1024,
        performers: 4_096,
        sets: 512,
        ..Self::INTERACTIVE
    };
}
```

| 上限 | 値 | 根拠 |
|---|---|---|
| `json_bytes` | 128 MiB | 上限規模の実測 66 MB に対し約 2 倍。128 MiB を全読みしても現代の作業機で問題ない一方、GB 級の爆弾は確実に止まる。 |
| `json_depth` | 32 | 現行スキーマの最大深度は 5（root→sets→set→positions→point）。`RouteTable` / `Section` 導入後でも 8。32 は 4 倍の余裕。**`serde_json` の既定再帰上限 128 より手前で自前スキャナが止めることが重要**（理由は 6.F2）。 |
| `json_nodes` | 16,000,000 | 上限規模の実測 3.1M に対し約 5 倍。パース時間を線形に縛る。 |
| `string_bytes` | 4,096 | 個別上限（title 256 / set 128 / label 64 / path 1,024）に加えた包括上限。パース中の 1 文字列あたりの確保を縛る。 |
| `performer_label_bytes` | 64 | ドリルナンバー "Tp12" は 4 バイト。実名 "田中太郎" は 12 バイト。64 は日本語 21 文字ぶん。 |
| `set_name_bytes` | 128 | "Set 42 – Ballad impact" 相当が 25 バイト。日本語 42 文字ぶん。 |
| `performers` | 10,000 | 上限規模 4,000 人の 2.5 倍。世界最大級のマーチングバンドでも 1,000 人未満。 |
| `sets` | 1,024 | 上限規模 256 の 4 倍。10 分のショーで 90〜120 セットが実務値。 |
| `counts_per_set` | 4,096 | 基準の総カウント 2,048 を 1 セットに全部入れてもなお 2 倍。`sets × counts_per_set = 4.2M` で `timeline_counts()` の `u32` 加算がオーバーフローしないことも同時に保証する。 |
| `tempo_events` | 8,192 | 1 カウント 1 テンポ変化でも 2,048 で足りる。4 倍。 |
| `grid_hashes` | 64 | 既定 2 本、サッカー 1 本。屋内の凝った設定でも 10 本未満。 |

**上限の性質による扱いの差**（利用者への提示に直結する。3.6 参照）:

- **規模上限**（`performers` / `sets` / `json_bytes` / `counts_per_set`）は、単に「想定より大きい」だけかもしれない。**利用者が自己責任で引き上げられる**。
- **攻撃面上限**（`json_depth` / `json_nodes` / 後述の zip 圧縮率 / PCM バイト数 / 画素数）は、正当なファイルが超える理由が無い。**引き上げ不可**。

#### コンテナ（zip）上限

```rust
// crates/drill-sandbox/src/container.rs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContainerLimits {
    /// Size of the archive file itself.
    pub archive_bytes: u64,
    pub entries: u32,
    /// Sum of decompressed bytes across all entries actually read.
    pub total_uncompressed: u64,
    pub entry_uncompressed: u64,
    /// Maximum decompressed:compressed ratio, enforced per entry while streaming.
    pub ratio: u32,
    pub name_bytes: u16,
    pub path_depth: u8,
}

impl ContainerLimits {
    pub const INTERACTIVE: Self = Self {
        archive_bytes: 1024 * 1024 * 1024,
        entries: 4_096,
        total_uncompressed: 2 * 1024 * 1024 * 1024,
        entry_uncompressed: 1024 * 1024 * 1024,
        ratio: 200,
        name_bytes: 255,
        path_depth: 8,
    };
}
```

| 上限 | 値 | 根拠 |
|---|---|---|
| `archive_bytes` | 1 GiB | 文書 66 MB + 12 分 WAV(48k/24bit/stereo) 207 MB + 背景画像数枚。実務の上限が 300 MB 程度で、3 倍の余裕。 |
| `entries` | 4,096 | `document.json` + `assets/`（音源 1〜3、画像 10 程度）+ サムネイル。実務は 20 未満。4,096 は極端に緩いが、100 万エントリの中央ディレクトリ爆弾は確実に止まる。 |
| `total_uncompressed` | 2 GiB | `archive_bytes` の 2 倍。音源が非圧縮 WAV で入る前提。 |
| `ratio` | 200:1 | 実測: 本設計の pretty JSON の deflate 圧縮率は **7.1:1**。座標に反復の多い実データでも 20:1 程度。一方、典型的な zip 爆弾は 1,000:1 以上（`42.zip` は 10¹¹ 倍）。200:1 は実データの 10 倍の余裕を取りつつ爆弾を止める線。 |
| `name_bytes` | 255 | 一般的なファイルシステムのファイル名上限。 |
| `path_depth` | 8 | 実際に使うのは `assets/audio/show.wav` の 3 段。 |

**最重要**: 圧縮率も展開後サイズも、**zip ヘッダの申告値を信用しない**。展開しながらバイトを数え、超えた瞬間に打ち切る（3.3 の `LimitedReader`）。ヘッダの `uncompressed_size` は攻撃者が自由に書ける値である。

#### 音声上限

```rust
// crates/drill-sandbox/src/media.rs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioLimits {
    pub file_bytes: u64,
    pub duration_seconds: u32,
    pub sample_rate: u32,
    pub channels: u16,
    /// Hard cap on decoded PCM held in memory, counted while decoding.
    pub decoded_bytes: u64,
    /// Wall-clock budget for the whole decode job.
    pub decode_deadline: Duration,
}

impl AudioLimits {
    pub const INTERACTIVE: Self = Self {
        file_bytes: 512 * 1024 * 1024,
        duration_seconds: 1_800,
        sample_rate: 192_000,
        channels: 8,
        decoded_bytes: 768 * 1024 * 1024,
        decode_deadline: Duration::from_secs(60),
    };
}
```

| 上限 | 値 | 根拠 |
|---|---|---|
| `file_bytes` | 512 MiB | 12 分の 24bit/48k ステレオ WAV = 207 MB。5.1ch なら 622 MB になるので、そこは `channels` 側で切る。 |
| `duration_seconds` | 1,800（30 分） | ショー本体は最長 12 分（DCI/BOA 規定）。通し練習の録音・複数曲メドレーを見込んで 2.5 倍。 |
| `sample_rate` | 192,000 | 実務は 44.1k / 48k。96k/192k のマスターを直接読む例があるので許容。 |
| `channels` | 8 | ステレオが実務。7.1 まで許容し、`WAVE_FORMAT_EXTENSIBLE` で 65,535ch を申告する爆弾を止める。 |
| `decoded_bytes` | 768 MiB | f32 展開で 12 分ステレオ 48k = 276 MB、30 分ステレオ 48k = 691 MB。実用の上端に 1.1 倍。**申告値ではなく実デコード量で数える。** |
| `decode_deadline` | 60 s | 30 分の音源のデコードは実測数秒。60 s を超えるものはループ／爆弾。 |

拡張子は信用せず、**マジックバイトで判定**する（`symphonia` の `probe`）。ダイアログのフィルタは UX であってセキュリティ境界ではない。

#### 画像上限

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageLimits {
    pub file_bytes: u64,
    pub max_side: u32,
    pub max_pixels: u64,
    pub max_alloc_bytes: u64,
}

impl ImageLimits {
    pub const INTERACTIVE: Self = Self {
        file_bytes: 64 * 1024 * 1024,
        max_side: 16_384,
        max_pixels: 64_000_000,
        max_alloc_bytes: 256 * 1024 * 1024,
    };
}
```

| 上限 | 値 | 根拠 |
|---|---|---|
| `max_pixels` | 64 MPix | RGBA8 で 256 MiB。4K 背景 = 8.3 MPix、8192×4096 のスタジアムパノラマ = 33 MPix。2 倍の余裕。 |
| `max_side` | 16,384 | wgpu の一般的な最大テクスチャ辺。これを超える画像は表示できない。 |

`image` クレートの `Limits` に `max_alloc` / `max_image_width` / `max_image_height` を渡し、**寸法を読んだ直後・確保の前に**判定する。`ImageReader::with_guessed_format()` でマジックバイト判定を強制する。

---

### 3.3 信頼できない入力の検査

#### JSON: 二段構え

`serde_json::Value` へ一度読むのは禁止（それ自体が爆弾の実行になる）。**バイト列を先に走査し、形が安全と分かってから型付きデシリアライズする。**

```rust
// crates/drill-core/src/ingest.rs
#![deny(
    clippy::unwrap_used, clippy::expect_used, clippy::panic,
    clippy::indexing_slicing, clippy::arithmetic_side_effects,
    clippy::integer_division, clippy::cast_possible_truncation,
)]

use crate::limits::{LimitKind, Limits};
use crate::{Document, DrillError};

/// Structural facts about a JSON payload, gathered without building a value tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JsonShape {
    pub bytes: u64,
    pub depth: u16,
    pub nodes: u64,
    pub max_string_bytes: u32,
}

/// Single-pass, allocation-free, **non-recursive** structural scan.
///
/// Uses an explicit depth counter rather than recursion so that a deeply
/// nested payload cannot exhaust the native stack (a stack overflow aborts the
/// process and cannot be caught, so it must be prevented, not handled).
pub fn scan_json(bytes: &[u8], limits: &Limits) -> Result<JsonShape, DrillError>;

/// Full ingest path for an untrusted document payload.
///
/// Guarantees: never panics, never allocates more than `limits.json_bytes`
/// plus the size of the resulting `Document`, and every `f32` in the returned
/// document is finite.
pub fn load_document(bytes: &[u8], limits: &Limits) -> Result<Document, DrillError> {
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > limits.json_bytes {
        return Err(DrillError::LimitExceeded {
            kind: LimitKind::FileBytes,
            limit: limits.json_bytes,
            found: len,
        });
    }
    let text = core::str::from_utf8(bytes).map_err(|e| DrillError::NotUtf8 {
        byte_offset: u64::try_from(e.valid_up_to()).unwrap_or(u64::MAX),
    })?;
    scan_json(text.as_bytes(), limits)?;
    let document: Document = serde_json::from_str(text).map_err(|e| DrillError::Json {
        line: u32::try_from(e.line()).unwrap_or(u32::MAX),
        column: u32::try_from(e.column()).unwrap_or(u32::MAX),
    })?;
    document.validate_untrusted(limits)?;
    Ok(document)
}
```

`scan_json` が検査するもの:

1. 深度 ≤ `json_depth`（明示スタックで数える。再帰しない）
2. ノード数 ≤ `json_nodes`
3. 個々の文字列の UTF-8 バイト長 ≤ `string_bytes`
4. 数値トークンの検査 — 指数部が f64 の範囲を超えるもの（`1e400` → `inf`）、`NaN` / `Infinity` / `-Infinity` のリテラル、先頭ゼロ等の不正形式を拒否
5. `U+0000`–`U+001F` の生の制御文字を文字列内に含まないこと（RFC 8259 違反であり、UI 表示・SVG 出力の汚染源）
6. BOM、末尾のごみバイト

`DrillError::Json` が **`serde_json` のメッセージ文字列を持たない**ことが要点。行・列だけを持つ。攻撃者が制御する文字列を UI に流さないため。

#### 数値の有限性

`serde_json` は `1e39` を f64 で読み、`as f32` で `inf` にする。デシリアライズ**後**に必ず全 f32 を検査する。エラーは文字列ではなく構造で持ち、42 のロケール層で文言化する。

```rust
// crates/drill-core/src/limits.rs

/// Machine-readable location of a rejected value. Carries no attacker-controlled
/// text, so it is safe to render directly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldPath {
    pub set: Option<u32>,
    pub performer: Option<u32>,
    pub field: Field,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    PositionX, PositionY,
    GridWidth, GridHeight, GridHashPosition, GridHashWeight,
    GridHorizontalUnits, GridVerticalUnits, GridMajorLineInterval,
    TempoBpm, TempoCount,
    AudioOffsetSeconds, AudioDurationSeconds, AudioGainDb,
    AudioTrimStart, AudioTrimEnd, AudioFadeIn, AudioFadeOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKind {
    FileBytes, JsonDepth, JsonNodes, StringBytes,
    TitleBytes, SetNameBytes, PerformerLabelBytes, AssetPathBytes,
    PerformerCount, SetCount, PositionCount, CountsPerSet,
    TempoEventCount, GridHashCount, SectionCount,
    ZipArchiveBytes, ZipEntryCount, ZipTotalUncompressed,
    ZipEntryUncompressed, ZipRatio, ZipNameBytes, ZipPathDepth,
    AudioFileBytes, AudioSeconds, AudioSampleRate, AudioChannels,
    AudioDecodedBytes, AudioDecodeDeadline,
    ImageFileBytes, ImageDimension, ImagePixels, ImageAlloc,
    PluginMemory, PluginDeadline, PluginOutputBytes,
}

impl LimitKind {
    /// `true` when the ceiling exists only to bound scale, so the user may
    /// knowingly raise it. `false` for ceilings that bound an attack primitive.
    pub const fn is_user_raisable(self) -> bool {
        matches!(
            self,
            Self::FileBytes | Self::PerformerCount | Self::SetCount
                | Self::CountsPerSet | Self::ZipArchiveBytes
                | Self::AudioFileBytes | Self::AudioSeconds
        )
    }
}
```

```rust
impl Document {
    /// Validate a document that came from outside this process.
    ///
    /// Strictly stronger than [`Document::validate`]: adds every ceiling in
    /// `limits`, rejects non-finite floats, bounds every string, and rejects
    /// asset references that are absolute, UNC, or contain traversal.
    pub fn validate_untrusted(&self, limits: &Limits) -> Result<(), DrillError>;
}
```

追加で `#[serde(deny_unknown_fields)]` を `Document` / `Set` / `Performer` / `GridConfig` / `AudioTrack` に付ける。
前方互換は `schema_version` + `migrate`（DESIGN_GAPS A-7）が担うので、**フィールド追加時は必ず `SCHEMA_VERSION` を上げる**という規律とセットで採用する。

#### 文書内アセット参照

`AudioTrack::path: String`（絶対パス）を廃止し、参照を型で分ける。**文書由来のパスを直接 `open` しない**ことを型で保証する。

```rust
// crates/drill-core/src/asset.rs

/// How a document refers to an external asset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetRef {
    /// Stored inside the `.drillproj` container. The only form that resolves
    /// without user interaction.
    Container { name: String },
    /// The sender's file name, kept as a *hint only*. Never opened directly:
    /// the app asks the user to locate the file with an OS dialog, once.
    External { hint_file_name: String },
    Missing,
}
```

`External` を自動で開かない理由は 3 つある。

1. `\\attacker.example\share\a.wav` のような UNC パスを開くと、Windows はその SMB サーバへ自動的に認証を試み、**NTLMv2 ハッシュが外部へ出る**。
2. `C:\Users\<name>\Documents\confidential.wav` のような絶対パスを開かせ、その音声を動画書き出しに乗せられると、任意ファイルの内容が外部へ出る。
3. 送り手の端末のパスは、受け手の端末では意味を持たない。実用上も再選択が必要。

`Container { name }` は必ず `SafeName`（後述）を通してから解決する。

#### コンテナ（zip）の検査

**方針: 既定では展開しない。** `.drillproj` は読み取り専用でメモリ上に必要なエントリだけを取り出す。ディスクへ展開するのは
利用者が明示的に「アセットを取り出す」を実行したときだけ。これで zip-slip の攻撃面が読み取り経路から消える。

```rust
// crates/drill-sandbox/src/container.rs

/// A zip entry name that has been proven safe to join onto a directory.
///
/// Invariant: for any `SafeName` `n` and any directory `root`,
/// `root.join(n.as_str())` normalizes to a path under `root`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SafeName(String);

impl SafeName {
    pub fn parse(raw: &str) -> Result<Self, DrillError>;
    pub fn as_str(&self) -> &str;
    /// Lowercased + NFC-normalized key used for duplicate detection.
    pub fn collision_key(&self) -> String;
}

pub struct Container { /* ZipArchive<File> + verified index + running byte counter */ }

impl Container {
    pub fn open(path: &Path, limits: &ContainerLimits) -> Result<Self, DrillError>;
    pub fn names(&self) -> impl Iterator<Item = &SafeName>;
    /// Streams the entry, counting decompressed bytes; aborts on limit breach.
    pub fn read_entry(&mut self, name: &SafeName, max: u64, out: &mut Vec<u8>)
        -> Result<(), DrillError>;
    /// Explicit user action only. Creates every file with `create_new(true)`.
    pub fn extract_all(&mut self, dest: &Path) -> Result<(), DrillError>;
}
```

`SafeName::parse` が拒否する条件（すべて実装し、それぞれに corpus のテストファイルを対応させる）:

| # | 条件 | 理由 |
|---|---|---|
| 1 | 空、または UTF-8 として 255 バイト超 | 基本境界 |
| 2 | 汎用フラグ bit 11 が立っていない名前を UTF-8 として解釈すること | 本来 CP437。誤解釈で `..` を作れる |
| 3 | `0x00`–`0x1F` / `0x7F` を含む | 制御文字。UI とログの汚染 |
| 4 | `\` を含む（`/` へ正規化した後に判定） | Windows のセパレータ。Unix 実装が見落とす典型 |
| 5 | 先頭が `/` | 絶対パス |
| 6 | 任意のコンポーネントが `.` または `..` または空 | **パス・トラバーサル（Zip Slip）** |
| 7 | `:` を含む | ドライブ指定 `C:` と NTFS 代替データストリーム `file.txt:evil.exe` |
| 8 | `<` `>` `"` `|` `?` `*` を含む | Windows の予約文字 |
| 9 | コンポーネントが `.` または半角空白で終わる | **Windows が末尾のドット・空白を除去するため、検査を通した名前と実際に作られる名前がずれる**（CVE-2024-43402 と同種の落とし穴） |
| 10 | コンポーネントの拡張子より前が `CON` `PRN` `AUX` `NUL` `COM0`–`COM9` `LPT0`–`LPT9` `CONIN$` `CONOUT$`（大小無視） | DOS デバイス名。開くとデバイスに繋がる |
| 11 | 深度が `path_depth` 超 | 深いディレクトリ木による資源消費 |
| 12 | 双方向制御文字 `U+202A`–`U+202E` / `U+2066`–`U+2069` を含む | ダイアログ上でのファイル名偽装 |
| 13 | 既出の名前と `collision_key()` が一致 | **同名エントリ**。Windows は大小無視なので `A.json` と `a.json` は同一 |

`Container::open` が追加で拒否するもの:

- シンボリックリンクエントリ: `entry.unix_mode()` の上位 4 ビットが `0xA000`（`S_IFLNK`）、または Windows の外部属性に `FILE_ATTRIBUTE_REPARSE_POINT`
- 暗号化エントリ（`entry.encrypted()`）— 開けないものは静かに無視せず明示的に拒否する
- `Stored` / `Deflate` 以外の圧縮方式
- エントリ数 > `entries`、アーカイブ本体 > `archive_bytes`

エントリ名は**必ず中央ディレクトリ由来のものだけを使う**（`zip` クレートの `by_name` はそう動く）。ローカルファイルヘッダに別の名前を書く偽装パターンを、名前解決に一切使わないことで無効化する。

展開後サイズの強制は申告値ではなくストリームで行う。

```rust
/// Wraps a decompressing reader and fails the read once `remaining` is exhausted.
struct LimitedReader<R> {
    inner: R,
    remaining: u64,
    kind: LimitKind,
    limit: u64,
}

impl<R: std::io::Read> std::io::Read for LimitedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> { /* ... */ }
}
```

`extract_all` の手順（明示操作のみ）:

1. `dest` を `create_dir_all` で用意し、`dest.canonicalize()?` を取る。
2. 各 `SafeName` について `dest.join(name)` を作る。
3. 親ディレクトリを `create_dir_all` した後、**`parent.canonicalize()?.starts_with(&dest_canon)` を再確認**する（`SafeName` の不変条件との二重化。既存のリンクが途中に置かれていた場合を捕まえる）。
4. `OpenOptions::new().write(true).create_new(true).open(target)` — `create_new` は既存ファイル・既存シンボリックリンクのいずれでも失敗する。追随も上書きも起きない。
5. `LimitedReader` 経由で書き、総バイト数を `total_uncompressed` に対して数える。
6. 途中で失敗したら、それまでに作ったファイルを削除して `dest` を元に戻す。

#### 音声・画像

```rust
// crates/drill-sandbox/src/media.rs

pub struct AudioProbe {
    pub format: AudioFormat,   // magic-byte derived, not extension derived
    pub sample_rate: u32,
    pub channels: u16,
    pub declared_seconds: Option<f32>,
}

/// Probes by magic bytes and checks the declared shape against `limits`
/// *before* any decode starts.
pub fn probe_audio(bytes: &[u8], limits: &AudioLimits) -> Result<AudioProbe, DrillError>;

/// Decodes with a running PCM byte counter and a wall-clock deadline.
/// `cancel` lets the job layer stop it; the deadline stops decode bombs that
/// the probe could not predict.
pub fn decode_audio(
    bytes: &[u8],
    limits: &AudioLimits,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<DecodedAudio, DrillError>;

pub fn load_image(bytes: &[u8], limits: &ImageLimits) -> Result<image::RgbaImage, DrillError>;
```

要点:

- **拡張子は判定に使わない。** `zip` を `.mp3` と名乗らせる偽装は probe で落ちる。
- 申告 `duration × sample_rate × channels × 4` を先に検算して `decoded_bytes` を超えるなら**デコードを始めない**。
- 申告が嘘の場合に備え、デコード中も実バイト数を数え、超えた瞬間に打ち切る。
- 画像は `ImageReader::with_guessed_format()` + `Limits { max_alloc, max_image_width, max_image_height }`。寸法を読んだ直後、ピクセルバッファ確保の**前に**判定する。
- SVG は**入力形式として受け付けない**（DrillForge は SVG を出力するだけ）。XML 外部実体・billion laughs の攻撃面を持ち込まない。

---

### 3.4 パニック禁止経路の機械的強制

「気をつけて書く」では守れない。ビルドが落ちる形にする。

#### ワークスペース lint

```toml
# Cargo.toml
[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"
unreachable_pub = "warn"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
todo = "deny"
unimplemented = "deny"
unreachable = "deny"
exit = "deny"
mem_forget = "deny"
indexing_slicing = "warn"     # 全体は warn、ingest 経路のみ deny（下記）
integer_division = "warn"
cast_possible_truncation = "warn"
float_cmp = "warn"
# arithmetic_side_effects は全体では noise が多すぎるので採用しない。
# 代わりに ingest 経路のモジュールで #![deny] する。

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
overflow-checks = true        # ← 追加
```

各クレートの `Cargo.toml` に `[lints] workspace = true` を書く。

`overflow-checks = true` を release で有効にする根拠: 性能予算に効くホットループ（`positions_at` の補間、衝突走査）は
**すべて f32 演算**であり、整数オーバーフローチェックの対象外。影響はループカウンタと索引計算に限られる。
`cargo bench -p drill-core --bench core_performance` の既定ベースライン（1,000人×60,000フレーム補間 9.24ms /
1,000人ドキュメント×100回 JSON 変換 28.75ms）に対し **+5% 以内**であることを CI の回帰条件にする。
超えた場合は該当箇所だけ `wrapping_*` / `checked_*` を明示して局所的に解決し、`overflow-checks` は外さない。

`panic = "abort"` は**採用しない**。DESIGN_GAPS B-5 のパニックフックによるクラッシュ復旧が使えなくなるため、
unwind を維持し、ワーカースレッド境界で `catch_unwind` して復旧候補を書き出す。

#### ingest 経路の追加締め付け

```rust
// 各ファイルの先頭（モジュール内部属性）
#![deny(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::float_arithmetic,       // ingest では計算をしない。読んで検査するだけ
)]
```

適用先: `drill-core/src/ingest.rs`, `drill-core/src/limits.rs`, `drill-core/src/migrate.rs`,
`drill-sandbox/src/container.rs`, `drill-sandbox/src/media.rs`, `drill-sandbox/src/path.rs`,
`drill-sandbox/src/process.rs`, `drill-sandbox/src/fs.rs`。

#### clippy.toml

```toml
# clippy.toml（ワークスペース root）
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-indexing-slicing-in-tests = true

disallowed-methods = [
  { path = "std::process::Command::new",
    reason = "drill_sandbox::process::ExternalTool を使うこと（Windows の探索順と .bat 引数注入）" },
  { path = "std::fs::write",
    reason = "drill_sandbox::fs::atomic_write を使うこと（原子的置換）" },
  { path = "std::fs::read_to_string",
    reason = "drill_sandbox::fs::read_capped を使うこと（サイズ上限）" },
  { path = "std::fs::read",
    reason = "drill_sandbox::fs::read_capped を使うこと（サイズ上限）" },
  { path = "std::env::set_var",
    reason = "プロセス全体の環境を書き換えない（子プロセスへは env_clear + allow-list）" },
  { path = "std::env::remove_var",
    reason = "同上" },
]
```

#### 既存パニック経路の修正（2.2 の一覧に対応）

```rust
// lib.rs:385 — `self.sets.len() - 1` のアンダーフローを構造で消す
pub fn positions_at(&self, set_index: usize, progress: f32, out: &mut Vec<Point>) {
    out.clear();
    let Some(from) = self.sets.get(set_index).or_else(|| self.sets.last()) else {
        return;
    };
    let to = self.sets.get(set_index.saturating_add(1)).unwrap_or(from);
    out.extend(
        from.positions.iter().zip(&to.positions)
            .map(|(&a, &b)| a.lerp(b, progress.clamp(0.0, 1.0))),
    );
}
```

- `editing.rs:65-67 / 83-85`: `order.first()` / `order.last()` を `let Some(..) = .. else { return points.to_vec() }` にし、`points.len() - 1` を `saturating_sub(1).max(1)` にする。
- `countsheet.rs:29`: `String::from_utf8` をやめ、`letters` を `Vec<u8>` のまま `char::from` で組み立てるか、`from_utf8_lossy` を使う。
- `main.rs:1351`: `if let Some(track) = self.document.audio.as_mut()` に書き換える。
- `svg.rs:44 px()`: NaN を通さない前提を検査可能にする。`debug_assert!(v.is_finite())` を置き、release では `if !v.is_finite() { return "0".into(); }` でフォールバックする（読込時に弾いているので到達しないが、出力を壊さない）。
- `svg.rs:18 xml_escape()`: C0 制御文字（`\t` `\n` `\r` を除く `U+0000`–`U+001F`）と双方向制御文字を除去する。

#### CI での強制

```yaml
# .github/workflows/ci.yml（新設）
# ジョブ: check（毎 push / PR）
#   cargo fmt --all -- --check
#   cargo clippy --workspace --all-targets --all-features -- -D warnings
#   cargo test --workspace --locked
#   cargo test -p drill-sandbox --test malicious_corpus
#   cargo deny check advisories bans licenses sources
#   cargo audit --deny warnings
#   cargo bench -p drill-core --bench core_performance   # ベースライン比 +5% で fail
# ジョブ: fuzz（毎日 02:00 JST）
#   cargo +nightly fuzz run document_json -- -max_total_time=1200
#   cargo +nightly fuzz run safe_name     -- -max_total_time=600
#   cargo +nightly fuzz run container     -- -max_total_time=1200
#   cargo +nightly fuzz run migrate_v1    -- -max_total_time=600
```

---

### 3.5 外部プロセス（FFmpeg）

#### Windows の実行ファイル探索順

`CreateProcessW` に `lpApplicationName = NULL` を渡し、コマンドラインの先頭トークンにパスを含まない名前
（例 `ffmpeg`）を書いた場合、Windows は次の順で探す。

1. **アプリケーションがロードされたディレクトリ**
2. **親プロセスのカレントディレクトリ** ← `PATH` より **先**
3. 32 ビットシステムディレクトリ（`GetSystemDirectory`）
4. 16 ビットシステムディレクトリ（`System`）
5. Windows ディレクトリ（`GetWindowsDirectory`）
6. `PATH` 環境変数のディレクトリ群

落とし穴は 4 つある。

- **(a) カレントディレクトリが `PATH` より先**。現行の [main.rs:1268](../../crates/drill-app/src/main.rs) `Command::new("ffmpeg")` は、
  「プロジェクト一式の zip を解凍して、その中の `.drillproj` をダブルクリックで開く」という自然な動線で
  攻撃者の `ffmpeg.exe` を実行し得る。**現状で最も危険な 1 行。**
- **(b) 空白を含むパスの分割解釈**。`lpApplicationName = NULL` のとき、`C:\Program Files\MyApp -L` は
  まず `C:\Program.exe` として解釈される。`C:\Program.exe` を置ける攻撃者がいれば、そちらが起動する。
- **(c) 拡張子の自動補完**。拡張子が無ければ `.exe` が付く。`.com` を狙うなら明示が要る。
  逆に `.bat` / `.cmd` を指定すると `cmd.exe` を経由する。
- **(d) バッチファイルの引数エスケープ**。Rust の `std::process::Command` は `.bat` / `.cmd` 起動時に
  `cmd.exe` 独自の分割規則へ合わせた独自エスケープを行う必要があり、そこに
  **CVE-2024-24576（CVSS 10.0, Rust < 1.77.2）** と、末尾の空白・ピリオドで回避できた不完全修正
  **CVE-2024-43402（Rust < 1.81.0）** があった。**バッチファイルは呼ばない**のが唯一確実な対策。

なお DLL 側にも同種の探索順があり、**プラグインを DLL として受け入れると、そのプラグインを置いたディレクトリが
アプリの DLL 探索経路に入る**。これは 3.7 のプラグイン方式決定の主要根拠になる。

#### 設計

```rust
// crates/drill-sandbox/src/process.rs

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolKind { Ffmpeg, Ffprobe }

/// Where a tool binary may come from. Order is the resolution order.
/// `PATH` and the current directory are deliberately absent.
#[derive(Clone, Debug)]
pub enum ToolSource {
    /// `<directory of the running executable>/tools/<name>.exe`, shipped and
    /// signed with the application.
    Bundled,
    /// An absolute path the user picked in the settings screen this session.
    Configured(PathBuf),
}

/// A verified, absolute path to an external executable.
pub struct ExternalTool { kind: ToolKind, exe: PathBuf }

impl ExternalTool {
    /// Resolves without consulting `PATH`, the current directory, `App Paths`,
    /// or any string stored in a document.
    pub fn resolve(kind: ToolKind, sources: &[ToolSource]) -> Result<Self, DrillError>;

    /// Builds a `Command` with a cleared environment, an explicit working
    /// directory, and explicit stdio. Never takes a shell string.
    pub fn command(&self, args: &[OsString], workdir: &Path)
        -> Result<std::process::Command, DrillError>;
}
```

`resolve` の検査（すべて満たさなければ `DrillError::UnsafeToolPath`）:

1. `path.is_absolute()`
2. どのコンポーネントも `.` または半角空白で終わらない（落とし穴 (c)(d)）
3. `cfg(windows)` では拡張子が `exe` に完全一致（小文字化して比較）。`bat` / `cmd` / `com` / `ps1` は拒否
4. `symlink_metadata()` が通常ファイル（リンク・ディレクトリ・デバイスを拒否）
5. `Bundled` の場合、親ディレクトリが実行中バイナリのディレクトリ配下であること、かつ同梱マニフェストの SHA-256 と一致すること

`command` の構築:

```rust
let mut cmd = std::process::Command::new(&self.exe);   // 絶対パス → 探索が発生しない
cmd.args(args);
cmd.env_clear();
for key in ["SystemRoot", "windir", "TEMP", "TMP", "NUMBER_OF_PROCESSORS"] {
    if let Some(value) = std::env::var_os(key) { cmd.env(key, value); }
}
cmd.env("PATH", system_directories_only());  // C:\Windows\system32;C:\Windows のみ
cmd.current_dir(workdir);                    // 専用作業ディレクトリ。出力先でもインストール先でもない
cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
#[cfg(windows)]
{
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);    // コンソールの一瞬の表示を防ぐ
}
```

`env_clear()` が必須である理由: FFmpeg は環境変数を解釈する。

| 変数 | 効果 |
|---|---|
| `FFREPORT` | 指定パスへレポートファイルを書く。**任意ファイル書き込みの原始能力**。 |
| `http_proxy` / `https_proxy` / `no_proxy` | 通信の宛先を変える。 |
| `FONTCONFIG_FILE` / `FONTCONFIG_PATH` | フォント設定の読み込み先を変える。 |
| `LD_PRELOAD` / `LD_LIBRARY_PATH` / `DYLD_*` | Linux/macOS ビルドで任意ライブラリを注入。 |
| `TMPDIR` / `TEMP` | 一時ファイルの位置。allow-list には入れるが、値は親のものをそのまま使う。 |

作業ディレクトリを明示するのは、FFmpeg が相対パスを CWD 基準で解決すること、および子プロセスが更に子を起こす場合に
その CWD が探索経路へ入ることを避けるため。

#### 引数の型付け

`video.rs` の `ffmpeg_args(&str, &str)` を、検証済みの型を要求する形へ変える。

```rust
// crates/drill-sandbox/src/process.rs

/// An absolute path this process created and owns. Only constructible through
/// `prepare_write`, which requires a directory the user chose in an OS dialog.
pub struct OutputTarget { path: PathBuf, container: VideoContainer }

/// An absolute path inside the export job's private work directory.
/// Never a path that came from a document.
pub struct WorkFile { path: PathBuf }

impl VideoExportConfig {
    pub fn ffmpeg_args(&self, audio: Option<&WorkFile>, out: &OutputTarget)
        -> Result<Vec<OsString>, DrillError>;
}
```

**音声入力は必ず自前で書いた一時 WAV にする。** `MEDIA_PIPELINE.md` は trim / gain / fade を反映して mux すると
定めており、どのみち `drill-audio` が加工済み PCM を持つ。それを `<workdir>/audio.wav` へ書いて渡せば、
FFmpeg が触るのは「rawvideo の pipe:0」と「自分たちが書いた WAV」だけになり、
**FFmpeg 経由の任意ファイル読み取りという能力そのものが消える**。

引数列に追加するもの:

```
-protocol_whitelist  file,pipe      # concat: / http: / データ URL 等を全面禁止
-f rawvideo ... -i pipe:0
-f wav          -i file:<workdir>\audio.wav   # 明示 demuxer + file: スキーム
-f mp4|mov|webm                                # 出力 muxer を拡張子推測に任せない
<absolute output path>.part
```

- `-y` は維持してよいが、対象は DrillForge が `create_new` で先に作った `<out>.part` に限る。完了して `ffprobe`
  検証を通ってから `atomic_rename` で本来の名前へ移す。途中失敗時は `.part` を削除する。
- 出力パスは常に絶対なので `-` 始まりにならない（フラグ誤認の回避）。`OutputTarget` のコンストラクタで
  `is_absolute()` を要求することでこれを保証する。
- フィルタグラフに**利用者文字列を一切埋めない**。`drawtext` の `text=` / `textfile=` は任意ファイル読み取りになる。
  タイトル焼き込みが必要なら、DrillForge 側でフレームに描いてから rawvideo として渡す。

#### プロセスの寿命管理

```rust
/// Kills and reaps the child on drop. `std::process::Child` does neither.
pub struct ChildGuard {
    child: std::process::Child,
    #[cfg(windows)]
    job: JobObject,
}

impl Drop for ChildGuard {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
```

- **Windows ジョブオブジェクト**: `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` を設定したジョブに子を割り当てる。
  DrillForge がクラッシュしても FFmpeg が孤児として CPU を食い続けない。これはワークスペースで唯一
  `unsafe` が必要な箇所であり、`drill-sandbox::process::job_object`（`cfg(windows)`）に閉じ込め、
  モジュール単位で `#![allow(unsafe_code)]` と安全性論証コメントを置く。他の全モジュールは `forbid` のまま。
- **パイプのデッドロック回避**: stdin へフレームを書きながら、stdout（`-progress` の出力）と stderr を
  **別スレッドで読み続ける**。読まないとパイプバッファが埋まり、FFmpeg も DrillForge も止まる（画面はフリーズする）。
  stderr の蓄積は 64 KiB で打ち切る。
- **ウォッチドッグ**: 60 秒間、進捗行が来ず stdin も 1 バイトも受け付けない場合は kill してエラーにする。
- **キャンセル**: `AtomicBool` を見て stdin を閉じ、5 秒待ってから kill。
- **終了検証**: `ffprobe`（同じ `resolve` を通した絶対パス）で解像度・FPS・尺・音声トラック有無を確認するまで
  完了扱いにしない（`MEDIA_PIPELINE.md` 品質ゲート）。

---

### 3.6 ファイルシステム

```rust
// crates/drill-sandbox/src/fs.rs

/// Reads at most `max` bytes. Rejects non-regular files (directories, pipes,
/// devices) and files whose size exceeds `max`, checked on the open handle
/// rather than on the path.
pub fn read_capped(path: &Path, max: u64, kind: LimitKind) -> Result<Vec<u8>, DrillError>;

/// A destination this process is allowed to write to.
///
/// Only constructible from a path the user chose in an OS dialog during this
/// session, or from a path derived from one (autosave/backup siblings).
/// No `From<&str>`, no `From<PathBuf>`: a path that came out of a document
/// cannot become a `WriteTarget`.
pub struct WriteTarget { path: PathBuf, link_warning: Option<LinkWarning> }

pub struct LinkWarning { pub resolved: PathBuf }

pub fn prepare_write(chosen: &Path) -> Result<WriteTarget, DrillError>;

/// Write-to-temp + fsync + atomic rename. On failure the original file is
/// byte-for-byte unchanged.
pub fn atomic_write(target: &WriteTarget, bytes: &[u8]) -> Result<(), DrillError>;
```

`atomic_write` の手順:

1. `dir = target.path.parent()`。一時ファイルは**同じディレクトリ**に作る（別ボリュームだと rename が原子的でなくなり、
   `%TEMP%` を使うと ACL も変わる）。
2. `tmp = dir.join(format!(".{stem}.{:016x}.tmp", rand_u64()))`
3. `OpenOptions::new().write(true).create_new(true).open(&tmp)` —
   `create_new` は既存ファイルでもシンボリックリンクでも失敗するので、**予測可能な名前を先に置いておく攻撃も、
   リンク追随も同時に防ぐ**。
4. 全バイト書き込み → `file.sync_all()`（メタデータまで確定させる）→ `drop(file)`
5. `std::fs::rename(&tmp, &target.path)` — Windows では `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` に対応し、
   同一ボリューム内で原子的に置換する。
6. 失敗時は `tmp` を削除。**原本には一度も触れていない**。

`prepare_write` の検査:

- パスが絶対であること。
- `symlink_metadata()` でシンボリックリンクかどうか、Windows では
  `MetadataExt::file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT` でジャンクションかどうかを判定し、
  該当すれば `LinkWarning { resolved }` を立てる。**削除も追随もせず、利用者に実体パスを見せて確認を求める**
  （勝手にリンクを壊すのは制作物の破壊になり得る）。
- 親ディレクトリが実行中バイナリのディレクトリ配下でないこと、`C:\Windows` 配下でないこと。

**TOCTOU について正直に書く**: `symlink_metadata` の判定と `rename` の間に対象を差し替える競合は、
`std` の API だけでは Windows で完全には塞げない（ハンドル基準の再検証 API が露出していない）。
本設計の立場は次のとおり。

1. 一時ファイルは `create_new` で作る（この経路の競合は塞げる）。
2. 書き込み先は**当該セッションで利用者が OS ダイアログから選んだディレクトリ配下に限る**。
   文書に書かれたパスは決して書き込みに使わない（型 `WriteTarget` で強制）。
3. これで残る競合は「同一ユーザ権限の攻撃者が同じ端末で同時に動いている」場合のみであり、
   その前提では既に文書ファイル自体を直接書き換えられる。**この脅威に対する防御は費用対効果が無いので設計しない**（3.1 の非対象）。

一時・作業ディレクトリの位置:

| 用途 | 位置 | 理由 |
|---|---|---|
| 保存の一時ファイル | 保存先と同一ディレクトリ | 原子的 rename と ACL の一致 |
| 書き出しジョブの作業領域 | `%LOCALAPPDATA%\DrillForge\work\<job-uuid>\` | ユーザ専用 ACL。`C:\Windows\Temp` のような共有領域を避ける |
| クラッシュ復旧・自動保存 | `%LOCALAPPDATA%\DrillForge\recovery\` | 同上。実名を含み得るので既定 ACL のユーザ専用領域 |
| 一時 WAV（FFmpeg 入力） | 書き出しジョブの作業領域 | 完了時にディレクトリごと削除 |

自動保存も `atomic_write` を通す（現状 [main.rs:482](../../crates/drill-app/src/main.rs) は `fs::write`）。
上書き前の確認は OS の保存ダイアログに委ねる（`rfd` が行う）が、`current_path` への「上書き保存」では
`LinkWarning` があるときのみ追加の確認を出す。

---

### 3.7 プラグイン API へのセキュリティ要件

52-interop-plugins.md が API の形を決める。本書は**サンドボックス方式**を決める。これは後から足せない決定である。

#### 決定: WebAssembly コンポーネント（wasmtime）のみ。ネイティブ動的ライブラリは永久に採用しない。

ネイティブ DLL / dylib を却下する理由:

1. **サンドボックス化が原理的に不可能**。ロードした瞬間に利用者と同一の全権限を持つ。後から制限を足せない。
2. **Windows の DLL 探索順を巻き込む**。プラグインを置いたディレクトリがアプリの探索経路に入り、
   DrillForge がロードする他のすべてのライブラリに対する探索順ハイジャックの足場になる。
   3.5 の実行ファイル探索と同型の問題が、より広い面で発生する。
3. **ABI 固定**。`Document` / `Edit` の構造をリファクタできなくなり、DESIGN_GAPS の Wave 群と正面衝突する。
4. **人気プラグインの乗っ取り = 全利用者のアカウント侵害**。マーチング業界のツール配布は個人・小規模団体が担うため、
   配布インフラの侵害耐性は期待できない。

#### 能力ベースの権限設計

```rust
// crates/drill-sandbox/src/plugin.rs   (feature = "plugins")

/// Everything a plugin may be allowed to do. There is no ambient authority:
/// a plugin that was not granted a capability has no host function to call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// Read the document snapshot (positions, counts, sections).
    ReadDocument,
    /// Read performer labels. Separate from `ReadDocument` because labels are
    /// personal data.
    ReadPerformerLabels,
    ReadSelection,
    /// Return a batch of `Edit`s for the host to validate and apply.
    ProposeEdits,
    /// Read one host-opened asset through an opaque handle. No path is ever
    /// visible to the plugin.
    ReadAsset,
    /// Write to one host-created output stream. No path, no second file.
    WriteExport,
}

#[derive(Clone, Debug)]
pub struct PluginManifest {
    pub id: String,
    pub version: String,
    pub publisher: String,
    pub requested: Vec<Capability>,
    /// Ed25519 over (wasm bytes || canonical manifest bytes).
    pub signature: Option<[u8; 64]>,
}

#[derive(Clone, Copy, Debug)]
pub struct PluginLimits {
    pub memory_bytes: usize,
    pub table_elements: usize,
    pub instances: usize,
    /// Deadline for a call made on the UI thread's behalf.
    pub interactive_deadline: Duration,
    /// Deadline for a call running inside a background job.
    pub job_deadline: Duration,
    pub output_bytes: u64,
}

impl PluginLimits {
    pub const DEFAULT: Self = Self {
        memory_bytes: 256 * 1024 * 1024,
        table_elements: 100_000,
        instances: 1,
        interactive_deadline: Duration::from_millis(250),
        job_deadline: Duration::from_secs(30),
        output_bytes: 64 * 1024 * 1024,
    };
}

pub struct PluginHost { /* wasmtime::Engine + epoch ticker thread */ }

impl PluginHost {
    pub fn new(limits: PluginLimits) -> Result<Self, DrillError>;
    pub fn load(&self, wasm: &[u8], manifest: &PluginManifest, granted: &[Capability])
        -> Result<Plugin, DrillError>;
}

pub struct Plugin { /* wasmtime::Store<HostState> + instance */ }

impl Plugin {
    /// Returns edits to be validated by the host. The plugin never mutates the
    /// document itself.
    pub fn transform(&mut self, doc: &Document, selection: &Selection)
        -> Result<Vec<Edit>, DrillError>;
}
```

ホスト側の必須設定:

- `Config::epoch_interruption(true)` + 1 ms 周期で `engine.increment_epoch()` する常駐スレッド 1 本。
  `store.set_epoch_deadline(n)` で呼び出しごとに締め切りを設定する。
  **エポック検査は wasm コード側から回避できない**ため、無限ループを確実に止められる。
- `Store::limiter` に `StoreLimitsBuilder::new().memory_size(256 MiB).instances(1).tables(4).build()`。
- `Config::wasm_threads(false)`（共有メモリと競合を持ち込まない）、`Config::wasm_nan_canonicalization(true)`
  （`00-conventions.md` の決定論不変条件を守るため）。
- **WASI を一切与えない。** `preopened_dir` を呼ぶコードを書かない。`wasi:filesystem` / `wasi:sockets` /
  `wasi:clocks` / `random_get` のいずれもインポートさせない。時刻と乱数が無いことは決定論の要件でもある。
- インポートは DrillForge が定義したホスト関数のみ。付与されていない `Capability` に対応する関数は
  そもそもリンクしない（呼べば instantiation エラー）。
- 出力は `output_bytes` で打ち切る。

ホストが返り値を扱う規則:

- プラグインが返した `Vec<Edit>` は、**`Document` の複製に適用してから `validate_untrusted(&Limits::DERIVED)` を通す**。
  1 件でも不正なら**全件を破棄**し、元の文書には触れない。不正な文書をプラグインが作れないことを型と検証で保証する。
- 適用は 1 個の `Edit::Batch` として履歴に積み、プラグイン ID をラベルに含める（否認防止 = STRIDE の R）。
- 未署名プラグインは、要求する `Capability` を 1 つずつ日本語で列挙した同意ダイアログを経てのみロードでき、
  UI 上で常に「未検証」と表示する。署名鍵の運用は 53 と 52 の決定に従う（9. 未決事項）。

---

### 3.8 ネットワーク

**既定: 一切通信しない。** 起動〜編集〜保存〜書き出しの全経路で外向き接続はゼロ（不変条件 I10 で検証する）。

通信が発生し得るのは次の 2 つだけで、いずれも既定オフまたは利用者の明示操作を起点とする。

| 用途 | 既定 | 失敗時 |
|---|---|---|
| 更新確認 | オフ（初回起動時に一度だけ意思を尋ねる） | 無視。ログに残すだけ。編集・保存・書き出しを一切妨げない |
| ライセンス認証 | 利用者が「オンラインで認証」を押したときのみ | オフライン認証ファイル（署名付き）へのフォールバックを常時提供 |

- TLS は `rustls` を使い、OS 証明書ストア（`rustls-native-certs`）を信頼する。
  企業の TLS 検査環境で更新確認だけが失敗して問い合わせが増えるのを避けるため。
- **証明書ピンニングは採用しない。** 理由: (a) CA ローテーションと企業 TLS 検査で確実に壊れ、
  「壊れたら通信しない」= 更新が届かない、という悪い方向へ倒れる。
  (b) 完全性は TLS ではなく**オフライン署名**で担保するので、輸送路が丸ごと乗っ取られても被害が出ない。
  ピンニングが守るものは既に別の手段で守られている。
- 代わりに**リリース署名鍵（Ed25519 公開鍵）をバイナリに埋め込む**。更新マニフェスト
  `{version, sha256, url, min_supported}` の署名を検証してからでなければダウンロードしない。
  鍵のローテーションは新旧 2 鍵を受理する重複期間を挟んだリリースで行う。
- タイムアウトは接続 10 s / 全体 30 s。試行は 24 時間に 1 回まで。失敗してもリトライしない。
- **文書に書かれた URL を取得することは無い。** アセット参照は `AssetRef`（3.3）に URL バリアントを持たない。
  FFmpeg 側は `-protocol_whitelist file,pipe` で同じ性質を強制する。
- ネットワークを恒久的に遮断した端末で、全機能（プラグインを含む）が動作すること。

---

### 3.9 プライバシー

- **テレメトリはビルドに含めない。** 「既定オフのフラグ」ではなく `--features telemetry` を既定で外し、
  出荷バイナリにコードパスが存在しない状態にする。「オフのつもりが送っていた」という事故を構造で防ぐ。
- **クラッシュレポートは送信前に全文提示する。**

  ```rust
  pub struct CrashReport {
      pub app_version: &'static str,
      pub os_build: String,
      pub gpu_adapter: String,
      /// `--remap-path-prefix` 済みのバックトレース
      pub backtrace: String,
      /// 構造のみ。内容は含めない。
      pub document_shape: DocumentShape,
  }

  #[derive(Clone, Copy, Debug)]
  pub struct DocumentShape {
      pub schema_version: u16,
      pub performers: u32,
      pub sets: u32,
      pub total_counts: u32,
      pub has_audio: bool,
      pub route_overrides: u32,
  }
  ```

  含めないもの: 文書本体、`title`、`Performer::label`、`Section::name`、ファイルパス、ユーザー名、
  マシン名、環境変数。パスは `<home>` / `<doc>` / `<work>` のマーカーへ置換する。
  ダイアログはスクロール可能なテキストボックスで**実際に送るバイト列そのもの**を見せ、
  「ファイルに保存」を常に提供し、「送信」は既定ボタンにしない。
- **演者の実名は個人情報として扱う。** マーチングの文書には未成年の実名が入る。
  - 復旧スナップショットと自動保存は `%LOCALAPPDATA%` のユーザ専用領域に置く。
  - PDF / CSV / ドリルブックには設計上実名が載る。**初回の書き出し時に一度だけ**
    「この出力には演者名が含まれます」と告知する。
  - 「匿名化して書き出す」オプション（`label` をドリルナンバーへ置換）を用意する。
  - クラッシュ払出物には決して含めない（不変条件 I11 の文字列検索テストで機械的に検証する）。
- `Document` に作成者名・端末名・メールアドレスのフィールドを**追加しない**。
  53 の要請でどうしても必要になった場合は、個人情報として明示し、クラッシュ払出物と
  「匿名化して書き出す」の除外対象に必ず加える。
- ビルド時に `--remap-path-prefix` を使い、開発者のユーザー名がバイナリ内の
  パス文字列として出荷されないようにする（供給網とプライバシーの両方に効く）。

---

### 3.10 供給網

`deny.toml`（新設）:

```toml
[advisories]
db-urls = ["https://github.com/rustsec/advisory-db"]
yanked = "deny"
unmaintained = "workspace"
ignore = []                      # 例外は必ず理由と期限をコメントで書く

[bans]
multiple-versions = "warn"
wildcards = "deny"
deny = [
  { name = "openssl-sys", reason = "TLS は rustls に一本化する" },
  { name = "git2",        reason = "不要な巨大 C 依存" },
]

[licenses]
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
  "Unicode-3.0", "CC0-1.0", "MPL-2.0",
]
confidence-threshold = 0.93

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

**`MPL-2.0` を allow に入れている点は意図的な要注意項目**: 音声デコードに使う `symphonia` は MPL-2.0 である。
MPL-2.0 はファイル単位のコピーレフトで、非改変のまま静的リンクして商用配布する分には問題ないが、
`symphonia` のソースを改変した場合はその改変ファイルの公開義務が生じる。
「改変しない」を運用規則として明文化する（9. 未決事項 1）。

`unsafe` の方針:

- 自作コードは全クレートで `unsafe_code = "forbid"`。**唯一の例外**は
  `drill-sandbox::process::job_object`（`cfg(windows)`、`windows-sys` の 3 関数呼び出し）。
  そのモジュールに限り `#![allow(unsafe_code)]` を置き、各 `unsafe` ブロックに
  `// SAFETY:` コメントで前提条件を書く。差分レビュー必須の対象としてマークする。
- GPU バッファのバイト列変換は `bytemuck`（安全 API）を使い、手書きの `transmute` を書かない。
- 依存側の `unsafe` は密度で優先度を付ける。本ワークスペースで `unsafe` の多い依存は
  `wgpu` / `ash` / `cpal`（デバイス API のため不可避）、少ないのは `zip` / `symphonia` / `image`。
  advisory が出たとき、`unsafe` 密度が高い依存の同一 CVSS は**より高い優先度**で扱う。

更新方針:

- `Cargo.lock` をコミットし、CI とリリースは常に `--locked`。
- 依存の更新は週次で `cargo update` を試し、`cargo deny` / `cargo audit` / 全テスト / ベンチ回帰が通ることを確認して 1 コミットにまとめる。緊急 advisory は即日。
- **直接依存を追加するときは `build.rs` の有無を確認し、あるものは中身を読む。**
  `cargo-deny` も `cargo-audit` も build script の任意実行を止められない。
  レビュー結果を `docs/deps.md` に日付付きで記録する。build script の無い代替があればそちらを選ぶ。
- `cargo-vet` は**現時点では導入しない**。監査を実際に読む人員が 1 人では機能せず、
  「全部 exempt」のファイルが増えるだけになる。維持者が 2 人以上になった時点で再検討する（9. 未決事項 5）。

ビルドの再現性:

- `rust-toolchain.toml` で stable のパッチバージョンまで固定し、`components = ["clippy", "rustfmt"]` を指定。
- リリースビルドは固定イメージ上で `--locked` かつ `CARGO_INCREMENTAL=0`、`--remap-path-prefix` 付きで行う。
- **Windows/MSVC でのビット単位再現ビルドは現時点で達成できない**（PE ヘッダのタイムスタンプ、PDB パス、
  リンカのバージョン差）。達成できないものを「達成した」と書かない。
  代わりに、成果物ごとに `BUILD.txt`（ツールチェーンのハッシュ、`Cargo.lock` のハッシュ、
  git commit、ビルド日時）を同梱し、SHA-256 と署名を公開する。

---

### 3.11 配布物の完全性

53-productization.md と分担する。本書はセキュリティ要件のみを定める。

- **Windows のコード署名は必須**。実行ファイルとインストーラの両方に署名する。
  CA/B フォーラムの規定により、2023 年 6 月以降コード署名鍵は FIPS 140-2 レベル 2 相当のハードウェア
  （HSM / トークン）に置く必要があり、ファイルベースの証明書は購入できない。
  クラウド署名サービス（Azure Trusted Signing 等）か、EV 証明書 + トークンのいずれかを選ぶ。
  EV は SmartScreen の評価が最初から付く点で初期の利用者体験が良い。
- 署名には**必ず RFC 3161 のタイムスタンプを付ける**
  （`signtool sign /fd SHA256 /tr <TSA URL> /td SHA256`）。証明書失効後も署名が有効であり続けるため。
- 公開する成果物すべてに SHA-256 を併記する。
- 更新の検証順序（すべて通らなければ実行しない）:
  1. マニフェスト `{version, sha256, url}` の Ed25519 署名を、埋め込み公開鍵で検証
  2. ダウンロードしたバイト列の SHA-256 がマニフェストと一致
  3. インストーラの Authenticode 署名を `WinVerifyTrust` で検証し、署名者が想定の発行者と一致
  4. 利用者が更新の実行を明示的に承認
- **サイレント自動更新は行わない。**
- 実行時の自己改竄検知は実装しない。同一権限の攻撃者に対して検査コードごと書き換えられるので効果が無く、
  誤検知でサポート負荷だけが増える。完全性は配布時点の性質として担保する。

---

## 4. 不変条件

テストで検証できる形で書く。括弧内は 7. の対応項目。

| # | 不変条件 | 検証 |
|---|---|---|
| I1 | `load_document(bytes, limits)` は**任意のバイト列**に対して panic しない | fuzz + corpus を `catch_unwind` で包む |
| I2 | `load_document` が `Ok(doc)` を返すとき、`doc` は `limits` の全上限を満たし、`doc` 内の全 `f32` が `is_finite()` | property test（再検証が常に `Ok`） |
| I3 | `SafeName::parse(n)` が `Ok` なら、任意の `root` に対し `root.join(n.as_str())` の正規化結果は `root` 配下 | property test + corpus |
| I4 | `Container` が展開するバイト総数は `total_uncompressed` を超えない（**ヘッダ申告値ではなく実測で**） | zip 爆弾 corpus + RSS 測定 |
| I5 | `ExternalTool::resolve` が返すパスは絶対・通常ファイル・`.exe`・`PATH`/CWD 由来でない | CWD と PATH にスタブを置くテスト |
| I6 | `ExternalTool::command` の子プロセス環境は allow-list の変数のみを含む | 環境をダンプするスタブ exe で検証 |
| I7 | `atomic_write` が `Err` を返したとき、対象ファイルの内容はビット単位で不変 | 障害注入テスト |
| I8 | 文書に由来する文字列から `WriteTarget` を構築できない | 型で保証（`WriteTarget` に公開コンストラクタが無い）+ コンパイル失敗テスト |
| I9 | プラグインの返した `Edit` 群は、適用後の文書が `validate_untrusted` を通らない限り一切適用されない | 不正 Edit を返すテスト用プラグイン |
| I10 | 既定設定での「起動 → 編集 → 保存 → 動画書き出し」で外向き TCP/UDP 接続は 0 | ループバック以外を遮断した環境での統合テスト |
| I11 | クラッシュ払出物に `title` / `Performer::label` / 絶対パス / ユーザー名が含まれない | 既知の文字列を仕込んで払出物を全文検索 |
| I12 | `drill-audio` / `drill-export` / `drill-app` に `std::fs` / `std::process` の直接呼び出しが無い | clippy `disallowed-methods`（CI で `-D warnings`） |
| I13 | 全クレートで `unsafe_code = "forbid"`（`process::job_object` を除く） | `[workspace.lints]` + grep テスト |
| I14 | ワークスペースの実コード（テスト除く）に `unwrap()` / `expect()` / `panic!()` が無い | clippy `-D warnings` |

---

## 5. 性能

**フレーム予算 16.6 ms のうち、本設計が使うのは 0 ms。** 検査はすべて読込時・書き出し時に一度だけ走り、
描画ループには一切入らない。40-jobs.md の `Job<T>` に載せてワーカースレッドで実行し、UI を止めない。

### 読込時のコスト（基準規模 1,000 人 × 256 セット = pretty JSON 16.6 MB）

| 処理 | 計算量 | 想定 | 根拠 |
|---|---|---|---|
| `read_capped` | O(n) | 約 20 ms | SSD の逐次読み出し |
| `scan_json` | O(n)・確保ゼロ | 約 17 ms | 1 バイト 1 回の状態遷移。目安 1 GB/s |
| `serde_json::from_str` → `Document` | O(n) | 約 37 ms | 既存ベンチ「1,000人ドキュメント×100回 JSON 変換 = 28.75 ms」から約 450 MB/s を外挿 |
| `validate_untrusted` | O(点数) | 約 1 ms | 256,000 点の 1 パス。確保ゼロ |
| **合計** | | **約 75 ms** | |

上限規模（4,000 人 × 256 セット = 66 MB）で約 **300 ms**。目標は基準規模 100 ms 以内、上限規模 500 ms 以内。
どちらもワーカースレッドで走るので UI のフレーム落ちは発生しない。

`scan_json` の追加コストは全体の約 23%。これは「事前スキャンをやめれば 17 ms 速くなる」という意味だが、
スタックオーバーフローによる**捕捉不能な abort**（6.F2）を防ぐ唯一の手段なので、この 17 ms は必要経費とする。

### メモリ

- 読込のピークは `json_bytes`（最大 128 MiB）+ `Document`（16.6 MB 規模なら約 6 MB）。
- `scan_json` の追加確保はゼロ（深度スタックは `[u8; 32]` の固定配列で足りる）。
- `Container::read_entry` は `out: &mut Vec<u8>` を呼び出し側から受け取り、再利用する。

### その他の経路

| 処理 | コスト |
|---|---|
| `overflow-checks = true`（release） | ホットループは f32 なので影響ほぼゼロ。ベンチ回帰 +5% 以内を CI 条件にする |
| `ExternalTool::resolve` | 起動時 1 回。`symlink_metadata` + SHA-256（約 80 MB の exe で約 200 ms）。結果はキャッシュ |
| `ExternalTool::command`（`env_clear` + allow-list） | プロセス起動あたり 1 ms 未満 |
| 書き出し中の stdout/stderr ドレインスレッド | 2 スレッド。フレーム生成と並行。追加コストは無視できる |
| プラグインの epoch ticker | 1 ms 周期でスリープするスレッド 1 本。CPU 使用率は測定限界以下 |
| `atomic_write` の `sync_all()` | 16.6 MB で約 30〜100 ms（デバイス依存）。自動保存はワーカーで実行する |

---

## 6. 失敗モードと安全性

### 失敗モード一覧

| # | 壊れ方 | 対処 | 利用者への伝え方 |
|---|---|---|---|
| **F1** | 巨大 JSON で OOM | 読む前に `metadata` でサイズ判定、読みながら `Read::take` で二重に制限 | `LimitExceeded { FileBytes }`。引き上げ可 |
| **F2** | 深いネストで**スタックオーバーフロー** | `scan_json` を非再帰で書き、深度 32 で先に弾く | `LimitExceeded { JsonDepth }`。引き上げ**不可** |
| **F3** | NaN / Inf 座標 | 読込時に `validate_untrusted` で拒否。`svg::px` にもフォールバックを置く | `NonFiniteNumber { FieldPath }` |
| **F4** | zip 爆弾（高圧縮率・巨大展開） | ストリームでバイトを数え、`ratio` / `entry_uncompressed` / `total_uncompressed` で打ち切る | `LimitExceeded { ZipRatio }` 等。引き上げ不可 |
| **F5** | zip slip（`../`・絶対パス・ドライブ・デバイス名・末尾ドット・ADS・同名） | `SafeName::parse` の 13 条件 + 展開時の canonicalize 再確認 + `create_new` | `UnsafeEntryName`。名前は無害化して表示 |
| **F6** | シンボリックリンクエントリで外部を上書き | エントリの mode / 属性で拒否。展開は `create_new` のみ | `UnsafeEntryName` |
| **F7** | 文書内の UNC パスで NTLM ハッシュが漏れる | `AssetRef::External` は自動で開かない。UNC は `validate_untrusted` で拒否 | 「音源の場所を選び直してください」 |
| **F8** | 文書内の絶対パスで他ファイルの内容が動画に混入 | 同上。FFmpeg の入力は自前で書いた WAV のみ | 同上 |
| **F9** | CWD / PATH の偽 `ffmpeg.exe` が実行される | `ExternalTool::resolve` が絶対パスのみを返す。`PATH`・CWD を見ない | `ToolNotFound`。「同梱の FFmpeg が見つかりません」 |
| **F10** | `.bat` / `.cmd` 呼び出しでの引数注入（CVE-2024-24576 / 43402） | 拡張子 `.exe` 以外を拒否 | `UnsafeToolPath` |
| **F11** | `FFREPORT` 等の環境変数で任意ファイル書き込み | `env_clear` + allow-list | ユーザー可視の失敗にはならない |
| **F12** | stdout を読まずに stdin へ書き続けてデッドロック（画面フリーズ） | 3 パイプを別スレッドで並行処理。60 秒ウォッチドッグ | 「書き出しが応答しません。中止しました」 |
| **F13** | 親クラッシュで FFmpeg が孤児化し CPU を占有 | Windows ジョブオブジェクト（`KILL_ON_JOB_CLOSE`）+ `ChildGuard` | — |
| **F14** | 保存中の電源断・ディスク満杯で原本喪失 | `atomic_write`（temp → fsync → rename）。原本には触れない | 「保存できませんでした。元のファイルは無事です」 |
| **F15** | 保存先がリンクで別の場所を上書き | `prepare_write` が `LinkWarning` を返し、実体パスを見せて確認 | 「このパスはリンクです。実体は … です」 |
| **F16** | 音声デコード爆弾でメモリ／CPU 枯渇 | probe による事前検算 + デコード中の実バイト計数 + 60 秒締め切り | `LimitExceeded { AudioDecodedBytes }` |
| **F17** | 画素爆弾 | `image::Limits` で寸法判定を確保より先に | `LimitExceeded { ImagePixels }`。引き上げ不可 |
| **F18** | プラグインの無限ループで UI 凍結 | wasmtime epoch 締め切り 250 ms（対話）/ 30 s（ジョブ） | 「プラグイン … が時間内に応答しませんでした」 |
| **F19** | プラグインが不正な文書を作る | 複製に適用 → `validate_untrusted` → 不合格なら全件破棄 | 「プラグイン … の編集は適用されませんでした」 |
| **F20** | クラッシュレポートに実名が載る | `DocumentShape` のみを含める。送信前に全文提示 | ダイアログで実物を見せる |
| **F21** | 更新配信の改竄 | マニフェストの Ed25519 検証 → SHA-256 検証 → Authenticode 検証 → 利用者承認 | 「更新の署名を検証できませんでした」 |
| **F22** | 依存の既知脆弱性 | `cargo deny` / `cargo audit` を毎 push と毎日 | — |
| **F23** | 攻撃者制御の文字列が UI へそのまま出て偽装に使われる | `DrillError` は文字列を持たない（`FieldPath` / `LimitKind` / 行列番号のみ）。やむを得ない場合は制御文字・双方向制御文字を除去し 64 文字で切る | — |

**F2 を特記する**: Rust の「パニック禁止」を徹底してもスタックオーバーフローは防げない。
これは panic ではなく SIGSEGV / `STATUS_STACK_OVERFLOW` によるプロセス**強制終了**であり、
`catch_unwind` でも `set_hook` でも捕まえられない。保存されていない編集がその場で失われる。
だから深度制限は「事後に捕まえる」ではなく「事前スキャンで到達させない」設計でなければならない。

### 利用者への伝え方（42-errors-i18n.md に載る形）

エラーは構造で持ち、文言はロケール層で組み立てる。

```rust
// 42 側のシグネチャに合わせる
impl DrillError {
    pub fn message(&self, locale: Locale) -> String;
    /// 利用者が取れる行動。UI がボタンに変換する。
    pub fn remedy(&self) -> Remedy;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
    /// 何もできない。送り手に確認を促す。
    ContactSender,
    /// 規模上限。自己責任で引き上げて再試行できる。
    RaiseLimit(LimitKind),
    /// アセットの場所を選び直す。
    RelocateAsset,
    /// 設定画面で外部ツールのパスを指定する。
    ConfigureTool(ToolKind),
    /// 別の保存先を選ぶ。
    ChooseAnotherPath,
}
```

文言の設計方針（`ja` の例。実際の文字列は 42 のテーブルに置く）:

- **原因の所在をはっきりさせる。** 「このファイルは DrillForge の安全上限を超えています」——
  受け取った側が「自分の操作ミスかもしれない」と迷わないようにする。
- **数値を出す。** 「セット数: 上限 1,024 に対し 50,000 が含まれています」。
- **次にすべきことを 1 つ示す。** `Remedy` がそのままボタンになる。
- **「破損しているか、意図的に細工されている可能性があります。送信者に確認してください」** を
  `Remedy::ContactSender` の定型文にする。受け渡し文化のある業界では、これが実際に有効な次の一手になる。
- 引き上げ可能な上限（`LimitKind::is_user_raisable()`）にだけ
  「上限を引き上げて開く（自己責任）」を出す。攻撃面上限には**出さない**。
  引き上げは当該ファイル 1 回限りで、設定として永続化しない。
- 攻撃者由来の文字列は絶対に生で出さない（F23）。

---

## 7. テスト計画

### 7.1 悪意あるサンプルのテストコーパス

`crates/drill-sandbox/tests/corpus/` にコミットする。各ファイルに同名の `.expect`
（期待する `LimitKind` / エラー種別を 1 行で記述）を添える。
**生成スクリプト `tests/corpus/generate.rs` も一緒に置き、巨大なファイルはコミットせず生成する**
（`wide_array_10m.json` などはリポジトリに置かない）。

**JSON（`corpus/json/`）**

`deep_nesting_10k.json` / `wide_array_10m.json`（生成） / `nan_literal.json` /
`inf_exponent.json`（`1e400`） / `huge_string_64mb.json`（生成） / `duplicate_keys.json` /
`utf8_invalid.bin` / `bom_prefix.json` / `raw_control_char_in_string.json` /
`negative_counts.json` / `performer_id_duplicate.json` / `positions_len_mismatch.json` /
`schema_version_65535.json` / `grid_width_nan.json` / `tempo_events_1m.json` /
`empty_sets.json` / `unknown_field.json`

**zip コンテナ（`corpus/zip/`）**

`traversal_dotdot.drillproj` / `traversal_encoded.drillproj` / `absolute_path.drillproj` /
`drive_letter.drillproj` / `unc_path.drillproj` / `backslash_separator.drillproj` /
`trailing_dot_name.drillproj` / `trailing_space_name.drillproj` / `device_name_con.drillproj` /
`ads_colon.drillproj` / `symlink_entry.drillproj` / `duplicate_names_case.drillproj` /
`bidi_override_name.drillproj` / `bomb_flat_1000x.drillproj` / `bomb_nested.drillproj` /
`entry_count_100k.drillproj`（生成） / `lfh_cd_name_mismatch.drillproj` /
`encrypted_entry.drillproj` / `zip64_lying_size.drillproj` / `unsupported_method_lzma.drillproj`

**音声（`corpus/audio/`）**

`ext_mp3_actually_zip.mp3` / `wav_declared_1000h.wav`（ヘッダ詐称） /
`wav_65535_channels.wav` / `flac_huge_blocksize.flac` / `truncated_mid_frame.ogg` /
`zero_sample_rate.wav`

**画像（`corpus/image/`）**

`png_64000x64000.png`（生成、寸法だけ巨大） / `apng_10k_frames.png` /
`ext_png_actually_exe.png` / `bmp_negative_height.bmp`

**パス／文書内参照（`corpus/path/`）**

`audio_unc_share.drill.json` / `audio_absolute_secret.drill.json` /
`audio_relative_traversal.drill.json`

### 7.2 パニック検出ハーネス

```rust
// crates/drill-sandbox/tests/malicious_corpus.rs

/// Every corpus file must produce a typed error, never a panic and never a
/// silent success.
#[test]
fn corpus_never_panics() {
    for path in corpus_files("json") {
        let bytes = std::fs::read(&path).expect("corpus file readable");
        let outcome = std::panic::catch_unwind(|| {
            drill_core::ingest::load_document(&bytes, &drill_core::limits::Limits::INTERACTIVE)
        });
        let Ok(result) = outcome else {
            panic!("panicked while loading {}", path.display());
        };
        assert_expected_error(&path, &result);
    }
}
```

`assert_expected_error` は `.expect` ファイルの 1 行（例 `LimitExceeded:JsonDepth`）と突き合わせる。
`Ok` を期待するファイルには `Ok` と書く。**期待値ファイルを持たせることで、「弾けているが理由が違う」を検出する。**

### 7.3 ファジング（`fuzz/fuzz_targets/`）

| ターゲット | 対象 | 不変条件 |
|---|---|---|
| `document_json` | `load_document(data, &Limits::INTERACTIVE)` | panic しない。`Ok` なら再検証も `Ok`。`to_json` → 再読込で往復一致 |
| `safe_name` | `SafeName::parse(s)`（`arbitrary` で構造化） | `Ok` なら `root.join()` が `root` を脱出しない |
| `container` | 任意バイト列を zip として `Container::open` + 全エントリ読み | panic しない。総展開バイトが上限以下 |
| `migrate_v1` | v1 JSON → v2 マイグレーション | panic しない。`Ok` なら v2 として妥当 |
| `ffmpeg_args` | 任意の `VideoExportConfig` | panic しない。`Ok` なら引数列に `-protocol_whitelist` が含まれ、`-i` の値が `pipe:0` か `file:` 始まり |

- 各リリース前に全ターゲットを **24 時間クラッシュ無し**で回すことを条件にする。
- シードコーパスは 7.1 のファイルから作る（`fuzz/corpus/` にはシードのみ置き、増殖分はコミットしない）。
- `-max_len` を `json_bytes` より十分小さく（例 1 MiB）して、探索効率を上げる。

### 7.4 property test（proptest）

- `∀ doc ∈ arbitrary_valid_document: load_document(&doc.to_json()?) == Ok(doc)`
- `∀ doc: validate_untrusted(&doc, L).is_ok() ⇒ 全 f32 が finite`（reflection ではなく手書きの走査で確認）
- `∀ name ∈ arbitrary_string: SafeName::parse(name).is_ok() ⇒ normalize(root.join(name)).starts_with(root)`
- `∀ (old, new) ∈ (bytes, bytes)`: `atomic_write` を任意のステップで失敗させると、
  対象ファイルの内容は `old` か `new` のいずれかと完全一致する（障害注入 FS ラッパ）

### 7.5 プロセステスト（`cfg(windows)` を含む）

| # | 手順 | 期待 |
|---|---|---|
| P1 | 一時ディレクトリに終了コード 3 のスタブ `ffmpeg.exe` を置き、そこを CWD にして `resolve` | 解決されない（`ToolNotFound`） |
| P2 | `PATH` の先頭に同じスタブを置いて `resolve` | 解決されない |
| P3 | `Configured("C:\\x\\ffmpeg.bat")` を渡す | `UnsafeToolPath` |
| P4 | `Configured("C:\\x\\ffmpeg.exe ")`（末尾空白） | `UnsafeToolPath` |
| P5 | 環境をダンプするスタブ exe を起動し、出力を検査 | allow-list 外の変数（`FFREPORT` を含む）が存在しない |
| P6 | `ffmpeg_args` のゴールデン比較 | `-protocol_whitelist file,pipe` を含む。`-i` の値は `pipe:0` と `file:<abs>` のみ。出力は `.part` |
| P7 | 親プロセスを強制終了 | ジョブオブジェクトにより子も終了する |
| P8 | 書き出し中に stdout を読まない実装へ差し替えたモックで 100 MB を流す | 現行実装ではデッドロックしないことを 60 秒以内に確認 |

### 7.6 ファイルシステムテスト

- `read_capped(path, 64 MiB)` に 2 GiB のファイル → 早期に `LimitExceeded`、RSS 増加が 64 MiB 未満
- `read_capped` にディレクトリ／名前付きパイプ → `NotRegularFile`
- `atomic_write` 中に一時ファイル名を先取りされる → `create_new` が失敗し、原本は無傷
- 保存先がシンボリックリンク／ジャンクション → `LinkWarning` が立つ（自動追随しない）
- `atomic_write` 失敗後、対象ファイルのハッシュが書き込み前と一致

### 7.7 ストレス・回帰

- 4,000 人 × 256 セット（66 MB）の読込が 500 ms 以内、ピーク RSS が 400 MB 以内
- `cargo bench -p drill-core --bench core_performance` が `overflow-checks = true` 有効下で
  ベースライン（9.24 ms / 28.75 ms）比 +5% 以内
- 7.1 の全 corpus を 1,000 回連続で読ませてもメモリが単調増加しない
- ネットワークをループバック以外遮断した環境で、起動〜編集〜保存〜書き出し〜プラグイン実行が全て成功する（I10）

### 7.8 CI 強制

3.4 の `.github/workflows/ci.yml` に加えて:

- リリースジョブ: `--locked` ビルド → `signtool` 署名 → SHA-256 生成 → Ed25519 マニフェスト署名 →
  **署名検証テスト**（自作の verifier で 3.11 の 3 段検証を実行）
- `cargo deny check` の失敗は即 fail（`ignore` に追加する場合は理由と期限のコメントを必須にする）

---

## 8. 実装タスク

1 タスク = 1〜3 時間相当。`⊂` は依存を表す。

### 段階 A — 土台（DESIGN_GAPS Wave 0 と同時。直列）

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| S-1 | `LimitKind` / `Field` / `FieldPath` / `Limits` を `drill-core/src/limits.rs` に定義。`DrillError` に `LimitExceeded` / `NonFiniteNumber` / `NotUtf8` / `Json{line,column}` / `UnsafeEntryName` / `UnsafeToolPath` / `NotRegularFile` / `LinkWarning` を追加（42 と共同） | Wave 0-1 の `DrillError` 導入 | — |
| S-2 | `[workspace.lints]` / `clippy.toml` / `overflow-checks = true` / `rust-toolchain.toml` / `.github/workflows/ci.yml` を新設。既存コードを clippy 通過させる | なし | S-1 と並行可 |
| S-3 | 既存パニック経路の修正: [lib.rs:385](../../crates/drill-core/src/lib.rs), [editing.rs:65-67/83-85](../../crates/drill-core/src/editing.rs), [countsheet.rs:29](../../crates/drill-core/src/countsheet.rs), [main.rs:1351](../../crates/drill-app/src/main.rs), [svg.rs:18/44](../../crates/drill-core/src/svg.rs) | S-2 | — |

### 段階 B — JSON ingest（並行 3 本）

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| S-4 | `scan_json`（非再帰）+ `JsonShape` + 単体テスト | ⊂ S-1 | B 内で並行 |
| S-5 | `Document::validate_untrusted` + 全 f32 の有限性検査 + 文字列長・要素数検査 + `deny_unknown_fields` | ⊂ S-1 | B 内で並行 |
| S-6 | corpus（JSON 17 件）+ 生成スクリプト + `catch_unwind` ハーネス | ⊂ S-1 | B 内で並行 |
| S-7 | `load_document` 統合。`Document::from_json` を非推奨にし、[main.rs:437](../../crates/drill-app/src/main.rs) の呼び出しを差し替え | ⊂ S-4, S-5 | — |
| S-8 | fuzz ターゲット `document_json` / `migrate_v1` + `fuzz/` の足場 | ⊂ S-7 | — |

### 段階 C — `drill-sandbox` 新設（B と並行可）

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| S-9 | クレート新設 + `fs::read_capped` / `WriteTarget` / `prepare_write` / `atomic_write` + FS テスト | ⊂ S-1 | C 内の先頭 |
| S-10 | `SafeName::parse`（13 条件）+ property test + fuzz ターゲット `safe_name` | ⊂ S-9 | S-12 と並行 |
| S-11 | `Container`（読み取り専用・`LimitedReader`・`extract_all`）+ zip corpus 20 件 | ⊂ S-10 | — |
| S-12 | `ExternalTool::resolve` / `command`（`env_clear` + allow-list + stdio + `CREATE_NO_WINDOW`）+ P1〜P6 | ⊂ S-9 | S-10 と並行 |
| S-13 | Windows ジョブオブジェクト + `ChildGuard`（唯一の `unsafe`）+ P7 | ⊂ S-12 | — |
| S-14 | `video::ffmpeg_args` を `WorkFile` / `OutputTarget` / `-protocol_whitelist` / `-f` 明示 / `.part` へ変更 + ゴールデン | ⊂ S-12 | — |
| S-15 | `AudioLimits` / `ImageLimits` と `probe_audio` / `decode_audio` / `load_image` のゲート + media corpus | ⊂ S-9 | S-11 と並行 |

### 段階 D — 文書側の連携（B・C 完了後）

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| S-16 | `AssetRef` 導入。`AudioTrack::path: String` を廃止し、`External` の再選択フローを実装（41・43 と共同） | ⊂ S-5, S-11 | — |
| S-17 | `std::fs` / `std::process` の直接呼び出しを全廃し、`disallowed-methods` を `deny` に上げる | ⊂ S-9, S-12 | — |
| S-18 | パニックフック + クラッシュ払出物のレダクション + 送信前提示 UI（43 と共同）+ I11 テスト | ⊂ S-9 | S-17 と並行 |

### 段階 E — 製品化（53 と共同）

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| S-19 | `deny.toml` + `cargo audit` の CI 組み込み + 依存ライセンス棚卸し（symphonia の MPL 判断を含む） | ⊂ S-2 | いつでも |
| S-20 | リリース署名パイプライン（signtool + タイムスタンプ + SHA-256 + Ed25519 マニフェスト）と 3 段検証テスト | ⊂ S-19 | — |
| S-21 | 更新確認クライアント（既定オフ・rustls・署名検証・失敗無視） | ⊂ S-20 | — |

### 段階 F — プラグイン（P2、52 の API 確定後）

| # | 内容 | 依存 | 並行 |
|---|---|---|---|
| S-22 | `PluginHost`（wasmtime・epoch・`StoreLimits`・WASI なし）+ `Capability` の付与フロー | ⊂ S-1, 52 の API | — |
| S-23 | プラグインが返す `Edit` の検証パイプライン（複製に適用 → `validate_untrusted` → 全件破棄）+ I9 テスト | ⊂ S-22 | — |

**クリティカルパス**: S-1 → S-2 → (S-4, S-5) → S-7 → S-16。
段階 C は S-1 完了後すぐ着手でき、段階 B と完全に並行できる。

---

## 9. 未決事項

1. **`symphonia` の MPL-2.0 が販売形態に与える影響。**
   ファイル単位コピーレフトなので非改変利用なら問題ないという理解だが、
   「改変しない」を運用規則として確定させ、`deny.toml` の allow に MPL を残すかを 53 と法務判断で決める。
   代替（`hound` + 自前 MP3/AAC デコーダ）は現実的でない。

2. **音声上限 30 分は妥当か。**
   インドアパーカッション、パレード、複数曲メドレーの実音源をいくつか集めて実測してから確定したい。
   30 分で足りないケースが実在するなら `AudioLimits::duration_seconds` を上げる（引き上げ可能な上限として設計してある）。

3. **書き出しジョブの作業ディレクトリの位置と多重起動時の衝突。**
   `%LOCALAPPDATA%\DrillForge\work\<job-uuid>\` を提案したが、
   複数インスタンス・複数ジョブ・アプリのクラッシュ後に残る作業領域の掃除方針を 40-jobs.md と詰める必要がある。

4. **プラグイン署名鍵の運用。**
   誰が署名するのか（DrillForge 開発元が審査して署名するのか、作者の自己署名を TOFU で受け入れるのか）。
   前者は審査コストが、後者は失効手段が問題になる。52 と 53 の決定待ち。

5. **`cargo-vet` の導入時期。**
   単独維持者では監査を実際に読めないため現時点では不採用としたが、
   「維持者 2 人以上」という条件が妥当かは要検討。代わりに直接依存の `build.rs` レビューを義務化した。

6. **Windows における再現可能ビルドをどこまで追うか。**
   現状ビット単位再現は不可能と判断し `BUILD.txt` + SHA-256 + 署名で代替したが、
   `/Brepro` リンカフラグと `SOURCE_DATE_EPOCH` でどこまで近づけるかは未調査。

7. **インポーターが対象にする外部形式ごとの追加上限値。**
   本書は JSON / zip / 音声 / 画像のみを規定した。
   52 が対象形式を決めた時点で、形式ごとに `Limits` 相当を追加する必要がある。

8. **企業 TLS 検査環境での更新確認。**
   `rustls-native-certs` で OS ストアを信頼する方針にしたが、
   検査プロキシが Ed25519 マニフェストを壊すことはないので実害は無いはず。実環境での検証が必要。

9. **`json_bytes = 128 MiB` を全読みしてから検査する方式の限界。**
   現行は「読み切ってからスキャン」だが、ストリーミングスキャン（読みながら判定）にすれば
   ピークメモリを下げられる。基準規模では不要だが、上限規模で 400 MB の RSS 目標を満たせない場合は
   `serde_json::Deserializer::from_reader` を使う形へ切り替える。判断は S-7 の実測後。
