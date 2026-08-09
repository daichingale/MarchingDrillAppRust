# DrillForge product quality bar

DrillForgeは「Pywareのコピー」ではなく、マーチング制作で必要な正確性を保ちながら、速度、発見性、安全性を改善する独立製品として開発する。

## Release gates

以下を満たさないビルドは販売版として扱わない。

### Correctness

- 全カウント、セット境界、可変BPM、部分ループで再生位置が決定論的である。
- 保存ファイルはスキーマ検証され、未対応の将来形式や破損データを黙って開かない。
- Undo/Redo後に演者数、ID、セット座標の不変条件が維持される。
- 座標表、ドリルブック、SVG/PDF出力は同じドキュメント座標を参照する。

### Performance

- 1,000人のアニメーション補間でフレーム内ヒープ再確保ゼロ。
- 1,000人、60fpsでUI入力から表示まで16.6ms以内を維持する。
- 2時間連続再生で常駐メモリの継続増加がない。
- 保存、自動保存、解析は描画スレッドを長時間停止させない。

2026-08-09の開発機ベースライン（release profile、ゲートは各bench内で自動判定）:

- 1,000人 × 60,000フレーム補間: 10.41ms（合計）、定常時allocation 0
- 1,000人の補間 + DisplayList構築: 16.6ms/フレーム未満、定常時allocation 0
- 1,000人 × 16カウント swept clinic: 89.1µs/解析（2msゲート）
- 1,000人ドキュメント × 100回JSON変換: 37.61ms（合計）
- 2時間・48kHz音声hot path: 675,000ブロック、allocation 0、PCM常駐量不変
- 1,000人ドキュメントで10,000編集 + 全Undo + 全Redo: 4.63ms（合計）

再計測は次の3コマンドで行う。壁時計に加え、計数アロケータで定常時の再確保ゼロを検証する。

- `cargo bench -p drill-core --bench core_performance`
- `cargo bench -p drill-render --bench display_list`
- `cargo bench -p drill-audio --bench audio_performance`

### Reliability

- 上書き保存前バックアップ、自動保存、クラッシュ復旧候補を提供する。
- 音声・画像・外部ファイルが欠落してもドリル本体を開ける。
- パニックはクラッシュレポートへ変換し、元ファイルを変更しない。
- 10,000回の編集コマンドを含むストレステストを通す。

### UX and accessibility

- 初回起動から「セット選択 → 演者選択 → 編集 → 再生」まで説明書なしで到達できる。
- 常用操作はツールバー、全操作とショートカットはメニューバーから発見できる。
- 再生範囲、現在カウント、セット境界、警告は色だけに依存せず文字と形でも示す。
- Windows 100–200%スケーリング、日本語・英語UI、キーボード操作を検証する。

## Delivery sequence

### P0 — trustworthy editor

- 再生・タイムライン状態機械
- 保存、バックアップ、自動保存、復旧画面
- 完全なUndo/Redoコマンド化
- 範囲選択、移動、回転、拡縮、整列
- グリッドデザイナーと座標出力

### P1 — production workflow

- 音源読込、波形、可変BPM同期、メトロノーム
- Count Sheet、Production Sheet、セット注釈
- PDFチャート、演者別ドリルブック、印刷プレビュー
- 衝突、歩幅、方向転換、到着タイミング解析

### P2 — differentiation

- GPUインスタンシングによる2D/3D統合表示
- 経路最適化と制約付きリライト候補
- セクション単位の比較、分岐、スナップショット履歴
- 公開プラグインAPIと互換インポーター

## Architecture constraints

- `drill-core`はUI、GPU、OSダイアログへ依存しない。
- 時間、座標、解析、保存検証は純粋ロジックとして単体テスト可能にする。
- `drill-app`は表示と入力の変換に限定し、業務ロジックを追加しない。
- フレームループでドキュメント全体のclone、JSON化、無制限allocationを行わない。
