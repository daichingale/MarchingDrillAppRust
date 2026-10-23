# DrillForge NextGen Quality & Delivery Matrix
**Edition:** 2026-10-24, Proposal 1.0  
**Related:** [Master Plan](../MASTER_PLAN_NEXTGEN_2026.md) · [Motion model](GEOMETRY_MOTION_SPEC_NEXTGEN_2026.md) · [Interaction](INTERACTION_SPEC_NEXTGEN_2026.md)

## 0. 最重要の運用規則

1. CI緑は**内部検証を通過**しただけ。署名・GPUドライバ・オーディオ・学校PC・指導者のユーザーテストは外部ゲートで別に確認する。
2. 通過していないテストや既存 Issue は必ず「未検証/失敗」と記録。仕様文書に「実装済み」とあるだけで達成としない。
3. **同じ入力・同じ版→同じ出力**を検証し、CPU/GPUではビット一致を強制しない既存設計に従う。
4. 破壊的操作や不確実なAIは明示的なプレビュー、承認、Undoに必ず通す。
5. 新規の品質ゲートを加える際には、テストの実行コストと本番障害のリスクに見合うことを記録する。

## 1. 開発の品質ループ

\`\`\`text
ユーザーシナリオ
  ↓
構造化した要件（機能ID・保存/Undo/共有への影響）
  ↓
ユニット/プロパティ/境界・曖昧な入力のテストを先に作る
  ↓
純粋なドメイン計算→アプリ画面へ接続
  ↓
UIのJA/EN・DPI・キーボードとアクセシビリティ確認
  ↓
クロスプラットフォームCI (format + clippy + tests + dependency audit)
  ↓
校内PC・スマホ・本番機材で実機検証
  ↓
指導者が1曲で使う→事故/手戻り/操作時間を観測
  ↓
ADR・設計表・実装証拠台帳・Issueの状態を更新
\`\`\`

## 2. 必須の受入テスト組

| ID | 仕様 | 自動/実機 | 最小の確認方法 |
|---|---|---|---|
| CORE-001 | 元ファイルを保存前に失わない | 両方 | ディスクフル/アクセス拒否/強制終了/OneDriveの書換え |
| CORE-002 | IDが永続、セット追加/削除後も正しい | 自動 | 行・列の入替え、ロスター変更、Undo/Redo |
| CORE-003 | 上書き前・自動保存・復旧候補が説明される | 両方 | 非正常終了のバックアップ確認 |
| CORE-004 | 旧/将来/壊れたファイルを安全に扱う | 自動 | version migration, fuzz, schema bounds |
| TIME-001 | カウントと秒/音源が一貫 | 両方 | 可変BPM、音源の遅延、途中テンポ変更、最後のセット |
| TIME-002 | ゲート/曲線/ホールドで2Dと書出しが一致 | 自動 | count=0, depart, arrive, hold, shared set boundary |
| TIME-003 | 1/8拍演出は時刻境界で不連続/丸め落ちしない | 自動 | proposed ShowTime proof, before schema migration |
| GEO-001 | 8-to-5, 5m/8歩, m/ydを混ぜない | 自動 | 5yd=4.572m, 1 step=0.5715m |
| GEO-002 | フィールド原点/前後方向/印刷座標が一致 | 両方 | 中央、端、30m、100yd、回転/ズーム |
| GEO-003 | カウント間交差を見逃さない | 自動 | 中間で最接近する2演者テスト |
| GEO-004 | 異なる時刻の交点を誤衝突にしない | 自動 | 時間差のある交差 |
| GEO-005 | 大型楽器の設定閾値が距離単位で説明される | 自動/現場 | 低音金管/打楽器/フラッグ半径の比較 |
| UI-001 | 一操作一Undo、Escなら未変更 | 自動/実機 | 選択→ドラッグ→取り消し→Undo |
| UI-002 | 選択後の追加・削除・並替でも意図しない人を動かさない | 自動 | selected IDs stable across reordering |
| UI-003 | 言語・DPI・キーボード・タッチで操作できる | 実機 | 1366×768〜4K、100/150/200% DPI、JA/EN |
| UI-004 | 印刷/エクスポート中も操作可能 | 両方 | 4K/数百ページ生成、cancel |
| IO-001 | CSV/XLSX取り込みで列/数値/前後方向が曖昧なら停止 | 自動/実機 | 列名が無い、カンマ小数、重複ID |
| IO-002 | Import Preview / Diffの元ファイルは不変 | 自動 | 保存前に文書ハッシュが同じ |
| MOB-001 | 端末で個人のドットと演奏の位置が読める | 実機 | 縦横画面、明るい屋外、片手操作 |
| MOB-002 | ダウンロード済みの練習版はオフラインで読める | 実機 | 機内モード→開く→閉じる→再開 |
| COL-001 | 同じ点の同時編集が無言上書きされない | 自動/実機 | 双方オフラインで編集→再接続 |
| COL-002 | 複数演者の同時編集は一括/競合提示 | 自動 | 24人変形を分割配信/途中切断 |
| SEC-001 | 匿名URLから教員/生徒の個人情報や音源が漏れない | 自動/運用 | 匿名化・期限・招待取り消し |
| SEC-002 | 外部AI・プラグインが拒否されてもドリルは壊れない | 自動 | 悪意/不正JSON/タイムアウト/失効トークン |
| PERF-001 | 1000演者 60fpsを目標とした実機計測 | 両方 | CPU/GPU、p50/p95/p99 frame time |
| PERF-002 | 4,000人×240セット解析はUIをブロックしない | 自動 | runtime/peak heap/cancel/queue |
| PERF-003 | 2時間の音声再生に継続メモリ増加なし | 両方 | デバイス切断、サンプルレート、seek |
| REL-001 | Windows/macOS/Linux配布が安全に起動 | 実機 | 署名・取り消し・更新・戻す |
| REL-002 | 依存の安全性検査を緩めず緑へ | 自動 | cargo-deny/locked/version inventory |

## 3. 負荷プロファイル（具体的な測定入力）

| フィクスチャ | 演者/セット | 演出/音源 | 試す操作 |
|---|---|---|---|
| Tiny | 8人/2セット | 16 counts / straight | 新規作成→移動→Undo→印刷 |
| School | 80人/64セット | 可変BPM + 30mフロア + 欠席者 | 全体編集・プリント・個人ビュー |
| Large | 300人/128セット | 曲線/交差/イージング/動画 | 3D・タイムライン・多人数アサイン |
| Stress | 1000人/256セット | 高密度/急角度/複合図形 | GPU/CPU比較、フレームループ |
| Max analytic | 4000人/240セット | 衝突/距離/ヒートマップ | 制限値・メモリ・ジョブ終了/キャンセル |
| Corrupt/unsafe | 破損/巨大/未来スキーマ | 入力が未検証または権利不明 | 解析拒否・診断・原本保持 |

**性能上限やサンプル値は現行 [PRODUCT_QUALITY](../../PRODUCT_QUALITY.md) が正**。本ファイルは増やす試験を決めるが既存しきい値を下げない。

## 4. CI と実機のゲートを分ける

### Gate A: PRをマージする条件
- Linux/Windows/macOSで \`cargo fmt --all -- --check\`。
- \`cargo clippy --workspace --all-targets --locked -- -D warnings\`。
- \`cargo test --workspace --locked\` と既存golden/release checks、依存/ライセンス監査成功。
- Rustの危険なunsafe/全量clone/同期I/Oの追加がある場合はコスト・設計根拠。
- UI文字追加時は登録済みJA/EN catalog。過去 \`MESSAGE_CATALOG.md\` の監査内容は再生成しない。
- バグ修正時は最小の再現テスト。新機能は使用法と保存/Undo/失敗例を含む。

### Gate B: ベータ配布の条件
- Gate A + 学校PC実機で1作品を制作/再生/保存/プリント。
- 主要画面の100/150/200% DPI、スクリーンリーダー可用性、GPU故障時のフォールバック。
- 実音声デバイスの短時間/長時間試験、ブラウザ・スマホ閲覧の切断試験。
- 既存の復旧バックアップが実際の破損状況から救えるか確認。

### Gate C: 一般配布の条件
- Gate B + Windowsの正式署名/配布、macOS notarization+stapling、Linux対象配布の証跡。
- 署名付き更新とロールバック検証、セキュリティ監査/依存状態。
- 100人以上の作品でドリル制作経験者の導線試験。問題分類・復帰時間と指標を保存。
- [IMPLEMENTATION_EVIDENCE](../IMPLEMENTATION_EVIDENCE.md) の外部未証明項目を明示し、合格/非対象を根拠つきで決定。

Gate Cを達成していない場合は**マスタープランが完成していても一般配布完了ではない**。

## 5. 優先度付きのリスク一覧

| リスク | 影響 | 早期検出 | 対策 |
|---|---|---|---|
| ファイル破損・不可逆編集 | 最重要 | 保存・移行・Undoのproperty/fuzz | 原子保存、確認、編集履歴とバックアップ |
| 同期一括操作の半分だけ適用 | 最重要 | 24人変形 + 切断テスト | atomic batch、競合UI、復旧 |
| 単位/フロアの誤解 | 重大 | m/ydと30m会場のgolden | 型付き単位と基準線の確認 |
| 衝突検出の見落とし | 重大 | カウント間の交差/曲線のtest | 保守的境界/不確実さ表示 |
| 学校・生徒情報漏えい | 重大 | 匿名共有/失効/偽装ルームの攻撃試験 | 匿名化、権限分離、外部送信OFF |
| AIが勝手に編集/漏洩 | 重大 | 不正スキーマと同意拒否テスト | AIを提案だけに、承認とsandbox |
| 低スペックPCでの操作詰まり | 中/重大 | School/Stressフレームとメモリ | 非同期、低品質表示、予算を計測 |
| バイナリ未署名・依存監査赤 | 重大 | CI/製品ストア検証 | 依存更新、署名/配布前停止 |
| スコープ拡大で使えないUI | 重大 | 初回5操作の観察 | SimpleとProfessionalを分離 |

## 6. 証拠台帳フォーマット

\`\`\`text
Evidence ID:
Feature/Issue/PR:
Git commit / branch:
Test case IDs:
OS / machine / CPU / GPU / audio device / display DPI:
Time and tooling:
Expected:
Observed:
Metrics (p50/p95/p99, memory, errors):
Artifact links / screenshots / logs:
Classification: code-proved | CI-verified | hardware-verified | field-accepted
Reviewer / date:
Remaining limitations:
\`\`\`

**管理ルール:** 自動テストが実行できなければ \`not run\`、機材がなければ \`not verified\` と書く。参考値と本番計測値を混ぜない。

## 7. 直近の決定事項と改善ループ

- PR #62のカウント別軌跡CSVは**まだ main 未マージ**。出力単位、文字化け、後半hold、出力サイズ、画面のメニュー動線を最初の新規受入対象にする。
- Issue #63の取り下げ依存は**出荷阻害のS0**。\`cargo-deny\`のポリシーを無条件に弱めて通さない。
- 経路/単位/フロアの合意が取れない機能は「未確定」として隔離。無理に作って仕様が増殖しないようにする。
- 月に1回、教員・ドリルデザイナーの指摘を「再現可能なテスト + UI修正 + 設計更新」に反映する（作業ループとしての提案。自動実行を設定したという意味ではない）。
