# DrillForge

Rust製・オープンソースのマーチングドリル制作環境。**ドット、経路、カウント、音楽、2D/3D、印刷、スマホ用練習ビュー**を同じ作品データにつなげる独立アプリです。Pyware製品とは別のプロジェクトであり、他社の独自ファイル形式との互換性を保証するものではありません。

> **次世代の製品設計:** [統合マスタープラン 2026 v2.1](docs/MASTER_PLAN_NEXTGEN_2026.md) · [画面/操作設計](docs/product/INTERACTION_SPEC_NEXTGEN_2026.md) · [機能バックログ90件](docs/product/FEATURE_BACKLOG_NEXTGEN_2026.md) · [動作・安全性設計](docs/product/GEOMETRY_MOTION_SPEC_NEXTGEN_2026.md) · [検証マトリクス](docs/product/QUALITY_MATRIX_NEXTGEN_2026.md) · [設計判断案](docs/adr/0003-nextgen-editing-and-time-model.md)  
> これらは**今後の設計案**であり、すべてが実装済みではありません。コード・テストによる実装根拠と、まだ必要な実機検証は [IMPLEMENTATION_EVIDENCE](docs/IMPLEMENTATION_EVIDENCE.md) で管理します。

## 起動（開発者向け）

Rust の対応ツールチェーンと、各OSで必要な描画/音声ライブラリが利用できる環境で実行してください。

```powershell
cargo run --release -p drill-app
```

**主な操作:** 演者をクリック・範囲選択し、ドラッグ、回転、反転、拡大縮小、整列、等間隔分配で隊形を編集できます。`Ctrl/Cmd+Z`でUndo、`Ctrl/Cmd+Shift+Z`でRedo、再生でセット間の移動と曲線経路を確認します。実際のキーバインドはアプリ内のショートカット表示に従ってください。

## 現行コードが備える主な機能

- **2D隊形編集:** 線、ブロック、アーク、円、螺旋、ベジェ、自由線、文字などの図形生成、選択/スナップ/変形、Undo/Redo。
- **移動と解析:** 人ごとの曲線・円弧・任意経路、移動開始/到着のゲート、ホールド、担当ドット自動割当、衝突/過大歩幅/方向変化の検証、ヒートマップなど。
- **音楽とタイムライン:** 可変BPM、音源デコード/再生、波形、クリック、範囲ループ、MIDI/MusicXMLの時間情報インポート。
- **フィールドと3D:** 屋内日本30mフロア、メートル/ヤード系、2D/3Dスタジアム、複数カメラ、物理的な視認性解析と描画のフォールバック。
- **出力:** CSV/TSV・SVG・PDF・ドリルブック/Production Sheet、2D/3D動画およびBlender用の場面書き出し機能（形式・外部実行環境には条件あり）。
- **作品の保存:** バージョン付きJSONと、参照アセットを含められる `.drillproj`、原子的保存、バックアップ、復旧経路。
- **相互運用と拡張:** CSV/TSV/TXT/XLSXなどの一般形式、隔離プラグインの基盤、Web/スマホ練習ビューとルームでの限定的な共同編集。

**「コード上の機能がある」ことと「正式な一般配布品質が証明された」ことは違います。** Windows/macOSの署名、実端末のDPI/音声/GPU、大人数でのデザイナー受入れテスト、依存関係の最新監査は別途必要です。

## アーキテクチャ

- `drill-core`: 安定した演者ID/セットID、幾何、カウント、編集コマンドと履歴。
- `drill-app`: egui UI とアプリの操作、2D/3D表示の接続。
- `drill-render`, `drill-gpu`, `drill-export`: 同じ作品データに基づく描画、PDF/SVG/動画。
- `drill-audio`: 音源・タイムライン・クリック・再生。
- `drill-project`, `drill-jobs`: 保存・検証・非同期ジョブ/キャンセル。
- `drill-interop`: 許可された一般的な公開フォーマットの入力と検証。
- `drill-mobile-viewer`, `drill-web`, `drill-sync`, `drill-relay`, `drill-presence`: 練習用モバイル/共同編集。
- `drill-plugin`, `drill-updater`: 隔離拡張・更新の基盤。

内部のデータ/時刻/セキュリティルールは [docs/design/](docs/design/)、製品の出荷条件は [PRODUCT_QUALITY.md](PRODUCT_QUALITY.md) を参照。

## 次の優先事項

1. **品質とデータ保護を先に直す:** [Issue #63](https://github.com/daichingale/MarchingDrillAppRust/issues/63) の取り下げ依存を更新し、全OSのCI、保存・復旧、実機検証を通す。
2. **日常の制作操作をさらに速く:** 選択→ドラッグ→スナップ→プレビュー→Undo、曲線/到着タイミング、印刷の正確さ。
3. **練習現場へつなぐ:** 先生の「今日の練習だけA4」→生徒のスマホ用個別動線→版の差分比較。
4. **将来の強み:** Facing、楽器/旗、3D小道具/照明、説明できる制約最適化、ユーザー承認式AI支援、衝突の連続時刻検証、より安全な共同編集。

新しいカウント別軌跡CSVは [draft PR #62](https://github.com/daichingale/MarchingDrillAppRust/pull/62) で作業中で、**mainにマージ済みではありません**。
