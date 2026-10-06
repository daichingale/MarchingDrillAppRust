# DrillForge message catalog inventory

Generated from Rust sources. Do not edit manually. `Text` enum entries are
validated exhaustively in `crates/drill-app/src/i18n.rs`; this inventory covers
the staged literal-pair API and direct locale conditionals.

| ID | Mechanism | Source | Japanese | English |
|---|---|---|---|---|
| legacy.0001 | conditional | `app_state.rs:1599` |  · プレビューは先頭のみ |  · preview truncated |
| legacy.0002 | tr | `app_ui.rs:912` | －カット | − Cut |
| legacy.0003 | tr | `app_ui.rs:859` | －キーフレーム | − Keyframe |
| legacy.0004 | tr | `inspector_media.rs:308` | ! 解決が必要です | ! Action Required |
| legacy.0005 | tr | `onboarding.rs:176` | .drillprojを開く… | Open .drillproj… |
| legacy.0006 | tr | `app_state.rs:1262` | （上限到達） |  (iteration limit) |
| legacy.0007 | tr | `workspace_inspector.rs:82` | ＋ セットを複製 | + Duplicate Set |
| legacy.0008 | tr | `workspace_inspector.rs:639` | ＋ ハッシュ/区切り線 | + Hash / Divider Line |
| legacy.0009 | tr | `workspace_inspector.rs:744` | ＋ 現在位置にテンポ変化を追加 | + Add Tempo Change Here |
| legacy.0010 | conditional | `section_manager.rs:90` | ＋ 追加 | + Add |
| legacy.0011 | tr | `app_ui.rs:923` | ＋カット | + Cut |
| legacy.0012 | tr | `app_ui.rs:803` | ＋キーフレーム | + Keyframe |
| legacy.0013 | tr | `inspector_media.rs:306` | ✓ 事前検査OK | ✓ Preflight Passed |
| legacy.0014 | tr | `app_ui.rs:950` | 1  演者をクリック | 1  Click a performer |
| legacy.0015 | tr | `onboarding.rs:206` | ① 左のセットを選択  ② フィールド上の演者を選択  ③ ドラッグで移動  ④ 再生で確認 | 1. Choose a set  2. Select performers on the field  3. Drag to move  4. Play to review |
| legacy.0016 | tr | `workspace_inspector.rs:68` | 1. セットを選ぶ | 1. Choose a Set |
| legacy.0017 | tr | `onboarding.rs:137` | 1. セットを選ぶ  →  2. 演者を選んで動かす  →  3. 再生する | 1. Choose a set  →  2. Select and move performers  →  3. Play |
| legacy.0018 | conditional | `app_state.rs:444` | 1. 内容 | 1. Content |
| legacy.0019 | tr | `app_ui.rs:959` | 2  Ctrl/Cmdで複数選択 | 2  Ctrl/Cmd to multi-select |
| legacy.0020 | tr | `workspace_inspector.rs:129` | 2. 演者を選ぶ | 2. Select Performers |
| legacy.0021 | conditional | `app_state.rs:496` | 2. 用紙 | 2. Page |
| legacy.0022 | tr | `workspace_inspector.rs:361` | 2方向対称 | 2-fold symmetry |
| legacy.0023 | tr | `app_state.rs:1203` | 2名以上を選択してください | Select at least two performers |
| legacy.0024 | tr | `app_ui.rs:968` | 3  ドラッグまたは配置ツール | 3  Drag or use formation tools |
| legacy.0025 | tr | `workspace_inspector.rs:364` | 4方向対称 | 4-fold symmetry |
| legacy.0026 | tr | `workspace_inspector.rs:531` | 5. グリッドデザイナー | 5. Grid Designer |
| legacy.0027 | tr | `workspace_inspector.rs:692` | 6. テンポマップ | 6. Tempo Map |
| legacy.0028 | tr | `inspector_media.rs:25` | 7. 書き出し / 最適化 | 7. Export / Optimize |
| legacy.0029 | tr | `inspector_media.rs:548` | 8. 音源 / カウント | 8. Audio / Count |
| legacy.0030 | conditional | `app_state.rs:1592` | CSV / TSV / UTF-8テキスト / Excel XLSXに対応。独自形式は意図的に読みません。 | Supports CSV, TSV, UTF-8 text and Excel XLSX. Proprietary formats are intentionally not parsed. |
| legacy.0031 | tr | `app_ui.rs:698` | Ctrl/Cmd+S · バックアップも自動作成します | Ctrl/Cmd+S · also creates a backup automatically |
| legacy.0032 | tr | `onboarding.rs:219` | Ctrl/Cmd+S: 保存　.drillproj: 音源もまとめられる推奨形式 | Ctrl/Cmd+S: save   .drillproj: recommended format that can bundle audio |
| legacy.0033 | conditional | `app_state.rs:256` | DrillForgeの更新 | DrillForge Update |
| legacy.0034 | tr | `onboarding.rs:121` | DrillForgeへようこそ | Welcome to DrillForge |
| legacy.0035 | conditional | `legal_notices.rs:18` | DrillForge本体と同梱コンポーネントのライセンスです。 | Licenses for DrillForge and bundled components. |
| legacy.0036 | conditional | `app_state.rs:1597` | Excelシート | Excel sheet |
| legacy.0037 | tr | `app_state.rs:1713` | Excelシート選択エラー | Excel sheet selection error |
| legacy.0038 | tr | `inspector_media.rs:275` | FFmpegとエンコーダーを確認中… | Checking FFmpeg and encoders… |
| legacy.0039 | conditional | `plugin_state.rs:79` | hash・署名済み発行元・全要求能力の承認が必要です | Requires matching hash, trusted signed publisher, and all requested capabilities |
| legacy.0040 | tr | `onboarding.rs:163` | JSONを開く… | Open JSON… |
| legacy.0041 | conditional | `plugin_state.rs:52` | Manifestと実行ファイルを追加… | Add manifest and executable… |
| legacy.0042 | tr | `app_ui.rs:331` | MusicXML / MIDIテンポをインポート… | Import MusicXML / MIDI Tempo… |
| legacy.0043 | conditional | `app_state.rs:511` | PDFの保存先を選ぶ… | Choose PDF Destination… |
| legacy.0044 | tr | `onboarding.rs:215` | Space: 再生/一時停止　I: 範囲開始　O: 範囲終了 | Space: play/pause   I: range start   O: range end |
| legacy.0045 | tr | `inspector_media.rs:492` | SVG下敷きを解決できません | Could not resolve SVG underlay |
| legacy.0046 | tr | `onboarding.rs:221` | Tab / Shift+Tab: 操作項目間を移動　Enter / Space: 実行　Esc: 閉じる | Tab / Shift+Tab: move focus   Enter / Space: activate   Esc: close |
| legacy.0047 | tr | `inspector_media.rs:249` | Web再生を高速化 (faststart) | Optimize for Web Playback (faststart) |
| legacy.0048 | tr | `onboarding.rs:190` | あとから「ヘルプ > はじめかた」でいつでも再表示できます。 | You can show this again later from Help > Getting Started. |
| legacy.0049 | tr | `inspector_media.rs:61` | あとで細かく調整できます。迷ったら「共有用」を選んでください。 | Fine-tune settings later. Choose Share if unsure. |
| legacy.0050 | tr | `inspector_media.rs:633` | アクセント音 | Accent Tone |
| legacy.0051 | conditional | `plugin_state.rs:57` | インストール済み | Installed |
| legacy.0052 | tr | `app_ui.rs:174` | インポートエラー | Import error |
| legacy.0053 | tr | `inspector_media.rs:198` | エンコーダー | Encoder |
| legacy.0054 | tr | `app_ui.rs:798` | エンドゾーン | End Zone |
| legacy.0055 | tr | `inspector_media.rs:604` | カウントイン | Count-in |
| legacy.0056 | conditional | `app_state.rs:448` | カウントシート | Count sheet |
| legacy.0057 | tr | `app_ui.rs:1161` | カウントトラック | Count Track |
| legacy.0058 | tr | `workspace_inspector.rs:703` | カウント位置ごとにBPMを変化させられます | Change BPM at any count position |
| legacy.0059 | conditional | `app_state.rs:469` | カスタム | Custom |
| legacy.0060 | tr | `app_state.rs:1517` | キャンセル | Cancel |
| legacy.0061 | tr | `app_state.rs:1519` | キャンセル | Cancel |
| legacy.0062 | conditional | `app_state.rs:1627` | キャンセル | Cancel |
| legacy.0063 | conditional | `app_state.rs:1696` | キャンセル | Cancel |
| legacy.0064 | tr | `inspector_media.rs:381` | キャンセル | Cancel |
| legacy.0065 | conditional | `subset_snapshot_state.rs:298` | キャンセル | Cancel |
| legacy.0066 | tr | `inspector_media.rs:596` | クリック / カウントイン | Click / Count-in |
| legacy.0067 | tr | `onboarding.rs:211` | クリック: 1人選択　Shift / Ctrl/Cmd+クリック: 追加または解除 | Click: select one   Shift or Ctrl/Cmd-click: add or remove |
| legacy.0068 | tr | `workspace_inspector.rs:130` | クリック／空白から囲む／Ctrl・Cmdで追加選択 | Click, drag a marquee from empty space, or Ctrl/Cmd-click to add. |
| legacy.0069 | tr | `inspector_media.rs:601` | クリックを再生 | Play Click |
| legacy.0070 | tr | `inspector_media.rs:648` | クリック音は再生出力へ反映されます。設定変更時だけ再生成します。 | The click is mixed into playback and rebuilt only when settings change. |
| legacy.0071 | tr | `inspector_media.rs:625` | クリック音量 | Click Volume |
| legacy.0072 | tr | `app_state.rs:1893` | グリッドデザイナー | Grid Designer |
| legacy.0073 | tr | `workspace_inspector.rs:608` | グリッドへスナップ | Snap to Grid |
| legacy.0074 | conditional | `app_state.rs:305` | このバージョンをスキップ | Skip this version |
| legacy.0075 | conditional | `plugin_state.rs:66` | この署名済み発行元を明示的に信頼する | Explicitly trust this signed publisher |
| legacy.0076 | tr | `inspector_media.rs:179` | コンテナ | Container |
| legacy.0077 | conditional | `app_state.rs:493` | コンパクト | Compact |
| legacy.0078 | conditional | `app_ui.rs:596` | サブセット・スナップショット… | Subsets & Snapshots… |
| legacy.0079 | conditional | `app_state.rs:583` | サブセットIDを採番できません | Could not allocate a subset ID |
| legacy.0080 | conditional | `app_state.rs:614` | サブセットのメンバーを変更できません | Could not update subset members |
| legacy.0081 | conditional | `app_state.rs:596` | サブセットを作成できません | Could not create the subset |
| legacy.0082 | conditional | `app_state.rs:648` | サブセットを削除できません | Could not delete the subset |
| legacy.0083 | conditional | `app_state.rs:606` | サブセット名を変更できません | Could not rename the subset |
| legacy.0084 | tr | `onboarding.rs:151` | サンプルで始める | Start with Sample |
| legacy.0085 | tr | `workspace_inspector.rs:142` | サンプルを作成できません | Could not create sample |
| legacy.0086 | tr | `workspace_inspector.rs:139` | サンプル隊形を作成 | Create Sample Formation |
| legacy.0087 | tr | `workspace_inspector.rs:150` | サンプル隊形を作成しました | Sample formation created |
| legacy.0088 | tr | `app_ui.rs:784` | ショット追従 | Follow Shot |
| legacy.0089 | tr | `workspace_inspector.rs:138` | すぐ試せる80人のサンプル隊形を作成できます。 | Create an 80-performer sample formation to try the editor. |
| legacy.0090 | conditional | `section_manager.rs:50` | すべての変更は Undo / Redo できます。 | Every change supports Undo / Redo. |
| legacy.0091 | conditional | `app_state.rs:1657` | すべて解除 | Clear all |
| legacy.0092 | conditional | `app_state.rs:1654` | すべて選択 | Select all |
| legacy.0093 | tr | `workspace_inspector.rs:607` | ステップグリッド表示 | Show Step Grid |
| legacy.0094 | conditional | `subset_snapshot_state.rs:262` | スナップショットを復元 | Restore snapshot |
| legacy.0095 | conditional | `app_state.rs:672` | スナップショットを復元できません | Could not restore the snapshot |
| legacy.0096 | conditional | `app_state.rs:865` | セクションを削除できません | Could not remove the section |
| legacy.0097 | conditional | `app_state.rs:829` | セクションを追加できません | Could not add the section |
| legacy.0098 | tr | `workspace_inspector.rs:368` | セクションを連続配置 | Keep sections contiguous |
| legacy.0099 | tr | `app_state.rs:1324` | セクション割当に失敗しました | Section assignment was incomplete |
| legacy.0100 | conditional | `section_manager.rs:45` | セクション管理 | Section Manager |
| legacy.0101 | conditional | `app_ui.rs:509` | セット | Set |
| legacy.0102 | conditional | `app_state.rs:1636` | セット | Sets |
| legacy.0103 | conditional | `app_state.rs:1661` | セット単位（選択するとそのセットの全変更を適用） | By set (selecting applies every change in that set) |
| legacy.0104 | conditional | `app_state.rs:445` | セット別フィールド図 | Set charts |
| legacy.0105 | conditional | `workspace_inspector.rs:90` | タイトル | Title |
| legacy.0106 | tr | `onboarding.rs:216` | タイムラインをクリック/ドラッグ: 1カウント単位でシーク | Click or drag the timeline: seek in exact whole counts |
| legacy.0107 | tr | `app_ui.rs:1182` | タイムラインを拡大 | Zoom timeline in |
| legacy.0108 | tr | `app_ui.rs:1167` | タイムラインを縮小 | Zoom timeline out |
| legacy.0109 | tr | `inspector_media.rs:849` | テキスト書き出し中 | Exporting Text |
| legacy.0110 | tr | `app_state.rs:1516` | テンポ・記号を適用 | Apply Tempo and Marks |
| legacy.0111 | tr | `app_state.rs:1894` | テンポマップ | Tempo Map |
| legacy.0112 | conditional | `workspace_inspector.rs:97` | テンポを指定 | Override tempo |
| legacy.0113 | tr | `app_state.rs:1565` | テンポ適用エラー | Tempo import error |
| legacy.0114 | tr | `app_state.rs:1508` | テンポ変更 | Tempo changes |
| legacy.0115 | conditional | `workspace_inspector.rs:103` | トランジション尺を指定 | Override transition duration |
| legacy.0116 | tr | `inspector_media.rs:35` | ドリルブックやコーチ用データを出力します | Export drill books and coaching data. |
| legacy.0117 | tr | `onboarding.rs:202` | はじめかた・操作ガイド | Getting Started & Controls |
| legacy.0118 | conditional | `section_manager.rs:49` | パートの色と略称を管理し、選択中の演者をまとめて割り当てます。 | Manage section colors and abbreviations, and assign selected performers in one step. |
| legacy.0119 | tr | `workspace_inspector.rs:611` | ハッシュ位置 | Hash Positions |
| legacy.0120 | tr | `inspector_media.rs:232` | ビットレート | Bitrate |
| legacy.0121 | tr | `workspace_inspector.rs:371` | フィールド境界・演者間隔・前セットからの最大歩幅・保存中の図形を同時に満たす提案。1回のUndoで戻せます。 | Proposes field bounds, performer spacing, maximum step and stored-shape adherence together. One Undo restores it. |
| legacy.0122 | tr | `app_state.rs:1021` | フィールド上をドラッグして自由曲線を描いてください | Drag on the field to draw a free path |
| legacy.0123 | tr | `workspace_inspector.rs:557` | フィールド寸法 | Field Dimensions |
| legacy.0124 | conditional | `app_state.rs:465` | フィールド設定 | Field setting |
| legacy.0125 | tr | `workspace_inspector.rs:279` | フォーメーションデザイナー | Formation Designer |
| legacy.0126 | conditional | `plugin_state.rs:51` | プラグインは必ず隔離プロセスで実行されます。DLLをアプリ内へ読み込むことはありません。 | Plugins always run in disposable isolated processes. DLLs are never loaded in-process. |
| legacy.0127 | conditional | `plugin_state.rs:62` | プラグインを選択してください | Select a plugin |
| legacy.0128 | conditional | `plugin_state.rs:49` | プラグイン管理 | Plugin Manager |
| legacy.0129 | conditional | `app_ui.rs:585` | プラグイン管理… | Plugin Manager… |
| legacy.0130 | tr | `workspace_inspector.rs:540` | プリセット | Preset |
| legacy.0131 | tr | `app_ui.rs:790` | プレス | Press Box |
| legacy.0132 | tr | `app_state.rs:1511` | プレビューです。音符・歌詞・パート・演者は取り込みません。適用はUndoできます。 | Preview only. Notes, lyrics, parts and performers are not imported. Applying supports Undo. |
| legacy.0133 | tr | `workspace_inspector.rs:297` | プレビューを適用 | Apply preview |
| legacy.0134 | tr | `app_state.rs:1103` | プレビューを適用できません | Cannot apply preview |
| legacy.0135 | tr | `app_state.rs:1013` | プレビューを破棄しました | Preview discarded |
| legacy.0136 | conditional | `app_state.rs:449` | プロダクションシート | Production sheet |
| legacy.0137 | conditional | `inspector_media.rs:522` | プロダクションシート (TSV) | Production sheet (TSV) |
| legacy.0138 | conditional | `workspace_inspector.rs:85` | プロダクション情報 | Production notes |
| legacy.0139 | conditional | `workspace_inspector.rs:111` | プロダクション情報を更新できません | Could not update production notes |
| legacy.0140 | conditional | `app_ui.rs:636` | ベータ版も確認 | Include beta releases |
| legacy.0141 | tr | `onboarding.rs:134` | まずは3ステップ | Start in three steps |
| legacy.0142 | tr | `inspector_media.rs:719` | ミュート | Mute |
| legacy.0143 | conditional | `legal_notices.rs:9` | ライセンス・第三者通知 | Licenses & Third-party Notices |
| legacy.0144 | conditional | `app_state.rs:494` | リハーサル | Rehearsal |
| legacy.0145 | conditional | `workspace_inspector.rs:92` | リハーサルマーク | Rehearsal mark |
| legacy.0146 | tr | `app_state.rs:1510` | リハーサル記号 | Rehearsal marks |
| legacy.0147 | tr | `app_ui.rs:1132` | ループ | Loop |
| legacy.0148 | tr | `inspector_media.rs:343` | レンダリング・エンコード中 | Rendering and Encoding |
| legacy.0149 | conditional | `app_ui.rs:576` | ワークスペース | Workspace |
| legacy.0150 | conditional | `app_state.rs:283` | 安全のため公式ダウンロードURLをコピーします | Copies the official download URL for safety |
| legacy.0151 | tr | `app_ui.rs:1493` | 移動できません | Could not move performers |
| legacy.0152 | tr | `app_state.rs:1449` | 移動距離を最小化するよう次セットを再割り当てしました | Reassigned the next set to minimize travel |
| legacy.0153 | conditional | `app_state.rs:438` | 印刷・PDFワークスペース | Print & PDF Workspace |
| legacy.0154 | conditional | `app_state.rs:515` | 印刷プレビュー | Print preview |
| legacy.0155 | conditional | `app_state.rs:441` | 右側のプレビューを確認し、PDFとして保存できます。フィールド図は編集画面と同じ座標・描画データを使用します。 | Review the preview and save as PDF. Field charts use the same coordinates and drawing data as the editor. |
| legacy.0156 | tr | `inspector_media.rs:190` | 映像コーデック | Video Codec |
| legacy.0157 | tr | `workspace_inspector.rs:341` | 円フィット | Fit circle |
| legacy.0158 | conditional | `app_state.rs:1635` | 演者 | Performers |
| legacy.0159 | tr | `workspace_inspector.rs:137` | 演者がまだいません | No Performers Yet |
| legacy.0160 | conditional | `app_state.rs:1640` | 演者セクション | Performer sections |
| legacy.0161 | tr | `app_ui.rs:775` | 演者を1回のインスタンス描画で表示。無効時・デバイス異常時はCPU描画へ戻ります。 | Draw performers in one instanced pass; falls back to CPU if disabled or unavailable. |
| legacy.0162 | conditional | `app_state.rs:855` | 演者を割り当てできません | Could not assign the performers |
| legacy.0163 | conditional | `app_state.rs:446` | 演者別ドリルブック | Performer drill book |
| legacy.0164 | conditional | `app_state.rs:504` | 横 | Landscape |
| legacy.0165 | tr | `app_ui.rs:396` | 横倍率 | Scale X |
| legacy.0166 | conditional | `app_state.rs:454` | 屋内・簡潔 | Indoor terse |
| legacy.0167 | tr | `app_state.rs:1555` | 音楽タイムラインのテンポと記号を適用しました | Applied musical timeline tempo and marks |
| legacy.0168 | tr | `app_state.rs:1503` | 音楽タイムラインをインポート | Import Musical Timeline |
| legacy.0169 | tr | `app_state.rs:1896` | 音源 / カウント | Audio / Count |
| legacy.0170 | tr | `inspector_media.rs:636` | 音源ダッキング | Audio Ducking |
| legacy.0171 | tr | `inspector_media.rs:794` | 音源を外す | Remove Audio |
| legacy.0172 | tr | `onboarding.rs:177` | 音源を含められる推奨プロジェクト形式を開きます | Open the recommended project format, which can include audio |
| legacy.0173 | tr | `app_state.rs:1898` | 音源解析をキャンセル | Cancel Audio Analysis |
| legacy.0174 | tr | `inspector_media.rs:571` | 音源解析をキャンセル | Cancel Audio Analysis |
| legacy.0175 | tr | `inspector_media.rs:838` | 音声 | Audio |
| legacy.0176 | tr | `inspector_media.rs:120` | 音声を含める | Include Audio |
| legacy.0177 | tr | `inspector_media.rs:715` | 音量 | Volume |
| legacy.0178 | tr | `app_ui.rs:420` | 下敷きを削除 | Remove underlay |
| legacy.0179 | tr | `app_ui.rs:366` | 下敷きを表示 | Show underlay |
| legacy.0180 | tr | `app_ui.rs:372` | 下敷き濃度 | Underlay opacity |
| legacy.0181 | tr | `app_ui.rs:391` | 下敷き配置 | Underlay placement |
| legacy.0182 | tr | `app_ui.rs:441` | 下敷き変更を元に戻しました | Undid underlay change |
| legacy.0183 | tr | `app_ui.rs:30` | 画像下敷きエラー | Image underlay error |
| legacy.0184 | tr | `app_ui.rs:133` | 画像下敷きが見つかりません。下敷きなしで開きました | Image underlay is missing; opened without it |
| legacy.0185 | tr | `app_ui.rs:11` | 画像下敷きを読み込みました | Loaded image underlay |
| legacy.0186 | tr | `app_ui.rs:347` | 画像下敷きを読み込む… | Load Image Underlay… |
| legacy.0187 | tr | `app_ui.rs:399` | 回転 | Rotation |
| legacy.0188 | conditional | `workspace_inspector.rs:454` | 改善案を生成 | Generate fixes |
| legacy.0189 | conditional | `workspace_inspector.rs:475` | 改善案を生成しました。比較してから適用してください | Fixes generated. Review before applying |
| legacy.0190 | conditional | `workspace_inspector.rs:519` | 改善案を適用しました（元に戻す可） | Fix applied (undo available) |
| legacy.0191 | conditional | `workspace_inspector.rs:518` | 改善案を適用できません | Could not apply fix |
| legacy.0192 | tr | `inspector_media.rs:695` | 開始オフセット | Start Offset |
| legacy.0193 | tr | `workspace_inspector.rs:69` | 各セットは停止位置、countsは次セットまでの拍数です | Each set is a stopping point; counts is the duration to the next set. |
| legacy.0194 | tr | `app_ui.rs:1186` | 拡大 | Zoom In |
| legacy.0195 | conditional | `app_state.rs:1703` | 確定して適用 | Confirm apply |
| legacy.0196 | tr | `inspector_media.rs:70` | 確認用（高速・小容量） | Review (fast / small) |
| legacy.0197 | conditional | `app_state.rs:476` | 簡潔 | Short |
| legacy.0198 | conditional | `stadium_inspector.rs:77` | 観客視点の遮蔽を診断 | Audience occlusion |
| legacy.0199 | tr | `app_ui.rs:786` | 観客席 | Audience |
| legacy.0200 | conditional | `app_state.rs:471` | 間隔 (yd) | Interval (yd) |
| legacy.0201 | tr | `app_ui.rs:750` | 基準テンポ（Count 0）。テンポマップで途中変化も設定できます | Base tempo at Count 0; use the tempo map for later changes |
| legacy.0202 | tr | `onboarding.rs:205` | 基本ワークフロー | Basic workflow |
| legacy.0203 | tr | `app_ui.rs:404` | 既定では2D編集画面だけに表示し、3D・印刷・動画には含めません | By default this appears only in the 2D editor, not in 3D, print, or video |
| legacy.0204 | tr | `inspector_media.rs:78` | 共有用（おすすめ） | Share (recommended) |
| legacy.0205 | tr | `app_state.rs:1079` | 曲線が短すぎます | The path is too short |
| legacy.0206 | tr | `app_state.rs:1890` | 曲全体 | Whole Show |
| legacy.0207 | tr | `app_ui.rs:1109` | 曲全体 | Whole Show |
| legacy.0208 | conditional | `app_state.rs:1599` | 区切り | Delimiter |
| legacy.0209 | tr | `onboarding.rs:212` | 選択: 空白ドラッグで囲み選択。移動: 空欄からでも移動。配置: クリックで追加 | Select: drag empty space to marquee. Move: drag from empty space. Place: click to add |
| legacy.0210 | conditional | `workspace_inspector.rs:86` | 空欄の数値はテンポマップから自動計算します。編集はUndo/Redoできます。 | Blank numeric fields follow the tempo map. Changes support Undo/Redo. |
| legacy.0211 | tr | `app_state.rs:1514` | 警告 | Warnings |
| legacy.0212 | tr | `inspector_media.rs:660` | 欠落した音源を再リンク… | Relink Missing Audio… |
| legacy.0213 | conditional | `app_state.rs:1599` | 検出行 | Rows detected |
| legacy.0214 | conditional | `workspace_inspector.rs:459` | 元の設計は変更せず、衝突・歩幅・到着ターンを改善する候補を最大8件比較します | Compare up to eight collision, stride, and arrival-turn fixes without changing the design |
| legacy.0215 | tr | `app_ui.rs:785` | 現在カウントの保存済みカメラキーフレームを評価します | Evaluate saved camera keyframes at the current count |
| legacy.0216 | tr | `onboarding.rs:142` | 現在のサンプル隊形は自由に編集できます。操作はすべて元に戻せます。 | You can freely edit the sample formation. Every edit can be undone. |
| legacy.0217 | tr | `app_state.rs:1889` | 現在のセット | Current Set |
| legacy.0218 | tr | `app_ui.rs:1100` | 現在のセット | Current Set |
| legacy.0219 | conditional | `app_state.rs:1632` | 現在のドキュメントとの差分 | Changes against current document |
| legacy.0220 | tr | `app_ui.rs:804` | 現在の自由視点を現在カウントへ保存（同じカウントは置換） | Save the free camera at this count (replaces an existing keyframe) |
| legacy.0221 | conditional | `app_state.rs:700` | 現在の設計から新しい分岐を作成しました | Created a new branch from the current design |
| legacy.0222 | conditional | `app_state.rs:713` | 現在の分岐を更新しました | Updated the active branch |
| legacy.0223 | conditional | `subset_snapshot_state.rs:279` | 現在の未保存変更は置き換わります。復元後もUndoできます。 | Current unsaved changes will be replaced. You can Undo afterward. |
| legacy.0224 | tr | `app_state.rs:1891` | 現在位置を開始 | Set In |
| legacy.0225 | tr | `app_ui.rs:1118` | 現在位置を開始 | Set In |
| legacy.0226 | tr | `app_state.rs:1892` | 現在位置を終了 | Set Out |
| legacy.0227 | tr | `app_ui.rs:1125` | 現在位置を終了 | Set Out |
| legacy.0228 | tr | `onboarding.rs:152` | 現在表示中のサンプルを編集します | Edit the sample currently shown |
| legacy.0229 | tr | `inspector_media.rs:227` | 固定品質 | Constant Quality |
| legacy.0230 | conditional | `app_state.rs:295` | 後で | Later |
| legacy.0231 | conditional | `app_state.rs:271` | 更新は強制されません。ダウンロード後も自動実行・自動インストールは行いません。 | Updates are optional. Downloads are never run or installed automatically. |
| legacy.0232 | conditional | `app_ui.rs:647` | 更新を確認 | Check for Updates |
| legacy.0233 | tr | `workspace_inspector.rs:348` | 構成操作（結果を確認後、Ctrl+Zで完全に戻せます） | Composition tools (inspect the result; Ctrl+Z restores it completely) |
| legacy.0234 | conditional | `app_state.rs:1614` | 行 | line |
| legacy.0235 | conditional | `app_state.rs:1616` | 行 | line |
| legacy.0236 | conditional | `app_state.rs:278` | 今すぐ | Update now |
| legacy.0237 | tr | `app_ui.rs:1000` | 今後表示しない | Don't show again |
| legacy.0238 | tr | `workspace_inspector.rs:573` | 左右: steps / units | Width: steps / units |
| legacy.0239 | conditional | `app_state.rs:1637` | 座標移動 | coordinates moved |
| legacy.0240 | conditional | `app_state.rs:1669` | 座標行単位（セット未選択時のみ個別指定） | Individual coordinate rows (used when its set is not selected) |
| legacy.0241 | conditional | `app_state.rs:1589` | 座標表をインポート | Import Coordinate Table |
| legacy.0242 | conditional | `app_ui.rs:320` | 座標表をインポート… | Import Coordinate Table… |
| legacy.0243 | conditional | `app_state.rs:450` | 座標表記プリセット | Coordinate notation |
| legacy.0244 | conditional | `app_state.rs:1611` | 座標文の解析プレビュー（曖昧な文は推測しません） | Coordinate phrase preview (ambiguous phrases are never guessed) |
| legacy.0245 | conditional | `stadium_inspector.rs:90` | 再解析 | Re-analyze |
| legacy.0246 | tr | `app_state.rs:1443` | 再割当できません | Could not reassign performers |
| legacy.0247 | tr | `onboarding.rs:214` | 再生とタイムライン | Playback and timeline |
| legacy.0248 | tr | `app_ui.rs:1207` | 再生位置を追従 | Follow Playhead |
| legacy.0249 | tr | `app_ui.rs:976` | 再生中もカウント単位でシークできます | You can seek to exact counts during playback |
| legacy.0250 | tr | `app_state.rs:1888` | 再生範囲 | Playback Range |
| legacy.0251 | tr | `app_ui.rs:1097` | 再生範囲 | Playback Range |
| legacy.0252 | tr | `app_ui.rs:1196` | 再生範囲に合わせる | Fit Range |
| legacy.0253 | conditional | `app_state.rs:484` | 最寄りのハッシュ | Nearest hash |
| legacy.0254 | conditional | `app_state.rs:487` | 最寄りのハッシュ | Nearest hash |
| legacy.0255 | conditional | `app_state.rs:484` | 最寄りの線 | Nearest line |
| legacy.0256 | conditional | `app_state.rs:486` | 最寄りの線 | Nearest line |
| legacy.0257 | tr | `inspector_media.rs:615` | 細分 | Subdivision |
| legacy.0258 | conditional | `section_manager.rs:75` | 削除して他パートへ移動 | Delete and reassign |
| legacy.0259 | tr | `inspector_media.rs:809` | 参照音源を選択 | Choose Reference Audio |
| legacy.0260 | tr | `inspector_media.rs:262` | 事前検査を更新 | Refresh Preflight |
| legacy.0261 | tr | `workspace_inspector.rs:355` | 次セットへ25%モーフ | Morph 25% to next set |
| legacy.0262 | tr | `workspace_inspector.rs:358` | 次セットへ50%モーフ | Morph 50% to next set |
| legacy.0263 | tr | `workspace_inspector.rs:424` | 次セットへの動きをリアルタイム検査 | Inspect movement to the next set in real time |
| legacy.0264 | tr | `app_state.rs:1136` | 次のセットがありません | There is no next set |
| legacy.0265 | tr | `inspector_media.rs:204` | 自動 | Auto |
| legacy.0266 | tr | `app_state.rs:1083` | 自由曲線プレビューです。適用または破棄を選んでください | Free-path preview ready. Apply or discard it |
| legacy.0267 | tr | `workspace_inspector.rs:293` | 自由描画を開始 | Start free drawing |
| legacy.0268 | conditional | `plugin_state.rs:75` | 実行プレビュー（送信されるbounded JSON） | Execution preview (bounded JSON sent to child) |
| legacy.0269 | conditional | `plugin_state.rs:80` | 実行条件を確認しました。実行時にもhash・署名・能力を再検証します。 | Execution conditions confirmed. Hash, signature, and capabilities will be revalidated at run time. |
| legacy.0270 | conditional | `plugin_state.rs:79` | 実行前チェックを確認 | Confirm execution preview |
| legacy.0271 | tr | `workspace_inspector.rs:595` | 主線間隔 | Major Line Spacing |
| legacy.0272 | tr | `workspace_inspector.rs:331` | 十字を適用 | Apply cross |
| legacy.0273 | tr | `onboarding.rs:164` | 従来形式のドリルJSONを開きます | Open a legacy drill JSON file |
| legacy.0274 | conditional | `app_state.rs:503` | 縦 | Portrait |
| legacy.0275 | tr | `app_ui.rs:397` | 縦倍率 | Scale Y |
| legacy.0276 | tr | `app_ui.rs:1171` | 縮小 | Zoom Out |
| legacy.0277 | conditional | `section_manager.rs:76` | 所属演者は一覧の別セクションへ移動します | Members move to another section |
| legacy.0278 | tr | `app_ui.rs:46` | 書き出しエラー | Export error |
| legacy.0279 | tr | `app_state.rs:1407` | 書き出しを開始できません | Could not start export |
| legacy.0280 | tr | `app_state.rs:1897` | 書き出し前の事前検査 | Run Export Preflight |
| legacy.0281 | tr | `inspector_media.rs:264` | 書き出し前の事前検査 | Run Export Preflight |
| legacy.0282 | tr | `app_state.rs:1400` | 書き出し中 | Exporting |
| legacy.0283 | tr | `app_state.rs:1510` | 小節 | Measures |
| legacy.0284 | conditional | `stadium_inspector.rs:101` | 照明 | Lighting |
| legacy.0285 | tr | `inspector_media.rs:125` | 詳細設定（コーデック・エンコーダー） | Advanced Settings (codec / encoder) |
| legacy.0286 | tr | `workspace_inspector.rs:583` | 上下: steps / units | Height: steps / units |
| legacy.0287 | tr | `inspector_media.rs:376` | 上書きして続行 | Overwrite and Continue |
| legacy.0288 | conditional | `plugin_state.rs:61` | 信頼と能力 | Trust & capabilities |
| legacy.0289 | conditional | `section_manager.rs:85` | 新しいセクション | New section |
| legacy.0290 | conditional | `app_state.rs:1624` | 新規ドキュメントとして読み込む | Import as New Document |
| legacy.0291 | tr | `app_ui.rs:794` | 真上 | Overhead |
| legacy.0292 | tr | `app_state.rs:953` | 図形パラメータが無効です | Invalid shape parameters |
| legacy.0293 | tr | `app_state.rs:982` | 図形を適用しました（元に戻せます） | Shape applied (undo available) |
| legacy.0294 | tr | `app_state.rs:980` | 図形を適用できません | Could not apply shape |
| legacy.0295 | tr | `workspace_inspector.rs:371` | 制約クリニック案を適用 | Apply constraint clinic proposal |
| legacy.0296 | tr | `app_state.rs:1252` | 制約クリニック案を適用しました | Applied constraint clinic proposal |
| legacy.0297 | tr | `app_state.rs:1269` | 制約ソルバーエラー | Constraint solver error |
| legacy.0298 | tr | `workspace_inspector.rs:325` | 星を適用 | Apply star |
| legacy.0299 | conditional | `stadium_inspector.rs:134` | 赤い輪ほど、このカメラ位置から他の演者に隠れています。カメラを動かした後は再解析してください。 | Red rings identify performers hidden by others from this camera. Re-analyze after moving it. |
| legacy.0300 | conditional | `workspace_inspector.rs:523` | 設計が変更されたため、改善案を再生成してください | The design changed; regenerate fixes |
| legacy.0301 | conditional | `app_state.rs:658` | 設計スナップショットを記録しました | Design snapshot captured |
| legacy.0302 | tr | `inspector_media.rs:290` | 設定が変わりました。事前検査を更新してください | Settings changed; refresh the preflight. |
| legacy.0303 | tr | `app_ui.rs:721` | 設定した再生範囲の開始位置へ戻ります | Return to the start of the playback range |
| legacy.0304 | tr | `inspector_media.rs:155` | 設定は有効です | Settings are valid |
| legacy.0305 | tr | `app_state.rs:1283` | 先に図形を適用してください | Apply a shape first |
| legacy.0306 | tr | `workspace_inspector.rs:602` | 線 | Lines |
| legacy.0307 | conditional | `app_state.rs:474` | 線上表記 | On-line style |
| legacy.0308 | conditional | `app_state.rs:1701` | 選択した差分を現在のドキュメントへ適用します。1回のUndoで完全に戻せます。 | Apply the selected changes to the current document? One Undo restores it completely. |
| legacy.0309 | tr | `onboarding.rs:210` | 選択と編集 | Selection and editing |
| legacy.0310 | conditional | `workspace_inspector.rs:514` | 選択案を適用 | Apply selected fix |
| legacy.0311 | conditional | `section_manager.rs:70` | 選択演者を割り当て | Assign selection |
| legacy.0312 | tr | `workspace_inspector.rs:158` | 選択解除 | Clear Selection |
| legacy.0313 | tr | `app_state.rs:1167` | 選択人数が対称数より少なすぎます | Selection is smaller than the symmetry fold |
| legacy.0314 | tr | `workspace_inspector.rs:281` | 選択中の演者へ弧長等間隔でプレビュー生成し、クリックで適用します。適用は1回で元に戻せます。 | Builds an arc-length preview for the selection; click to apply as one undoable edit. |
| legacy.0315 | tr | `workspace_inspector.rs:155` | 全員選択 | Select All |
| legacy.0316 | tr | `app_ui.rs:1192` | 全体表示 | Fit All |
| legacy.0317 | tr | `app_state.rs:1509` | 総カウント | Total counts |
| legacy.0318 | tr | `workspace_inspector.rs:290` | 楕円をプレビュー | Preview ellipse |
| legacy.0319 | tr | `workspace_inspector.rs:316` | 楕円を適用 | Apply ellipse |
| legacy.0320 | tr | `onboarding.rs:122` | 隊形を作り、カウントに合わせて動きを確認できるマーチング・デザイン環境です。 | Create formations and preview movement precisely against musical counts. |
| legacy.0321 | tr | `app_state.rs:941` | 隊形を変更できません | Could not change formation |
| legacy.0322 | tr | `app_state.rs:1058` | 代替表示できない文字数 | Unsupported glyphs |
| legacy.0323 | conditional | `app_state.rs:1600` | 置換文字があります。文字化けを確認してください。 | Replacement characters found; verify encoding. |
| legacy.0324 | tr | `inspector_media.rs:705` | 長さ | Duration |
| legacy.0325 | tr | `workspace_inspector.rs:336` | 直線フィット | Fit line |
| legacy.0326 | tr | `app_ui.rs:433` | 直前の下敷き変更を元に戻す | Undo last underlay change |
| legacy.0327 | conditional | `app_state.rs:1683` | 追加 | add |
| legacy.0328 | conditional | `app_state.rs:1691` | 適用する座標変更 | Selected coordinate changes |
| legacy.0329 | conditional | `app_state.rs:1693` | 適用前の最終確認 | Review and apply |
| legacy.0330 | tr | `workspace_inspector.rs:603` | 点 | Dots |
| legacy.0331 | tr | `app_state.rs:1895` | 動画エンコード | Video Encoding |
| legacy.0332 | tr | `inspector_media.rs:41` | 動画エンコード | Video Encoding |
| legacy.0333 | tr | `inspector_media.rs:360` | 動画を書き出す… | Export Video… |
| legacy.0334 | tr | `inspector_media.rs:350` | 動画書き出しをキャンセル | Cancel Video Export |
| legacy.0335 | conditional | `workspace_inspector.rs:100` | 同期時刻を指定 | Override sync time |
| legacy.0336 | tr | `inspector_media.rs:368` | 同名ファイルを上書きしますか？ この操作は元に戻せません。 | Overwrite the file with the same name? This cannot be undone. |
| legacy.0337 | conditional | `app_state.rs:1620` | 読み込み前プレビュー（元ファイルは変更されません） | Pre-import preview (source is never modified) |
| legacy.0338 | tr | `workspace_inspector.rs:306` | 日本語・英数字（1行） | Japanese / Latin, one line |
| legacy.0339 | tr | `workspace_inspector.rs:322` | 波形を適用 | Apply sine wave |
| legacy.0340 | tr | `workspace_inspector.rs:300` | 破棄 | Discard |
| legacy.0341 | conditional | `app_state.rs:440` | 配布物をつくる | Create handouts |
| legacy.0342 | tr | `inspector_media.rs:558` | 拍子(分子) | Time Signature (numerator) |
| legacy.0343 | tr | `app_state.rs:1509` | 拍子変更 | Meter changes |
| legacy.0344 | tr | `app_state.rs:1258` | 反復 | iterations |
| legacy.0345 | tr | `app_state.rs:1887` | 範囲先頭 | Range Start |
| legacy.0346 | tr | `app_ui.rs:720` | 範囲先頭 | Range Start |
| legacy.0347 | tr | `inspector_media.rs:723` | 非破壊トリム / フェード | Non-destructive Trim / Fade |
| legacy.0348 | conditional | `workspace_inspector.rs:94` | 備考／キュー | Notes / cues |
| legacy.0349 | conditional | `app_state.rs:452` | 標準 | Standard |
| legacy.0350 | conditional | `app_state.rs:492` | 標準帳票 | Standard report |
| legacy.0351 | tr | `workspace_inspector.rs:312` | 描画中: フィールド上をドラッグ | Drawing: drag on the field |
| legacy.0352 | tr | `inspector_media.rs:116` | 品質 | Quality |
| legacy.0353 | conditional | `subset_snapshot_state.rs:287` | 復元する | Restore |
| legacy.0354 | tr | `workspace_inspector.rs:597` | 分割 | Divisions |
| legacy.0355 | conditional | `app_state.rs:727` | 分岐へ切り替えられません | Could not switch branches |
| legacy.0356 | conditional | `app_state.rs:785` | 分岐をマージしました。Undoで戻せます | Merged the branch. Undo is available |
| legacy.0357 | conditional | `app_state.rs:772` | 分岐をマージできません | Could not merge the branch |
| legacy.0358 | conditional | `app_state.rs:749` | 分岐を切り替えました。Undoで文書を戻せます | Switched branches. Undo restores the document |
| legacy.0359 | tr | `workspace_inspector.rs:307` | 文字をプレビュー | Preview text |
| legacy.0360 | tr | `workspace_inspector.rs:305` | 文字隊形 | Formation text |
| legacy.0361 | tr | `app_state.rs:1067` | 文字隊形エラー | Formation text error |
| legacy.0362 | tr | `app_state.rs:1049` | 文字隊形をプレビュー中 | Previewing formation text |
| legacy.0363 | tr | `app_state.rs:1124` | 変更を適用できません | Could not apply change |
| legacy.0364 | tr | `onboarding.rs:218` | 保存 | Saving |
| legacy.0365 | tr | `app_ui.rs:691` | 保存済みの .drill.json を開きます | Open a saved .drill.json file |
| legacy.0366 | tr | `inspector_media.rs:391` | 保存先を開く | Open Destination |
| legacy.0367 | tr | `workspace_inspector.rs:374` | 保存中の図形 | Stored shape |
| legacy.0368 | conditional | `app_state.rs:480` | 歩数のみ | Steps only |
| legacy.0369 | tr | `app_ui.rs:877` | 補間: | Interpolation: |
| legacy.0370 | tr | `workspace_inspector.rs:319` | 放物線を適用 | Apply parabola |
| legacy.0371 | tr | `inspector_media.rs:82` | 本番上映用（高画質） | Presentation (high quality) |
| legacy.0372 | conditional | `app_state.rs:1622` | 未取込: カメラ、音源同期、経路、印刷設定、演者記号。確定後に検算してください。 | Not imported: cameras, audio sync, paths, print layout, performer symbols. Verify after import. |
| legacy.0373 | conditional | `section_manager.rs:60` | 名称 | Name |
| legacy.0374 | conditional | `app_state.rs:840` | 名称を変更できません | Could not rename the section |
| legacy.0375 | conditional | `section_manager.rs:67` | 名称を保存 | Save name |
| legacy.0376 | tr | `app_ui.rs:1041` | 名称未設定 | Untitled |
| legacy.0377 | conditional | `app_state.rs:475` | 明示 | Explicit |
| legacy.0378 | conditional | `app_state.rs:1704` | 戻る | Back |
| legacy.0379 | conditional | `workspace_inspector.rs:474` | 有効な改善案はありません | No improving fix found |
| legacy.0380 | tr | `inspector_media.rs:54` | 用途を選ぶだけの簡単設定 | Choose a Purpose to Get Started |
| legacy.0381 | conditional | `section_manager.rs:62` | 略称 | Short |
| legacy.0382 | conditional | `section_manager.rs:87` | 例: カラーガード | e.g. Color Guard |
| legacy.0383 | tr | `workspace_inspector.rs:328` | 六角形を適用 | Apply hexagon |
