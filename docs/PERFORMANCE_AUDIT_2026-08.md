# DrillForge 性能監査 2026-08

実施日: 2026-08-10。目的は「新機能追加」ではなく、既存実装の実測とボトルネック解消。
`PRODUCT_QUALITY.md` の性能ゲート（1,000人・60fps・16.6ms予算、フレーム内ヒープ確保ゼロ、
Undo 2ms未満）を基準に、`docs/IMPLEMENTATION_EVIDENCE.md`（2026-08-09自己申告）の数値を
鵜呑みにせず再実測した。

実行環境: Windows 11、`rustc 1.97.1`、`cargo 1.97.1`、16論理コア。
`cargo bench` はワンショットの `main()` バイナリ（Criterion 不使用）で、壁時計と
カウントアロケータの両方をアサートする既存方式をそのまま使用した。

## 1. 最重要の発見: `cargo bench` が `[profile.release]` を継承していなかった

`Cargo.toml` には `[profile.release]`（`lto = "thin"`, `codegen-units = 1`,
`strip = "symbols"`, `overflow-checks = true`）のみが定義されており、
`[profile.bench]` は存在しなかった。Cargo の `bench` は組み込みプロファイルだが
`release` とは別物で、既定値は `lto = false`, `codegen-units = 16`,
`overflow-checks = false` である。

つまり、これまで `cargo bench` で得られていた「release プロファイル」という
自己申告（`PRODUCT_QUALITY.md` 2026-08-09時点の基準値、`docs/IMPLEMENTATION_EVIDENCE.md`）は、
**実際に出荷される `cargo build --release` の成果物とは異なるコード生成設定で
測定されていた**。今回、この2つのプロファイルを実際にビルド・実行して比較したところ、
数値は無視できない差（後述、多くの指標で30〜65%高速化）を示した。実測に基づき、

```toml
[profile.bench]
inherits = "release"
```

を `Cargo.toml` に追加した。これはロジック変更ではなくビルド設定のみの変更であり、
`cargo build --release` で作られる出荷バイナリの挙動・依存クレート・公開APIには
一切影響しない（`cargo build --release` は元々 `[profile.release]` をそのまま使うため
無変更で同一)。既存テストへの影響もない。

以後の「修正後」列はすべてこの `[profile.bench] inherits = "release"` を反映した数値。
「修正前」列は元の `Cargo.toml`（`[profile.bench]` なし、Cargo既定値）での実測。

## 2. ベンチマーク実測値（1,000人基準規模）

| ベンチマーク | 修正前（旧bench既定プロファイル） | 修正後（release相当プロファイル） | ゲート | 判定 |
|---|---|---|---|---|
| 1,000人×60,000補間フレーム（生ドキュメント） | 7.031 s（117.2µs/frame） | 5.938 s（99.0µs/frame） | 16.6ms/frame | 合格（大幅余裕） |
| 1,000人×60,000補間フレーム（コンパイル済みカーブ） | 1.539 s | 1.015 s | — | 参考値 |
| 1,000人×100 JSON往復 | 77.93 ms | 65.00 ms | — | 参考値（累計） |
| 65,536テンポイベント×100万回引き | 146.19 ms | 95.75 ms | — | 参考値 |
| 1,000人×16カウント swept clinic（平均） | 124.85 µs | 101.58 µs | 2 ms | 合格（約20倍の余裕） |
| 1,000人×10,000編集+全Undo+全Redo | 10.903 ms | 5.116 ms | — | 参考値（累計） |
| 定常時アロケーション（補間・コンパイル済み） | 0 / 0 | 0 / 0 | 0 | 合格 |
| DisplayList構築（`build_field_2d` 単体、1,000人） | 10.078 µs/frame, alloc=0 | 6.983 µs/frame, alloc=0 | 16.6ms | 合格（大幅余裕） |
| 補間+DisplayList構築（1,000人合成） | 17.537 µs/frame, alloc=0 | 12.935 µs/frame, alloc=0 | 16.6ms | 合格 |
| GPUフレーム更新（`update_from_display_list`） | 5.372 µs/frame | 3.260 µs/frame | — | 参考値 |
| GPUスタジアム更新 | 6.562 µs/frame | 3.270 µs/frame | — | 参考値 |
| 10分ステレオPeak構築 | 151.02 ms | 49.77 ms | — | 参考値 |
| 1920pxピークレンジ取得 | 95.98 µs | 26.46 µs | — | 参考値 |
| 512フレームステレオmix | 12.58 µs | 2.87 µs | 実時間10.67ms相当 | 合格（大幅余裕） |
| 512フレームクリックmix | 532 ns | 372 ns | — | 参考値 |
| 512フレーム標準ピッチ保持 | 745.83 µs | 212.12 µs | 実時間10.67ms相当 | 合格（大幅余裕） |
| 2時間48kHz音声hot path（675,000ブロック） | 4.372 s, alloc=0 | 1.750 s, alloc=0 | alloc=0 | 合格 |
| 1,000人×100回1080p CPUラスタ書き出し | 1.9844 s（19.84ms/frame） | 1.1251 s（11.25ms/frame） | — （バッチ書き出し、60fps対象外） | 参考値。修正前は仮に対話フレーム予算と比較すると超過していたが、書き出しはオフライン処理でありPRODUCT_QUALITY.mdの対話60fpsゲート対象ではない |
| UI遅延（6ジョブ×120フレームpoll） | p95=100ns, p99=100ns, worst=9.1µs | p95=100ns, p99=200ns, worst=2.4µs | p95≤2ms, p99≤4ms, worst≤8ms | 合格 |
| 最大保存latency（4,000人×240セット、3,500アセット、61.8MB JSON） | p95=100ns, p99=100ns, worst=18.6µs, peak_heap=209,281,731B | p95=100ns, p99=100ns, worst=3.0µs, peak_heap=209,281,731B | p95≤2ms, p99≤4ms, worst≤8ms, heap≤768MiB | 合格 |

