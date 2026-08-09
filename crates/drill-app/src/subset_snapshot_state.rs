use drill_core::snapshot::{BranchId, BranchStore, DocumentSnapshot, MergePreview, SnapshotDiff};
use drill_core::{Document, Locale, PerformerId, SubsetId};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SnapshotStateError {
    Unavailable,
    Invalid,
    Stale,
    Conflict,
    Io,
    Decode,
    Version,
    Validation,
}

impl SnapshotStateError {
    pub(crate) fn localized(self, locale: Locale) -> &'static str {
        use Locale::{En, Ja};
        match (self, locale) {
            (Self::Unavailable, Ja) => "分岐履歴を利用できません",
            (Self::Unavailable, En) => "Branch history is unavailable",
            (Self::Invalid, Ja) => "名前または操作内容が無効です",
            (Self::Invalid, En) => "The name or operation is invalid",
            (Self::Stale, Ja) => "分岐が変更されています。再度確認してください",
            (Self::Stale, En) => "The branch changed; review it again",
            (Self::Conflict, Ja) => "未解決のマージ競合があります",
            (Self::Conflict, En) => "There are unresolved merge conflicts",
            (Self::Io, Ja) => "アーカイブを読み書きできません",
            (Self::Io, En) => "The archive could not be read or written",
            (Self::Decode, Ja) => "アーカイブを解析できません",
            (Self::Decode, En) => "The archive could not be decoded",
            (Self::Version, Ja) => "このアーカイブ版には対応していません",
            (Self::Version, En) => "This archive version is unsupported",
            (Self::Validation, Ja) => "アーカイブ内容が無効です",
            (Self::Validation, En) => "The archive contents are invalid",
        }
    }
}

#[derive(Debug)]
pub(crate) enum Action {
    AddSubset {
        name: String,
    },
    RenameSubset {
        id: SubsetId,
        name: String,
    },
    SetMembers {
        id: SubsetId,
        members: Vec<PerformerId>,
    },
    SelectMembers {
        id: SubsetId,
    },
    RemoveSubset {
        id: SubsetId,
    },
    CaptureSnapshot {
        name: String,
    },
    RestoreSnapshot {
        index: usize,
    },
    ForkBranch {
        name: String,
    },
    CheckpointBranch,
    SwitchBranch {
        id: BranchId,
    },
    MergeBranch {
        id: BranchId,
        use_theirs: bool,
    },
}

#[derive(Default)]
pub(crate) struct SubsetSnapshotState {
    pub open: bool,
    new_subset_name: String,
    subset_names: BTreeMap<SubsetId, String>,
    snapshot_name: String,
    snapshots: Vec<DocumentSnapshot>,
    selected_snapshot: Option<usize>,
    confirm_restore: Option<usize>,
    status: String,
    branch_name: String,
    branches: Option<BranchStore>,
    selected_branch: Option<BranchId>,
    confirm_switch: Option<BranchId>,
    merge_preview: Option<MergePreview>,
}

#[derive(Serialize, Deserialize)]
struct SnapshotArchive {
    format_version: u32,
    snapshots: Vec<DocumentSnapshot>,
    #[serde(default)]
    branches: Option<BranchStore>,
}

