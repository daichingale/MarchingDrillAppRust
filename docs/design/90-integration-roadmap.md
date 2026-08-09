# 90. 統合と実装ロードマップ

22本の設計文書（10〜53）を突き合わせ、**クレート構成・文書間の裁定・実装順**を確定する。
個別の設計は各文書が正。本書は**それらの間**を決める。矛盾が生じた場合は本書が優先する。

対象文書: [00](00-conventions.md) 規約 / [10](10-document-model.md)〜[17](17-printing-drillbook.md) ドメイン /
[20](20-display-list.md)〜[23](23-camera-system.md) 描画 / [30](30-audio-engine.md)[31](31-video-export.md) メディア /
[40](40-jobs-threading.md)〜[43](43-app-architecture.md) 基盤 / [50](50-testing-ci.md)〜[53](53-productization.md) 品質と製品

---

## 1. クレート構成の確定

各文書が独立に新クレートを要求した結果、提案の総数は **17** に達した。
クレート1つにつきコンパイル単位・バージョン・CI行列・依存辺が増えるため、そのまま作ると保守が破綻する。
**段階ごとに「実在するクレート」を絞る**方針で確定する。設計自体は全て残し、作る時期だけを遅らせる。

### P0/P1 で実在させる（7本）

```
drill-core        ドキュメント・時間・座標・解析・検証。依存は serde/serde_json のみ。
  ↑
drill-render      DisplayList の生成。drill-core のみに依存。描画APIを知らない。
  ↑
drill-export      DisplayList の消費: SVG / PDF(krilla) / ラスタ(tiny-skia) / 動画(FFmpeg起動)。
drill-audio       symphonia + cpal + rtrb。drill-core に依存。
drill-project     .drillproj コンテナ・原子的保存・復旧。drill-core に依存。
drill-jobs        Job<T>。標準ライブラリのみ。上記の重い処理を載せる。
  ↑
drill-app         egui/wgpu。表示と入力の変換のみ。
```

依存は必ず上から下へ。`drill-core` は他のどれも知らない。

**統合時の変更点**: doc 17（PDF）と doc 31（動画・ラスタ）は別クレートを想定していたが、
どちらも `DisplayList` を消費し、tiny-skia / krilla / フォント処理を共有するため **`drill-export` に統合**する。

### P2 以降に作る（設計は完成済み、着手を遅らせる）

| クレート | 出典 | 遅らせる理由 |
|---|---|---|
| `drill-gpu` | [21](21-gpu-renderer.md) | GPU は opt-in 加速であり、参照実装は CPU ラスタ（§2.1 の裁定）。無くても製品は成立する。 |
| `drill-interop` / `drill-cli` | [52](52-interop-plugins.md) | 乗り換え支援は売上に直結するが、自社形式が固まる前に作ると作り直しになる。 |
| ~~`drill-license`~~ | [53](53-productization.md) | **廃止。** 有料ティアと権限機構を作らない決定により不要（§5.1）。 |
| `drill-updater` | [53](53-productization.md) | 一般配布の開始時点で必要。認証は伴わず、更新の配信のみ。 |
| `drill-sandbox` / `drill-plugin` | [51](51-security.md) / [52](52-interop-plugins.md) | P0/P1 では実装しない（Wasmtime の月次メジャー、Extism が4版遅れ、スキーマ移行中）。ただし**所有者の決定により将来必ず入れる**ため、破棄はしない。doc 52 が「後から入れられることを今の設計で保証する条件」を定義する。 |

### 開発専用（必要になった時点で追加）

`drill-testkit` / `drill-conformance` / `drill-bench`（[50](50-testing-ci.md)）。
**`drill-core` に dev-dependency を足さない**という doc 50 の原則を採用する（`cargo test -p drill-core` を無依存に保つ）。

### 外部依存の確定

