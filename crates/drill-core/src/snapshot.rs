//! Immutable document checkpoints and deterministic branch comparison.

use crate::{Document, DrillError, MAX_TEXT_BYTES, PerformerId, SetId, SubsetId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DocumentSnapshot {
    pub name: String,
    pub document: Document,
}

impl DocumentSnapshot {
    pub fn capture(name: impl Into<String>, document: &Document) -> Result<Self, DrillError> {
        document.validate()?;
        let name = name.into();
        if name.is_empty() || name.len() > MAX_TEXT_BYTES {
            return Err(DrillError::InvalidEdit);
        }
        Ok(Self {
            name,
            document: document.clone(),
        })
    }

    pub fn compare(&self, other: &Self) -> SnapshotDiff {
        SnapshotDiff::between(&self.document, &other.document)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SnapshotDiff {
    pub added_sets: Vec<SetId>,
    pub removed_sets: Vec<SetId>,
    pub changed_sets: Vec<SetId>,
    pub changed_performers: Vec<PerformerId>,
    pub added_subsets: Vec<SubsetId>,
    pub removed_subsets: Vec<SubsetId>,
    pub changed_subsets: Vec<SubsetId>,
    pub document_properties_changed: bool,
    pub presentation_order_changed: bool,
}

impl SnapshotDiff {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    pub fn between(left: &Document, right: &Document) -> Self {
        let left_sets = left
            .sets
            .iter()
            .map(|set| (set.id, set))
            .collect::<std::collections::BTreeMap<_, _>>();
        let right_sets = right
            .sets
            .iter()
            .map(|set| (set.id, set))
            .collect::<std::collections::BTreeMap<_, _>>();
        let left_performers = left
            .performers
            .iter()
            .map(|p| (p.id, p))
            .collect::<std::collections::BTreeMap<_, _>>();
        let right_performers = right
            .performers
            .iter()
            .map(|p| (p.id, p))
            .collect::<std::collections::BTreeMap<_, _>>();
        let left_subsets = left
            .subsets
            .iter()
            .map(|s| (s.id, s))
            .collect::<std::collections::BTreeMap<_, _>>();
        let right_subsets = right
            .subsets
            .iter()
            .map(|s| (s.id, s))
            .collect::<std::collections::BTreeMap<_, _>>();
        let left_set_ids = left_sets.keys().copied().collect::<BTreeSet<_>>();
        let right_set_ids = right_sets.keys().copied().collect::<BTreeSet<_>>();
        let left_subset_ids = left_subsets.keys().copied().collect::<BTreeSet<_>>();
        let right_subset_ids = right_subsets.keys().copied().collect::<BTreeSet<_>>();
        Self {
            added_sets: right_set_ids.difference(&left_set_ids).copied().collect(),
            removed_sets: left_set_ids.difference(&right_set_ids).copied().collect(),
            changed_sets: left_set_ids
                .intersection(&right_set_ids)
                .copied()
                .filter(|id| left_sets[id] != right_sets[id])
                .collect(),
            changed_performers: left_performers
                .keys()
                .copied()
                .chain(right_performers.keys().copied())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter(|id| left_performers.get(id) != right_performers.get(id))
                .collect(),
            added_subsets: right_subset_ids
                .difference(&left_subset_ids)
                .copied()
                .collect(),
            removed_subsets: left_subset_ids
                .difference(&right_subset_ids)
                .copied()
                .collect(),
            changed_subsets: left_subset_ids
                .intersection(&right_subset_ids)
                .copied()
                .filter(|id| left_subsets[id] != right_subsets[id])
                .collect(),
            document_properties_changed: left.title != right.title
                || left.grid != right.grid
                || left.tempo != right.tempo
                || left.audio != right.audio
                || left.sections != right.sections
                || left.camera_program != right.camera_program,
            presentation_order_changed: left
                .sets
                .iter()
                .map(|set| set.id)
                .ne(right.sets.iter().map(|set| set.id))
                || left
                    .performers
                    .iter()
                    .map(|performer| performer.id)
                    .ne(right.performers.iter().map(|performer| performer.id))
                || left
                    .subsets
                    .iter()
                    .map(|subset| subset.id)
                    .ne(right.subsets.iter().map(|subset| subset.id)),
        }
    }
}

/// Stable identity for a design branch. It is persisted and never derived from
/// a presentation-order index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BranchId(NonZeroU64);

impl BranchId {
    pub const fn new(raw: u64) -> Option<Self> {
        match NonZeroU64::new(raw) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

impl core::fmt::Display for BranchId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.get().fmt(f)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesignBranch {
    pub id: BranchId,
    pub name: String,
    /// Immutable common ancestor used for three-way merge.
    pub base: DocumentSnapshot,
    pub current: DocumentSnapshot,
    pub created_unix_ms: u64,
    pub revision: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MergeField {
    Title,
    Grid,
    Tempo,
    Audio,
    Camera,
    Sections,
    Subsets,
    Performers,
    Sets,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MergeConflict {
    pub field: MergeField,
    pub summary: String,
}

/// Pure preview. Creating this value cannot mutate either branch or the open
/// document. A caller must explicitly select conflict resolutions and apply it.
#[derive(Clone, Debug, PartialEq)]
pub struct MergePreview {
    pub ours: BranchId,
    pub theirs: BranchId,
    pub candidate: Document,
    pub conflicts: Vec<MergeConflict>,
    pub diff: SnapshotDiff,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BranchStore {
    pub format_version: u16,
    pub active: BranchId,
    pub branches: BTreeMap<BranchId, DesignBranch>,
    next_id: u64,
}

impl BranchStore {
    pub const FORMAT_VERSION: u16 = 1;

    pub fn new(name: impl Into<String>, document: &Document) -> Result<Self, DrillError> {
        let id = BranchId::new(1).expect("one is a valid branch id");
        let name = validated_name(name.into())?;
        let snapshot = DocumentSnapshot::capture(name.clone(), document)?;
        let branch = DesignBranch {
            id,
            name,
            base: snapshot.clone(),
            current: snapshot,
            created_unix_ms: now_unix_ms(),
            revision: 1,
        };
        Ok(Self {
            format_version: Self::FORMAT_VERSION,
            active: id,
            branches: [(id, branch)].into(),
            next_id: 2,
        })
    }

    pub fn validate(&self) -> Result<(), DrillError> {
        if self.format_version != Self::FORMAT_VERSION || !self.branches.contains_key(&self.active)
        {
            return Err(DrillError::InvalidEdit);
        }
        for (id, branch) in &self.branches {
            if id != &branch.id
                || branch.name.is_empty()
                || branch.name.len() > MAX_TEXT_BYTES
                || branch.revision == 0
            {
                return Err(DrillError::InvalidEdit);
            }
            branch.base.document.validate()?;
            branch.current.document.validate()?;
        }
        if self.next_id == 0 || self.branches.keys().any(|id| id.get() >= self.next_id) {
            return Err(DrillError::InvalidEdit);
        }
        Ok(())
    }

    pub fn active_branch(&self) -> &DesignBranch {
        &self.branches[&self.active]
    }

    pub fn fork(
        &mut self,
        name: impl Into<String>,
        document: &Document,
    ) -> Result<BranchId, DrillError> {
        document.validate()?;
        let name = validated_name(name.into())?;
        let id = BranchId::new(self.next_id).ok_or(DrillError::InvalidEdit)?;
        self.next_id = self.next_id.checked_add(1).ok_or(DrillError::InvalidEdit)?;
        let base = DocumentSnapshot::capture(format!("{} base", name), document)?;
        let current = DocumentSnapshot::capture(name.clone(), document)?;
        self.branches.insert(
            id,
            DesignBranch {
                id,
                name,
                base,
                current,
                created_unix_ms: now_unix_ms(),
                revision: 1,
            },
        );
        Ok(id)
    }

    /// Records the open document in the active branch. Other branches and the
    /// immutable base snapshot are left untouched.
    pub fn checkpoint_active(&mut self, document: &Document) -> Result<(), DrillError> {
        document.validate()?;
        let branch = self
            .branches
            .get_mut(&self.active)
            .ok_or(DrillError::InvalidEdit)?;
        branch.revision = branch
            .revision
            .checked_add(1)
            .ok_or(DrillError::InvalidEdit)?;
        branch.current = DocumentSnapshot::capture(branch.name.clone(), document)?;
        Ok(())
    }

    pub fn switch_preview(&self, id: BranchId) -> Option<Document> {
        self.branches
            .get(&id)
            .map(|branch| branch.current.document.clone())
    }

    /// Changes only branch metadata after the caller has applied the preview via
    /// its normal undoable `Edit::ReplaceDocument` path.
    pub fn confirm_switch(&mut self, id: BranchId) -> Result<(), DrillError> {
        if !self.branches.contains_key(&id) {
            return Err(DrillError::InvalidEdit);
        }
        self.active = id;
        Ok(())
    }

    pub fn compare(&self, left: BranchId, right: BranchId) -> Option<SnapshotDiff> {
        Some(
            self.branches
                .get(&left)?
                .current
                .compare(&self.branches.get(&right)?.current),
        )
    }

    pub fn merge_preview(&self, theirs: BranchId) -> Result<MergePreview, DrillError> {
        let ours = self.active_branch();
        let theirs_branch = self.branches.get(&theirs).ok_or(DrillError::InvalidEdit)?;
        let base = &theirs_branch.base.document;
        let ours_doc = &ours.current.document;
        let theirs_doc = &theirs_branch.current.document;
        let mut candidate = ours_doc.clone();
        let mut conflicts = Vec::new();
        macro_rules! merge_field {
            ($field:ident, $kind:expr, $label:expr) => {
                if ours_doc.$field == base.$field {
                    candidate.$field = theirs_doc.$field.clone();
                } else if theirs_doc.$field != base.$field && ours_doc.$field != theirs_doc.$field {
                    conflicts.push(MergeConflict {
                        field: $kind,
                        summary: $label.into(),
                    });
                }
            };
        }
        merge_field!(title, MergeField::Title, "title changed on both branches");
        merge_field!(grid, MergeField::Grid, "grid changed on both branches");
        merge_field!(tempo, MergeField::Tempo, "tempo changed on both branches");
        merge_field!(audio, MergeField::Audio, "audio changed on both branches");
        merge_field!(
            camera_program,
            MergeField::Camera,
            "camera program changed on both branches"
        );
        merge_field!(
            sections,
            MergeField::Sections,
            "sections changed on both branches"
        );
        merge_field!(
            subsets,
            MergeField::Subsets,
            "subsets changed on both branches"
        );
        merge_field!(
            performers,
            MergeField::Performers,
            "performers changed on both branches"
        );
        merge_field!(sets, MergeField::Sets, "sets changed on both branches");
        candidate.validate()?;
        Ok(MergePreview {
            ours: ours.id,
            theirs,
            diff: SnapshotDiff::between(ours_doc, &candidate),
            candidate,
            conflicts,
        })
    }

    /// Explicit bulk resolution used after the caller has shown every conflict.
    /// `use_theirs=false` keeps the active branch values; `true` takes the
    /// incoming values for conflicting fields. Non-conflicting auto-merges stay.
    pub fn resolve_all_conflicts(
        &self,
        preview: &mut MergePreview,
        use_theirs: bool,
    ) -> Result<(), DrillError> {
        if preview.ours != self.active {
            return Err(DrillError::InvalidEdit);
        }
        let incoming = &self
            .branches
            .get(&preview.theirs)
            .ok_or(DrillError::InvalidEdit)?
            .current
            .document;
        if use_theirs {
            for conflict in &preview.conflicts {
                match conflict.field {
                    MergeField::Title => preview.candidate.title = incoming.title.clone(),
                    MergeField::Grid => preview.candidate.grid = incoming.grid.clone(),
                    MergeField::Tempo => preview.candidate.tempo = incoming.tempo.clone(),
                    MergeField::Audio => preview.candidate.audio = incoming.audio.clone(),
                    MergeField::Camera => {
                        preview.candidate.camera_program = incoming.camera_program.clone()
                    }
                    MergeField::Sections => preview.candidate.sections = incoming.sections.clone(),
                    MergeField::Subsets => preview.candidate.subsets = incoming.subsets.clone(),
                    MergeField::Performers => {
                        preview.candidate.performers = incoming.performers.clone()
                    }
                    MergeField::Sets => preview.candidate.sets = incoming.sets.clone(),
                }
            }
        }
        preview.conflicts.clear();
        preview.diff =
            SnapshotDiff::between(&self.active_branch().current.document, &preview.candidate);
        preview.candidate.validate()
    }

    pub fn confirm_merge(
        &mut self,
        preview: &MergePreview,
        document: &Document,
    ) -> Result<(), DrillError> {
        if preview.ours != self.active
            || !preview.conflicts.is_empty()
            || &preview.candidate != document
        {
            return Err(DrillError::InvalidEdit);
        }
        self.checkpoint_active(document)
    }
}

fn validated_name(name: String) -> Result<String, DrillError> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.len() > MAX_TEXT_BYTES {
        Err(DrillError::InvalidEdit)
    } else {
        Ok(name)
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod branch_tests {
    use super::*;

    #[test]
    fn ids_remain_stable_across_json_round_trip() {
        let document = Document::demo(2, 3);
        let mut store = BranchStore::new("Main", &document).unwrap();
        let idea = store.fork("Idea", &document).unwrap();
        let decoded: BranchStore =
            serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded.active, BranchId::new(1).unwrap());
        assert_eq!(decoded.branches[&idea].name, "Idea");
    }

    #[test]
    fn preview_never_mutates_source_and_switch_is_explicit() {
        let document = Document::demo(2, 2);
        let mut store = BranchStore::new("Main", &document).unwrap();
        let branch = store.fork("Alternative", &document).unwrap();
        let before = store.clone();
        let preview = store.switch_preview(branch).unwrap();
        assert_eq!(preview, document);
        assert_eq!(store, before);
        store.confirm_switch(branch).unwrap();
        assert_eq!(store.active, branch);
    }

    #[test]
    fn three_way_merge_auto_merges_independent_fields() {
        let base = Document::demo(2, 2);
        let mut store = BranchStore::new("Main", &base).unwrap();
        let idea = store.fork("Idea", &base).unwrap();
        let mut theirs = base.clone();
        theirs.title = "Theirs".into();
        store.confirm_switch(idea).unwrap();
        store.checkpoint_active(&theirs).unwrap();
        store.confirm_switch(BranchId::new(1).unwrap()).unwrap();
        let mut ours = base.clone();
        ours.tempo = crate::tempo::TempoMap::constant(144.0);
        store.checkpoint_active(&ours).unwrap();
        let preview = store.merge_preview(idea).unwrap();
        assert!(preview.conflicts.is_empty());
        assert_eq!(preview.candidate.title, "Theirs");
        assert_eq!(preview.candidate.tempo, ours.tempo);
    }

    #[test]
    fn divergent_field_requires_explicit_resolution() {
        let base = Document::demo(1, 2);
        let mut store = BranchStore::new("Main", &base).unwrap();
        let idea = store.fork("Idea", &base).unwrap();
        let mut theirs = base.clone();
        theirs.title = "B".into();
        store.confirm_switch(idea).unwrap();
        store.checkpoint_active(&theirs).unwrap();
        store.confirm_switch(BranchId::new(1).unwrap()).unwrap();
        let mut ours = base;
        ours.title = "A".into();
        store.checkpoint_active(&ours).unwrap();
        let preview = store.merge_preview(idea).unwrap();
        assert_eq!(preview.conflicts.len(), 1);
        assert!(store.confirm_merge(&preview, &preview.candidate).is_err());

        let mut keep_ours = preview.clone();
        store.resolve_all_conflicts(&mut keep_ours, false).unwrap();
        assert_eq!(keep_ours.candidate.title, "A");
        assert!(keep_ours.conflicts.is_empty());

        let mut take_theirs = preview;
        store.resolve_all_conflicts(&mut take_theirs, true).unwrap();
        assert_eq!(take_theirs.candidate.title, "B");
        assert!(take_theirs.conflicts.is_empty());
    }
}