impl SubsetSnapshotState {
    pub fn show(
        &mut self,
        context: &egui::Context,
        locale: Locale,
        document: &Document,
        selected_ids: &BTreeSet<PerformerId>,
    ) -> Option<Action> {
        if !self.open {
            return None;
        }
        let ja = locale == Locale::Ja;
        if self.branches.is_none() {
            self.branches = BranchStore::new(if ja { "メイン" } else { "Main" }, document).ok();
            self.selected_branch = self.branches.as_ref().map(|value| value.active);
        }
        let mut open = self.open;
        let mut action = None;
        egui::Window::new(if ja { "サブセット・スナップショット" } else { "Subsets & Snapshots" })
            .open(&mut open)
            .default_width(680.0)
            .default_height(600.0)
            .resizable(true)
            .show(context, |ui| {
                ui.heading(if ja { "保存した演者グループ" } else { "Saved performer groups" });
                ui.label(if ja {
                    "同じ演者を何度でも呼び出せます。作成・名称・メンバー・削除はすべてUndo/Redoできます。"
                } else {
                    "Recall performer groups at any time. Creation, names, membership and removal all support Undo/Redo."
                });
                ui.small(if ja {
                    format!("現在の選択: {}人", selected_ids.len())
                } else {
                    format!("Current selection: {} performers", selected_ids.len())
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_subset_name)
                        .hint_text(if ja { "例: ソリスト" } else { "e.g. Soloists" })
                        .desired_width(260.0));
                    let valid = !self.new_subset_name.trim().is_empty() && !selected_ids.is_empty();
                    if ui.add_enabled(valid, egui::Button::new(if ja { "＋ 選択から作成" } else { "+ Create from selection" })).clicked() {
                        action = Some(Action::AddSubset { name: self.new_subset_name.trim().to_owned() });
                    }
                });
                if selected_ids.is_empty() {
                    ui.small(if ja { "フィールド上で演者を選ぶと新規サブセットを作成できます。" } else { "Select performers on the field to create a subset." });
                }
                ui.add_space(6.0);
                egui::ScrollArea::vertical().id_salt("subset-list").max_height(230.0).show(ui, |ui| {
                    if document.subsets.is_empty() {
                        ui.weak(if ja { "保存済みサブセットはありません" } else { "No saved subsets" });
                    }
                    for subset in &document.subsets {
                        let draft = self.subset_names.entry(subset.id).or_insert_with(|| subset.name.clone());
                        ui.group(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong(format!("{} · {}", subset.name, if ja { format!("{}人", subset.members.len()) } else { format!("{} members", subset.members.len()) }));
                                if ui.button(if ja { "選択" } else { "Select" }).clicked() {
                                    action = Some(Action::SelectMembers { id: subset.id });
                                }
                                if ui.add_enabled(!selected_ids.is_empty(), egui::Button::new(if ja { "現在の選択で置換" } else { "Replace with selection" })).clicked() {
                                    action = Some(Action::SetMembers { id: subset.id, members: selected_ids.iter().copied().collect() });
                                }
                            });
                            ui.horizontal_wrapped(|ui| {
                                ui.add(egui::TextEdit::singleline(draft).desired_width(260.0));
                                let can_rename = !draft.trim().is_empty() && draft.trim() != subset.name;
                                if ui.add_enabled(can_rename, egui::Button::new(if ja { "名称を保存" } else { "Save name" })).clicked() {
                                    action = Some(Action::RenameSubset { id: subset.id, name: draft.trim().to_owned() });
                                }
                                if ui.button(if ja { "削除" } else { "Delete" }).clicked() {
                                    action = Some(Action::RemoveSubset { id: subset.id });
                                }
                            });
                        });
                    }
                });

                ui.separator();
                ui.heading(if ja { "設計スナップショット" } else { "Design snapshots" });
                ui.label(if ja {
                    "現在の設計を丸ごと記録し、後で差分を確認してから安全に復元できます。復元は1回のUndoで戻せます。"
                } else {
                    "Capture the entire design, inspect changes later, then restore safely. A restore is reversible with one Undo."
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.snapshot_name)
                        .hint_text(if ja { "例: 第2稿" } else { "e.g. Revision 2" })
                        .desired_width(260.0));
                    if ui.add_enabled(!self.snapshot_name.trim().is_empty(), egui::Button::new(if ja { "現在を記録" } else { "Capture current" })).clicked() {
                        action = Some(Action::CaptureSnapshot { name: self.snapshot_name.trim().to_owned() });
                    }
                    if ui.button(if ja { "アーカイブ保存…" } else { "Save archive…" }).clicked() {
                        self.save_dialog(ja);
                    }
                    if ui.button(if ja { "アーカイブ読込…" } else { "Load archive…" }).clicked() {
                        self.load_dialog(ja);
                    }
                });
                if !self.status.is_empty() { ui.small(&self.status); }
                egui::ScrollArea::vertical().id_salt("snapshot-list").max_height(180.0).show(ui, |ui| {
                    for (index, snapshot) in self.snapshots.iter().enumerate() {
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(self.selected_snapshot == Some(index), format!("{} · {} sets · {} performers", snapshot.name, snapshot.document.sets.len(), snapshot.document.performers.len())).clicked() {
                                self.selected_snapshot = Some(index);
                            }
                            if ui.button(if ja { "復元…" } else { "Restore…" }).clicked() { self.confirm_restore = Some(index); }
                        });
                    }
                });
                if let Some(index) = self.selected_snapshot.and_then(|i| self.snapshots.get(i).map(|_| i)) {
                    let diff = SnapshotDiff::between(&self.snapshots[index].document, document);
                    ui.group(|ui| show_diff(ui, ja, &diff));
                }

                ui.separator();
                ui.heading(if ja { "分岐履歴" } else { "Design branches" });
                ui.label(if ja {
                    "別案を原本から分離して保存します。切替とマージは差分確認後に実行され、Undoで元へ戻せます。"
                } else {
                    "Keep alternatives isolated from the original. Switching and merging require preview and remain undoable."
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.branch_name).hint_text(if ja { "例: エンディング案B" } else { "e.g. Ending B" }).desired_width(250.0));
                    if ui.add_enabled(!self.branch_name.trim().is_empty(), egui::Button::new(if ja { "現在から分岐" } else { "Fork current" })).clicked() {
                        action = Some(Action::ForkBranch { name: self.branch_name.trim().to_owned() });
                    }
                    if ui.button(if ja { "現在の分岐を更新" } else { "Checkpoint active" }).clicked() {
                        action = Some(Action::CheckpointBranch);
                    }
                });
                if let Some(branches) = &self.branches {
                    for branch in branches.branches.values() {
                        let active = branch.id == branches.active;
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(self.selected_branch == Some(branch.id), format!("{}{} · rev {}", if active { "● " } else { "" }, branch.name, branch.revision)).clicked() {
                                self.selected_branch = Some(branch.id);
                            }
                            if !active && ui.button(if ja { "切替…" } else { "Switch…" }).clicked() { self.confirm_switch = Some(branch.id); }
                            if !active && ui.button(if ja { "マージ確認…" } else { "Preview merge…" }).clicked() {
                                self.merge_preview = branches.merge_preview(branch.id).ok();
                            }
                        });
                    }
                    if let Some(selected) = self.selected_branch.filter(|id| *id != branches.active)
                        && let Some(diff) = branches.compare(branches.active, selected)
                    {
                        ui.group(|ui| show_diff(ui, ja, &diff));
                    }
                }
            });
        self.open = open;

        if let Some(index) = self.confirm_restore {
            let mut confirmation_open = true;
            egui::Window::new(super::i18n::registered(locale, "subset-snapshot-state.001"))
                .collapsible(false)
                .resizable(false)
                .open(&mut confirmation_open)
                .show(context, |ui| {
                    if let Some(snapshot) = self.snapshots.get(index) {
                        ui.label(if locale == Locale::Ja {
                            format!("「{}」へ戻しますか？", snapshot.name)
                        } else {
                            format!("Restore “{}”?", snapshot.name)
                        });
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 195, 75),
                            super::i18n::registered(locale, "subset-snapshot-state.002"),
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .button(super::i18n::registered(
                                    locale,
                                    "subset-snapshot-state.003",
                                ))
                                .clicked()
                            {
                                action = Some(Action::RestoreSnapshot { index });
                                self.confirm_restore = None;
                            }
                            if ui
                                .button(super::i18n::registered(
                                    locale,
                                    "subset-snapshot-state.004",
                                ))
                                .clicked()
                            {
                                self.confirm_restore = None;
                            }
                        });
                    }
                });
            if !confirmation_open {
                self.confirm_restore = None;
            }
        }
        if let Some(id) = self.confirm_switch {
            let mut popup = true;
            egui::Window::new(if ja {
                "分岐へ切替"
            } else {
                "Switch branch"
            })
            .open(&mut popup)
            .collapsible(false)
            .resizable(false)
            .show(context, |ui| {
                ui.label(if ja {
                    "現在の文書を選択した分岐へ置き換えます。続行しますか？"
                } else {
                    "Replace the open document with the selected branch?"
                });
                ui.small(if ja {
                    "現在の分岐を先に更新してください。切替操作自体はUndoできます。"
                } else {
                    "Checkpoint the current branch first. The switch itself is undoable."
                });
                ui.horizontal(|ui| {
                    if ui
                        .button(if ja { "切り替える" } else { "Switch" })
                        .clicked()
                    {
                        action = Some(Action::SwitchBranch { id });
                        self.confirm_switch = None;
                    }
                    if ui
                        .button(if ja { "キャンセル" } else { "Cancel" })
                        .clicked()
                    {
                        self.confirm_switch = None;
                    }
                });
            });
            if !popup {
                self.confirm_switch = None;
            }
        }
        if let Some(preview) = self.merge_preview.clone() {
            let mut popup = true;
            egui::Window::new(if ja { "マージ差分" } else { "Merge preview" }).open(&mut popup).default_width(480.0).show(context, |ui| {
                show_diff(ui, ja, &preview.diff);
                if preview.conflicts.is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(90, 210, 140), if ja { "競合はありません。原本は確認するまで変更されません。" } else { "No conflicts. The original remains unchanged until confirmation." });
                    if ui.button(if ja { "この内容をマージ" } else { "Apply this merge" }).clicked() { action = Some(Action::MergeBranch { id: preview.theirs, use_theirs: false }); self.merge_preview = None; }
                } else {
                    ui.colored_label(egui::Color32::from_rgb(255, 195, 75), if ja { "両方で変更された項目があります。自動適用は行いません。" } else { "Both branches changed these fields. Nothing will be applied automatically." });
                    for conflict in &preview.conflicts { ui.label(format!("• {:?}: {}", conflict.field, conflict.summary)); }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(if ja { "競合は現在側を採用してマージ" } else { "Keep current conflicts & merge" }).clicked() { action = Some(Action::MergeBranch { id: preview.theirs, use_theirs: false }); self.merge_preview = None; }
                        if ui.button(if ja { "競合は取込側を採用してマージ" } else { "Use incoming conflicts & merge" }).clicked() { action = Some(Action::MergeBranch { id: preview.theirs, use_theirs: true }); self.merge_preview = None; }
                    });
                }
            });
            if !popup {
                self.merge_preview = None;
            }
        }
        action
    }

    pub fn snapshot(&self, index: usize) -> Option<&DocumentSnapshot> {
        self.snapshots.get(index)
    }

    pub fn captured(
        &mut self,
        name: String,
        document: &Document,
    ) -> Result<(), SnapshotStateError> {
        let snapshot =
            DocumentSnapshot::capture(name, document).map_err(|_| SnapshotStateError::Invalid)?;
        self.snapshots.push(snapshot);
        self.selected_snapshot = Some(self.snapshots.len() - 1);
        self.snapshot_name.clear();
        Ok(())
    }

    pub fn subset_added(&mut self) {
        self.new_subset_name.clear();
    }

    pub fn forked(&mut self, name: String, document: &Document) -> Result<(), SnapshotStateError> {
        let store = self
            .branches
            .as_mut()
            .ok_or(SnapshotStateError::Unavailable)?;
        let id = store
            .fork(name, document)
            .map_err(|_| SnapshotStateError::Invalid)?;
        self.selected_branch = Some(id);
        self.branch_name.clear();
        Ok(())
    }

    pub fn checkpoint(&mut self, document: &Document) -> Result<(), SnapshotStateError> {
        self.branches
            .as_mut()
            .ok_or(SnapshotStateError::Unavailable)?
            .checkpoint_active(document)
            .map_err(|_| SnapshotStateError::Invalid)
    }

    pub fn branch_document(&self, id: BranchId) -> Option<Document> {
        self.branches.as_ref()?.switch_preview(id)
    }

    pub fn switched(&mut self, id: BranchId) -> Result<(), SnapshotStateError> {
        self.branches
            .as_mut()
            .ok_or(SnapshotStateError::Unavailable)?
            .confirm_switch(id)
            .map_err(|_| SnapshotStateError::Stale)
    }

    pub fn merge_candidate(&self, id: BranchId) -> Result<MergePreview, SnapshotStateError> {
        self.branches
            .as_ref()
            .ok_or(SnapshotStateError::Unavailable)?
            .merge_preview(id)
            .map_err(|_| SnapshotStateError::Stale)
    }

    pub fn resolve_merge(
        &self,
        preview: &mut MergePreview,
        use_theirs: bool,
    ) -> Result<(), SnapshotStateError> {
        self.branches
            .as_ref()
            .ok_or(SnapshotStateError::Unavailable)?
            .resolve_all_conflicts(preview, use_theirs)
            .map_err(|_| SnapshotStateError::Conflict)
    }

    pub fn merged(
        &mut self,
        preview: &MergePreview,
        document: &Document,
    ) -> Result<(), SnapshotStateError> {
        self.branches
            .as_mut()
            .ok_or(SnapshotStateError::Unavailable)?
            .confirm_merge(preview, document)
            .map_err(|_| SnapshotStateError::Stale)
    }

    fn save_dialog(&mut self, ja: bool) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DrillForge Snapshots", &["drillsnapshots"])
            .set_file_name("design.drillsnapshots")
            .save_file()
        else {
            return;
        };
        match save_archive(&path, &self.snapshots, self.branches.as_ref()) {
            Ok(()) => {
                self.status = if ja {
                    "スナップショットを保存しました"
                } else {
                    "Snapshot archive saved"
                }
                .into()
            }
            Err(error) => {
                self.status = error
                    .localized(if ja { Locale::Ja } else { Locale::En })
                    .into()
            }
        }
    }

    fn load_dialog(&mut self, ja: bool) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DrillForge Snapshots", &["drillsnapshots"])
            .pick_file()
        else {
            return;
        };
        match load_archive(&path) {
            Ok((snapshots, branches)) => {
                self.snapshots = snapshots;
                self.branches = branches;
                self.selected_snapshot = (!self.snapshots.is_empty()).then_some(0);
                self.status = if ja {
                    "スナップショットを読み込みました"
                } else {
                    "Snapshot archive loaded"
                }
                .into();
            }
            Err(error) => {
                self.status = error
                    .localized(if ja { Locale::Ja } else { Locale::En })
                    .into()
            }
        }
    }
}