| 用途 | 採用 | ライセンス上の注意 |
|---|---|---|
| PDF | krilla | MIT/Apache-2.0。`assets/NotoSansJP.ttf`(9.6MB) のサブセット化を先行検証（doc 17 T0）。 |
| ラスタ | tiny-skia | BSD-3。純Rust。 |
| 音声デコード | symphonia | **MPL-2.0**（ファイル単位コピーレフト）。open-core 化の判断に影響。§5-3。 |
| 音声出力 | cpal + rtrb | Apache-2.0 / MIT。 |
| ゴールデン | insta | doc 50 が DESIGN_GAPS の「自前で足りる」を上書き。承認フローの再実装コストが根拠。 |
| property | proptest | `Edit`/`Document` は型単位生成が破綻するため quickcheck 不可。 |
| 退行検出 | criterion + gungraun | **壁時計では CI を落とさず、Callgrind 命令数 +5% で落とす**（マシン差の吸収）。 |
| 動画 | FFmpeg（外部プロセス） | **同梱しない**。libx264 は GPL で、同梱するとライセンスが伝播する。 |
| MIDI / XML | midly / roxmltree | 依存ゼロ構成が可能。musicxml クレートは不採用（doc 52 に根拠）。 |

---

## 2. 文書間の裁定

### 2.1 決定論と GPU（最大の論点・**解決済み**）

`MEDIA_PIPELINE.md` は「同じ project/config から同じフレーム列」を要求する一方、GPU 描画は
シェーダの FP 精度・FMA 融合・MSAA サンプル位置がベンダ依存で、ビット一致を約束できない。

[21](21-gpu-renderer.md) と [31](31-video-export.md) が**独立に同じ結論**へ到達したため、これを採用する。

- 動画書き出しの**参照実装は CPU ラスタ（tiny-skia）**。2D の P0/P1 ではこれが既定かつ唯一。
- GPU は**画面表示の opt-in 加速**と、3D Real View（P2）のオプトイン書き出しに限定。
- 決定論を「アプリの性質」から**「バックエンドの性質」へ等級化**する（`BitExact` / `PerceptualOnly`）。
- 両者の一致は 2 段階で縛る: 測光差 ≤2/255 が 99.5% **かつ** ドット重心 ≤0.5px。
- 書き出しファイルに `raster_backend` を記録し、**バックエンドを跨いだ中断再開を拒否**する。

### 2.2 Document に載る状態の所有権（**解決済み**）

[10](10-document-model.md) が唯一の所有者。統合の過程で 14 フィールド・`Edit` 40 バリアントへ拡張された。
[14](14-formations-shapes.md) の `Set.shape`、[15](15-performer-sections.md) の `subsets`、
[23](23-camera-system.md) の `camera_program` は doc 10 に取り込み済み。

**キャッシュ無効化の分離**が要点。カメラ編集は `Scopes::CAMERA` のみを上げるので、
衝突走査（`TransitionKey`）・2D DisplayList（`FrameKey`）・帳票（`SheetKey`）は再計算されない。
逆向きの需要のために `Scopes::GEOMETRY`（「どれかのセットが動いた」の集約）を新設した。

### 2.3 `Selection` の二重定義（**解決済み**）

[15](15-performer-sections.md) が `PerformerId` 集合として定義し、[43](43-app-architecture.md) も
`selected: BTreeSet<usize>` → `PerformerId` へ移す設計にしていた。両者は一致している。
**doc 15 の定義を正**とし、doc 43 は参照に徹する。

### 2.4 同期アンカーとテンポマップ（**解決済み**）

[30](30-audio-engine.md) の裁定を採用する。音源の時間伸縮も TempoMap の降格も行わない。
**アンカーは提案、TempoMap が唯一の正**。不一致（>0.5%）を検出したら「テンポマップへ反映」を
`Edit` として提示し、未反映なら線形補間＋警告。**未反映のままの動画書き出しは拒否**する。

### 2.5 f32 / f64 の境界（**解決済み。ただしスキーマ型は doc 10 が最終決定中**）

`tempo.rs` は秒もカウントも f32 で扱っている。[30](30-audio-engine.md) が指摘し、
[11](11-transition-model.md) が検算して確定させた事実:

| 軸 | 位置 | f32 の ulp | 48kHz 換算 |
|---|---|---|---|
| 秒 | 480 s（8分のショー） | 3.05×10⁻⁵ s | **1.46 サンプル** |
| カウント | 960 counts | 6.1×10⁻⁵ counts | **1.46 サンプル** |
| カウント | `MAX_TIMELINE_COUNTS` | — | **750 サンプル** |

つまり「秒だけ f64 にする」では足りない。**サンプルへ焼く経路はカウントも f64** が要る。

裁定（doc 11 の設計を採用）:

- `TempoMap::seconds_at_f64` / `count_at_f64` を**前置和＋二分探索の閉形式**で追加。
  問い合わせ経路から逐次加算を完全に除去する。
