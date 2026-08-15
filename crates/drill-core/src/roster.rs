//! Performer sections and presentation metadata.

use crate::{Document, Performer, PerformerId, SectionId, SubsetId};
use serde::{Deserialize, Serialize};
use std::ops::Index;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OptionalColor(pub Option<[u8; 3]>);

impl From<Option<[u8; 3]>> for OptionalColor {
    fn from(value: Option<[u8; 3]>) -> Self {
        Self(value)
    }
}

impl Index<usize> for OptionalColor {
    type Output = u8;

    fn index(&self, index: usize) -> &Self::Output {
        const NEUTRAL: [u8; 3] = [128, 128, 128];
        &self.0.as_ref().unwrap_or(&NEUTRAL)[index]
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub id: SectionId,
    pub name: String,
    pub short: String,
    pub color: [u8; 3],
    pub order: u16,
}

/// A reusable, user-defined selection of performers.
///
/// Unlike [`Section`], a subset is not performer taxonomy: membership may
/// overlap, a performer may belong to any number of subsets, and deleting a
/// subset never changes performer metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subset {
    pub id: SubsetId,
    pub name: String,
    #[serde(default)]
    pub members: Vec<PerformerId>,
}

impl Subset {
    pub fn contains(&self, performer: PerformerId) -> bool {
        self.members.binary_search(&performer).is_ok()
    }
}

pub fn performers_in_subset(document: &Document, subset: SubsetId) -> Option<&[PerformerId]> {
    document
        .subsets
        .iter()
        .find(|candidate| candidate.id == subset)
        .map(|candidate| candidate.members.as_slice())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Symbol {
    Circle,
    Square,
    Triangle,
    Diamond,
    /// Default marker: the standard "X" used on printed drill charts.
    #[default]
    Cross,
    Star,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PerformerKind {
    #[default]
    Wind,
    Percussion,
    Guard,
    Prop,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerformerMetadata {
    pub label: String,
    pub symbol: Symbol,
    pub color: Option<[u8; 3]>,
    pub height_m: f32,
    pub kind: PerformerKind,
}

impl Performer {
    pub fn resolved_color(&self, sections: &[Section]) -> [u8; 3] {
        self.color.0.unwrap_or_else(|| {
            sections
                .iter()
                .find(|section| section.id == self.section)
                .map_or([128, 128, 128], |section| section.color)
        })
    }

    pub fn metadata(&self) -> PerformerMetadata {
        PerformerMetadata {
            label: self.label.clone(),
            symbol: self.symbol,
            color: self.color.0,
            height_m: self.height_m,
            kind: self.kind,
        }
    }
}

pub fn performers_in_section(document: &Document, section: SectionId) -> Vec<PerformerId> {
    document
        .performers
        .iter()
        .filter(|performer| performer.section == section)
        .map(|performer| performer.id)
        .collect()
}

pub fn sections_in_display_order(document: &Document) -> Vec<&Section> {
    let mut sections = document.sections.iter().collect::<Vec<_>>();
    sections.sort_by_key(|section| (section.order, section.id));
    sections
}

pub fn performers_of_kind(document: &Document, kind: PerformerKind) -> Vec<PerformerId> {
    document
        .performers
        .iter()
        .filter(|performer| performer.kind == kind)
        .map(|performer| performer.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_resolution_uses_override_section_then_neutral() {
        let mut document = Document::demo(1, 1);
        let performer = &mut document.performers[0];
        assert_eq!(performer.resolved_color(&document.sections), [245, 197, 66]);
        performer.color = OptionalColor(None);
        document.sections[0].color = [1, 2, 3];
        assert_eq!(performer.resolved_color(&document.sections), [1, 2, 3]);
        performer.section = SectionId::new(99).unwrap();
        assert_eq!(
            performer.resolved_color(&document.sections),
            [128, 128, 128]
        );
    }

    #[test]
    fn queries_are_stable_and_section_order_is_explicit() {
        let mut document = Document::demo(1, 2);
        let second = SectionId::new(2).unwrap();
        document.sections.push(Section {
            id: second,
            name: "Guard".into(),
            short: "CG".into(),
            color: [255, 0, 128],
            order: 0,
        });
        document.sections[0].order = 10;
        document.performers[1].section = second;
        document.performers[1].kind = PerformerKind::Guard;
        assert_eq!(
            performers_in_section(&document, second),
            vec![document.performers[1].id]
        );
        assert_eq!(performers_of_kind(&document, PerformerKind::Guard).len(), 1);
        assert_eq!(sections_in_display_order(&document)[0].id, second);
    }
}