補足: 「修正前」列は `cargo bench` の既定 `bench` プロファイル（`overflow-checks=false`,
`lto=false`, `codegen-units=16`）。数値がすべて「修正後」より遅いのは、
`lto="thin"` + `codegen-units=1` の最適化効果が `overflow-checks=true` の追加コストを
上回っているため。つまり実際に出荷される release バイナリは、これまで文書化されていた
基準値より全体的に高速である。

## 3. 4,000人規模ストレステスト（上限規模、`docs/design/00-conventions.md` の演者4,000/セット256）

既存の `crates/drill-jobs/benches/max_project_save_latency.rs` は既に上限規模
（`Document::demo(40, 100)` を240セットへ拡張 = 4,000人×240セット、61.8MB JSON、
3,500アセット）で保存/自動保存レイテンシとヒープ増加を検証しており、上表の通り合格。

補間とclinic走査については1,000人規模の既存ベンチしかなかったため、一時的な
`cargo run --release --example` ハーネスを `crates/drill-core/examples/stress_4000_tmp.rs`
として作成し実行後、削除した（恒久的なベンチとしては追加しないという判断。理由は
下記「今後の改善余地」参照）。

| 項目 | 実測値 | 備考 |
|---|---|---|
| 4,000人×60,000補間フレーム | 1.523 s（25.4µs/frame） | 16.6msに対し十分な余裕（約650倍） |
| 4,000人×16カウントswept clinic（`Document::demo`直接、フィールド外に配置されたデータ） | 6.036 ms | **要注意（下記参照）**: フィールド境界外の人為的データによる見かけ上の劣化 |
| 4,000人×16カウントswept clinic（フィールド内に密に再配置） | 179.9 µs | 実態としてはこちらが正しい参考値 |
| 4,000人×10,000フレーム 補間+DisplayList構築 | 580.0 ms（58.0µs/frame） | 16.6msに対し十分な余裕（約280倍） |

### 3.1 重要な訂正: 最初に見えた「4,000人でclinicが6ms」は空間ハッシュの欠陥ではない

`Document::demo(rows, columns)` は演者位置を `x = 15 + (i % columns) * 5` のように
配置する。`demo(40, 100)`（4,000人）ではこの式が既定フィールド（100ヤード×53.333ヤード、
`GridConfig::default()`）を大きく超え、x座標が最大510ヤードまで達する。
`clinic.rs::cell_of()` はセル座標を `clamp(0, cols - 1)` でフィールド内に丸めるため、
本来ならフィールド全体に分散するはずの演者の大半が、境界セルへ人為的に密集してしまう。
これは実運用ではあり得ない入力であり、空間ハッシュの実装上の問題ではなく、
`Document::demo` のフィクスチャがそもそも4,000人規模を想定していないことに起因する
測定アーティファクトである。