fn show_diff(ui: &mut egui::Ui, ja: bool, diff: &SnapshotDiff) {
    ui.strong(if ja {
        "選択した記録 → 現在 の差分"
    } else {
        "Selected snapshot → current differences"
    });
    if diff.is_empty() {
        ui.label(if ja {
            "変更はありません"
        } else {
            "No changes"
        });
        return;
    }
    ui.label(if ja {
        format!(
            "セット: +{} / −{} / 変更{}",
            diff.added_sets.len(),
            diff.removed_sets.len(),
            diff.changed_sets.len()
        )
    } else {
        format!(
            "Sets: +{} / −{} / {} changed",
            diff.added_sets.len(),
            diff.removed_sets.len(),
            diff.changed_sets.len()
        )
    });
    ui.label(if ja {
        format!("演者変更: {}人", diff.changed_performers.len())
    } else {
        format!("Changed performers: {}", diff.changed_performers.len())
    });
    ui.label(if ja {
        format!(
            "サブセット: +{} / −{} / 変更{}",
            diff.added_subsets.len(),
            diff.removed_subsets.len(),
            diff.changed_subsets.len()
        )
    } else {
        format!(
            "Subsets: +{} / −{} / {} changed",
            diff.added_subsets.len(),
            diff.removed_subsets.len(),
            diff.changed_subsets.len()
        )
    });
    if diff.document_properties_changed {
        ui.label(if ja {
            "文書設定・グリッド・テンポ等に変更あり"
        } else {
            "Document, grid, tempo or related properties changed"
        });
    }
    if diff.presentation_order_changed {
        ui.label(if ja {
            "表示順に変更あり"
        } else {
            "Presentation order changed"
        });
    }
}