- 既存 f32 API は**ラッパ化**する。実装を一本化しないと、UI と音声でセット判定が割れる。
- **`positions_at_count` は f32 のまま**。セット内ローカルカウントは `MAX_SET_COUNTS = 4,096` で
  上限され、幾何誤差 0.3mm は弧長テーブルの誤差 2.4mm より小さい。f64 化すると `Lane` が
  72→120 バイトに膨れ SIMD 幅が半減する損しかない。
- 決定論の担保: 拡張（f32→f64）は厳密、縮小は出口1か所のみ、整数判定は必ず f64 側、
  f64 の四則は IEEE-754 でプラットフォーム間ビット同一。

**未決（doc 10 が決定中）**: `TempoChange.count` を v2 で f64 にするか。
これは**永続形式の型**であり、v2 を切った後に変えると v3 マイグレーションが要る。v2 凍結前に決着させる。

### 2.7 doc 11 が副産物として発見した欠陥（修正済み）

- `Gate::arrive` の `f32::INFINITY` 番兵は JSON で `null` に化けて壊れる → `Option<f32>` へ
- `Edit::SetCounts` でセットを縮めるとゲートが不変条件 V16 違反 → `RouteTable::clamped_to` ＋ `Batch[SetCounts, SetRoutes]`
- `CoalesceKey::Routes(SetId)` 欠落でルートのドラッグが履歴を埋める

### 2.6 参照側3文書への差し戻し（**対応中**）

doc 10 が最終化の過程で、先に完成していた文書の欠陥を検出した。所有者側で修正する。

| 文書 | 内容 |
|---|---|
| [23](23-camera-system.md) | `alloc_camera_id` のオーバーフロー未検査 / `remove_cut`・`drain_cuts_for` 不足で Undo が壊れる / `PartialEq` derive 不足 / `edit_camera(FnOnce)` 橋渡し案は不変条件1に反するため不採用 / doc 21 向けに `view_matrix()` を公開 |
| [15](15-performer-sections.md) | `AddPerformers.positions` がセット**索引**順で、`DESIGN_GAPS` §0 #2 と同型のズレを演者側で再発させる → `Vec<(SetId, Vec<Point>)>` へ / `SubsetId` newtype 化 / `PartialEq` derive 不足 |
| [11](11-transition-model.md) | §2.5 の f64 API 追加 / doc 10 の `Edit` バリアント名との整合 |

---

## 3. 実装ロードマップ

`PRODUCT_QUALITY.md` の P0/P1/P2 に対応させる。**Wave 0 は直列**、以降は並行可能。
各波の括弧内は担当設計文書。

### Wave 0 — 土台（直列。これ抜きで先へ進むと全部やり直しになる）

| # | 内容 | 文書 |
|---|---|---|
| 0-1 | `DrillError` 導入、`Result<_, String>` 全廃、`Locale` 導入、drill-core から日本語リテラル排除 | [42](42-errors-i18n.md) |
| 0-2 | 安定ID（`SetId`/`PerformerId`/`SectionId`/`SubsetId`）、`IdAllocator`、`IdIndex` | [10](10-document-model.md) |
| 0-3 | `Edit` 40バリアント、`apply→逆操作`、合流、`VecDeque` History、全変更経路の集約 | [10](10-document-model.md) |
| 0-4 | `Scopes` によるリビジョン管理とキャッシュキー | [10](10-document-model.md) |
| 0-5 | スキーマ v2 + `migrate_v1_to_v2` + v1フィクスチャ回帰 | [10](10-document-model.md) |
| 0-6 | **`.gitignore` の `*.drill.json` を外す**（フィクスチャがコミットできない）・`.gitattributes` 追加 | [50](50-testing-ci.md) |
| 0-7 | **保存の原子化**（temp→`ReplaceFileW`）。利用者の制作物を失う唯一の経路を先に塞ぐ | [41](41-persistence-recovery.md) |
| 0-8 | 最小CI（fmt / clippy / test）、`[workspace.lints]`、`overflow-checks` | [50](50-testing-ci.md)[51](51-security.md) |

0-6 と 0-7 は他と独立なので、0-1〜0-5 と並行してよい。
**0-7 を Wave 0 に入れたのは、これが `DESIGN_GAPS.md` §0 で唯一「利用者のデータが消える」項目だから。**

### Wave 1 — 中核（4系統並行）