演者をフィールド内（80×50グリッド、100ヤード×53.333ヤードに収まる間隔）に
現実的な密度で再配置して再測定したところ、179.9 µs（1,000人時の101.58µsの
約1.8倍。演者4倍に対して1.8倍で、`begin_grid`/`insert_grid`のセル数がフィールド
サイズ一定のまま増えるため準線形の伸びに収まっている）となり、2msゲート
（1,000人向けの数値ではあるが）に対して圧倒的な余裕がある。

**結論**: `crates/drill-core/src/clinic.rs` の空間ハッシュ実装は4,000人規模でも
実用上問題ない。過去に存在したというO(n²)衝突走査バグは、現在のスクラッチ再利用+
空間ハッシュ実装で解消されたままである。コード変更は不要と判断した。

## 4. その他の調査（コード変更なし、実測で問題なしと確認）

- **`crates/drill-render/src/lib.rs::build_field_2d`**: 1,000人・4,000人いずれも
  16.6ms予算に対して桁違いの余裕（1,000人で7〜17µs、4,000人で58µs）。`FieldMap`
  導入後の退行は見られない。`_scratch: &mut BuildScratch`（ゼロサイズ型）を含め、
  ホットパスは既存の `DisplayList::clear()` 方式でアロケーションゼロを維持している
  （ベンチのアロケーションカウンタで確認、`allocations=0`）。
- **`crates/drill-app/src/*.rs` の `.clone()`**: `grep -rn "\.clone()" crates/drill-app/src/*.rs`
  で108件確認。最も多い `app_state.rs`（20件）・`app_ui.rs`（13件）を確認したところ、
  `self.document.clone()` 等の重い clone はいずれもユーザー操作（メニュー選択、
  グリッド変更の適用、プレビュー生成、Undo/Redo用スナップショット等）に紐づく
  一回限りの経路であり、`eframe::App::ui()` の毎フレーム無条件実行パスには
  含まれていない。毎フレーム実行される `ui()` 冒頭（`app_ui.rs` 1〜75行目付近）は
  ジョブのpoll、ビジュアル設定の代入など軽量な処理のみで、明確なホットパスの
  無駄なclone/allocは見つからなかった。実測で裏付けられた具体的な問題がない限り
  変更しないという方針に従い、コード変更は行っていない。
- **`crates/drill-audio` のリアルタイム出力コールバック**: `audio_performance.rs`
  の2時間soakベンチで、カウントアロケータにより `allocations=0` を継続実測。
  ソースコード上もコールバック内で `Vec::new()` 等の新規確保は見当たらず、
  既存の不変条件（確保・ロック・panicなし）は維持されていると判断した。
- **アプリ起動時間 / フォント埋め込み**: `crates/drill-app/src/bootstrap.rs` で
  `include_bytes!("../../../assets/NotoSansJP.ttf")`（9.6MB）をコンパイル時に
  埋め込み、`egui::FontData::from_static` でゼロコピー登録している。egui/ab_glyph
  のグリフラスタライズは遅延評価（実際に描画された文字だけが処理される）であり、
  `set_fonts` 自体はテーブルオフセットの読み込みのみで重いフルパースは発生しない。
  本監査ではGUIプロセスをこのヘッドレス作業環境で対話的に起動して体感計測することが
  できなかったため（ディスプレイ/対話セッションが前提のGUIアプリを安全に検証する
  手段がなかった）、起動時間について新たな実測は追加していない。静的読解の範囲では
  明確な無駄は見つからなかった。プロファイラでの実測は今後の改善余地とする。

## 5. 実施した最適化のまとめ

| 変更 | ファイル | 種別 | 効果 |
|---|---|---|---|
| `cargo bench` が `release` プロファイル（`lto=thin`, `codegen-units=1`, `overflow-checks=true`）を継承するよう `[profile.bench] inherits = "release"` を追加 | `Cargo.toml` | ビルド設定のみ（ロジック変更なし） | 上記2節の通り、ほぼ全ベンチマークで30〜65%の実行時間短縮。**出荷バイナリ自体は無変更**（`cargo build --release` は元々 `[profile.release]` を使用済み）。今後の `cargo bench` 実測が実際の出荷構成を正しく反映するようになった |

コード（`.rs`）の変更は行っていない。実測の結果、`clinic.rs` の空間ハッシュ、
`build_field_2d`、audioホットパス、jobsレイテンシのいずれも
`PRODUCT_QUALITY.md` のゲートに対して大きな余裕があり、推測に基づく最適化を
加えるべき根拠が見つからなかったため。