fn save_archive(
    path: &Path,
    snapshots: &[DocumentSnapshot],
    branches: Option<&BranchStore>,
) -> Result<(), SnapshotStateError> {
    let data = serde_json::to_vec_pretty(&SnapshotArchive {
        format_version: 2,
        snapshots: snapshots.to_vec(),
        branches: branches.cloned(),
    })
    .map_err(|_| SnapshotStateError::Validation)?;
    drill_project::atomic_write(path, &data, None).map_err(|_| SnapshotStateError::Io)
}

fn load_archive(
    path: &Path,
) -> Result<(Vec<DocumentSnapshot>, Option<BranchStore>), SnapshotStateError> {
    let bytes = std::fs::read(path).map_err(|_| SnapshotStateError::Io)?;
    let archive: SnapshotArchive =
        serde_json::from_slice(&bytes).map_err(|_| SnapshotStateError::Decode)?;
    if !(1..=2).contains(&archive.format_version) {
        return Err(SnapshotStateError::Version);
    }
    for snapshot in &archive.snapshots {
        if snapshot.name.is_empty() {
            return Err(SnapshotStateError::Validation);
        }
        snapshot
            .document
            .validate()
            .map_err(|_| SnapshotStateError::Validation)?;
    }
    if let Some(branches) = &archive.branches {
        branches
            .validate()
            .map_err(|_| SnapshotStateError::Validation)?;
    }
    Ok((archive.snapshots, archive.branches))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_round_trip_and_validation() {
        let document = Document::demo(2, 3);
        let snapshot = DocumentSnapshot::capture("draft", &document).unwrap();
        let mut branches = BranchStore::new("Main", &document).unwrap();
        let branch_id = branches.fork("Alternative", &document).unwrap();
        let json = serde_json::to_vec(&SnapshotArchive {
            format_version: 2,
            snapshots: vec![snapshot.clone()],
            branches: Some(branches.clone()),
        })
        .unwrap();
        let archive: SnapshotArchive = serde_json::from_slice(&json).unwrap();
        assert_eq!(archive.snapshots, vec![snapshot]);
        assert_eq!(archive.format_version, 2);
        let restored = archive.branches.unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.branches[&branch_id].name, "Alternative");
    }

    #[test]
    fn version_one_archive_without_branches_remains_readable() {
        let json = r#"{"format_version":1,"snapshots":[]}"#;
        let archive: SnapshotArchive = serde_json::from_str(json).unwrap();
        assert!(archive.branches.is_none());
    }

    #[test]
    fn archive_errors_are_typed_and_localized() {
        let missing = std::env::temp_dir().join(format!(
            "drillforge-missing-snapshot-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&missing);
        assert_eq!(load_archive(&missing), Err(SnapshotStateError::Io));

        let path = std::env::temp_dir().join(format!(
            "drillforge-invalid-snapshot-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, b"not json").unwrap();
        assert_eq!(load_archive(&path), Err(SnapshotStateError::Decode));
        std::fs::write(&path, br#"{"format_version":99,"snapshots":[]}"#).unwrap();
        assert_eq!(load_archive(&path), Err(SnapshotStateError::Version));
        std::fs::write(
            &path,
            serde_json::to_vec(&SnapshotArchive {
                format_version: 2,
                snapshots: vec![DocumentSnapshot {
                    name: String::new(),
                    document: Document::demo(1, 1),
                }],
                branches: None,
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(load_archive(&path), Err(SnapshotStateError::Validation));
        let _ = std::fs::remove_file(path);

        for error in [
            SnapshotStateError::Unavailable,
            SnapshotStateError::Invalid,
            SnapshotStateError::Stale,
            SnapshotStateError::Conflict,
            SnapshotStateError::Io,
            SnapshotStateError::Decode,
            SnapshotStateError::Version,
            SnapshotStateError::Validation,
        ] {
            let ja = error.localized(Locale::Ja);
            let en = error.localized(Locale::En);
            assert_ne!(ja, en);
            assert!(
                !en.chars()
                    .any(|ch| matches!(ch, '\u{3040}'..='\u{30ff}' | '\u{4e00}'..='\u{9fff}'))
            );
        }
    }
}