| 系統 | 内容 | 文書 |
|---|---|---|
| **A ドメイン** | `RouteTable`/`Gate`/`SetCounts`/`positions_at_count`、`StepStyle`、掃引衝突検査、`Section`/`Performer` 拡張、`Subset` | [11](11-transition-model.md)[12](12-step-style-difficulty.md)[13](13-collision-clinic.md)[15](15-performer-sections.md) |
| **B 描画** | `drill-render` 新設、`DisplayList`、egui backend、SVG backend（`svg.rs` をシリアライザへ縮小）。**§0 #6/#7 の y軸反転・アスペクト不一致がここで消える** | [20](20-display-list.md) |
| **C 音声** | `drill-audio` 新設、デコード job、`PeakPyramid`、cpal 出力、`ClickSynth`、複数アンカー | [30](30-audio-engine.md) |
| **D 基盤** | `drill-jobs`、`drill-project`（.drillproj・復旧・パニックフック） | [40](40-jobs-threading.md)[41](41-persistence-recovery.md) |

### Wave 2 — 製品化の下地

| # | 内容 | 文書 |
|---|---|---|
| 2-1 | `drill-export`: ラスタ backend、PDF backend、帳票レイアウト、ドリルブック | [17](17-printing-drillbook.md)[20](20-display-list.md) |
| 2-2 | 動画書き出し P0（フレーム生成→FFmpegパイプ→ffprobe検証、進捗・キャンセル・fallback） | [31](31-video-export.md) |
| 2-3 | `main.rs` 分解（目標60行以下）、メニューバー、**accesskit 有効化**、色以外の表現、`Command` 表 | [43](43-app-architecture.md) |
| 2-4 | 図形ツール（弧長等間隔・モーフ・対称・フィッティング）、座標表記規約 | [14](14-formations-shapes.md)[16](16-coordinates-notation.md) |
| 2-5 | 入力検査の全面適用（`Limits`、zip、音声、画像）、FFmpeg 起動の安全化 | [51](51-security.md) |
| 2-6 | テスト全層（ゴールデン・property・確保ゼロ・ストレス・fuzz）、CI 行列 | [50](50-testing-ci.md) |

### Wave 3 — 差別化と販売

カメラキーフレーム（[23](23-camera-system.md)）/ 3D Real View と可視性解析（[22](22-stadium-3d.md)）/
`drill-gpu`（[21](21-gpu-renderer.md)）/ ドリル難易度スコア（[12](12-step-style-difficulty.md)）/
乗り換えインポータ（[52](52-interop-plugins.md)）/ 署名・配布・更新（[53](53-productization.md)）。

**最初の一般配布は P1 完了時点**。P0 の品質ゲートが未達のうちは広く配らない。
無償配布であっても、この基準は緩めない（§5.1-b）。

---

## 4. 既存コードの欠陥との対応

[DESIGN_GAPS.md](../../DESIGN_GAPS.md) §0 に検証済みの欠陥16件＋法務3件を記録した。
どの波で消えるかの対応:

| 欠陥 | 消える場所 |
|---|---|
| #1 #2 #5（Undo の添字ズレ・履歴漏れ・O(n)） | Wave 0-3（`Edit` 代数） |
| #3 #4（毎フレーム O(n²)・確保） | Wave 1-A（掃引衝突検査＋revision ゲート） |
| #6 #7（y軸反転・アスペクト不一致） | Wave 1-B（`FieldMap` 一本化） |
| #8（ベジェの弧長非等分） | Wave 2-4 |
| #9 #10（非原子保存・OneDrive 固まり） | **Wave 0-7** |
| #11 #12（reserve 計算誤り・空セットのアンダーフロー） | Wave 0-3 と Wave 1-A |
| #13（FFmpeg 裸名起動・UI文言） | Wave 2-5 |
| #14 #15（.gitignore・.gitattributes） | **Wave 0-6** |
| #16（accesskit 無効） | Wave 2-3 |
| L1 L2（OFL 表示・LICENSE ファイル） | Wave 3（有料リリース前に必須） |
| L3（libx264 GPL） | 判断済み: FFmpeg を同梱しない |

---

## 5. 事業方針（所有者決定済み）

2026-08-09、プロジェクト所有者が以下を決定した。設計はこれに従う。