## 6. 見つかったが今回は直さなかった問題

- **1,000人×100回1080p CPUラスタ書き出しが旧プロファイルで19.84ms/frameだった件**:
  `PRODUCT_QUALITY.md` の16.6ms/60fpsゲートは「UI入力から表示まで」の対話フレームに
  対するものであり、`drill-export::RasterSurface` によるオフライン書き出しは
  この対話ゲートの対象ではない（ドキュメント上も明記なし）。プロファイル修正後は
  11.25ms/frameまで下がったため実害はないが、書き出しが本当にリアルタイム制約から
  除外されていることを明文化する価値はある。将来的に `PRODUCT_QUALITY.md` の
  書き出しパスの扱いを明記することを提案する（本タスクの範囲外のドキュメント整備
  のため今回は変更していない）。
- **4,000人規模の補間・clinicには恒久的なベンチが無い**: 現状は `max_project_save_latency.rs`
  が保存/自動保存latencyのみ上限規模をカバーしており、CPU補間・DisplayList構築・
  clinic走査は1,000人規模のベンチしか常設されていない。今回一時的なハーネスで
  余裕を確認したが、恒久的な回帰検知としては、`core_performance.rs` /
  `display_list.rs` に4,000人版のセクションを追加する価値がある。今回は
  「実測のみで裏付けられた最小限の変更」という方針のため、ベンチ本体への追加は
  見送った（コード変更ではなく計測資産の拡張であり、実装ロジックへの影響はないため
  低リスクだが、既存ベンチのフォーマット・出力契約を変えることになるため、
  オーナーの判断を仰ぐべきと考えた）。
- **`app_state.rs`/`app_ui.rs` の108件の `.clone()` の全数監査は行っていない**:
  最も大きい2ファイルの主要な呼び出し箇所は確認したが、残り15ファイルすべての
  clone一つひとつをホットパス判定するには至っていない。実行時プロファイラ
  （例: Windows Performance Recorder / `cargo flamegraph`）による対話操作中の
  実測が、次に行うべき最も価値のある追加調査だと考える。
- **`overflow-checks = true` を release で有効にしていること自体**: これは意図的な
  安全性設計（パニックへの変換によるクラッシュレポート化、`PRODUCT_QUALITY.md`の
  信頼性ゲートと整合）であり、本監査のスコープ外として変更していない。ただし
  今回の実測で分かる通り、無効化すれば理論上さらに速くなる可能性はある
  （安全性とのトレードオフであり、オーナー判断が必要）。

## 7. 退行確認 (`cargo test --workspace`)

`cargo test --workspace`（dev）と `cargo test --workspace --release`
の両方を実行した。結果はどちらも同一で、`drill-conformance` の
`golden_approval::approved_product_goldens_are_current` 1件のみが失敗し、
他の全クレート（`drill-core`, `drill-app`, `drill-audio`, `drill-jobs`,
`drill-render`, `drill-gpu`, `drill-project`, `drill-export`,
`drill-conformance` の他テスト群含む）はすべて成功した。

この失敗の内容は、golden fixture（`production.tsv`, `production-pdf.digest`,
`render-rgba.digest`, `message-catalog.digest`）が改行コード `\r\n` で
コミットされているのに対し、このワークツリーでコード側が生成する出力は
`\n` になっているという、改行コードのみの不一致であり、性能や本監査の
変更とは無関係である。`git stash` で `Cargo.toml` の変更を一時的に外して
同じテストを再実行し、**変更前から同一の失敗が再現する**ことを確認した
（本監査の変更が原因ではないことの直接証拠）。この作業ツリー（`git worktree`）
固有の改行コード正規化設定に起因すると考えられ、`docs/PERFORMANCE_AUDIT_2026-08.md`
のスコープ外（性能ではなく改行コード/フィクスチャ管理の問題）のため
修正していない。オーナーへの報告事項として明記する。

## 8. 再現手順

```powershell
cargo bench -p drill-core --bench core_performance
cargo bench -p drill-render --bench display_list
cargo bench -p drill-gpu --bench frame_prepare
cargo bench -p drill-audio --bench audio_performance
cargo bench -p drill-export --bench raster_performance
cargo bench -p drill-jobs --bench ui_latency
cargo bench -p drill-jobs --bench max_project_save_latency
```

`Cargo.toml` に `[profile.bench] inherits = "release"` がある状態で実行すれば、
出荷 (`cargo build --release`) と同一のコード生成設定で測定される。
