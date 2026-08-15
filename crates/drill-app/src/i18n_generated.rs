// @generated from docs/MESSAGE_CATALOG.md; catalog literals live only here.
use drill_core::Locale;
pub(crate) fn registered(locale: Locale, id: &str) -> &'static str {
    match (locale, id) {
        (Locale::Ja, "workspace-preset.001") => "デザインワークスペース",
        (Locale::En, "workspace-preset.001") => "Design Workspace",
        (Locale::Ja, "workspace-preset.002") => "レビューワークスペース",
        (Locale::En, "workspace-preset.002") => "Review Workspace",
        (Locale::Ja, "workspace-preset.003") => "プレゼンテーションワークスペース",
        (Locale::En, "workspace-preset.003") => "Presentation Workspace",
        (Locale::Ja, "workspace-preset.004") => "デザインワークスペースに切り替えました",
        (Locale::En, "workspace-preset.004") => "Switched to Design workspace",
        (Locale::Ja, "workspace-preset.005") => "レビューワークスペースに切り替えました",
        (Locale::En, "workspace-preset.005") => "Switched to Review workspace",
        (Locale::Ja, "workspace-preset.006") => "プレゼンテーションワークスペースに切り替えました",
        (Locale::En, "workspace-preset.006") => "Switched to Presentation workspace",
        (Locale::Ja, "workspace-preset.007") => "ワークスペース",
        (Locale::En, "workspace-preset.007") => "Workspace",
        (Locale::Ja, "production-sheet.001") => "プロダクションシート",
        (Locale::En, "production-sheet.001") => "Production Sheet",
        (Locale::Ja, "production-sheet.002") => "リハーサル用のセット一覧",
        (Locale::En, "production-sheet.002") => "Rehearsal set list",
        (Locale::Ja, "production-sheet.003") => "常に表示",
        (Locale::En, "production-sheet.003") => "Keep open",
        (Locale::Ja, "production-sheet.004") => {
            "行をクリックすると停止してそのセット先頭へ移動します。"
        }
        (Locale::En, "production-sheet.004") => {
            "Click a row to pause and go to that set's first count."
        }
        (Locale::Ja, "production-sheet.005") => "閉じる",
        (Locale::En, "production-sheet.005") => "Close",
        (Locale::Ja, "production-sheet.006") => "セット・マーク・タイトル・備考・テンポを検索",
        (Locale::En, "production-sheet.006") => "Search set, mark, title, notes, or tempo",
        (Locale::Ja, "production-sheet.007") => "拍数",
        (Locale::En, "production-sheet.007") => "Counts",
        (Locale::Ja, "production-sheet.008") => "マーク",
        (Locale::En, "production-sheet.008") => "Mark",
        (Locale::Ja, "production-sheet.009") => "タイトル",
        (Locale::En, "production-sheet.009") => "Title",
        (Locale::Ja, "production-sheet.010") => "テンポ",
        (Locale::En, "production-sheet.010") => "Tempo",
        (Locale::Ja, "production-sheet.011") => "備考／キュー",
        (Locale::En, "production-sheet.011") => "Notes / cues",
        (Locale::Ja, "production-sheet.012") => "クリック: 再生を停止してセット先頭へ移動",
        (Locale::En, "production-sheet.012") => "Click: pause playback and go to set start",
        (Locale::Ja, "production-sheet.013") => "編集するセットを一覧から選択してください。",
        (Locale::En, "production-sheet.013") => "Select a set from the list to edit it.",
        (Locale::Ja, "production-sheet.014") => "セット",
        (Locale::En, "production-sheet.014") => "Set",
        (Locale::Ja, "production-sheet.015") => "保存",
        (Locale::En, "production-sheet.015") => "Save",
        (Locale::Ja, "production-sheet.016") => "キャンセル",
        (Locale::En, "production-sheet.016") => "Cancel",
        (Locale::Ja, "production-sheet.017") => "プロダクション情報を更新できません",
        (Locale::En, "production-sheet.017") => "Could not update production information",
        (Locale::Ja, "production-sheet.018") => "同期時刻を指定",
        (Locale::En, "production-sheet.018") => "Override sync time",
        (Locale::Ja, "production-sheet.019") => "トランジション尺を指定",
        (Locale::En, "production-sheet.019") => "Override transition duration",
        (Locale::Ja, "production-sheet.020") => "タイトル",
        (Locale::En, "production-sheet.020") => "Title",
        (Locale::Ja, "production-sheet.021") => "マーク",
        (Locale::En, "production-sheet.021") => "Mark",
        (Locale::Ja, "production-sheet.022") => "備考／キュー",
        (Locale::En, "production-sheet.022") => "Notes / cues",
        (Locale::Ja, "production-sheet.023") => "テンポ",
        (Locale::En, "production-sheet.023") => "Tempo",
        (Locale::Ja, "production-sheet.024") => "総",
        (Locale::En, "production-sheet.024") => "G",
        (Locale::Ja, "production-sheet.025") => "再生位置に追従",
        (Locale::En, "production-sheet.025") => "Follow Playhead",
        (Locale::Ja, "production-sheet.026") => {
            "再生位置のセットを自動選択します。未保存の編集がある間は切り替えません。"
        }
        (Locale::En, "production-sheet.026") => {
            "Select the set under the playhead automatically. Unsaved edits are never replaced."
        }
        (Locale::Ja, "production-sheet.027") => {
            "未保存の編集があります。保存またはキャンセルしてから別のセットを選択してください。"
        }
        (Locale::En, "production-sheet.027") => {
            "You have unsaved edits. Save or cancel before selecting another set."
        }
        (Locale::Ja, "production-sheet.028") => "セット先頭へ移動",
        (Locale::En, "production-sheet.028") => "Go to Set Start",
        (Locale::Ja, "production-sheet.029") => "クリック: このセットを選択して編集",
        (Locale::En, "production-sheet.029") => "Click to select this set for editing",
        (Locale::Ja, "production-sheet.030") => {
            "行で編集対象を選び、「セット先頭へ移動」で再生位置を移動します。"
        }
        (Locale::En, "production-sheet.030") => {
            "Select a row to edit it; use Go to Set Start to move the playhead."
        }
        (Locale::Ja, "production-sheet.031") => "再生位置",
        (Locale::En, "production-sheet.031") => "Playhead",
        (Locale::Ja, "production-sheet.034") => "↑↓・Home・End: 行を選択　Enter: セット先頭へ移動",
        (Locale::En, "production-sheet.034") => {
            "↑↓, Home, End: select a row · Enter: go to set start"
        }
        (Locale::Ja, "commands.105") => "プロダクションシートを表示",
        (Locale::En, "commands.105") => "Show Production Sheet",
        (Locale::Ja, "commands.106") => "前のセットへ",
        (Locale::En, "commands.106") => "Previous Set",
        (Locale::Ja, "commands.107") => "次のセットへ",
        (Locale::En, "commands.107") => "Next Set",
        (Locale::Ja, "commands.108") => "全体拍へ移動…",
        (Locale::En, "commands.108") => "Go to Global Count…",
        (Locale::Ja, "commands.109") => "前のセットはありません",
        (Locale::En, "commands.109") => "No previous set",
        (Locale::Ja, "commands.110") => "次のセットはありません",
        (Locale::En, "commands.110") => "No next set",
        (Locale::Ja, "commands.115") => "開く…",
        (Locale::En, "commands.115") => "Open…",
        (Locale::Ja, "commands.116") => "プロジェクトを開く…",
        (Locale::En, "commands.116") => "Open Project…",
        (Locale::Ja, "commands.117") => "保存",
        (Locale::En, "commands.117") => "Save",
        (Locale::Ja, "commands.118") => "別名で保存…",
        (Locale::En, "commands.118") => "Save As…",
        (Locale::Ja, "recent-projects.001") => "最近使った項目",
        (Locale::En, "recent-projects.001") => "Open Recent",
        (Locale::Ja, "recent-projects.002") => "最近開いたコンテはありません",
        (Locale::En, "recent-projects.002") => "No Recent Documents",
        (Locale::Ja, "recent-projects.003") => "最近使った項目を消去",
        (Locale::En, "recent-projects.003") => "Clear Recent Documents",
        (Locale::Ja, "recent-projects.004") => {
            "開くファイルを選択します。現在の未保存変更は先に確認されます。"
        }
        (Locale::En, "recent-projects.004") => {
            "Choose a file to open. Unsaved changes are protected first."
        }
        (Locale::Ja, "recent-projects.005") => {
            "この最近使った項目は見つからなかったため、一覧から除きました"
        }
        (Locale::En, "recent-projects.005") => {
            "That recent document was unavailable and has been removed"
        }
        (Locale::Ja, "recent-projects.006") => "この項目を一覧から除く",
        (Locale::En, "recent-projects.006") => "Remove from Recent Documents",
        (Locale::Ja, "recent-projects.007") => "最近使った項目",
        (Locale::En, "recent-projects.007") => "Open Recent",
        (Locale::Ja, "recent-projects.008") => "最近開いたコンテはありません",
        (Locale::En, "recent-projects.008") => "No Recent Documents",
        (Locale::Ja, "recent-projects.009") => "最近使った項目を消去",
        (Locale::En, "recent-projects.009") => "Clear Recent Documents",
        (Locale::Ja, "recent-projects.010") => "最近使った項目",
        (Locale::En, "recent-projects.010") => "Open Recent",
        (Locale::Ja, "recent-projects.011") => "最近開いたコンテはありません",
        (Locale::En, "recent-projects.011") => "No Recent Documents",
        (Locale::Ja, "recent-projects.012") => "最近使った項目を消去",
        (Locale::En, "recent-projects.012") => "Clear Recent Documents",
        (Locale::Ja, "recent-projects.013") => "最近使った項目",
        (Locale::En, "recent-projects.013") => "Open Recent",
        (Locale::Ja, "commands.119") => "プロジェクトとして保存…",
        (Locale::En, "commands.119") => "Save as Project…",
        (Locale::Ja, "commands.120") => "座標表をインポート…",
        (Locale::En, "commands.120") => "Import Coordinate Table…",
        (Locale::Ja, "commands.121") => "MusicXML / MIDIテンポをインポート…",
        (Locale::En, "commands.121") => "Import MusicXML / MIDI Tempo…",
        (Locale::Ja, "commands.122") => "画像下敷きを読み込む…",
        (Locale::En, "commands.122") => "Load Image Underlay…",
        (Locale::Ja, "commands.123") => "水平に整列",
        (Locale::En, "commands.123") => "Align Horizontally",
        (Locale::Ja, "commands.124") => "垂直に整列",
        (Locale::En, "commands.124") => "Align Vertically",
        (Locale::Ja, "commands.125") => "横方向に均等分配",
        (Locale::En, "commands.125") => "Distribute Horizontally",
        (Locale::Ja, "commands.126") => "縦方向に均等分配",
        (Locale::En, "commands.126") => "Distribute Vertically",
        (Locale::Ja, "commands.127") => "左右反転",
        (Locale::En, "commands.127") => "Flip Horizontally",
        (Locale::Ja, "commands.128") => "上下反転",
        (Locale::En, "commands.128") => "Flip Vertically",
        (Locale::Ja, "commands.129") => "直線を作成…",
        (Locale::En, "commands.129") => "Make Line…",
        (Locale::Ja, "commands.130") => "選択をロック",
        (Locale::En, "commands.130") => "Lock Selection",
        (Locale::Ja, "commands.131") => "選択を非表示",
        (Locale::En, "commands.131") => "Hide Selection",
        (Locale::Ja, "commands.132") => "2人以上を選択してください",
        (Locale::En, "commands.132") => "Select at least two performers",
        (Locale::Ja, "commands.133") => "先に演者を選択してください",
        (Locale::En, "commands.133") => "Select performers first",
        (Locale::Ja, "commands.134") => "セット先頭に戻ると配置を編集できます",
        (Locale::En, "commands.134") => "Return to the set start to arrange performers",
        (Locale::Ja, "commands.135") => "整列",
        (Locale::En, "commands.135") => "Arrange",
        (Locale::Ja, "commands.136") => "先に演者を選択してください",
        (Locale::En, "commands.136") => "Select performers first",
        (Locale::Ja, "app-state.001") => "DrillForgeの更新",
        (Locale::En, "app-state.001") => "DrillForge Update",
        (Locale::Ja, "app-state.002") => {
            "更新は強制されません。ダウンロード後も自動実行・自動インストールは行いません。"
        }
        (Locale::En, "app-state.002") => {
            "Updates are optional. Downloads are never run or installed automatically."
        }
        (Locale::Ja, "app-state.003") => "今すぐ",
        (Locale::En, "app-state.003") => "Update now",
        (Locale::Ja, "app-state.004") => "安全のため公式ダウンロードURLをコピーします",
        (Locale::En, "app-state.004") => "Copies the official download URL for safety",
        (Locale::Ja, "app-state.005") => "後で",
        (Locale::En, "app-state.005") => "Later",
        (Locale::Ja, "app-state.006") => "このバージョンをスキップ",
        (Locale::En, "app-state.006") => "Skip this version",
        (Locale::Ja, "app-state.007") => "印刷・PDFワークスペース",
        (Locale::En, "app-state.007") => "Print & PDF Workspace",
        (Locale::Ja, "app-state.008") => "配布物をつくる",
        (Locale::En, "app-state.008") => "Create handouts",
        (Locale::Ja, "app-state.009") => {
            "右側のプレビューを確認し、PDFとして保存できます。フィールド図は編集画面と同じ座標・描画データを使用します。"
        }
        (Locale::En, "app-state.009") => {
            "Review the preview and save as PDF. Field charts use the same coordinates and drawing data as the editor."
        }
        (Locale::Ja, "app-state.010") => "1. 内容",
        (Locale::En, "app-state.010") => "1. Content",
        (Locale::Ja, "app-state.011") => "セット別フィールド図",
        (Locale::En, "app-state.011") => "Set charts",
        (Locale::Ja, "app-state.012") => "演者別ドリルブック",
        (Locale::En, "app-state.012") => "Performer drill book",
        (Locale::Ja, "app-state.013") => "カウントシート",
        (Locale::En, "app-state.013") => "Count sheet",
        (Locale::Ja, "app-state.014") => "プロダクションシート",
        (Locale::En, "app-state.014") => "Production sheet",
        (Locale::Ja, "app-state.015") => "座標表記プリセット",
        (Locale::En, "app-state.015") => "Coordinate notation",
        (Locale::Ja, "app-state.016") => "標準",
        (Locale::En, "app-state.016") => "Standard",
        (Locale::Ja, "app-state.017") => "屋内・簡潔",
        (Locale::En, "app-state.017") => "Indoor terse",
        (Locale::Ja, "app-state.018") => "フィールド設定",
        (Locale::En, "app-state.018") => "Field setting",
        (Locale::Ja, "app-state.019") => "カスタム",
        (Locale::En, "app-state.019") => "Custom",
        (Locale::Ja, "app-state.020") => "間隔 (yd)",
        (Locale::En, "app-state.020") => "Interval (yd)",
        (Locale::Ja, "app-state.021") => "線上表記",
        (Locale::En, "app-state.021") => "On-line style",
        (Locale::Ja, "app-state.022") => "明示",
        (Locale::En, "app-state.022") => "Explicit",
        (Locale::Ja, "app-state.023") => "簡潔",
        (Locale::En, "app-state.023") => "Short",
        (Locale::Ja, "app-state.024") => "歩数のみ",
        (Locale::En, "app-state.024") => "Steps only",
        (Locale::Ja, "app-state.025") => "最寄りのハッシュ",
        (Locale::En, "app-state.025") => "Nearest hash",
        (Locale::Ja, "app-state.026") => "最寄りの線",
        (Locale::En, "app-state.026") => "Nearest line",
        (Locale::Ja, "app-state.027") => "最寄りの線",
        (Locale::En, "app-state.027") => "Nearest line",
        (Locale::Ja, "app-state.028") => "最寄りのハッシュ",
        (Locale::En, "app-state.028") => "Nearest hash",
        (Locale::Ja, "app-state.029") => "標準帳票",
        (Locale::En, "app-state.029") => "Standard report",
        (Locale::Ja, "app-state.030") => "コンパクト",
        (Locale::En, "app-state.030") => "Compact",
        (Locale::Ja, "app-state.031") => "リハーサル",
        (Locale::En, "app-state.031") => "Rehearsal",
        (Locale::Ja, "app-state.032") => "2. 用紙",
        (Locale::En, "app-state.032") => "2. Page",
        (Locale::Ja, "app-state.033") => "縦",
        (Locale::En, "app-state.033") => "Portrait",
        (Locale::Ja, "app-state.034") => "横",
        (Locale::En, "app-state.034") => "Landscape",
        (Locale::Ja, "app-state.035") => "PDFの保存先を選ぶ…",
        (Locale::En, "app-state.035") => "Choose PDF Destination…",
        (Locale::Ja, "app-state.036") => "印刷プレビュー",
        (Locale::En, "app-state.036") => "Print preview",
        (Locale::Ja, "app-state.037") => "サブセットIDを採番できません",
        (Locale::En, "app-state.037") => "Could not allocate a subset ID",
        (Locale::Ja, "app-state.038") => "サブセットを作成できません",
        (Locale::En, "app-state.038") => "Could not create the subset",
        (Locale::Ja, "app-state.039") => "サブセット名を変更できません",
        (Locale::En, "app-state.039") => "Could not rename the subset",
        (Locale::Ja, "app-state.040") => "サブセットのメンバーを変更できません",
        (Locale::En, "app-state.040") => "Could not update subset members",
        (Locale::Ja, "app-state.041") => "サブセットを削除できません",
        (Locale::En, "app-state.041") => "Could not delete the subset",
        (Locale::Ja, "app-state.042") => "設計スナップショットを記録しました",
        (Locale::En, "app-state.042") => "Design snapshot captured",
        (Locale::Ja, "app-state.043") => "スナップショットを復元できません",
        (Locale::En, "app-state.043") => "Could not restore the snapshot",
        (Locale::Ja, "app-state.044") => "現在の設計から新しい分岐を作成しました",
        (Locale::En, "app-state.044") => "Created a new branch from the current design",
        (Locale::Ja, "app-state.045") => "現在の分岐を更新しました",
        (Locale::En, "app-state.045") => "Updated the active branch",
        (Locale::Ja, "app-state.046") => "分岐へ切り替えられません",
        (Locale::En, "app-state.046") => "Could not switch branches",
        (Locale::Ja, "app-state.047") => "分岐を切り替えました。Undoで文書を戻せます",
        (Locale::En, "app-state.047") => "Switched branches. Undo restores the document",
        (Locale::Ja, "app-state.048") => "分岐をマージできません",
        (Locale::En, "app-state.048") => "Could not merge the branch",
        (Locale::Ja, "app-state.049") => "分岐をマージしました。Undoで戻せます",
        (Locale::En, "app-state.049") => "Merged the branch. Undo is available",
        (Locale::Ja, "app-state.050") => "セクションを追加できません",
        (Locale::En, "app-state.050") => "Could not add the section",
        (Locale::Ja, "app-state.051") => "名称を変更できません",
        (Locale::En, "app-state.051") => "Could not rename the section",
        (Locale::Ja, "app-state.052") => "演者を割り当てできません",
        (Locale::En, "app-state.052") => "Could not assign the performers",
        (Locale::Ja, "app-state.053") => "セクションを削除できません",
        (Locale::En, "app-state.053") => "Could not remove the section",
        (Locale::Ja, "app-state.054") => "隊形を変更できません",
        (Locale::En, "app-state.054") => "Could not change formation",
        (Locale::Ja, "app-state.055") => "図形パラメータが無効です",
        (Locale::En, "app-state.055") => "Invalid shape parameters",
        (Locale::Ja, "app-state.056") => "図形を適用できません",
        (Locale::En, "app-state.056") => "Could not apply shape",
        (Locale::Ja, "app-state.057") => "図形を適用しました（元に戻せます）",
        (Locale::En, "app-state.057") => "Shape applied (undo available)",
        (Locale::Ja, "app-state.058") => "プレビューを破棄しました",
        (Locale::En, "app-state.058") => "Preview discarded",
        (Locale::Ja, "app-state.059") => "フィールド上をドラッグして自由曲線を描いてください",
        (Locale::En, "app-state.059") => "Drag on the field to draw a free path",
        (Locale::Ja, "app-state.060") => "文字隊形をプレビュー中",
        (Locale::En, "app-state.060") => "Previewing formation text",
        (Locale::Ja, "app-state.061") => "代替表示できない文字数",
        (Locale::En, "app-state.061") => "Unsupported glyphs",
        (Locale::Ja, "app-state.062") => "文字隊形エラー",
        (Locale::En, "app-state.062") => "Formation text error",
        (Locale::Ja, "app-state.063") => "曲線が短すぎます",
        (Locale::En, "app-state.063") => "The path is too short",
        (Locale::Ja, "app-state.064") => "自由曲線プレビューです。適用または破棄を選んでください",
        (Locale::En, "app-state.064") => "Free-path preview ready. Apply or discard it",
        (Locale::Ja, "app-state.065") => "プレビューを適用できません",
        (Locale::En, "app-state.065") => "Cannot apply preview",
        (Locale::Ja, "app-state.066") => "変更を適用できません",
        (Locale::En, "app-state.066") => "Could not apply change",
        (Locale::Ja, "app-state.067") => "次のセットがありません",
        (Locale::En, "app-state.067") => "There is no next set",
        (Locale::Ja, "app-state.068") => "選択人数が対称数より少なすぎます",
        (Locale::En, "app-state.068") => "Selection is smaller than the symmetry fold",
        (Locale::Ja, "app-state.069") => "2名以上を選択してください",
        (Locale::En, "app-state.069") => "Select at least two performers",
        (Locale::Ja, "app-state.070") => "制約クリニック案を適用しました",
        (Locale::En, "app-state.070") => "Applied constraint clinic proposal",
        (Locale::Ja, "app-state.071") => "反復",
        (Locale::En, "app-state.071") => "iterations",
        (Locale::Ja, "app-state.072") => "（上限到達）",
        (Locale::En, "app-state.072") => " (iteration limit)",
        (Locale::Ja, "app-state.073") => "制約ソルバーエラー",
        (Locale::En, "app-state.073") => "Constraint solver error",
        (Locale::Ja, "app-state.074") => "先に図形を適用してください",
        (Locale::En, "app-state.074") => "Apply a shape first",
        (Locale::Ja, "app-state.075") => "セクション割当に失敗しました",
        (Locale::En, "app-state.075") => "Section assignment was incomplete",
        (Locale::Ja, "app-state.076") => "書き出し中",
        (Locale::En, "app-state.076") => "Exporting",
        (Locale::Ja, "app-state.077") => "書き出しを開始できません",
        (Locale::En, "app-state.077") => "Could not start export",
        (Locale::Ja, "app-state.078") => "再割当できません",
        (Locale::En, "app-state.078") => "Could not reassign performers",
        (Locale::Ja, "app-state.079") => "移動距離を最小化するよう次セットを再割り当てしました",
        (Locale::En, "app-state.079") => "Reassigned the next set to minimize travel",
        (Locale::Ja, "app-state.080") => "音楽タイムラインをインポート",
        (Locale::En, "app-state.080") => "Import Musical Timeline",
        (Locale::Ja, "app-state.081") => "テンポ変更",
        (Locale::En, "app-state.081") => "Tempo changes",
        (Locale::Ja, "app-state.082") => "拍子変更",
        (Locale::En, "app-state.082") => "Meter changes",
        (Locale::Ja, "app-state.083") => "総カウント",
        (Locale::En, "app-state.083") => "Total counts",
        (Locale::Ja, "app-state.084") => "小節",
        (Locale::En, "app-state.084") => "Measures",
        (Locale::Ja, "app-state.085") => "リハーサル記号",
        (Locale::En, "app-state.085") => "Rehearsal marks",
        (Locale::Ja, "app-state.086") => {
            "プレビューです。音符・歌詞・パート・演者は取り込みません。適用はUndoできます。"
        }
        (Locale::En, "app-state.086") => {
            "Preview only. Notes, lyrics, parts and performers are not imported. Applying supports Undo."
        }
        (Locale::Ja, "app-state.087") => "警告",
        (Locale::En, "app-state.087") => "Warnings",
        (Locale::Ja, "app-state.088") => "テンポ・記号を適用",
        (Locale::En, "app-state.088") => "Apply Tempo and Marks",
        (Locale::Ja, "app-state.089") => "キャンセル",
        (Locale::En, "app-state.089") => "Cancel",
        (Locale::Ja, "app-state.090") => "キャンセル",
        (Locale::En, "app-state.090") => "Cancel",
        (Locale::Ja, "app-state.091") => "音楽タイムラインのテンポと記号を適用しました",
        (Locale::En, "app-state.091") => "Applied musical timeline tempo and marks",
        (Locale::Ja, "app-state.092") => "テンポ適用エラー",
        (Locale::En, "app-state.092") => "Tempo import error",
        (Locale::Ja, "app-state.093") => "座標表をインポート",
        (Locale::En, "app-state.093") => "Import Coordinate Table",
        (Locale::Ja, "app-state.094") => {
            "CSV / TSV / UTF-8テキスト / Excel XLSXに対応。独自形式は意図的に読みません。"
        }
        (Locale::En, "app-state.094") => {
            "Supports CSV, TSV, UTF-8 text and Excel XLSX. Proprietary formats are intentionally not parsed."
        }
        (Locale::Ja, "app-state.095") => "Excelシート",
        (Locale::En, "app-state.095") => "Excel sheet",
        (Locale::Ja, "app-state.096") => " · プレビューは先頭のみ",
        (Locale::En, "app-state.096") => " · preview truncated",
        (Locale::Ja, "app-state.097") => "検出行",
        (Locale::En, "app-state.097") => "Rows detected",
        (Locale::Ja, "app-state.098") => "区切り",
        (Locale::En, "app-state.098") => "Delimiter",
        (Locale::Ja, "app-state.099") => "置換文字があります。文字化けを確認してください。",
        (Locale::En, "app-state.099") => "Replacement characters found; verify encoding.",
        (Locale::Ja, "app-state.100") => "座標文の解析プレビュー（曖昧な文は推測しません）",
        (Locale::En, "app-state.100") => {
            "Coordinate phrase preview (ambiguous phrases are never guessed)"
        }
        (Locale::Ja, "app-state.101") => "行",
        (Locale::En, "app-state.101") => "line",
        (Locale::Ja, "app-state.102") => "行",
        (Locale::En, "app-state.102") => "line",
        (Locale::Ja, "app-state.103") => "読み込み前プレビュー（元ファイルは変更されません）",
        (Locale::En, "app-state.103") => "Pre-import preview (source is never modified)",
        (Locale::Ja, "app-state.104") => {
            "未取込: カメラ、音源同期、経路、印刷設定、演者記号。確定後に検算してください。"
        }
        (Locale::En, "app-state.104") => {
            "Not imported: cameras, audio sync, paths, print layout, performer symbols. Verify after import."
        }
        (Locale::Ja, "app-state.105") => "新規ドキュメントとして読み込む",
        (Locale::En, "app-state.105") => "Import as New Document",
        (Locale::Ja, "app-state.106") => "キャンセル",
        (Locale::En, "app-state.106") => "Cancel",
        (Locale::Ja, "app-state.107") => "現在のドキュメントとの差分",
        (Locale::En, "app-state.107") => "Changes against current document",
        (Locale::Ja, "app-state.108") => "演者",
        (Locale::En, "app-state.108") => "Performers",
        (Locale::Ja, "app-state.109") => "セット",
        (Locale::En, "app-state.109") => "Sets",
        (Locale::Ja, "app-state.110") => "座標移動",
        (Locale::En, "app-state.110") => "coordinates moved",
        (Locale::Ja, "app-state.111") => "演者セクション",
        (Locale::En, "app-state.111") => "Performer sections",
        (Locale::Ja, "app-state.112") => "すべて選択",
        (Locale::En, "app-state.112") => "Select all",
        (Locale::Ja, "app-state.113") => "すべて解除",
        (Locale::En, "app-state.113") => "Clear all",
        (Locale::Ja, "app-state.114") => "セット単位（選択するとそのセットの全変更を適用）",
        (Locale::En, "app-state.114") => "By set (selecting applies every change in that set)",
        (Locale::Ja, "app-state.115") => "座標行単位（セット未選択時のみ個別指定）",
        (Locale::En, "app-state.115") => {
            "Individual coordinate rows (used when its set is not selected)"
        }
        (Locale::Ja, "app-state.116") => "追加",
        (Locale::En, "app-state.116") => "add",
        (Locale::Ja, "app-state.117") => "適用する座標変更",
        (Locale::En, "app-state.117") => "Selected coordinate changes",
        (Locale::Ja, "app-state.118") => "適用前の最終確認",
        (Locale::En, "app-state.118") => "Review and apply",
        (Locale::Ja, "app-state.119") => "キャンセル",
        (Locale::En, "app-state.119") => "Cancel",
        (Locale::Ja, "app-state.120") => {
            "選択した差分を現在のドキュメントへ適用します。1回のUndoで完全に戻せます。"
        }
        (Locale::En, "app-state.120") => {
            "Apply the selected changes to the current document? One Undo restores it completely."
        }
        (Locale::Ja, "app-state.121") => "確定して適用",
        (Locale::En, "app-state.121") => "Confirm apply",
        (Locale::Ja, "app-state.122") => "戻る",
        (Locale::En, "app-state.122") => "Back",
        (Locale::Ja, "app-state.123") => "Excelシート選択エラー",
        (Locale::En, "app-state.123") => "Excel sheet selection error",
        (Locale::Ja, "app-state.124") => "範囲先頭",
        (Locale::En, "app-state.124") => "Range Start",
        (Locale::Ja, "app-state.125") => "再生範囲",
        (Locale::En, "app-state.125") => "Playback Range",
        (Locale::Ja, "app-state.126") => "現在のセット",
        (Locale::En, "app-state.126") => "Current Set",
        (Locale::Ja, "app-state.127") => "曲全体",
        (Locale::En, "app-state.127") => "Whole Show",
        (Locale::Ja, "app-state.128") => "現在位置を開始",
        (Locale::En, "app-state.128") => "Set In",
        (Locale::Ja, "app-state.129") => "現在位置を終了",
        (Locale::En, "app-state.129") => "Set Out",
        (Locale::Ja, "app-state.130") => "グリッドデザイナー",
        (Locale::En, "app-state.130") => "Grid Designer",
        (Locale::Ja, "app-state.131") => "テンポマップ",
        (Locale::En, "app-state.131") => "Tempo Map",
        (Locale::Ja, "app-state.132") => "動画エンコード",
        (Locale::En, "app-state.132") => "Video Encoding",
        (Locale::Ja, "app-state.133") => "音源 / カウント",
        (Locale::En, "app-state.133") => "Audio / Count",
        (Locale::Ja, "app-state.134") => "書き出し前の事前検査",
        (Locale::En, "app-state.134") => "Run Export Preflight",
        (Locale::Ja, "app-state.135") => "音源解析をキャンセル",
        (Locale::En, "app-state.135") => "Cancel Audio Analysis",
        (Locale::Ja, "app-ui.001") => "画像下敷きを読み込みました",
        (Locale::En, "app-ui.001") => "Loaded image underlay",
        (Locale::Ja, "app-ui.002") => "画像下敷きエラー",
        (Locale::En, "app-ui.002") => "Image underlay error",
        (Locale::Ja, "app-ui.003") => "書き出しエラー",
        (Locale::En, "app-ui.003") => "Export error",
        (Locale::Ja, "app-ui.004") => "画像下敷きが見つかりません。下敷きなしで開きました",
        (Locale::En, "app-ui.004") => "Image underlay is missing; opened without it",
        (Locale::Ja, "app-ui.005") => "インポートエラー",
        (Locale::En, "app-ui.005") => "Import error",
        (Locale::Ja, "app-ui.006") => "座標表をインポート…",
        (Locale::En, "app-ui.006") => "Import Coordinate Table…",
        (Locale::Ja, "app-ui.007") => "MusicXML / MIDIテンポをインポート…",
        (Locale::En, "app-ui.007") => "Import MusicXML / MIDI Tempo…",
        (Locale::Ja, "app-ui.008") => "画像下敷きを読み込む…",
        (Locale::En, "app-ui.008") => "Load Image Underlay…",
        (Locale::Ja, "app-ui.009") => "下敷きを表示",
        (Locale::En, "app-ui.009") => "Show underlay",
        (Locale::Ja, "app-ui.010") => "下敷き濃度",
        (Locale::En, "app-ui.010") => "Underlay opacity",
        (Locale::Ja, "app-ui.011") => "下敷き配置",
        (Locale::En, "app-ui.011") => "Underlay placement",
        (Locale::Ja, "app-ui.012") => "横倍率",
        (Locale::En, "app-ui.012") => "Scale X",
        (Locale::Ja, "app-ui.013") => "縦倍率",
        (Locale::En, "app-ui.013") => "Scale Y",
        (Locale::Ja, "app-ui.014") => "回転",
        (Locale::En, "app-ui.014") => "Rotation",
        (Locale::Ja, "app-ui.015") => {
            "既定では2D編集画面だけに表示し、3D・印刷・動画には含めません"
        }
        (Locale::En, "app-ui.015") => {
            "By default this appears only in the 2D editor, not in 3D, print, or video"
        }
        (Locale::Ja, "app-ui.016") => "下敷きを削除",
        (Locale::En, "app-ui.016") => "Remove underlay",
        (Locale::Ja, "app-ui.017") => "直前の下敷き変更を元に戻す",
        (Locale::En, "app-ui.017") => "Undo last underlay change",
        (Locale::Ja, "app-ui.018") => "下敷き変更を元に戻しました",
        (Locale::En, "app-ui.018") => "Undid underlay change",
        (Locale::Ja, "app-ui.019") => "セット",
        (Locale::En, "app-ui.019") => "Set",
        (Locale::Ja, "app-ui.020") => "ワークスペース",
        (Locale::En, "app-ui.020") => "Workspace",
        (Locale::Ja, "app-ui.021") => "プラグイン管理…",
        (Locale::En, "app-ui.021") => "Plugin Manager…",
        (Locale::Ja, "app-ui.022") => "サブセット・スナップショット…",
        (Locale::En, "app-ui.022") => "Subsets & Snapshots…",
        (Locale::Ja, "app-ui.023") => "ベータ版も確認",
        (Locale::En, "app-ui.023") => "Include beta releases",
        (Locale::Ja, "app-ui.024") => "更新を確認",
        (Locale::En, "app-ui.024") => "Check for Updates",
        (Locale::Ja, "app-ui.025") => "保存済みの .drill.json を開きます",
        (Locale::En, "app-ui.025") => "Open a saved .drill.json file",
        (Locale::Ja, "app-ui.026") => "Ctrl/Cmd+S · バックアップも自動作成します",
        (Locale::En, "app-ui.026") => "Ctrl/Cmd+S · also creates a backup automatically",
        (Locale::Ja, "app-ui.027") => "範囲先頭",
        (Locale::En, "app-ui.027") => "Range Start",
        (Locale::Ja, "app-ui.028") => "設定した再生範囲の開始位置へ戻ります",
        (Locale::En, "app-ui.028") => "Return to the start of the playback range",
        (Locale::Ja, "app-ui.029") => "基準テンポ（Count 0）。テンポマップで途中変化も設定できます",
        (Locale::En, "app-ui.029") => "Base tempo at Count 0; use the tempo map for later changes",
        (Locale::Ja, "app-ui.030") => {
            "演者を1回のインスタンス描画で表示。無効時・デバイス異常時はCPU描画へ戻ります。"
        }
        (Locale::En, "app-ui.030") => {
            "Draw performers in one instanced pass; falls back to CPU if disabled or unavailable."
        }
        (Locale::Ja, "app-ui.031") => "ショット追従",
        (Locale::En, "app-ui.031") => "Follow Shot",
        (Locale::Ja, "app-ui.032") => "現在カウントの保存済みカメラキーフレームを評価します",
        (Locale::En, "app-ui.032") => "Evaluate saved camera keyframes at the current count",
        (Locale::Ja, "app-ui.033") => "観客席",
        (Locale::En, "app-ui.033") => "Audience",
        (Locale::Ja, "app-ui.034") => "プレス",
        (Locale::En, "app-ui.034") => "Press Box",
        (Locale::Ja, "app-ui.035") => "真上",
        (Locale::En, "app-ui.035") => "Overhead",
        (Locale::Ja, "app-ui.036") => "エンドゾーン",
        (Locale::En, "app-ui.036") => "End Zone",
        (Locale::Ja, "app-ui.037") => "＋キーフレーム",
        (Locale::En, "app-ui.037") => "+ Keyframe",
        (Locale::Ja, "app-ui.038") => "現在の自由視点を現在カウントへ保存（同じカウントは置換）",
        (Locale::En, "app-ui.038") => {
            "Save the free camera at this count (replaces an existing keyframe)"
        }
        (Locale::Ja, "app-ui.039") => "－キーフレーム",
        (Locale::En, "app-ui.039") => "− Keyframe",
        (Locale::Ja, "app-ui.040") => "補間:",
        (Locale::En, "app-ui.040") => "Interpolation:",
        (Locale::Ja, "app-ui.041") => "－カット",
        (Locale::En, "app-ui.041") => "− Cut",
        (Locale::Ja, "app-ui.042") => "＋カット",
        (Locale::En, "app-ui.042") => "+ Cut",
        (Locale::Ja, "app-ui.043") => "1  演者をクリック",
        (Locale::En, "app-ui.043") => "1  Click a performer",
        (Locale::Ja, "app-ui.044") => "2  Ctrl/Cmdで複数選択",
        (Locale::En, "app-ui.044") => "2  Ctrl/Cmd to multi-select",
        (Locale::Ja, "app-ui.045") => "3  ドラッグまたは配置ツール",
        (Locale::En, "app-ui.045") => "3  Drag or use formation tools",
        (Locale::Ja, "app-ui.046") => "再生中もカウント単位でシークできます",
        (Locale::En, "app-ui.046") => "You can seek to exact counts during playback",
        (Locale::Ja, "app-ui.047") => "今後表示しない",
        (Locale::En, "app-ui.047") => "Don't show again",
        (Locale::Ja, "app-ui.048") => "名称未設定",
        (Locale::En, "app-ui.048") => "Untitled",
        (Locale::Ja, "app-ui.049") => "再生範囲",
        (Locale::En, "app-ui.049") => "Playback Range",
        (Locale::Ja, "app-ui.050") => "現在のセット",
        (Locale::En, "app-ui.050") => "Current Set",
        (Locale::Ja, "app-ui.051") => "曲全体",
        (Locale::En, "app-ui.051") => "Whole Show",
        (Locale::Ja, "app-ui.052") => "現在位置を開始",
        (Locale::En, "app-ui.052") => "Set In",
        (Locale::Ja, "app-ui.053") => "現在位置を終了",
        (Locale::En, "app-ui.053") => "Set Out",
        (Locale::Ja, "app-ui.054") => "ループ",
        (Locale::En, "app-ui.054") => "Loop",
        (Locale::Ja, "app-ui.055") => "カウントトラック",
        (Locale::En, "app-ui.055") => "Count Track",
        (Locale::Ja, "app-ui.056") => "タイムラインを縮小",
        (Locale::En, "app-ui.056") => "Zoom timeline out",
        (Locale::Ja, "app-ui.057") => "縮小",
        (Locale::En, "app-ui.057") => "Zoom Out",
        (Locale::Ja, "app-ui.058") => "タイムラインを拡大",
        (Locale::En, "app-ui.058") => "Zoom timeline in",
        (Locale::Ja, "app-ui.059") => "拡大",
        (Locale::En, "app-ui.059") => "Zoom In",
        (Locale::Ja, "app-ui.060") => "全体表示",
        (Locale::En, "app-ui.060") => "Fit All",
        (Locale::Ja, "app-ui.061") => "再生範囲に合わせる",
        (Locale::En, "app-ui.061") => "Fit Range",
        (Locale::Ja, "app-ui.062") => "再生位置を追従",
        (Locale::En, "app-ui.062") => "Follow Playhead",
        (Locale::Ja, "app-ui.063") => "移動できません",
        (Locale::En, "app-ui.063") => "Could not move performers",
        (Locale::Ja, "inspector-media.001") => "7. 書き出し / 最適化",
        (Locale::En, "inspector-media.001") => "7. Export / Optimize",
        (Locale::Ja, "inspector-media.002") => "ドリルブックやコーチ用データを出力します",
        (Locale::En, "inspector-media.002") => "Export drill books and coaching data.",
        (Locale::Ja, "inspector-media.003") => "動画エンコード",
        (Locale::En, "inspector-media.003") => "Video Encoding",
        (Locale::Ja, "inspector-media.004") => "用途を選ぶだけの簡単設定",
        (Locale::En, "inspector-media.004") => "Choose a Purpose to Get Started",
        (Locale::Ja, "inspector-media.005") => {
            "あとで細かく調整できます。迷ったら「共有用」を選んでください。"
        }
        (Locale::En, "inspector-media.005") => "Fine-tune settings later. Choose Share if unsure.",
        (Locale::Ja, "inspector-media.006") => "確認用（高速・小容量）",
        (Locale::En, "inspector-media.006") => "Review (fast / small)",
        (Locale::Ja, "inspector-media.007") => "共有用（おすすめ）",
        (Locale::En, "inspector-media.007") => "Share (recommended)",
        (Locale::Ja, "inspector-media.008") => "本番上映用（高画質）",
        (Locale::En, "inspector-media.008") => "Presentation (high quality)",
        (Locale::Ja, "inspector-media.009") => "品質",
        (Locale::En, "inspector-media.009") => "Quality",
        (Locale::Ja, "inspector-media.010") => "音声を含める",
        (Locale::En, "inspector-media.010") => "Include Audio",
        (Locale::Ja, "inspector-media.011") => "詳細設定（コーデック・エンコーダー）",
        (Locale::En, "inspector-media.011") => "Advanced Settings (codec / encoder)",
        (Locale::Ja, "inspector-media.012") => "設定は有効です",
        (Locale::En, "inspector-media.012") => "Settings are valid",
        (Locale::Ja, "inspector-media.013") => "コンテナ",
        (Locale::En, "inspector-media.013") => "Container",
        (Locale::Ja, "inspector-media.014") => "映像コーデック",
        (Locale::En, "inspector-media.014") => "Video Codec",
        (Locale::Ja, "inspector-media.015") => "エンコーダー",
        (Locale::En, "inspector-media.015") => "Encoder",
        (Locale::Ja, "inspector-media.016") => "自動",
        (Locale::En, "inspector-media.016") => "Auto",
        (Locale::Ja, "inspector-media.017") => "固定品質",
        (Locale::En, "inspector-media.017") => "Constant Quality",
        (Locale::Ja, "inspector-media.018") => "ビットレート",
        (Locale::En, "inspector-media.018") => "Bitrate",
        (Locale::Ja, "inspector-media.019") => "Web再生を高速化 (faststart)",
        (Locale::En, "inspector-media.019") => "Optimize for Web Playback (faststart)",
        (Locale::Ja, "inspector-media.020") => "事前検査を更新",
        (Locale::En, "inspector-media.020") => "Refresh Preflight",
        (Locale::Ja, "inspector-media.021") => "書き出し前の事前検査",
        (Locale::En, "inspector-media.021") => "Run Export Preflight",
        (Locale::Ja, "inspector-media.022") => "FFmpegとエンコーダーを確認中…",
        (Locale::En, "inspector-media.022") => "Checking FFmpeg and encoders…",
        (Locale::Ja, "inspector-media.023") => "設定が変わりました。事前検査を更新してください",
        (Locale::En, "inspector-media.023") => "Settings changed; refresh the preflight.",
        (Locale::Ja, "inspector-media.024") => "✓ 事前検査OK",
        (Locale::En, "inspector-media.024") => "✓ Preflight Passed",
        (Locale::Ja, "inspector-media.025") => "! 解決が必要です",
        (Locale::En, "inspector-media.025") => "! Action Required",
        (Locale::Ja, "inspector-media.026") => "レンダリング・エンコード中",
        (Locale::En, "inspector-media.026") => "Rendering and Encoding",
        (Locale::Ja, "inspector-media.027") => "動画書き出しをキャンセル",
        (Locale::En, "inspector-media.027") => "Cancel Video Export",
        (Locale::Ja, "inspector-media.028") => "動画を書き出す…",
        (Locale::En, "inspector-media.028") => "Export Video…",
        (Locale::Ja, "inspector-media.029") => {
            "同名ファイルを上書きしますか？ この操作は元に戻せません。"
        }
        (Locale::En, "inspector-media.029") => {
            "Overwrite the file with the same name? This cannot be undone."
        }
        (Locale::Ja, "inspector-media.030") => "上書きして続行",
        (Locale::En, "inspector-media.030") => "Overwrite and Continue",
        (Locale::Ja, "inspector-media.031") => "キャンセル",
        (Locale::En, "inspector-media.031") => "Cancel",
        (Locale::Ja, "inspector-media.032") => "保存先を開く",
        (Locale::En, "inspector-media.032") => "Open Destination",
        (Locale::Ja, "inspector-media.033") => "SVG下敷きを解決できません",
        (Locale::En, "inspector-media.033") => "Could not resolve SVG underlay",
        (Locale::Ja, "inspector-media.034") => "プロダクションシート (TSV)",
        (Locale::En, "inspector-media.034") => "Production sheet (TSV)",
        (Locale::Ja, "inspector-media.035") => "8. 音源 / カウント",
        (Locale::En, "inspector-media.035") => "8. Audio / Count",
        (Locale::Ja, "inspector-media.036") => "拍子(分子)",
        (Locale::En, "inspector-media.036") => "Time Signature (numerator)",
        (Locale::Ja, "inspector-media.037") => "音源解析をキャンセル",
        (Locale::En, "inspector-media.037") => "Cancel Audio Analysis",
        (Locale::Ja, "inspector-media.038") => "クリック / カウントイン",
        (Locale::En, "inspector-media.038") => "Click / Count-in",
        (Locale::Ja, "inspector-media.039") => "クリックを再生",
        (Locale::En, "inspector-media.039") => "Play Click",
        (Locale::Ja, "inspector-media.040") => "カウントイン",
        (Locale::En, "inspector-media.040") => "Count-in",
        (Locale::Ja, "inspector-media.041") => "細分",
        (Locale::En, "inspector-media.041") => "Subdivision",
        (Locale::Ja, "inspector-media.042") => "クリック音量",
        (Locale::En, "inspector-media.042") => "Click Volume",
        (Locale::Ja, "inspector-media.043") => "アクセント音",
        (Locale::En, "inspector-media.043") => "Accent Tone",
        (Locale::Ja, "inspector-media.044") => "音源ダッキング",
        (Locale::En, "inspector-media.044") => "Audio Ducking",
        (Locale::Ja, "inspector-media.045") => {
            "クリック音は再生出力へ反映されます。設定変更時だけ再生成します。"
        }
        (Locale::En, "inspector-media.045") => {
            "The click is mixed into playback and rebuilt only when settings change."
        }
        (Locale::Ja, "inspector-media.046") => "欠落した音源を再リンク…",
        (Locale::En, "inspector-media.046") => "Relink Missing Audio…",
        (Locale::Ja, "inspector-media.047") => "開始オフセット",
        (Locale::En, "inspector-media.047") => "Start Offset",
        (Locale::Ja, "inspector-media.048") => "長さ",
        (Locale::En, "inspector-media.048") => "Duration",
        (Locale::Ja, "inspector-media.049") => "音量",
        (Locale::En, "inspector-media.049") => "Volume",
        (Locale::Ja, "inspector-media.050") => "ミュート",
        (Locale::En, "inspector-media.050") => "Mute",
        (Locale::Ja, "inspector-media.051") => "非破壊トリム / フェード",
        (Locale::En, "inspector-media.051") => "Non-destructive Trim / Fade",
        (Locale::Ja, "inspector-media.052") => "音源を外す",
        (Locale::En, "inspector-media.052") => "Remove Audio",
        (Locale::Ja, "inspector-media.053") => "参照音源を選択",
        (Locale::En, "inspector-media.053") => "Choose Reference Audio",
        (Locale::Ja, "inspector-media.054") => "音声",
        (Locale::En, "inspector-media.054") => "Audio",
        (Locale::Ja, "inspector-media.055") => "テキスト書き出し中",
        (Locale::En, "inspector-media.055") => "Exporting Text",
        (Locale::Ja, "legal-notices.001") => "ライセンス・第三者通知",
        (Locale::En, "legal-notices.001") => "Licenses & Third-party Notices",
        (Locale::Ja, "legal-notices.002") => "DrillForge本体と同梱コンポーネントのライセンスです。",
        (Locale::En, "legal-notices.002") => "Licenses for DrillForge and bundled components.",
        (Locale::Ja, "onboarding.001") => "DrillForgeへようこそ",
        (Locale::En, "onboarding.001") => "Welcome to DrillForge",
        (Locale::Ja, "onboarding.002") => {
            "隊形を作り、カウントに合わせて動きを確認できるマーチング・デザイン環境です。"
        }
        (Locale::En, "onboarding.002") => {
            "Create formations and preview movement precisely against musical counts."
        }
        (Locale::Ja, "onboarding.003") => "まずは3ステップ",
        (Locale::En, "onboarding.003") => "Start in three steps",
        (Locale::Ja, "onboarding.004") => {
            "1. セットを選ぶ  →  2. 演者を選んで動かす  →  3. 再生する"
        }
        (Locale::En, "onboarding.004") => {
            "1. Choose a set  →  2. Select and move performers  →  3. Play"
        }
        (Locale::Ja, "onboarding.005") => {
            "現在のサンプル隊形は自由に編集できます。操作はすべて元に戻せます。"
        }
        (Locale::En, "onboarding.005") => {
            "You can freely edit the sample formation. Every edit can be undone."
        }
        (Locale::Ja, "onboarding.006") => "サンプルで始める",
        (Locale::En, "onboarding.006") => "Start with Sample",
        (Locale::Ja, "onboarding.007") => "現在表示中のサンプルを編集します",
        (Locale::En, "onboarding.007") => "Edit the sample currently shown",
        (Locale::Ja, "onboarding.008") => "JSONを開く…",
        (Locale::En, "onboarding.008") => "Open JSON…",
        (Locale::Ja, "onboarding.009") => "従来形式のドリルJSONを開きます",
        (Locale::En, "onboarding.009") => "Open a legacy drill JSON file",
        (Locale::Ja, "onboarding.010") => ".drillprojを開く…",
        (Locale::En, "onboarding.010") => "Open .drillproj…",
        (Locale::Ja, "onboarding.011") => "音源を含められる推奨プロジェクト形式を開きます",
        (Locale::En, "onboarding.011") => {
            "Open the recommended project format, which can include audio"
        }
        (Locale::Ja, "onboarding.012") => {
            "あとから「ヘルプ > はじめかた」でいつでも再表示できます。"
        }
        (Locale::En, "onboarding.012") => {
            "You can show this again later from Help > Getting Started."
        }
        (Locale::Ja, "onboarding.013") => "はじめかた・操作ガイド",
        (Locale::En, "onboarding.013") => "Getting Started & Controls",
        (Locale::Ja, "onboarding.014") => "基本ワークフロー",
        (Locale::En, "onboarding.014") => "Basic workflow",
        (Locale::Ja, "onboarding.015") => {
            "① 左のセットを選択  ② フィールド上の演者を選択  ③ ドラッグで移動  ④ 再生で確認"
        }
        (Locale::En, "onboarding.015") => {
            "1. Choose a set  2. Select performers on the field  3. Drag to move  4. Play to review"
        }
        (Locale::Ja, "onboarding.016") => "選択と編集",
        (Locale::En, "onboarding.016") => "Selection and editing",
        (Locale::Ja, "onboarding.017") => "クリック: 1人選択　Ctrl/Cmd+クリック: 追加選択",
        (Locale::En, "onboarding.017") => "Click: select one   Ctrl/Cmd+click: add to selection",
        (Locale::Ja, "onboarding.018") => "空白からドラッグ: 範囲選択　演者をドラッグ: 隊形を移動",
        (Locale::En, "onboarding.018") => {
            "Drag from empty space: box-select   Drag a performer: move formation"
        }
        (Locale::Ja, "onboarding.019") => "再生とタイムライン",
        (Locale::En, "onboarding.019") => "Playback and timeline",
        (Locale::Ja, "onboarding.020") => "Space: 再生/一時停止　I: 範囲開始　O: 範囲終了",
        (Locale::En, "onboarding.020") => "Space: play/pause   I: range start   O: range end",
        (Locale::Ja, "onboarding.021") => "タイムラインをクリック/ドラッグ: 1カウント単位でシーク",
        (Locale::En, "onboarding.021") => "Click or drag the timeline: seek in exact whole counts",
        (Locale::Ja, "onboarding.022") => "保存",
        (Locale::En, "onboarding.022") => "Saving",
        (Locale::Ja, "onboarding.023") => {
            "Ctrl/Cmd+S: 保存　.drillproj: 音源もまとめられる推奨形式"
        }
        (Locale::En, "onboarding.023") => {
            "Ctrl/Cmd+S: save   .drillproj: recommended format that can bundle audio"
        }
        (Locale::Ja, "onboarding.024") => {
            "Tab / Shift+Tab: 操作項目間を移動　Enter / Space: 実行　Esc: 閉じる"
        }
        (Locale::En, "onboarding.024") => {
            "Tab / Shift+Tab: move focus   Enter / Space: activate   Esc: close"
        }
        (Locale::Ja, "plugin-state.001") => "プラグイン管理",
        (Locale::En, "plugin-state.001") => "Plugin Manager",
        (Locale::Ja, "plugin-state.002") => {
            "プラグインは必ず隔離プロセスで実行されます。DLLをアプリ内へ読み込むことはありません。"
        }
        (Locale::En, "plugin-state.002") => {
            "Plugins always run in disposable isolated processes. DLLs are never loaded in-process."
        }
        (Locale::Ja, "plugin-state.003") => "Manifestと実行ファイルを追加…",
        (Locale::En, "plugin-state.003") => "Add manifest and executable…",
        (Locale::Ja, "plugin-state.004") => "インストール済み",
        (Locale::En, "plugin-state.004") => "Installed",
        (Locale::Ja, "plugin-state.005") => "信頼と能力",
        (Locale::En, "plugin-state.005") => "Trust & capabilities",
        (Locale::Ja, "plugin-state.006") => "プラグインを選択してください",
        (Locale::En, "plugin-state.006") => "Select a plugin",
        (Locale::Ja, "plugin-state.007") => "この署名済み発行元を明示的に信頼する",
        (Locale::En, "plugin-state.007") => "Explicitly trust this signed publisher",
        (Locale::Ja, "plugin-state.008") => "実行プレビュー（送信されるbounded JSON）",
        (Locale::En, "plugin-state.008") => "Execution preview (bounded JSON sent to child)",
        (Locale::Ja, "plugin-state.009") => "hash・署名済み発行元・全要求能力の承認が必要です",
        (Locale::En, "plugin-state.009") => {
            "Requires matching hash, trusted signed publisher, and all requested capabilities"
        }
        (Locale::Ja, "plugin-state.010") => "実行前チェックを確認",
        (Locale::En, "plugin-state.010") => "Confirm execution preview",
        (Locale::Ja, "plugin-state.011") => {
            "実行条件を確認しました。実行時にもhash・署名・能力を再検証します。"
        }
        (Locale::En, "plugin-state.011") => {
            "Execution conditions confirmed. Hash, signature, and capabilities will be revalidated at run time."
        }
        (Locale::Ja, "section-manager.001") => "セクション管理",
        (Locale::En, "section-manager.001") => "Section Manager",
        (Locale::Ja, "section-manager.002") => {
            "パートの色と略称を管理し、選択中の演者をまとめて割り当てます。"
        }
        (Locale::En, "section-manager.002") => {
            "Manage section colors and abbreviations, and assign selected performers in one step."
        }
        (Locale::Ja, "section-manager.003") => "すべての変更は Undo / Redo できます。",
        (Locale::En, "section-manager.003") => "Every change supports Undo / Redo.",
        (Locale::Ja, "section-manager.004") => "名称",
        (Locale::En, "section-manager.004") => "Name",
        (Locale::Ja, "section-manager.005") => "略称",
        (Locale::En, "section-manager.005") => "Short",
        (Locale::Ja, "section-manager.006") => "名称を保存",
        (Locale::En, "section-manager.006") => "Save name",
        (Locale::Ja, "section-manager.007") => "選択演者を割り当て",
        (Locale::En, "section-manager.007") => "Assign selection",
        (Locale::Ja, "section-manager.008") => "削除して他パートへ移動",
        (Locale::En, "section-manager.008") => "Delete and reassign",
        (Locale::Ja, "section-manager.009") => "所属演者は一覧の別セクションへ移動します",
        (Locale::En, "section-manager.009") => "Members move to another section",
        (Locale::Ja, "section-manager.010") => "新しいセクション",
        (Locale::En, "section-manager.010") => "New section",
        (Locale::Ja, "section-manager.011") => "例: カラーガード",
        (Locale::En, "section-manager.011") => "e.g. Color Guard",
        (Locale::Ja, "section-manager.012") => "＋ 追加",
        (Locale::En, "section-manager.012") => "+ Add",
        (Locale::Ja, "stadium-inspector.001") => "観客視点の遮蔽を診断",
        (Locale::En, "stadium-inspector.001") => "Audience occlusion",
        (Locale::Ja, "stadium-inspector.002") => "再解析",
        (Locale::En, "stadium-inspector.002") => "Re-analyze",
        (Locale::Ja, "stadium-inspector.003") => "照明",
        (Locale::En, "stadium-inspector.003") => "Lighting",
        (Locale::Ja, "stadium-inspector.004") => {
            "赤い輪ほど、このカメラ位置から他の演者に隠れています。カメラを動かした後は再解析してください。"
        }
        (Locale::En, "stadium-inspector.004") => {
            "Red rings identify performers hidden by others from this camera. Re-analyze after moving it."
        }
        (Locale::Ja, "stadium-inspector.005") => "現在のセット・拍・カメラで解析済み",
        (Locale::En, "stadium-inspector.005") => "Current set, count, and camera analyzed",
        (Locale::Ja, "stadium-inspector.006") => "表示状態が変わったため、結果を更新中",
        (Locale::En, "stadium-inspector.006") => "View changed; updating results",
        (Locale::Ja, "stadium-inspector.007") => "視認性の問題を巡回",
        (Locale::En, "stadium-inspector.007") => "Review visibility issues",
        (Locale::Ja, "stadium-inspector.008") => "人を選択",
        (Locale::En, "stadium-inspector.008") => "Select performers",
        (Locale::Ja, "stadium-inspector.009") => "完全遮蔽に近い",
        (Locale::En, "stadium-inspector.009") => "Nearly hidden",
        (Locale::Ja, "stadium-inspector.010") => "見えにくい全員",
        (Locale::En, "stadium-inspector.010") => "All impaired",
        (Locale::Ja, "stadium-inspector.011") => "前へ",
        (Locale::En, "stadium-inspector.011") => "Previous",
        (Locale::Ja, "stadium-inspector.012") => "次へ",
        (Locale::En, "stadium-inspector.012") => "Next",
        (Locale::Ja, "stadium-inspector.013") => {
            "ロックまたは非表示の演者は巡回対象から除外されます。"
        }
        (Locale::En, "stadium-inspector.013") => {
            "Locked or hidden performers are excluded from review."
        }
        (Locale::Ja, "subset-snapshot-state.001") => "スナップショットを復元",
        (Locale::En, "subset-snapshot-state.001") => "Restore snapshot",
        (Locale::Ja, "subset-snapshot-state.002") => {
            "現在の未保存変更は置き換わります。復元後もUndoできます。"
        }
        (Locale::En, "subset-snapshot-state.002") => {
            "Current unsaved changes will be replaced. You can Undo afterward."
        }
        (Locale::Ja, "subset-snapshot-state.003") => "復元する",
        (Locale::En, "subset-snapshot-state.003") => "Restore",
        (Locale::Ja, "subset-snapshot-state.004") => "キャンセル",
        (Locale::En, "subset-snapshot-state.004") => "Cancel",
        (Locale::Ja, "subset-snapshot-state.005") => "変更セットへ移動",
        (Locale::En, "subset-snapshot-state.005") => "Go to changed set",
        (Locale::Ja, "subset-snapshot-state.006") => {
            "現在の原稿に存在する最初の変更セットを開きます（保存・Undoには影響しません）"
        }
        (Locale::En, "subset-snapshot-state.006") => {
            "Open the first changed set present in this document (does not affect save or Undo)"
        }
        (Locale::Ja, "subset-snapshot-state.007") => "変更演者を選択",
        (Locale::En, "subset-snapshot-state.007") => "Select changed performers",
        (Locale::Ja, "subset-snapshot-state.008") => {
            "現在の原稿に残る変更演者だけを選択します（保存・Undoには影響しません）"
        }
        (Locale::En, "subset-snapshot-state.008") => {
            "Select only changed performers still in this document (does not affect save or Undo)"
        }
        (Locale::Ja, "subset-snapshot-state.009") => "変更演者を選択",
        (Locale::En, "subset-snapshot-state.009") => "Selected changed performers",
        (Locale::Ja, "workspace-inspector.001") => "1. セットを選ぶ",
        (Locale::En, "workspace-inspector.001") => "1. Choose a Set",
        (Locale::Ja, "workspace-inspector.002") => {
            "各セットは停止位置、countsは次セットまでの拍数です"
        }
        (Locale::En, "workspace-inspector.002") => {
            "Each set is a stopping point; counts is the duration to the next set."
        }
        (Locale::Ja, "workspace-inspector.003") => "＋ セットを複製",
        (Locale::En, "workspace-inspector.003") => "+ Duplicate Set",
        (Locale::Ja, "workspace-inspector.004") => "プロダクション情報",
        (Locale::En, "workspace-inspector.004") => "Production notes",
        (Locale::Ja, "workspace-inspector.005") => {
            "空欄の数値はテンポマップから自動計算します。編集はUndo/Redoできます。"
        }
        (Locale::En, "workspace-inspector.005") => {
            "Blank numeric fields follow the tempo map. Changes support Undo/Redo."
        }
        (Locale::Ja, "workspace-inspector.006") => "タイトル",
        (Locale::En, "workspace-inspector.006") => "Title",
        (Locale::Ja, "workspace-inspector.007") => "リハーサルマーク",
        (Locale::En, "workspace-inspector.007") => "Rehearsal mark",
        (Locale::Ja, "workspace-inspector.008") => "備考／キュー",
        (Locale::En, "workspace-inspector.008") => "Notes / cues",
        (Locale::Ja, "workspace-inspector.009") => "テンポを指定",
        (Locale::En, "workspace-inspector.009") => "Override tempo",
        (Locale::Ja, "workspace-inspector.010") => "同期時刻を指定",
        (Locale::En, "workspace-inspector.010") => "Override sync time",
        (Locale::Ja, "workspace-inspector.011") => "トランジション尺を指定",
        (Locale::En, "workspace-inspector.011") => "Override transition duration",
        (Locale::Ja, "workspace-inspector.012") => "プロダクション情報を更新できません",
        (Locale::En, "workspace-inspector.012") => "Could not update production notes",
        (Locale::Ja, "workspace-inspector.013") => "2. 演者を選ぶ",
        (Locale::En, "workspace-inspector.013") => "2. Select Performers",
        (Locale::Ja, "workspace-inspector.014") => "クリック／空白から囲む／Ctrl・Cmdで追加選択",
        (Locale::En, "workspace-inspector.014") => {
            "Click, drag a marquee from empty space, or Ctrl/Cmd-click to add."
        }
        (Locale::Ja, "workspace-inspector.015") => "演者がまだいません",
        (Locale::En, "workspace-inspector.015") => "No Performers Yet",
        (Locale::Ja, "workspace-inspector.016") => "すぐ試せる80人のサンプル隊形を作成できます。",
        (Locale::En, "workspace-inspector.016") => {
            "Create an 80-performer sample formation to try the editor."
        }
        (Locale::Ja, "workspace-inspector.017") => "サンプル隊形を作成",
        (Locale::En, "workspace-inspector.017") => "Create Sample Formation",
        (Locale::Ja, "workspace-inspector.018") => "サンプルを作成できません",
        (Locale::En, "workspace-inspector.018") => "Could not create sample",
        (Locale::Ja, "workspace-inspector.019") => "サンプル隊形を作成しました",
        (Locale::En, "workspace-inspector.019") => "Sample formation created",
        (Locale::Ja, "workspace-inspector.020") => "全員選択",
        (Locale::En, "workspace-inspector.020") => "Select All",
        (Locale::Ja, "workspace-inspector.021") => "選択解除",
        (Locale::En, "workspace-inspector.021") => "Clear Selection",
        (Locale::Ja, "workspace-inspector.022") => "フォーメーションデザイナー",
        (Locale::En, "workspace-inspector.022") => "Formation Designer",
        (Locale::Ja, "workspace-inspector.023") => {
            "選択中の演者へ弧長等間隔でプレビュー生成し、クリックで適用します。適用は1回で元に戻せます。"
        }
        (Locale::En, "workspace-inspector.023") => {
            "Builds an arc-length preview for the selection; click to apply as one undoable edit."
        }
        (Locale::Ja, "workspace-inspector.024") => "楕円をプレビュー",
        (Locale::En, "workspace-inspector.024") => "Preview ellipse",
        (Locale::Ja, "workspace-inspector.025") => "自由描画を開始",
        (Locale::En, "workspace-inspector.025") => "Start free drawing",
        (Locale::Ja, "workspace-inspector.026") => "プレビューを適用",
        (Locale::En, "workspace-inspector.026") => "Apply preview",
        (Locale::Ja, "workspace-inspector.027") => "破棄",
        (Locale::En, "workspace-inspector.027") => "Discard",
        (Locale::Ja, "workspace-inspector.028") => "文字隊形",
        (Locale::En, "workspace-inspector.028") => "Formation text",
        (Locale::Ja, "workspace-inspector.029") => "日本語・英数字（1行）",
        (Locale::En, "workspace-inspector.029") => "Japanese / Latin, one line",
        (Locale::Ja, "workspace-inspector.030") => "文字をプレビュー",
        (Locale::En, "workspace-inspector.030") => "Preview text",
        (Locale::Ja, "workspace-inspector.031") => "描画中: フィールド上をドラッグ",
        (Locale::En, "workspace-inspector.031") => "Drawing: drag on the field",
        (Locale::Ja, "workspace-inspector.032") => "楕円を適用",
        (Locale::En, "workspace-inspector.032") => "Apply ellipse",
        (Locale::Ja, "workspace-inspector.033") => "放物線を適用",
        (Locale::En, "workspace-inspector.033") => "Apply parabola",
        (Locale::Ja, "workspace-inspector.034") => "波形を適用",
        (Locale::En, "workspace-inspector.034") => "Apply sine wave",
        (Locale::Ja, "workspace-inspector.035") => "星を適用",
        (Locale::En, "workspace-inspector.035") => "Apply star",
        (Locale::Ja, "workspace-inspector.036") => "六角形を適用",
        (Locale::En, "workspace-inspector.036") => "Apply hexagon",
        (Locale::Ja, "workspace-inspector.037") => "十字を適用",
        (Locale::En, "workspace-inspector.037") => "Apply cross",
        (Locale::Ja, "workspace-inspector.038") => "直線フィット",
        (Locale::En, "workspace-inspector.038") => "Fit line",
        (Locale::Ja, "workspace-inspector.039") => "円フィット",
        (Locale::En, "workspace-inspector.039") => "Fit circle",
        (Locale::Ja, "workspace-inspector.040") => {
            "構成操作（結果を確認後、Ctrl+Zで完全に戻せます）"
        }
        (Locale::En, "workspace-inspector.040") => {
            "Composition tools (inspect the result; Ctrl+Z restores it completely)"
        }
        (Locale::Ja, "workspace-inspector.041") => "次セットへ25%モーフ",
        (Locale::En, "workspace-inspector.041") => "Morph 25% to next set",
        (Locale::Ja, "workspace-inspector.042") => "次セットへ50%モーフ",
        (Locale::En, "workspace-inspector.042") => "Morph 50% to next set",
        (Locale::Ja, "workspace-inspector.043") => "2方向対称",
        (Locale::En, "workspace-inspector.043") => "2-fold symmetry",
        (Locale::Ja, "workspace-inspector.044") => "4方向対称",
        (Locale::En, "workspace-inspector.044") => "4-fold symmetry",
        (Locale::Ja, "workspace-inspector.045") => "セクションを連続配置",
        (Locale::En, "workspace-inspector.045") => "Keep sections contiguous",
        (Locale::Ja, "workspace-inspector.046") => "制約クリニック案を適用",
        (Locale::En, "workspace-inspector.046") => "Apply constraint clinic proposal",
        (Locale::Ja, "workspace-inspector.047") => {
            "フィールド境界・演者間隔・前セットからの最大歩幅・保存中の図形を同時に満たす提案。1回のUndoで戻せます。"
        }
        (Locale::En, "workspace-inspector.047") => {
            "Proposes field bounds, performer spacing, maximum step and stored-shape adherence together. One Undo restores it."
        }
        (Locale::Ja, "workspace-inspector.048") => "保存中の図形",
        (Locale::En, "workspace-inspector.048") => "Stored shape",
        (Locale::Ja, "workspace-inspector.049") => "次セットへの動きをリアルタイム検査",
        (Locale::En, "workspace-inspector.049") => "Inspect movement to the next set in real time",
        (Locale::Ja, "workspace-inspector.050") => "改善案を生成",
        (Locale::En, "workspace-inspector.050") => "Generate fixes",
        (Locale::Ja, "workspace-inspector.051") => {
            "元の設計は変更せず、衝突・歩幅・到着ターンを改善する候補を最大8件比較します"
        }
        (Locale::En, "workspace-inspector.051") => {
            "Compare up to eight collision, stride, and arrival-turn fixes without changing the design"
        }
        (Locale::Ja, "workspace-inspector.052") => "有効な改善案はありません",
        (Locale::En, "workspace-inspector.052") => "No improving fix found",
        (Locale::Ja, "workspace-inspector.053") => {
            "改善案を生成しました。比較してから適用してください"
        }
        (Locale::En, "workspace-inspector.053") => "Fixes generated. Review before applying",
        (Locale::Ja, "workspace-inspector.054") => "選択案を適用",
        (Locale::En, "workspace-inspector.054") => "Apply selected fix",
        (Locale::Ja, "workspace-inspector.055") => "改善案を適用できません",
        (Locale::En, "workspace-inspector.055") => "Could not apply fix",
        (Locale::Ja, "workspace-inspector.056") => "改善案を適用しました（元に戻す可）",
        (Locale::En, "workspace-inspector.056") => "Fix applied (undo available)",
        (Locale::Ja, "workspace-inspector.057") => {
            "設計が変更されたため、改善案を再生成してください"
        }
        (Locale::En, "workspace-inspector.057") => "The design changed; regenerate fixes",
        (Locale::Ja, "workspace-inspector.058") => "5. グリッドデザイナー",
        (Locale::En, "workspace-inspector.058") => "5. Grid Designer",
        (Locale::Ja, "workspace-inspector.059") => "プリセット",
        (Locale::En, "workspace-inspector.059") => "Preset",
        (Locale::Ja, "workspace-inspector.060") => "フィールド寸法",
        (Locale::En, "workspace-inspector.060") => "Field Dimensions",
        (Locale::Ja, "workspace-inspector.061") => "左右: steps / units",
        (Locale::En, "workspace-inspector.061") => "Width: steps / units",
        (Locale::Ja, "workspace-inspector.062") => "上下: steps / units",
        (Locale::En, "workspace-inspector.062") => "Height: steps / units",
        (Locale::Ja, "workspace-inspector.063") => "主線間隔",
        (Locale::En, "workspace-inspector.063") => "Major Line Spacing",
        (Locale::Ja, "workspace-inspector.064") => "分割",
        (Locale::En, "workspace-inspector.064") => "Divisions",
        (Locale::Ja, "workspace-inspector.065") => "線",
        (Locale::En, "workspace-inspector.065") => "Lines",
        (Locale::Ja, "workspace-inspector.066") => "点",
        (Locale::En, "workspace-inspector.066") => "Dots",
        (Locale::Ja, "workspace-inspector.067") => "ステップグリッド表示",
        (Locale::En, "workspace-inspector.067") => "Show Step Grid",
        (Locale::Ja, "workspace-inspector.068") => "グリッドへスナップ",
        (Locale::En, "workspace-inspector.068") => "Snap to Grid",
        (Locale::Ja, "workspace-inspector.069") => "ハッシュ位置",
        (Locale::En, "workspace-inspector.069") => "Hash Positions",
        (Locale::Ja, "workspace-inspector.070") => "＋ ハッシュ/区切り線",
        (Locale::En, "workspace-inspector.070") => "+ Hash / Divider Line",
        (Locale::Ja, "workspace-inspector.071") => "6. テンポマップ",
        (Locale::En, "workspace-inspector.071") => "6. Tempo Map",
        (Locale::Ja, "workspace-inspector.072") => "カウント位置ごとにBPMを変化させられます",
        (Locale::En, "workspace-inspector.072") => "Change BPM at any count position",
        (Locale::Ja, "workspace-inspector.073") => "＋ 現在位置にテンポ変化を追加",
        (Locale::En, "workspace-inspector.073") => "+ Add Tempo Change Here",
        (Locale::Ja, "workspace-inspector.074") => "同期残差 最大",
        (Locale::En, "workspace-inspector.074") => "Maximum sync residual",
        (Locale::Ja, "workspace-inspector.075") => {
            "波形をクリック: アンカー追加 · ドラッグ: 移動 · Alt: 自由位置 · 右クリック: 削除"
        }
        (Locale::En, "workspace-inspector.075") => {
            "Click waveform: add anchor · Drag: move · Alt: free position · Right-click: remove"
        }
        (Locale::Ja, "workspace-inspector.076") => "セクション管理…",
        (Locale::En, "workspace-inspector.076") => "Manage Sections…",
        (Locale::Ja, "workspace-inspector.077") => "パート追加・名称変更・選択演者の割当・削除",
        (Locale::En, "workspace-inspector.077") => {
            "Add, rename, assign selected performers, or remove sections"
        }
        (Locale::Ja, "workspace-inspector.078") => {
            "編集するにはセット境界（Count 0）へ移動してください"
        }
        (Locale::En, "workspace-inspector.078") => "Move to a set boundary (Count 0) to edit",
        (Locale::Ja, "workspace-inspector.079") => "フォーメーション",
        (Locale::En, "workspace-inspector.079") => "Formation",
        (Locale::Ja, "workspace-inspector.080") => "横一列",
        (Locale::En, "workspace-inspector.080") => "Horizontal line",
        (Locale::Ja, "workspace-inspector.081") => "縦一列",
        (Locale::En, "workspace-inspector.081") => "Vertical line",
        (Locale::Ja, "workspace-inspector.082") => "斜線",
        (Locale::En, "workspace-inspector.082") => "Diagonal",
        (Locale::Ja, "workspace-inspector.083") => "円弧",
        (Locale::En, "workspace-inspector.083") => "Arc",
        (Locale::Ja, "workspace-inspector.084") => "円",
        (Locale::En, "workspace-inspector.084") => "Circle",
        (Locale::Ja, "workspace-inspector.085") => "ブロック",
        (Locale::En, "workspace-inspector.085") => "Block",
        (Locale::Ja, "workspace-inspector.086") => "螺旋",
        (Locale::En, "workspace-inspector.086") => "Spiral",
        (Locale::Ja, "workspace-inspector.087") => "回転・サイズ変更",
        (Locale::En, "workspace-inspector.087") => "Rotate and Resize",
        (Locale::Ja, "workspace-inspector.088") => "整列・分配・反転",
        (Locale::En, "workspace-inspector.088") => "Align, Distribute, and Flip",
        (Locale::Ja, "workspace-inspector.089") => "横整列",
        (Locale::En, "workspace-inspector.089") => "Align horizontally",
        (Locale::Ja, "workspace-inspector.090") => "縦整列",
        (Locale::En, "workspace-inspector.090") => "Align vertically",
        (Locale::Ja, "workspace-inspector.091") => "横等間隔",
        (Locale::En, "workspace-inspector.091") => "Distribute horizontally",
        (Locale::Ja, "workspace-inspector.092") => "縦等間隔",
        (Locale::En, "workspace-inspector.092") => "Distribute vertically",
        (Locale::Ja, "workspace-inspector.093") => "左右反転",
        (Locale::En, "workspace-inspector.093") => "Flip horizontally",
        (Locale::Ja, "workspace-inspector.094") => "前後反転",
        (Locale::En, "workspace-inspector.094") => "Flip vertically",
        (Locale::Ja, "workspace-inspector.095") => "4. ライブクリニック",
        (Locale::En, "workspace-inspector.095") => "4. LIVE CLINIC",
        (Locale::Ja, "workspace-inspector.096") => "フットボール",
        (Locale::En, "workspace-inspector.096") => "Football",
        (Locale::Ja, "workspace-inspector.097") => "屋内",
        (Locale::En, "workspace-inspector.097") => "Indoor",
        (Locale::Ja, "workspace-inspector.098") => "サッカー",
        (Locale::En, "workspace-inspector.098") => "Soccer",
        (Locale::Ja, "workspace-inspector.127") => "日本の大会規格 (30m四方)",
        (Locale::En, "workspace-inspector.127") => "Japan Floor (30m square)",
        (Locale::Ja, "workspace-inspector.128") => "パレード隊形をプレビュー",
        (Locale::En, "workspace-inspector.128") => "Preview parade formation",
        (Locale::Ja, "workspace-inspector.129") => "Uターンをプレビュー",
        (Locale::En, "workspace-inspector.129") => "Preview U-turn",
        (Locale::Ja, "workspace-inspector.099") => "ヤード",
        (Locale::En, "workspace-inspector.099") => "yards",
        (Locale::Ja, "workspace-inspector.100") => "メートル",
        (Locale::En, "workspace-inspector.100") => "meters",
        (Locale::Ja, "workspace-inspector.101") => "カウント",
        (Locale::En, "workspace-inspector.101") => "Count",
        (Locale::Ja, "workspace-inspector.102") => {
            "同期アンカーが前後関係と矛盾するため変更できません"
        }
        (Locale::En, "workspace-inspector.102") => {
            "The sync anchor conflicts with adjacent anchors and cannot be changed"
        }
        (Locale::Ja, "workspace-inspector.103") => "3. 演者編集（演者を選択してください）",
        (Locale::En, "workspace-inspector.103") => "3. Edit Performers (select performers first)",
        (Locale::Ja, "workspace-inspector.104") => "3. 選択中:",
        (Locale::En, "workspace-inspector.104") => "3. Selected:",
        (Locale::Ja, "workspace-inspector.105") => "の座標",
        (Locale::En, "workspace-inspector.105") => "coordinates",
        (Locale::Ja, "workspace-inspector.106") => "衝突候補",
        (Locale::En, "workspace-inspector.106") => "Potential collisions",
        (Locale::Ja, "workspace-inspector.107") => "過大歩幅",
        (Locale::En, "workspace-inspector.107") => "Excessive strides",
        (Locale::Ja, "workspace-inspector.111") => "楕円をプレビュー",
        (Locale::En, "workspace-inspector.111") => "Preview ellipse",
        (Locale::Ja, "workspace-inspector.112") => "放物線をプレビュー",
        (Locale::En, "workspace-inspector.112") => "Preview parabola",
        (Locale::Ja, "workspace-inspector.113") => "波形をプレビュー",
        (Locale::En, "workspace-inspector.113") => "Preview sine wave",
        (Locale::Ja, "workspace-inspector.114") => "星をプレビュー",
        (Locale::En, "workspace-inspector.114") => "Preview star",
        (Locale::Ja, "workspace-inspector.115") => "六角形をプレビュー",
        (Locale::En, "workspace-inspector.115") => "Preview hexagon",
        (Locale::Ja, "workspace-inspector.116") => "十字をプレビュー",
        (Locale::En, "workspace-inspector.116") => "Preview cross",
        (Locale::Ja, "workspace-inspector.117") => "形から始める",
        (Locale::En, "workspace-inspector.117") => "Start with a form",
        (Locale::Ja, "workspace-inspector.118") => {
            "選択せずに形を描き、あとから全員へ割り当てられます。"
        }
        (Locale::En, "workspace-inspector.118") => {
            "Draw a form first, then assign the full cast when you apply it."
        }
        (Locale::Ja, "workspace-inspector.119") => "全員の形を描く",
        (Locale::En, "workspace-inspector.119") => "Draw a form for everyone",
        (Locale::Ja, "workspace-inspector.120") => {
            "全員を一時的に対象にし、フィールド上で形を描きます。適用するまで元の隊形は変わりません。"
        }
        (Locale::En, "workspace-inspector.120") => {
            "Temporarily targets the full cast and lets you draw on the field. Your drill changes only when you apply the preview."
        }
        (Locale::Ja, "workspace-inspector.121") => "直前の選択を復元",
        (Locale::En, "workspace-inspector.121") => "Restore Previous Selection",
        (Locale::Ja, "workspace-inspector.122") => {
            "直前に選んでいた演者グループへ戻ります。配置やUndo履歴は変更しません。"
        }
        (Locale::En, "workspace-inspector.122") => {
            "Returns to the previous performer group without changing the drill or its undo history."
        }
        (Locale::Ja, "workspace-inspector.123") => "選択履歴",
        (Locale::En, "workspace-inspector.123") => "Recent Groups",
        (Locale::Ja, "workspace-inspector.124") => {
            "以前の演者グループを選びます。現在の選択も履歴として残ります。"
        }
        (Locale::En, "workspace-inspector.124") => {
            "Choose an earlier working group. Your current selection stays in the session history."
        }
        (Locale::Ja, "workspace-inspector.125") => "選択した演者を反時計回りに15°回転します",
        (Locale::En, "workspace-inspector.125") => "Rotate the selected performers 15° counter-clockwise",
        (Locale::Ja, "workspace-inspector.126") => "選択した演者を時計回りに15°回転します",
        (Locale::En, "workspace-inspector.126") => "Rotate the selected performers 15° clockwise",
        (Locale::Ja, "workspace-inspector.128") => "フォローザリーダー",
        (Locale::En, "workspace-inspector.128") => "Follow the Leader",
        (Locale::Ja, "workspace-inspector.129") => {
            "選択した演者(2名以上)が同じ経路を順番に追いかけるように、中間セットを自動生成します。生成後は他のセットと同じように個々のドットを自由に編集できます。"
        }
        (Locale::En, "workspace-inspector.129") => {
            "Generates intermediate sets so the selected group (2+) traces the same path one after another, like a snake maneuver. Every generated set can be hand-edited afterward just like any other set."
        }
        (Locale::Ja, "workspace-inspector.130") => "経路を描く",
        (Locale::En, "workspace-inspector.130") => "Draw Path",
        (Locale::Ja, "workspace-inspector.131") => {
            "選択した演者がなぞる経路をドラッグして描きます"
        }
        (Locale::En, "workspace-inspector.131") => {
            "Drag to draw the path the selected group will follow"
        }
        (Locale::Ja, "workspace-inspector.132") => "フォロー",
        (Locale::En, "workspace-inspector.132") => "Follow",
        (Locale::Ja, "workspace-inspector.133") => {
            "フォローザリーダーには2名以上の選択が必要です"
        }
        (Locale::En, "workspace-inspector.133") => {
            "Follow the Leader needs at least two selected performers"
        }
        (Locale::Ja, "workspace-inspector.134") => "セットIDを割り当てられませんでした",
        (Locale::En, "workspace-inspector.134") => "Couldn't allocate a new set ID",
        (Locale::Ja, "workspace-inspector.135") => "フォローザリーダーの適用に失敗しました",
        (Locale::En, "workspace-inspector.135") => "Failed to apply Follow the Leader",
        (Locale::Ja, "workspace-inspector.136") => {
            "フォローザリーダーの中間セットを追加しました"
        }
        (Locale::En, "workspace-inspector.136") => {
            "Added Follow the Leader intermediate sets"
        }
        (Locale::Ja, "workspace-inspector.137") => "経路が無効です",
        (Locale::En, "workspace-inspector.137") => "Invalid path",
        (Locale::Ja, "workspace-inspector.138") => "中間セット数",
        (Locale::En, "workspace-inspector.138") => "Intermediate sets",
        (Locale::Ja, "workspace-inspector.139") => "生成する中間セットの数",
        (Locale::En, "workspace-inspector.139") => "Number of intermediate sets to generate",
        (Locale::Ja, "workspace-inspector.140") => "フォローザリーダーを適用",
        (Locale::En, "workspace-inspector.140") => "Apply Follow the Leader",
        (Locale::Ja, "workspace-inspector.141") => "先に経路を描いてください",
        (Locale::En, "workspace-inspector.141") => "Draw a path first",
        (Locale::Ja, "workspace-inspector.142") => "直線を使用",
        (Locale::En, "workspace-inspector.142") => "Use a Straight Line",
        (Locale::Ja, "workspace-inspector.143") => {
            "選択範囲の対角線を経路として使用します"
        }
        (Locale::En, "workspace-inspector.143") => {
            "Uses the selection's bounding diagonal as the path"
        }
        (Locale::Ja, "export-status.016") => {
            "先に事前検査を実行し、表示された問題を解決してください"
        }
        (Locale::En, "export-status.016") => "Run preflight first and resolve the reported issues",
        (Locale::Ja, "export-status.017") => "設定エラー: {0}",
        (Locale::En, "export-status.017") => "Settings error: {0}",
        (Locale::Ja, "export-status.018") => "再生範囲が空です",
        (Locale::En, "export-status.018") => "Playback range is empty",
        (Locale::Ja, "audio-status.001") => "音源未読込",
        (Locale::En, "audio-status.001") => "No audio loaded",
        (Locale::Ja, "audio-status.002") => "音源を解析しています…",
        (Locale::En, "audio-status.002") => "Analyzing audio…",
        (Locale::Ja, "audio-status.003") => "埋込音源を解析しています…",
        (Locale::En, "audio-status.003") => "Analyzing embedded audio…",
        (Locale::Ja, "audio-status.004") => "音源を解析中 {0}%",
        (Locale::En, "audio-status.004") => "Analyzing audio {0}%",
        (Locale::Ja, "audio-status.005") => "音源準備完了",
        (Locale::En, "audio-status.005") => "Audio ready",
        (Locale::Ja, "audio-status.006") => "波形のみ利用可能（音声デバイスなし: {0}）",
        (Locale::En, "audio-status.006") => "Waveform only (no audio device: {0})",
        (Locale::Ja, "audio-status.007") => "音源エラー: {0}",
        (Locale::En, "audio-status.007") => "Audio error: {0}",
        (Locale::Ja, "audio-status.008") => "音源解析をキャンセルしました",
        (Locale::En, "audio-status.008") => "Audio analysis cancelled",
        (Locale::Ja, "audio-status.009") => "クリック音を設定できません: {0}",
        (Locale::En, "audio-status.009") => "Could not configure the click track: {0}",
        (Locale::Ja, "export-status.001") => "書き出し待機中",
        (Locale::En, "export-status.001") => "Export ready",
        (Locale::Ja, "export-status.002") => "書き出し環境を検査しています…",
        (Locale::En, "export-status.002") => "Inspecting export environment…",
        (Locale::Ja, "export-status.003") => "別の動画を書き出し中です",
        (Locale::En, "export-status.003") => "Another video export is already running",
        (Locale::Ja, "export-status.004") => "同名ファイルがあります。上書きを確認してください",
        (Locale::En, "export-status.004") => "A file with this name exists; confirm replacement",
        (Locale::Ja, "export-status.005") => "動画を書き出しています…",
        (Locale::En, "export-status.005") => "Exporting video…",
        (Locale::Ja, "export-status.006") => "既存動画をバックアップして書き出しています…",
        (Locale::En, "export-status.006") => "Backing up the existing video and exporting…",
        (Locale::Ja, "export-status.007") => "動画書き出しをキャンセルしました",
        (Locale::En, "export-status.007") => "Video export cancelled",
        (Locale::Ja, "export-status.008") => "事前検査完了：書き出しできます",
        (Locale::En, "export-status.008") => "Preflight complete: ready to export",
        (Locale::Ja, "export-status.009") => "事前検査で解決が必要な項目があります",
        (Locale::En, "export-status.009") => "Preflight found items that must be resolved",
        (Locale::Ja, "export-status.010") => "事前検査をキャンセルしました",
        (Locale::En, "export-status.010") => "Preflight cancelled",
        (Locale::Ja, "export-status.011") => "事前検査失敗: {0}",
        (Locale::En, "export-status.011") => "Preflight failed: {0}",
        (Locale::Ja, "export-status.012") => "書き出し完了: {0}（GPUが使えずCPUへ切替）",
        (Locale::En, "export-status.012") => "Export complete: {0} (GPU unavailable; used CPU)",
        (Locale::Ja, "export-status.013") => "書き出し完了: {0}（{1} frames、ffprobe検証済み）",
        (Locale::En, "export-status.013") => {
            "Export complete: {0} ({1} frames, verified by ffprobe)"
        }
        (Locale::Ja, "export-status.014") => {
            "動画書き出しをキャンセルしました（完成前の一時ファイルは破棄済み）"
        }
        (Locale::En, "export-status.014") => {
            "Video export cancelled (unfinished temporary file removed)"
        }
        (Locale::Ja, "export-status.015") => "動画書き出し失敗: {0}",
        (Locale::En, "export-status.015") => "Video export failed: {0}",
        (Locale::Ja, "project-status.001") => "プロジェクト準備中",
        (Locale::En, "project-status.001") => "Preparing project services",
        (Locale::Ja, "project-status.002") => "プロジェクトを保存中…",
        (Locale::En, "project-status.002") => "Saving project…",
        (Locale::Ja, "project-status.003") => "JSONを保存中…",
        (Locale::En, "project-status.003") => "Saving JSON…",
        (Locale::Ja, "project-status.004") => "JSONを読込中…",
        (Locale::En, "project-status.004") => "Loading JSON…",
        (Locale::Ja, "project-status.005") => "プロジェクトを読込中…",
        (Locale::En, "project-status.005") => "Loading project…",
        (Locale::Ja, "project-status.006") => "プロジェクト準備完了",
        (Locale::En, "project-status.006") => "Project services ready",
        (Locale::Ja, "project-status.007") => "復旧機能を開始できません: {0}",
        (Locale::En, "project-status.007") => "Could not start recovery services: {0}",
        (Locale::Ja, "timeline.001") => "カット",
        (Locale::En, "timeline.001") => "CUT",
        (Locale::Ja, "timeline.002") => "再生開始位置（IN）をドラッグ · Iキーで現在位置に設定",
        (Locale::En, "timeline.002") => {
            "Drag playback start (IN) · Press I to set it to the current position"
        }
        (Locale::Ja, "timeline.003") => "再生終了位置（OUT）をドラッグ · Oキーで現在位置に設定",
        (Locale::En, "timeline.003") => {
            "Drag playback end (OUT) · Press O to set it to the current position"
        }
        (Locale::Ja, "timeline.004") => "再生範囲の開始（IN）",
        (Locale::En, "timeline.004") => "Playback range start (IN)",
        (Locale::Ja, "timeline.005") => "再生範囲の終了（OUT）",
        (Locale::En, "timeline.005") => "Playback range end (OUT)",
        (Locale::Ja, "timeline.006") => "カウント",
        (Locale::En, "timeline.006") => "Count",
        (Locale::Ja, "timeline.007") => "ここを再生開始（IN）",
        (Locale::En, "timeline.007") => "Set playback start here (IN)",
        (Locale::Ja, "timeline.008") => "ここを再生終了（OUT）",
        (Locale::En, "timeline.008") => "Set playback end here (OUT)",
        (Locale::Ja, "app-state.136") => "セットIDを採番できないため複製できません",
        (Locale::En, "app-state.136") => {
            "The set cannot be duplicated because no set ID is available"
        }
        (Locale::Ja, "app-state.137") => "余白（mm）",
        (Locale::En, "app-state.137") => "Margins (mm)",
        (Locale::Ja, "app-state.138") => "上",
        (Locale::En, "app-state.138") => "Top",
        (Locale::Ja, "app-state.139") => "下",
        (Locale::En, "app-state.139") => "Bottom",
        (Locale::Ja, "app-state.140") => "左",
        (Locale::En, "app-state.140") => "Left",
        (Locale::Ja, "app-state.141") => "右",
        (Locale::En, "app-state.141") => "Right",
        (Locale::Ja, "app-state.142") => "PDF生成中",
        (Locale::En, "app-state.142") => "Generating PDF",
        (Locale::Ja, "app-state.143") => "キャンセル",
        (Locale::En, "app-state.143") => "Cancel",
        (Locale::Ja, "app-state.144") => "保存先を開く",
        (Locale::En, "app-state.144") => "Open Output Folder",
        (Locale::Ja, "app-state.145") => "次のセットがありません",
        (Locale::En, "app-state.145") => "There is no next set",
        (Locale::Ja, "app-state.146") => "割り当ては既に最適です",
        (Locale::En, "app-state.146") => "Assignments are already optimal",
        (Locale::Ja, "app-state.147") => "カウント",
        (Locale::En, "app-state.147") => "Counts",
        (Locale::Ja, "app-state.148") => "セクション",
        (Locale::En, "app-state.148") => "Section",
        (Locale::Ja, "app-ui.064") => "現在のセットを範囲にする",
        (Locale::En, "app-ui.064") => "Set Range to Current Set",
        (Locale::Ja, "app-ui.065") => "初回ガイドをもう一度見る",
        (Locale::En, "app-ui.065") => "Show Getting Started Again",
        (Locale::Ja, "app-ui.066") => "Space: 再生／一時停止",
        (Locale::En, "app-ui.066") => "Space: Play / Pause",
        (Locale::Ja, "app-ui.067") => "Ctrl/Cmd+S: 保存　Ctrl/Cmd+Z: 元に戻す",
        (Locale::En, "app-ui.067") => "Ctrl/Cmd+S: Save  Ctrl/Cmd+Z: Undo",
        (Locale::Ja, "inspector-media.056") => "⇄ 次セットを自動割り当て（移動最小化）",
        (Locale::En, "inspector-media.056") => "⇄ Auto-assign Next Set (minimize movement)",
        (Locale::Ja, "inspector-media.057") => {
            "演者の担当ドットを入れ替え、隊形はそのままに総移動距離を最小化します"
        }
        (Locale::En, "inspector-media.057") => {
            "Swap performer dot assignments to minimize total movement without changing the formation"
        }
        (Locale::Ja, "inspector-media.058") => "座標CSVを書き出し",
        (Locale::En, "inspector-media.058") => "Export Coordinates CSV",
        (Locale::Ja, "inspector-media.059") => "印刷・PDFワークスペース…",
        (Locale::En, "inspector-media.059") => "Print / PDF Workspace…",
        (Locale::Ja, "inspector-media.060") => "現在セットのフィールド図 (SVG)",
        (Locale::En, "inspector-media.060") => "Current Set Field Chart (SVG)",
        (Locale::Ja, "inspector-media.061") => "座標シート (HTML)",
        (Locale::En, "inspector-media.061") => "Coordinate Sheet (HTML)",
        (Locale::Ja, "inspector-media.062") => "ドリルブック全員 (HTML)",
        (Locale::En, "inspector-media.062") => "Full Drill Book (HTML)",
        (Locale::Ja, "inspector-media.063") => "カウントシート (TXT)",
        (Locale::En, "inspector-media.063") => "Count Sheet (TXT)",
        (Locale::Ja, "inspector-media.064") => "キャンセル",
        (Locale::En, "inspector-media.064") => "Cancel",
        (Locale::Ja, "inspector-media.065") => "個人練習ビューア (HTML)",
        (Locale::En, "inspector-media.065") => "Practice Viewer (HTML)",
        (Locale::Ja, "inspector-media.066") => {
            "スマホのブラウザでオフライン再生できる、演者ごとの自主練習用ページを書き出します"
        }
        (Locale::En, "inspector-media.066") => {
            "Exports an offline, phone-friendly self-practice page for each performer"
        }
        (Locale::Ja, "inspector-media.067") => "練習ビューアを書き出せません",
        (Locale::En, "inspector-media.067") => "Could not export the practice viewer",
        (Locale::Ja, "inspector-media.068") => "前回書き出した演者数",
        (Locale::En, "inspector-media.068") => "Performers in last export",
        (Locale::Ja, "inspector-media.069") => "キャンセル",
        (Locale::En, "inspector-media.069") => "Cancel",
        (Locale::Ja, "import-status.001") => "音楽タイムラインを安全に解析中…",
        (Locale::En, "import-status.001") => "Safely analyzing musical timeline…",
        (Locale::Ja, "import-status.002") => "表を安全に検査中…",
        (Locale::En, "import-status.002") => "Safely inspecting table…",
        (Locale::Ja, "import-status.003") => "座標表と現在のドキュメントを比較中…",
        (Locale::En, "import-status.003") => "Comparing coordinate table with current document…",
        (Locale::Ja, "import-status.004") => "選択した変更を検証中…",
        (Locale::En, "import-status.004") => "Validating selected changes…",
        (Locale::Ja, "import-status.005") => "タイムラインを確認して適用してください",
        (Locale::En, "import-status.005") => "Review and apply the timeline",
        (Locale::Ja, "import-status.006") => "{0} 行を検出しました。列対応と見本を確認してください",
        (Locale::En, "import-status.006") => "Detected {0} rows. Review column mapping and preview",
        (Locale::Ja, "import-status.007") => "差分を選択し、適用内容を確認してください",
        (Locale::En, "import-status.007") => "Select differences and review what will be applied",
        (Locale::Ja, "plugin-status.001") => {
            "マニフェストを読み込みました。実行前に毎回署名を再検証します。"
        }
        (Locale::En, "plugin-status.001") => {
            "Manifest loaded; signature is rechecked before every run."
        }
        (Locale::Ja, "plugin-status.002") => "プラグインを読み込めません: {0}",
        (Locale::En, "plugin-status.002") => "Plugin load failed: {0}",
        (Locale::Ja, "app-ui.068") => "プロジェクトを保存しました",
        (Locale::En, "app-ui.068") => "Project saved",
        (Locale::Ja, "app-ui.069") => "プロジェクトを開きました",
        (Locale::En, "app-ui.069") => "Project opened",
        (Locale::Ja, "app-ui.070") => "プロジェクトエラー",
        (Locale::En, "app-ui.070") => "Project error",
        (Locale::Ja, "app-ui.071") => "インポート適用エラー",
        (Locale::En, "app-ui.071") => "Import apply error",
        (Locale::Ja, "app-ui.072") => "音源エラー",
        (Locale::En, "app-ui.072") => "Audio error",
        (Locale::Ja, "app-ui.073") => "音源解析をキャンセルしました",
        (Locale::En, "app-ui.073") => "Audio analysis cancelled",
        (Locale::Ja, "app-ui.074") => "インスペクタを表示",
        (Locale::En, "app-ui.074") => "Show Inspector",
        (Locale::Ja, "app-ui.075") => "インスペクタ",
        (Locale::En, "app-ui.075") => "Inspector",
        (Locale::Ja, "app-ui.076") => {
            "編集パネルを表示／非表示にします。ワークスペースのコマンドを選ぶと自動的に開きます。"
        }
        (Locale::En, "app-ui.076") => {
            "Show or hide editing panels. Workspace commands reopen the inspector automatically."
        }
        (Locale::Ja, "app-ui.077") => "選択中",
        (Locale::En, "app-ui.077") => "Selected",
        (Locale::Ja, "app-ui.078") => "水平に整列",
        (Locale::En, "app-ui.078") => "Align Horizontally",
        (Locale::Ja, "app-ui.079") => "横方向に分配",
        (Locale::En, "app-ui.079") => "Distribute Horizontally",
        (Locale::Ja, "app-ui.080") => "直線をプレビュー",
        (Locale::En, "app-ui.080") => "Preview Line",
        (Locale::Ja, "app-ui.081") => "詳細インスペクタ",
        (Locale::En, "app-ui.081") => "More in Inspector",
        (Locale::Ja, "app-ui.082") => "プレビューを適用",
        (Locale::En, "app-ui.082") => "Apply Preview",
        (Locale::Ja, "app-ui.083") => "破棄",
        (Locale::En, "app-ui.083") => "Discard",
        (Locale::Ja, "app-ui.084") => "プレビューを破棄します（Esc）",
        (Locale::En, "app-ui.084") => "Discard preview (Esc)",
        (Locale::Ja, "app-ui.085") => "整列…",
        (Locale::En, "app-ui.085") => "Arrange…",
        (Locale::Ja, "app-ui.086") => "垂直に整列",
        (Locale::En, "app-ui.086") => "Align Vertically",
        (Locale::Ja, "app-ui.087") => "縦方向に分配",
        (Locale::En, "app-ui.087") => "Distribute Vertically",
        (Locale::Ja, "app-ui.088") => "左右反転",
        (Locale::En, "app-ui.088") => "Flip Horizontally",
        (Locale::Ja, "app-ui.089") => "上下反転",
        (Locale::En, "app-ui.089") => "Flip Vertically",
        (Locale::Ja, "app-ui.090") => "プレビュー中",
        (Locale::En, "app-ui.090") => "Preview",
        (Locale::Ja, "app-ui.091") => "人を適用待ち",
        (Locale::En, "app-ui.091") => "performers pending",
        (Locale::Ja, "app-ui.092") => "Escで破棄",
        (Locale::En, "app-ui.092") => "Press Esc to discard",
        (Locale::Ja, "app-ui.093") => "プレビューを適用",
        (Locale::En, "app-ui.093") => "Apply Preview",
        (Locale::Ja, "app-ui.094") => "破棄",
        (Locale::En, "app-ui.094") => "Discard",
        (Locale::Ja, "app-ui.095") => "形を描画中",
        (Locale::En, "app-ui.095") => "Drawing a form",
        (Locale::Ja, "app-ui.096") => {
            "フィールド上でドラッグし、離すとプレビューになります。Escで破棄"
        }
        (Locale::En, "app-ui.096") => {
            "Drag on the field, then release to preview. Press Esc to discard."
        }
        (Locale::Ja, "app-ui.097") => "人を選択中",
        (Locale::En, "app-ui.097") => "performers selected",
        (Locale::Ja, "app-ui.098") => "横に整列",
        (Locale::En, "app-ui.098") => "Align Horizontally",
        (Locale::Ja, "app-ui.099") => "縦に整列",
        (Locale::En, "app-ui.099") => "Align Vertically",
        (Locale::Ja, "app-ui.100") => "横に均等分配",
        (Locale::En, "app-ui.100") => "Distribute Horizontally",
        (Locale::Ja, "app-ui.101") => "縦に均等分配",
        (Locale::En, "app-ui.101") => "Distribute Vertically",
        (Locale::Ja, "app-ui.102") => "直線をプレビュー",
        (Locale::En, "app-ui.102") => "Preview Line",
        (Locale::Ja, "app-ui.103") => "● 未保存",
        (Locale::En, "app-ui.103") => "● Unsaved",
        (Locale::Ja, "app-ui.104") => "✓ 保存済み",
        (Locale::En, "app-ui.104") => "✓ Saved",
        (Locale::Ja, "document-feedback.001") => "名称未設定",
        (Locale::En, "document-feedback.001") => "Untitled",
        (Locale::Ja, "document-feedback.002") => "保存中…",
        (Locale::En, "document-feedback.002") => "Saving…",
        (Locale::Ja, "document-feedback.003") => "保存済み",
        (Locale::En, "document-feedback.003") => "Saved",
        (Locale::Ja, "document-feedback.004") => "編集済み",
        (Locale::En, "document-feedback.004") => "Edited",
        (Locale::Ja, "document-feedback.005") => "場所",
        (Locale::En, "document-feedback.005") => "Location",
        (Locale::Ja, "app-ui.105") => "セット",
        (Locale::En, "app-ui.105") => "Set",
        (Locale::Ja, "app-ui.106") => "カウント",
        (Locale::En, "app-ui.106") => "Count",
        (Locale::Ja, "app-ui.107") => "人を選択",
        (Locale::En, "app-ui.107") => "selected",
        (Locale::Ja, "app-ui.108") => "状態",
        (Locale::En, "app-ui.108") => "Status",
        (Locale::Ja, "app-ui.119") => "矢印キーで移動",
        (Locale::En, "app-ui.119") => "Arrow keys move",
        (Locale::Ja, "app-ui.120") => {
            "フィールドをクリック後、矢印キーで1目盛り、Shift+矢印キーで4目盛り移動します"
        }
        (Locale::En, "app-ui.120") => {
            "Click the field, then use Arrow keys for one grid division or Shift+Arrow keys for four"
        }
        (Locale::Ja, "app-ui.121") => "再生プレビュー中です",
        (Locale::En, "app-ui.121") => "Playback preview",
        (Locale::Ja, "app-ui.122") => "このセットの開始位置へ戻って編集",
        (Locale::En, "app-ui.122") => "Return to this set start to edit",
        (Locale::Ja, "app-ui.123") => {
            "再生を停止し、選択を保持したままセット開始の正確な位置へ移動します"
        }
        (Locale::En, "app-ui.123") => {
            "Pauses playback and returns to this set's exact start without changing the selection"
        }
        (Locale::Ja, "app-ui.124") => "全体表示",
        (Locale::En, "app-ui.124") => "Fit Field",
        (Locale::Ja, "app-ui.125") => "全体を表示 (F)",
        (Locale::En, "app-ui.125") => "Fit the complete field (F)",
        (Locale::Ja, "app-ui.126") => "選択を表示",
        (Locale::En, "app-ui.126") => "Focus Selection",
        (Locale::Ja, "app-ui.127") => "選択した演者の中心へ移動 (⌘1 / Ctrl+1)",
        (Locale::En, "app-ui.127") => "Center on selected performers (Cmd+1 / Ctrl+1)",
        (Locale::Ja, "app-ui.128") => "⌘/Ctrl+ホイール: 拡大　中ボタンまたはSpace+ドラッグ: 移動",
        (Locale::En, "app-ui.128") => "Cmd/Ctrl+wheel: zoom · Middle-button or Space-drag: pan",
        (Locale::Ja, "app-state.149") => "直前の編集を元に戻しました",
        (Locale::En, "app-state.149") => "Undid the previous edit",
        (Locale::Ja, "app-state.150") => "編集をやり直しました",
        (Locale::En, "app-state.150") => "Redid the edit",
        (Locale::Ja, "app-state.151") => "次のプロダクションマーカーへ移動しました",
        (Locale::En, "app-state.151") => "Moved to the next production marker",
        (Locale::Ja, "app-state.152") => "前のプロダクションマーカーへ移動しました",
        (Locale::En, "app-state.152") => "Moved to the previous production marker",
        (Locale::Ja, "app-state.153") => "選択した演者をグリッド目盛り分移動しました",
        (Locale::En, "app-state.153") => "Moved selected performers by grid divisions",
        (Locale::Ja, "app-state.154") => {
            "編集中のフォーメーションはセット開始位置でのみ変更できます。セット開始へ戻ってください。"
        }
        (Locale::En, "app-state.154") => {
            "Formation edits are available only at a set start. Return to the set start to edit."
        }
        (Locale::Ja, "app-state.155") => "セット開始位置に戻りました。選択はそのままです。",
        (Locale::En, "app-state.155") => "Returned to the set start. Your selection is unchanged.",
        (Locale::Ja, "commands.101") => "前のプロダクションマーカーへ",
        (Locale::En, "commands.101") => "Previous Production Marker",
        (Locale::Ja, "commands.102") => "次のプロダクションマーカーへ",
        (Locale::En, "commands.102") => "Next Production Marker",
        (Locale::Ja, "commands.103") => "前のプロダクションマーカーはありません",
        (Locale::En, "commands.103") => "No previous production marker",
        (Locale::Ja, "commands.104") => "次のプロダクションマーカーはありません",
        (Locale::En, "commands.104") => "No next production marker",
        (Locale::Ja, "timeline.009") => "カウント",
        (Locale::En, "timeline.009") => "Count",
        (Locale::Ja, "timeline.018") => "カウント",
        (Locale::En, "timeline.018") => "Count",
        (Locale::Ja, "timeline.010") => "ヒット",
        (Locale::En, "timeline.010") => "Hit",
        (Locale::Ja, "timeline.011") => "リハーサル",
        (Locale::En, "timeline.011") => "Rehearsal",
        (Locale::Ja, "timeline.012") => "メモ",
        (Locale::En, "timeline.012") => "Note",
        (Locale::Ja, "timeline.013") => "プロダクションマーカーを編集",
        (Locale::En, "timeline.013") => "Edit production marker",
        (Locale::Ja, "timeline.014") => "名前",
        (Locale::En, "timeline.014") => "Label",
        (Locale::Ja, "timeline.015") => "詳細",
        (Locale::En, "timeline.015") => "Details",
        (Locale::Ja, "timeline.016") => "マーカーを削除",
        (Locale::En, "timeline.016") => "Delete Marker",
        (Locale::Ja, "timeline.017") => "プロダクションマーカーを追加",
        (Locale::En, "timeline.017") => "Add production marker",
        (Locale::Ja, "timeline.019") => "保存",
        (Locale::En, "timeline.019") => "Save",
        (Locale::Ja, "timeline.020") => "キャンセル",
        (Locale::En, "timeline.020") => "Cancel",
        (Locale::Ja, "app-ui.109") => "プロダクションマーカーを更新できませんでした",
        (Locale::En, "app-ui.109") => "Could not update production marker",
        (Locale::Ja, "app-ui.111") => "現在位置を再生開始（IN）にします",
        (Locale::En, "app-ui.111") => "Set the playback start (IN) at the current position",
        (Locale::Ja, "app-ui.112") => {
            "現在位置を再生終了（OUT）にします。終了カウントの直前で停止します"
        }
        (Locale::En, "app-ui.112") => {
            "Set the playback end (OUT) at the current position; playback stops before this count"
        }
        (Locale::Ja, "app-ui.113") => "開始（含む）",
        (Locale::En, "app-ui.113") => "Starts at",
        (Locale::Ja, "app-ui.114") => "拍",
        (Locale::En, "app-ui.114") => "count",
        (Locale::Ja, "app-ui.115") => "終了（この拍の直前）",
        (Locale::En, "app-ui.115") => "Stops before",
        (Locale::Ja, "app-ui.116") => "再生長",
        (Locale::En, "app-ui.116") => "Length",
        (Locale::Ja, "app-ui.117") => "拍",
        (Locale::En, "app-ui.117") => "counts",
        (Locale::Ja, "app-ui.118") => {
            "I: 現在位置を開始（IN） · O: 現在位置を終了（OUT） · 上のリハーサル／Hitマーカーをクリックして移動"
        }
        (Locale::En, "app-ui.118") => {
            "I: set Start (IN) at the playhead · O: set End (OUT) · Click rehearsal or Hit markers above the count track to jump"
        }
        (Locale::Ja, "command-palette.001") => "コマンドを検索",
        (Locale::En, "command-palette.001") => "Search Commands",
        (Locale::Ja, "command-palette.002") => "コマンドを検索…",
        (Locale::En, "command-palette.002") => "Search commands…",
        (Locale::Ja, "command-palette.003") => "一致するコマンドがありません",
        (Locale::En, "command-palette.003") => "No matching commands",
        (Locale::Ja, "command-palette.004") => "↑↓で選択  •  Enterで実行  •  Escで閉じる",
        (Locale::En, "command-palette.004") => "↑↓ to select  •  Enter to run  •  Esc to close",
        (Locale::Ja, "set-navigator.001") => "セットへ移動",
        (Locale::En, "set-navigator.001") => "Go to Set",
        (Locale::Ja, "set-navigator.002") => "セット名・番号・リハーサル記号を検索…",
        (Locale::En, "set-navigator.002") => "Search set name, number, or rehearsal mark…",
        (Locale::Ja, "set-navigator.003") => "一致するセットがありません",
        (Locale::En, "set-navigator.003") => "No matching sets",
        (Locale::Ja, "set-navigator.004") => "セット",
        (Locale::En, "set-navigator.004") => "Set",
        (Locale::Ja, "set-navigator.005") => "開始拍",
        (Locale::En, "set-navigator.005") => "Starts at count",
        (Locale::Ja, "set-navigator.006") => "拍",
        (Locale::En, "set-navigator.006") => "counts",
        (Locale::Ja, "set-navigator.007") => "セットへ移動",
        (Locale::En, "set-navigator.007") => "Go to set",
        (Locale::Ja, "set-navigator.008") => {
            "↑↓で選択  •  Enterで移動  •  Escで閉じる  •  ⌘Jで開く"
        }
        (Locale::En, "set-navigator.008") => {
            "↑↓ to select  •  Enter to go  •  Esc to close  •  ⌘J to open"
        }
        (Locale::Ja, "set-navigator.009") => "セットへ移動…  ⌘J",
        (Locale::En, "set-navigator.009") => "Go to Set…  ⌘J",
        (Locale::Ja, "set-navigator.010") => "セットへ移動しました",
        (Locale::En, "set-navigator.010") => "Moved to set",
        (Locale::Ja, "go-to-count.001") => "全体拍へ移動",
        (Locale::En, "go-to-count.001") => "Go to Global Count",
        (Locale::Ja, "go-to-count.002") => "移動先の拍を入力（1から",
        (Locale::En, "go-to-count.002") => "Enter a count (1 through",
        (Locale::Ja, "go-to-count.003") => "例: 33",
        (Locale::En, "go-to-count.003") => "For example: 33",
        (Locale::Ja, "go-to-count.004") => "1以上、曲全体の拍数以下の整数を入力してください",
        (Locale::En, "go-to-count.004") => "Enter a whole number within the show's count range",
        (Locale::Ja, "go-to-count.005") => "移動",
        (Locale::En, "go-to-count.005") => "Go",
        (Locale::Ja, "go-to-count.006") => "キャンセル",
        (Locale::En, "go-to-count.006") => "Cancel",
        (Locale::Ja, "go-to-count.007") => {
            "Enterで移動  •  Escでキャンセル。移動時は再生を停止します。"
        }
        (Locale::En, "go-to-count.007") => {
            "Enter to go  •  Esc to cancel. Navigation pauses playback."
        }
        (Locale::Ja, "go-to-count.008") => "全体拍へ移動しました:",
        (Locale::En, "go-to-count.008") => "Moved to global count:",
        (Locale::Ja, "analytics.001") => "アナリティクス",
        (Locale::En, "analytics.001") => "Analytics",
        (Locale::Ja, "analytics.002") => {
            "ショーの評価指標をまとめて確認できます。数値は目安であり、最終判断は演出担当が行ってください。"
        }
        (Locale::En, "analytics.002") => {
            "Review the show's evaluation metrics in one place. These are reference numbers, not a final verdict — the design staff should always have the last word."
        }
        (Locale::Ja, "analytics.003") => "リズム同期",
        (Locale::En, "analytics.003") => "Rhythm Sync",
        (Locale::Ja, "analytics.004") => {
            "出発・到着が音楽の拍にどれだけ合っているかを解析します（曲全体・全セット対象）"
        }
        (Locale::En, "analytics.004") => {
            "Analyzes how closely every departure and arrival lands on the musical beat, across the whole show"
        }
        (Locale::Ja, "analytics.005") => "オンビート率",
        (Locale::En, "analytics.005") => "On-beat rate",
        (Locale::Ja, "analytics.006") => "内訳",
        (Locale::En, "analytics.006") => "Breakdown",
        (Locale::Ja, "analytics.007") => "強拍到着",
        (Locale::En, "analytics.007") => "On downbeat",
        (Locale::Ja, "analytics.008") => "弱拍到着",
        (Locale::En, "analytics.008") => "On backbeat",
        (Locale::Ja, "analytics.009") => "シンコペーション",
        (Locale::En, "analytics.009") => "Syncopated",
        (Locale::Ja, "analytics.010") => "分析対象がありません（セットが2つ以上必要です）",
        (Locale::En, "analytics.010") => "Nothing to analyze yet (at least two sets are needed)",
        (Locale::Ja, "analytics.011") => "審美・対称性スコア",
        (Locale::En, "analytics.011") => "Aesthetics & Symmetry Score",
        (Locale::Ja, "analytics.012") => {
            "現在のセットの左右対称性と密度の均一性を採点します（美醜の絶対的な判定ではありません）"
        }
        (Locale::En, "analytics.012") => {
            "Scores the current set's left-right symmetry and density evenness (a reference indicator, not an absolute beauty judgement)"
        }
        (Locale::Ja, "analytics.013") => "総合スコア",
        (Locale::En, "analytics.013") => "Overall score",
        (Locale::Ja, "analytics.014") => "対称性",
        (Locale::En, "analytics.014") => "Symmetry",
        (Locale::Ja, "analytics.015") => "密度均一性",
        (Locale::En, "analytics.015") => "Density evenness",
        (Locale::Ja, "analytics.016") => "対称性を崩している演者 トップ5",
        (Locale::En, "analytics.016") => "Top 5 performers breaking symmetry",
        (Locale::Ja, "analytics.017") => "このセットは採点できません（演者または座標がありません）",
        (Locale::En, "analytics.017") => "This set can't be scored (no performers or positions)",
        (Locale::Ja, "analytics.018") => "ショーDNA（ヒートマップ）",
        (Locale::En, "analytics.018") => "Show DNA (Heatmap)",
        (Locale::Ja, "analytics.019") => {
            "ショー全体でフィールドのどこが多く使われたかを色の濃さで重ねて表示します（青=少ない、赤=多い）"
        }
        (Locale::En, "analytics.019") => {
            "Overlays how much each part of the field was used across the whole show (blue = light use, red = heavy use)"
        }
        (Locale::Ja, "analytics.020") => "フィールド使用頻度を表示",
        (Locale::En, "analytics.020") => "Show field usage heatmap",
        (Locale::Ja, "analytics.021") => "軌跡（トレイル）",
        (Locale::En, "analytics.021") => "Trails",
        (Locale::Ja, "analytics.022") => {
            "現在のセットから次のセットへ向かう演者の移動経路を、速度で色分けして表示します（青=遅い、赤=速い）"
        }
        (Locale::En, "analytics.022") => {
            "Shows the movement path from the current set to the next, color-coded by speed (blue = slow, red = fast)"
        }
        (Locale::Ja, "analytics.023") => "表示対象",
        (Locale::En, "analytics.023") => "Show for",
        (Locale::Ja, "analytics.024") => "非表示",
        (Locale::En, "analytics.024") => "Hidden",
        (Locale::Ja, "analytics.025") => "選択中の演者のみ",
        (Locale::En, "analytics.025") => "Selected performers only",
        (Locale::Ja, "analytics.026") => "全員",
        (Locale::En, "analytics.026") => "Everyone",
        (Locale::Ja, "simple-mode.001") => "1. フォーメーションを選ぶ",
        (Locale::En, "simple-mode.001") => "1. Choose a Formation",
        (Locale::Ja, "simple-mode.002") => "2. 演者を選ぶ",
        (Locale::En, "simple-mode.002") => "2. Select Performers",
        (Locale::Ja, "simple-mode.003") => "3. 動かす",
        (Locale::En, "simple-mode.003") => "3. Move",
        (Locale::Ja, "simple-mode.004") => "4. 再生する",
        (Locale::En, "simple-mode.004") => "4. Play",
        (Locale::Ja, "simple-mode.005") => "編集したいフォーメーション（セット）を選びましょう。",
        (Locale::En, "simple-mode.005") => "Choose the formation (set) you want to edit.",
        (Locale::Ja, "simple-mode.006") => {
            "次は演者を選んでみましょう。フィールドの演者をタップするか、「全員を選択」を押します。"
        }
        (Locale::En, "simple-mode.006") => {
            "Next, let's select performers. Tap performers on the field, or press Select All."
        }
        (Locale::Ja, "simple-mode.007") => {
            "選んだ演者をドラッグするか、並べ方のボタンで動かしましょう。矢印ボタンで表示範囲を移動できます。"
        }
        (Locale::En, "simple-mode.007") => {
            "Drag the selected performers, or use a layout button. Use the arrow buttons to pan the view."
        }
        (Locale::Ja, "simple-mode.008") => "再生ボタンを押して、動きを確認しましょう。",
        (Locale::En, "simple-mode.008") => "Press Play to check how it moves.",
        (Locale::Ja, "simple-mode.009") => "かんたんモード",
        (Locale::En, "simple-mode.009") => "Simple Mode",
        (Locale::Ja, "simple-mode.010") => "通常モードに戻る",
        (Locale::En, "simple-mode.010") => "Back to Full Mode",
        (Locale::Ja, "simple-mode.011") => "いつでも通常の画面に戻れます",
        (Locale::En, "simple-mode.011") => "You can return to the full desktop UI anytime",
        (Locale::Ja, "simple-mode.012") => "メトロノーム",
        (Locale::En, "simple-mode.012") => "Metronome",
        (Locale::Ja, "simple-mode.013") => "◀ もどる",
        (Locale::En, "simple-mode.013") => "◀ Back",
        (Locale::Ja, "simple-mode.014") => "つぎへ ▶",
        (Locale::En, "simple-mode.014") => "Next ▶",
        (Locale::Ja, "simple-mode.015") => "◀ 前のフォーメーション",
        (Locale::En, "simple-mode.015") => "◀ Previous",
        (Locale::Ja, "simple-mode.016") => "フォーメーション",
        (Locale::En, "simple-mode.016") => "Formation",
        (Locale::Ja, "simple-mode.017") => "次のフォーメーション ▶",
        (Locale::En, "simple-mode.017") => "Next ▶",
        (Locale::Ja, "simple-mode.018") => "＋ 新しいフォーメーション",
        (Locale::En, "simple-mode.018") => "+ Add Formation",
        (Locale::Ja, "simple-mode.019") => "選択中",
        (Locale::En, "simple-mode.019") => "Selected",
        (Locale::Ja, "simple-mode.020") => "先に「2. 演者を選ぶ」で演者を選びましょう",
        (Locale::En, "simple-mode.020") => "Select performers in step 2 first",
        (Locale::Ja, "simple-mode.021") => "直線に並べる",
        (Locale::En, "simple-mode.021") => "Line Up",
        (Locale::Ja, "simple-mode.022") => "円に並べる",
        (Locale::En, "simple-mode.022") => "Arrange in Circle",
        (Locale::Ja, "simple-mode.023") => "ブロックに並べる",
        (Locale::En, "simple-mode.023") => "Arrange in Block",
        (Locale::Ja, "simple-mode.024") => "選択箇所を表示",
        (Locale::En, "simple-mode.024") => "Center on Selection",
        (Locale::Ja, "simple-mode.025") => "最初から",
        (Locale::En, "simple-mode.025") => "From the Start",
        (Locale::Ja, "simple-mode.026") => "保存する",
        (Locale::En, "simple-mode.026") => "Save",
        (Locale::Ja, "simple-mode.027") => "カウント",
        (Locale::En, "simple-mode.027") => "Count",
        (Locale::Ja, "simple-mode.028") => "拡大表示：矢印ボタンやキーで移動できます",
        (Locale::En, "simple-mode.028") => "Zoomed in: pan with the arrow buttons or arrow keys",
        (Locale::Ja, "simple-mode.029") => {
            "ドリルとは関係なく、いつでも使える練習用メトロノームです"
        }
        (Locale::En, "simple-mode.029") => {
            "A practice metronome, independent of the drill you're editing"
        }
        (Locale::Ja, "simple-mode.030") => "停止",
        (Locale::En, "simple-mode.030") => "Stop",
        (Locale::Ja, "simple-mode.031") => "開始",
        (Locale::En, "simple-mode.031") => "Start",
        (Locale::Ja, "simple-mode.032") => "BPM",
        (Locale::En, "simple-mode.032") => "BPM",
        (Locale::Ja, "simple-mode.033") => "▲",
        (Locale::En, "simple-mode.033") => "▲",
        (Locale::Ja, "simple-mode.034") => "◀",
        (Locale::En, "simple-mode.034") => "◀",
        (Locale::Ja, "simple-mode.035") => "▶",
        (Locale::En, "simple-mode.035") => "▶",
        (Locale::Ja, "simple-mode.036") => "▼",
        (Locale::En, "simple-mode.036") => "▼",
        (Locale::Ja, "simple-mode.037") => "音声出力デバイスを開けませんでした",
        (Locale::En, "simple-mode.037") => "Could not open the audio output device",
        (Locale::Ja, "simple-mode.038") => "メトロノーム音を準備できませんでした",
        (Locale::En, "simple-mode.038") => "Could not prepare the metronome clicks",
        (Locale::Ja, "simple-mode.039") => "再生範囲",
        (Locale::En, "simple-mode.039") => "Playback Range",
        (Locale::Ja, "simple-mode.040") => "現在のセット",
        (Locale::En, "simple-mode.040") => "Current Set",
        (Locale::Ja, "simple-mode.041") => "曲全体",
        (Locale::En, "simple-mode.041") => "Whole Show",
        (Locale::Ja, "simple-mode.042") => "現在位置を開始",
        (Locale::En, "simple-mode.042") => "Set In",
        (Locale::Ja, "simple-mode.043") => "現在位置を終了",
        (Locale::En, "simple-mode.043") => "Set Out",
        (Locale::Ja, "simple-mode.044") => "ループ",
        (Locale::En, "simple-mode.044") => "Loop",
        (Locale::Ja, "simple-mode.045") => "カウント",
        (Locale::En, "simple-mode.045") => "Counts",
        (Locale::Ja, "simple-mode.046") => "現在位置を再生開始（IN）にします",
        (Locale::En, "simple-mode.046") => "Set the playback start (IN) at the current position",
        (Locale::Ja, "simple-mode.047") => {
            "現在位置を再生終了（OUT）にします。終了カウントの直前で停止します"
        }
        (Locale::En, "simple-mode.047") => {
            "Set the playback end (OUT) at the current position; playback stops before this count"
        }
        (Locale::Ja, "simple-mode.048") => "開始（含む）",
        (Locale::En, "simple-mode.048") => "Starts at",
        (Locale::Ja, "simple-mode.049") => "拍",
        (Locale::En, "simple-mode.049") => "count",
        (Locale::Ja, "simple-mode.050") => "終了（この拍の直前）",
        (Locale::En, "simple-mode.050") => "Stops before",
        (Locale::Ja, "simple-mode.051") => "再生長",
        (Locale::En, "simple-mode.051") => "Length",
        (Locale::Ja, "simple-mode.052") => "拍",
        (Locale::En, "simple-mode.052") => "counts",
        (Locale::Ja, "simple-mode.053") => {
            "I: 現在位置を開始（IN） · O: 現在位置を終了（OUT） · タイムラインのマーカーをクリックして移動"
        }
        (Locale::En, "simple-mode.053") => {
            "I: set Start (IN) at the playhead · O: set End (OUT) · Click timeline markers to jump"
        }
        (Locale::Ja, "production-markers.001") => "プロダクションマーカー",
        (Locale::En, "production-markers.001") => "Production Markers",
        (Locale::Ja, "production-markers.002") => {
            "ヒット、リハーサル、メモを検索し、クリックして正確な拍へ移動します。"
        }
        (Locale::En, "production-markers.002") => {
            "Search hits, rehearsals, and notes; click one to jump to its exact count."
        }
        (Locale::Ja, "production-markers.003") => "名前、種類、拍で検索",
        (Locale::En, "production-markers.003") => "Search name, type, or count",
        (Locale::Ja, "production-markers.004") => "プロダクションマーカーを検索",
        (Locale::En, "production-markers.004") => "Search production markers",
        (Locale::Ja, "production-markers.005") => "ヒット",
        (Locale::En, "production-markers.005") => "Hit",
        (Locale::Ja, "production-markers.006") => "リハーサル",
        (Locale::En, "production-markers.006") => "Rehearsal",
        (Locale::Ja, "production-markers.007") => "メモ",
        (Locale::En, "production-markers.007") => "Note",
        (Locale::Ja, "production-markers.008") => {
            "一致するマーカーはありません。タイムラインを右クリックして追加できます。"
        }
        (Locale::En, "production-markers.008") => {
            "No matching markers. Right-click the timeline to add one."
        }
        (Locale::Ja, "production-markers.009") => "詳細はありません",
        (Locale::En, "production-markers.009") => "No details",
        (Locale::Ja, "production-markers.010") => "再生範囲の開始にする",
        (Locale::En, "production-markers.010") => "Use as Range Start",
        (Locale::Ja, "production-markers.011") => "ここまでを再生範囲にする",
        (Locale::En, "production-markers.011") => "Set Range to Here",
        (Locale::Ja, "production-markers.012") => {
            "開始マーカーを選択してから、後のマーカーを選んでください"
        }
        (Locale::En, "production-markers.012") => {
            "Choose a start marker, then select a later marker."
        }
        (Locale::Ja, "production-markers.013") => "選択したマーカー間を再生範囲にしました",
        (Locale::En, "production-markers.013") => "Set playback range between the selected markers",
        (Locale::Ja, "production-markers.014") => "再生開始マーカー:",
        (Locale::En, "production-markers.014") => "Range start marker:",
        (Locale::Ja, "production-markers.015") => "種類",
        (Locale::En, "production-markers.015") => "Type",
        (Locale::Ja, "production-markers.016") => "名前",
        (Locale::En, "production-markers.016") => "Label",
        (Locale::Ja, "production-markers.017") => "詳細",
        (Locale::En, "production-markers.017") => "Details",
        (Locale::Ja, "production-markers.018") => "保存",
        (Locale::En, "production-markers.018") => "Save",
        (Locale::Ja, "production-markers.019") => "編集を戻す",
        (Locale::En, "production-markers.019") => "Revert Edit",
        (Locale::Ja, "production-markers.020") => "プロダクションマーカーを更新できませんでした",
        (Locale::En, "production-markers.020") => "Could not update production marker",
        (Locale::Ja, "production-markers.021") => "前のマーカー",
        (Locale::En, "production-markers.021") => "Previous",
        (Locale::Ja, "production-markers.022") => {
            "Option + 左矢印: 前のプロダクションマーカーへ移動"
        }
        (Locale::En, "production-markers.022") => "Option + Left Arrow: previous production marker",
        (Locale::Ja, "production-markers.023") => "次のマーカー",
        (Locale::En, "production-markers.023") => "Next",
        (Locale::Ja, "production-markers.024") => {
            "Option + 右矢印: 次のプロダクションマーカーへ移動"
        }
        (Locale::En, "production-markers.024") => "Option + Right Arrow: next production marker",
        (Locale::Ja, "production-markers.025") => "現在のマーカー:",
        (Locale::En, "production-markers.025") => "At marker:",
        (Locale::Ja, "production-markers.026") => "範囲開始を解除",
        (Locale::En, "production-markers.026") => "Clear Range Start",
        (Locale::Ja, "production-markers.027") => "再生範囲の開始マーカーを解除しました",
        (Locale::En, "production-markers.027") => "Cleared playback range start marker",
        (Locale::Ja, "comparison.001") => "A/B 比較",
        (Locale::En, "comparison.001") => "A/B Compare",
        (Locale::Ja, "comparison.002") => "閉じる",
        (Locale::En, "comparison.002") => "Close",
        (Locale::Ja, "comparison.003") => "A/B比較を閉じました",
        (Locale::En, "comparison.003") => "Closed A/B comparison",
        (Locale::Ja, "comparison.004") => "移動ラインを表示",
        (Locale::En, "comparison.004") => "Show movement lines",
        (Locale::Ja, "comparison.005") => "黄: 参照セット　シアン: 現在位置との差分　Esc: 閉じる",
        (Locale::En, "comparison.005") => "Amber: reference set · Cyan: difference · Esc: close",
        (Locale::Ja, "comparison.006") => "A/B比較を始める",
        (Locale::En, "comparison.006") => "Start A/B Compare",
        (Locale::Ja, "comparison.007") => "別セットを重ねて、位置と移動差を確認します",
        (Locale::En, "comparison.007") => {
            "Overlay another set to inspect positions and movement differences"
        }
        (Locale::Ja, "comparison.008") => "A/B比較を開始しました（保存・Undoには影響しません）",
        (Locale::En, "comparison.008") => "Started A/B comparison (does not affect save or Undo)",
        (Locale::Ja, "comparison.009") => "EscでA/B比較を閉じました",
        (Locale::En, "comparison.009") => "Closed A/B comparison with Escape",
        (Locale::Ja, "app-ui.129") => "選択した演者をロック",
        (Locale::En, "app-ui.129") => "Lock Selected Performers",
        (Locale::Ja, "app-ui.130") => {
            "ロックした演者は選択・移動できません。ファイルと書き出しは変更しません。"
        }
        (Locale::En, "app-ui.130") => {
            "Locked performers cannot be selected or moved. Files and exports are unchanged."
        }
        (Locale::Ja, "app-ui.131") => "選択した演者を一時的に隠す",
        (Locale::En, "app-ui.131") => "Temporarily Hide Selected",
        (Locale::Ja, "app-ui.132") => "隠した演者は斜線で表示され、選択・移動できません。",
        (Locale::En, "app-ui.132") => {
            "Hidden performers are slashed on the field and cannot be selected or moved."
        }
        (Locale::Ja, "app-ui.133") => "ロック中",
        (Locale::En, "app-ui.133") => "locked",
        (Locale::Ja, "app-ui.134") => "一時非表示",
        (Locale::En, "app-ui.134") => "temporarily hidden",
        (Locale::Ja, "app-ui.135") => "ロック・非表示をすべて解除",
        (Locale::En, "app-ui.135") => "Clear Locks & Hidden",
        (Locale::Ja, "app-ui.136") => {
            "このセッションの表示・操作フィルターだけを解除します。ドリルとUndo履歴は変更しません。"
        }
        (Locale::En, "app-ui.136") => {
            "Clears only this session's display and interaction filters. The drill and undo history are unchanged."
        }
        (Locale::Ja, "app-ui.137") => "演者を戻す…",
        (Locale::En, "app-ui.137") => "Restore performers…",
        (Locale::Ja, "app-ui.138") => {
            "この操作はセッション中の表示・操作フィルターだけを戻します。"
        }
        (Locale::En, "app-ui.138") => "This restores only session display and interaction filters.",
        (Locale::Ja, "app-ui.139") => "このセクションのロックと非表示をすべて解除",
        (Locale::En, "app-ui.139") => "Restore every filtered performer in this section",
        (Locale::Ja, "app-ui.140") => "フィルターを解除した演者数:",
        (Locale::En, "app-ui.140") => "Restored performers:",
        (Locale::Ja, "app-ui.141") => "戻す",
        (Locale::En, "app-ui.141") => "Restore",
        (Locale::Ja, "app-ui.142") => "この演者のロックと一時非表示を解除",
        (Locale::En, "app-ui.142") => "Remove this performer's lock and temporary hide",
        (Locale::Ja, "app-ui.143") => "ロック中・一時非表示",
        (Locale::En, "app-ui.143") => "locked · temporarily hidden",
        (Locale::Ja, "app-ui.144") => "演者を編集可能な表示に戻しました",
        (Locale::En, "app-ui.144") => "Restored performer to the editable field",
        (Locale::Ja, "app-ui.145") => "ロック中",
        (Locale::En, "app-ui.145") => "locked",
        (Locale::Ja, "app-ui.146") => "一時非表示",
        (Locale::En, "app-ui.146") => "temporarily hidden",
        (Locale::Ja, "app-ui.147") => "セッションフィルター",
        (Locale::En, "app-ui.147") => "Session filters",
        (Locale::Ja, "app-ui.148") => "ロック",
        (Locale::En, "app-ui.148") => "locked",
        (Locale::Ja, "app-ui.149") => "非表示",
        (Locale::En, "app-ui.149") => "hidden",
        (Locale::Ja, "app-ui.150") => "演者のロック・非表示を個別またはセクション単位で戻す",
        (Locale::En, "app-ui.150") => {
            "Restore performer locks and hidden performers individually or by section"
        }
        (Locale::Ja, "app-ui.151") => "人をロックしました。上部の「直前を戻す」で復帰できます",
        (Locale::En, "app-ui.151") => {
            "performer(s) locked. Use Restore last in the status strip to recover"
        }
        (Locale::Ja, "app-ui.152") => {
            "人を一時非表示にしました。上部の「直前を戻す」で復帰できます"
        }
        (Locale::En, "app-ui.152") => {
            "performer(s) temporarily hidden. Use Restore last in the status strip to recover"
        }
        (Locale::Ja, "app-ui.153") => "直前を戻す",
        (Locale::En, "app-ui.153") => "Restore last",
        (Locale::Ja, "app-ui.154") => {
            "直前のロック／一時非表示をすべて戻します。ドリルとUndo履歴は変更しません。"
        }
        (Locale::En, "app-ui.154") => {
            "Restore the latest lock or temporary hide. The drill and undo history are unchanged."
        }
        (Locale::Ja, "app-ui.155") => "人を編集可能な表示に戻しました",
        (Locale::En, "app-ui.155") => "performer(s) restored to the editable field",
        (Locale::Ja, "app-ui.156") => "選択した演者をロック",
        (Locale::En, "app-ui.156") => "Lock selected",
        (Locale::Ja, "app-ui.157") => {
            "この選択を固定して、ほかの演者の編集に集中します。直後に上部から戻せます。"
        }
        (Locale::En, "app-ui.157") => {
            "Lock this group to focus on other performers. You can restore it immediately from the status strip."
        }
        (Locale::Ja, "app-ui.158") => "選択した演者を一時的に隠す",
        (Locale::En, "app-ui.158") => "Hide selected",
        (Locale::Ja, "app-ui.159") => "この選択を一時的に隠します。直後に上部から戻せます。",
        (Locale::En, "app-ui.159") => {
            "Temporarily hide this group. You can restore it immediately from the status strip."
        }
        (Locale::Ja, "app-ui.160") => "元に戻す (Ctrl/Cmd+Z)",
        (Locale::En, "app-ui.160") => "Undo (Ctrl/Cmd+Z)",
        (Locale::Ja, "app-ui.161") => "やり直す (Ctrl/Cmd+Shift+Z)",
        (Locale::En, "app-ui.161") => "Redo (Ctrl/Cmd+Shift+Z)",
        (Locale::Ja, "app-ui.162") => "カラーテーマ",
        (Locale::En, "app-ui.162") => "Color Theme",
        (Locale::Ja, "app-ui.163") => "ピンウィール",
        (Locale::En, "app-ui.163") => "Pinwheel",
        (Locale::Ja, "app-ui.164") => {
            "選択した演者を中心点を軸に回転します。"
        }
        (Locale::En, "app-ui.164") => {
            "Rotate the selected performers around their centroid."
        }
        (Locale::Ja, "clinic-ui.001") => "警告から演者をフォーカス",
        (Locale::En, "clinic-ui.001") => "Focus performers from warnings",
        (Locale::Ja, "clinic-ui.002") => "衝突",
        (Locale::En, "clinic-ui.002") => "Collision",
        (Locale::Ja, "clinic-ui.003") => "この警告の演者を選択してフィールドに表示します",
        (Locale::En, "clinic-ui.003") => {
            "Select the performers in this warning and focus them on the field"
        }
        (Locale::Ja, "clinic-ui.004") => "衝突の演者を選択しました",
        (Locale::En, "clinic-ui.004") => "Selected performers in the collision",
        (Locale::Ja, "clinic-ui.005") => "大きな歩幅",
        (Locale::En, "clinic-ui.005") => "Large stride",
        (Locale::Ja, "clinic-ui.006") => "歩幅を確認する演者を選択しました",
        (Locale::En, "clinic-ui.006") => "Selected performer to inspect stride",
        (Locale::Ja, "clinic-ui.007") => {
            "選択と表示位置だけを変更します。ドリルとUndo履歴は変更しません。"
        }
        (Locale::En, "clinic-ui.007") => {
            "Only selection and the viewport change. The drill and undo history stay unchanged."
        }
        (Locale::Ja, "close-guard.001") => "保存していない変更があります",
        (Locale::En, "close-guard.001") => "You have unsaved changes",
        (Locale::Ja, "close-guard.002") => {
            "閉じる前に変更を保存しますか？ 保存が完了するまでこのウインドウは開いたままです。"
        }
        (Locale::En, "close-guard.002") => {
            "Do you want to save your changes before closing? This window stays open until saving finishes."
        }
        (Locale::Ja, "close-guard.003") => "変更を保存",
        (Locale::En, "close-guard.003") => "Save Changes",
        (Locale::Ja, "close-guard.004") => "保存せずに閉じる",
        (Locale::En, "close-guard.004") => "Discard Changes",
        (Locale::Ja, "close-guard.005") => "キャンセル",
        (Locale::En, "close-guard.005") => "Cancel",
        (Locale::Ja, "close-guard.006") => "保存中… 完了後に閉じます",
        (Locale::En, "close-guard.006") => "Saving… will close when finished",
        (Locale::Ja, "close-guard.007") => "保存が完了しました。ウインドウを閉じます。",
        (Locale::En, "close-guard.007") => "Saved. Closing the window.",
        (Locale::Ja, "close-guard.008") => "終了をキャンセルしました",
        (Locale::En, "close-guard.008") => "Cancelled closing the window",
        (Locale::Ja, "close-guard.009") => "変更を残したまま終了をキャンセルしました",
        (Locale::En, "close-guard.009") => "Cancelled closing and kept the changes",
        (Locale::Ja, "document-open-guard.001") => "開く前に変更を保存しますか？",
        (Locale::En, "document-open-guard.001") => "Save changes before opening?",
        (Locale::Ja, "document-open-guard.002") => {
            "現在の変更は、別のドキュメントを開くと置き換えられます。保存してから開くか、変更を破棄してください。"
        }
        (Locale::En, "document-open-guard.002") => {
            "Opening another document replaces your current edits. Save them first, or explicitly discard them."
        }
        (Locale::Ja, "document-open-guard.003") => "変更を保存して開く",
        (Locale::En, "document-open-guard.003") => "Save and Open",
        (Locale::Ja, "document-open-guard.004") => "変更を破棄して開く",
        (Locale::En, "document-open-guard.004") => "Discard and Open",
        (Locale::Ja, "document-open-guard.005") => "キャンセル",
        (Locale::En, "document-open-guard.005") => "Cancel",
        (Locale::Ja, "document-open-guard.006") => "保存中… 保存が完了するとファイルを選択できます",
        (Locale::En, "document-open-guard.006") => {
            "Saving… choose the document after saving finishes"
        }
        (Locale::Ja, "document-open-guard.008") => "ドキュメントを開く操作をキャンセルしました",
        (Locale::En, "document-open-guard.008") => "Cancelled opening another document",
        (Locale::Ja, "document-open-guard.009") => {
            "変更を残したままドキュメントを開く操作をキャンセルしました"
        }
        (Locale::En, "document-open-guard.009") => {
            "Cancelled opening another document and kept the changes"
        }
        (Locale::Ja, "count-adjust.001") => "セット尺を調整",
        (Locale::En, "count-adjust.001") => "Adjust set counts",
        (Locale::Ja, "count-adjust.002") => {
            "プレビュー中は保存しません。適用すると1回のUndoで戻せます。後続セットの開始カウントだけが移動します。"
        }
        (Locale::En, "count-adjust.002") => {
            "Preview does not save. Apply creates one Undo step; only later set start counts move."
        }
        (Locale::Ja, "count-adjust.003") => "尺をプレビュー",
        (Locale::En, "count-adjust.003") => "Preview count change",
        (Locale::Ja, "count-adjust.004") => "後続セットの開始位置",
        (Locale::En, "count-adjust.004") => "Later set starts",
        (Locale::Ja, "count-adjust.005") => {
            "演者位置・ルート・テンポデータは変えません。ルートに合わない尺は適用時に安全に拒否されます。"
        }
        (Locale::En, "count-adjust.005") => {
            "Positions, routes, and tempo data stay unchanged. Unsafe route timing is rejected when applied."
        }
        (Locale::Ja, "count-adjust.006") => "適用（Undo 1回）",
        (Locale::En, "count-adjust.006") => "Apply (one Undo)",
        (Locale::Ja, "count-adjust.007") => "プレビューを破棄",
        (Locale::En, "count-adjust.007") => "Discard preview",
        (Locale::Ja, "count-adjust.008") => {
            "セット尺を更新できません。ルートのタイミングを確認してください。"
        }
        (Locale::En, "count-adjust.008") => "Could not update set counts. Check route timing.",
        (Locale::Ja, "count-adjust.009") => "セット尺を更新しました。Cmd+Zで戻せます。",
        (Locale::En, "count-adjust.009") => "Set counts updated. Press Cmd+Z to undo.",
        (Locale::Ja, "clipboard.001") => "コピーする演者を選択してください",
        (Locale::En, "clipboard.001") => "Select performers to copy",
        (Locale::Ja, "clipboard.002") => "人の位置をコピーしました。Cmd+Vでプレビューします。",
        (Locale::En, "clipboard.002") => "performer positions copied. Press Cmd+V to preview.",
        (Locale::Ja, "clipboard.003") => "貼り付けるコピーがありません",
        (Locale::En, "clipboard.003") => "Nothing has been copied yet",
        (Locale::Ja, "clipboard.004") => "現在のドキュメントに貼り付け可能な演者がいません",
        (Locale::En, "clipboard.004") => "No copied performers can be pasted into this document",
        (Locale::Ja, "clipboard.005") => "貼り付けプレビュー中です。適用またはEscで破棄できます。",
        (Locale::En, "clipboard.005") => "Paste preview ready. Apply it or press Esc to discard.",
        (Locale::Ja, "clipboard.006") => "貼り付けプレビューを破棄しました",
        (Locale::En, "clipboard.006") => "Discarded paste preview",
        (Locale::Ja, "clipboard.007") => "コピーした隊形を貼り付け",
        (Locale::En, "clipboard.007") => "Paste copied formation",
        (Locale::Ja, "clipboard.008") => "隊形を貼り付けました。Cmd+Zで戻せます。",
        (Locale::En, "clipboard.008") => "Formation pasted. Press Cmd+Z to undo.",
        (Locale::Ja, "clipboard.009") => "隊形をコピー",
        (Locale::En, "clipboard.009") => "Copy Formation",
        (Locale::Ja, "clipboard.010") => "隊形を貼り付け…",
        (Locale::En, "clipboard.010") => "Paste Formation…",
        (Locale::Ja, "clipboard.011") => "先にコピーする演者を選択してください",
        (Locale::En, "clipboard.011") => "Select performers to copy first",
        (Locale::Ja, "clipboard.012") => "先に隊形をコピーしてください",
        (Locale::En, "clipboard.012") => "Copy a formation first",
        (Locale::Ja, "clipboard.013") => "貼り付けをプレビュー",
        (Locale::En, "clipboard.013") => "Preview pasted formation",
        (Locale::Ja, "clipboard.014") => "人の同じIDの位置を更新します",
        (Locale::En, "clipboard.014") => "matching stable performer IDs will update",
        (Locale::Ja, "clipboard.015") => "適用（Undo 1回）",
        (Locale::En, "clipboard.015") => "Apply (one Undo)",
        (Locale::Ja, "clipboard.016") => "破棄（Esc）",
        (Locale::En, "clipboard.016") => "Discard (Esc)",
        (Locale::Ja, "clipboard.017") => {
            "コピー元と同じ演者だけを更新します。文書は適用まで変更しません。"
        }
        (Locale::En, "clipboard.017") => {
            "Only the same performers update. The document stays unchanged until Apply."
        }
        (Locale::Ja, "clipboard.018") => "隊形をコピー",
        (Locale::En, "clipboard.018") => "Copy Formation",
        (Locale::Ja, "clipboard.019") => "隊形を貼り付け…",
        (Locale::En, "clipboard.019") => "Paste Formation…",
        (Locale::Ja, "clipboard.020") => "隊形をコピー",
        (Locale::En, "clipboard.020") => "Copy Formation",
        (Locale::Ja, "clipboard.021") => "隊形を貼り付け…",
        (Locale::En, "clipboard.021") => "Paste Formation…",
        (Locale::Ja, "clipboard.022") => "現在のドキュメントに貼り付け可能な演者がいません",
        (Locale::En, "clipboard.022") => "No copied performers can be pasted into this document",
        (Locale::Ja, "clipboard.023") => "選択した同人数の演者に、中心を保って隊形を配置します",
        (Locale::En, "clipboard.023") => {
            "the selected equal-size group receives this shape at its current centre"
        }
        (Locale::Ja, "clipboard.024") => "行・列順で対応します。文書は適用まで変更しません。",
        (Locale::En, "clipboard.024") => {
            "Correspondence follows row/file order. The document stays unchanged until Apply."
        }
        (Locale::Ja, "focus-field.001") => "フィールドに集中",
        (Locale::En, "focus-field.001") => "Focus Field",
        (Locale::Ja, "focus-field.002") => "フィールド集中モードを開始しました",
        (Locale::En, "focus-field.002") => "Focus Field enabled",
        (Locale::Ja, "focus-field.003") => "通常のワークスペースに戻りました",
        (Locale::En, "focus-field.003") => "Returned to the full workspace",
        (Locale::Ja, "focus-field.004") => "集中モードを終了",
        (Locale::En, "focus-field.004") => "Exit Focus",
        (Locale::Ja, "focus-field.005") => "Esc または ⌘⇧F で通常表示に戻る",
        (Locale::En, "focus-field.005") => "Press Esc or Cmd+Shift+F to return",
        (Locale::Ja, "app-state.156") => "フィールド上をドラッグして分割線を描いてください",
        (Locale::En, "app-state.156") => "Drag across the field to draw a cut line",
        (Locale::Ja, "app-state.157") => "ナイフ: 分割できませんでした（線が短すぎます）",
        (Locale::En, "app-state.157") => "Knife: could not split (line was too short)",
        (Locale::Ja, "app-state.158") => "ナイフで選択を分割しました",
        (Locale::En, "app-state.158") => "Knife split the selection",
        (Locale::Ja, "workspace-inspector.128") => "ナイフ",
        (Locale::En, "workspace-inspector.128") => "Knife",
        (Locale::Ja, "workspace-inspector.129") => {
            "フィールド上をドラッグして現在の選択を2つに分割します（未選択の場合は全員が対象）"
        }
        (Locale::En, "workspace-inspector.129") => {
            "Drag across the field to split the current selection in two (the whole cast if nothing is selected)"
        }
        (Locale::Ja, "workspace-inspector.130") => "グルー",
        (Locale::En, "workspace-inspector.130") => "Glue",
        (Locale::Ja, "workspace-inspector.131") => "直近の選択を現在の選択に結合します",
        (Locale::En, "workspace-inspector.131") => "Merge recent selections into the current one",
        (Locale::Ja, "workspace-inspector.132") => "直近3件をすべて結合",
        (Locale::En, "workspace-inspector.132") => "Combine last 3",
        (Locale::Ja, "workspace-inspector.133") => "直近の選択がありません",
        (Locale::En, "workspace-inspector.133") => "No recent selections",
        (Locale::Ja, "workspace-inspector.134") => "ナイフ: 分割線をドラッグしてください",
        (Locale::En, "workspace-inspector.134") => "Knife: drag to draw the cut line",
        (Locale::Ja, "workspace-inspector.135") => "キャンセル",
        (Locale::En, "workspace-inspector.135") => "Cancel",
        (Locale::Ja, "workspace-inspector.136") => "ナイフ結果",
        (Locale::En, "workspace-inspector.136") => "Knife result",
        (Locale::Ja, "workspace-inspector.137") => "反転",
        (Locale::En, "workspace-inspector.137") => "Invert",
        (Locale::Ja, "workspace-inspector.138") => "閉じる",
        (Locale::En, "workspace-inspector.138") => "Close",
        _ => "[missing message]",
    }
}