| # | 決定 | 設計への影響 |
|---|---|---|
| 1 | **完全オープンソース。全機能を全員に無償で開放する** | doc 53 を全面改訂。「販売」ではなく「配布と持続可能性」の文書になる |
| 2 | **有料ティアと権限機構を作らない** | ライセンス認証・機能ゲート・シート管理・海賊版対策・`drill-license` を全て設計から削除 |
| 3 | MPL-2.0（symphonia）と署名証明書は設計側で成立させる | doc 53 が手順化。署名は残る（§5.1） |
| 4 | **将来プラグインを入れられるようにする** | doc 52 が「今decideしないと手遅れになるもの」を確定。`drill-sandbox`/`drill-plugin` は破棄しない |

所有者の原文: 「基本的にオープンでいきたい」「機能をたくさん使えるなら完全オープンでいいです。
ややこしい権限とかが必要なら有料はやめます」

### 5.1 有料化しないことの帰結

**この判断は設計上も妥当である。** 完全オープンソースでは認証機構は容易に迂回でき、
守る実益に対して実装量と攻撃面が見合わない。有料化をやめたことで以下が丸ごと消える。

- ライセンス認証（鍵ファイル / オンライン / オフラインを問わず）と `drill-license` クレート
- 機能ゲート、シート数管理、端末数制限、海賊版対策
- 認証まわりが持ち込むはずだった攻撃面（doc 51 の対象から除外）

**一方、コード署名は残る。** 無償配布でも Windows SmartScreen と macOS Gatekeeper を通らなければ
利用者はインストールできない。これは「利用者に課す権限機構」ではなく
「利用者の手間を減らすための開発側の作業」であり、所有者の意図に反しない。
OSS 向けの無償・低額の署名手段は doc 53 が確定する。

**費用は残る**（証明書、Apple Developer Program、配布インフラ、保守工数）。
強制を伴わない手段（GitHub Sponsors、任意の寄付、Microsoft Store の無償枠）で賄えるかを doc 53 が扱う。
「寄付した人だけ使える機能」のような発想は不採用。

### 5.1-b `PRODUCT_QUALITY.md` の語の読み替え

同文書は「以下を満たさないビルドは**販売版**として扱わない」という形で品質ゲートを定義している。
有料販売がなくなったため、**「販売版」は「一般配布版（general release）」と読み替える**。
**品質ゲートそのものは一切緩めない。** 無償であることは品質基準を下げる理由にならない。

### 5.2 全面オープンによって難易度が下がる項目

- **MPL-2.0**: symphonia を未改変で依存する限り、ファイル単位コピーレフトの要求は容易に満たせる。
- **署名**: OSS プロジェクト向けの無償・低額の署名手段（SignPath の OSS プラン、
  Microsoft Store 経由の MSIX 署名、Sigstore による来歴証明など）が選択肢に入る。
  ただし macOS の notarization に必要な Apple Developer Program は回避手段が無い見込み。

### 5.2-b FFmpeg 同梱の再検討余地（決定を保留）

[31](31-video-export.md) と旧 [53](53-productization.md) は「libx264 が GPL なので FFmpeg を同梱しない」と結論した。
これは**閉じたソースの製品を前提にした判断**だった。全面オープン化により前提が変わったため、
再検討の余地がある。同梱できれば「動画書き出しが最初から動く」という UX 上の利得は大きい。

ただし、別実行ファイルをサブプロセスとして呼ぶ形態が GPL 上の「単なる集積」に当たるかは
配布形態の細部に依存する論点であり、**本書は結論を出さない**。実際に同梱へ踏み切る前に
一次情報および必要なら専門家の確認を取ること。当面は現行方針（同梱せず、利用者インストール品を検出）を維持する。

### 5.3 残る未検証事項

doc 53 が調査した価格・費用（Pyware の価格体系、証明書費用など）は出典付きだが、
**統合担当（本書）は未検証**である。実際に課金・契約する前に一次情報で確認すること。

---

## 6. 本書の位置づけ

- 個別設計の細部は各文書が正。
- クレート構成・段階・文書間の矛盾は本書が正。
- 既存コードの欠陥一覧は [DESIGN_GAPS.md](../../DESIGN_GAPS.md) §0 が正。
- 受け入れ基準は [PRODUCT_QUALITY.md](../../PRODUCT_QUALITY.md) が正。

実装は Codex が各設計文書の「8. 実装タスク」を単位に進める。
着手順は本書 §3 の波に従い、**Wave 0 を直列で終えてから並行に入る**こと。
