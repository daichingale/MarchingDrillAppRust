# DrillForge Media Pipeline

DrillForgeの音声同期と動画出力は、ドリルのcountを唯一の編集基準にする。UI上の編集位置は整数countへ既定で吸着し、再生と書き出しだけを連続時間・sample/frame精度で評価する。これにより、滑らかな再生を維持しながら「152分の1拍」のような曖昧な停止位置を避ける。

## 調査で確認したPywareの基準

- 3DXは音源の自動同期、Visual Audio Adjuster、複数カメラ、アプリ内画面録画を案内している。
- 3D v11系では波形を見ながら同期アンカーを調整でき、カメラ移動もCount Track上で確認できる。
- Real Viewは複数カメラや高フレームレート表示に対応する。ただし公開情報から録画コーデックやビットレート等の詳細仕様は確認できない。

参考:

- https://www.pyware.com/3dx/
- https://www.pyware.com/updates/
- https://www.pyware.com/3d/
- https://www.pyware.com/3d-pricing/

## DrillForgeの設計

### 音声

- 原音を変更しないgain、mute、trim、fade設定
- countから秒への変換はTempoMapで行い、浮動小数のフレーム加算に依存しない
- 編集シークはcount吸着、再生は滑らかな連続時間
- 次段階ではデコード済みPCMのリングバッファと、ズーム段階別の波形peak cacheを導入する
- 自動BPM/ダウンビート候補、複数同期アンカー、click/count-in、入出力レイテンシ較正を追加する

### 動画

- 画面録画ではなく、countとTempoMapから各フレームを決定論的に生成するオフラインレンダリング
- Preview FPSとExport FPSを分離する
- 内部音声を直接muxし、OSのループバック録音を不要にする
- EasyプリセットとAdvanced設定を同じ構成モデルから生成する
- FFmpegは引数配列で起動し、シェル文字列を組み立てない
- 書き出し前に範囲、解像度、codec/container、空き容量、encoder対応を検査する

FFmpeg仕様の基準:

- https://ffmpeg.org/ffmpeg.html
- https://ffmpeg.org/ffmpeg-codecs.html
- https://ffmpeg.org/ffmpeg-formats.html
- https://ffmpeg.org/ffprobe.html

## 実装段階

### 現在実装済み

- 可変テンポ対応の決定論的Playback Engine
- ループ端の余剰時間保持と範囲終端停止
- 音声の非破壊gain/mute/trim/fadeモデルと調整UI
- 動画プリセット、解像度、FPS、codec、container、encoder、rate control、音声設定UI
- FFmpeg引数生成、設定検証、フレーム数と概算容量表示、FFmpeg存在確認

### P0: 最初に売り物として成立させる範囲

1. 2Dフィールドを決定論的にRGBAフレームへ描画
2. 背景workerでFFmpegへpipeし、H.264 MP4へ出力
3. 内部音声のtrim/gain/fadeを反映してmux
4. 進捗、キャンセル、失敗理由、software fallback
5. ffprobeで解像度、FPS、長さ、音声有無を出力後検証

### P1

- 波形LOD、拍・小節・セット・同期アンカーを重ねたAudio Workspace
- GPU encoder自動検出と短い事前ベンチによる推奨
- カメラショット、count単位keyframe、追従、補間、動画トラック
- 1080p/4K、横・縦・正方形プリセット、書き出しQueue

### P2

- 3D Real Viewのオフラインレンダリング
- H.265/AV1、10-bit、色管理、字幕、ロゴ、ジャンボトロン
- 中断再開、複数端末向け一括書き出し、カメラ急加速・衝突診断

## 品質ゲート

- UI threadでdecode、frame render、encodeを行わない
- 音声時刻はsample index、映像時刻は有理数frame timeから求める
- 同じproject/configから同じフレーム列を生成する
- 1000人規模のドリルでも再生中に継続的allocationを発生させない
- 書き出し後はffprobe検証に成功するまで完了扱いにしない
