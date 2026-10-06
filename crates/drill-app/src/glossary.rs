//! Marching glossary: short everyday definitions for first-time users.
//! Session-only UI. Never touches the document or undo.
use drill_core::Locale;
use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Category {
    Field,
    Motion,
    Formation,
    Music,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    Frequent,
    Category(Category),
}

#[derive(Clone, Copy)]
struct Term {
    name_ja: &'static str,
    name_en: &'static str,
    category: Category,
    body_ja: &'static str,
    body_en: &'static str,
    app_ja: Option<&'static str>,
    app_en: Option<&'static str>,
    frequent: bool,
    sort_key: &'static str,
}

const TERMS: &[Term] = &[
    Term {
        name_ja: "セット",
        name_en: "Set",
        category: Category::Formation,
        body_ja: "隊形の一枚です。メンバーの立ち位置を、ひとつの場面として残します。",
        body_en: "One picture of the formation. It stores where everyone stands in that moment.",
        app_ja: Some("アプリでは「場面」"),
        app_en: Some("In the app: Scene"),
        frequent: true,
        sort_key: "せっと",
    },
    Term {
        name_ja: "カウント",
        name_en: "Count",
        category: Category::Music,
        body_ja: "音楽に合わせた拍の数え方です。何拍で次の形に着くかを決めます。",
        body_en: "Beats counted with the music. They decide how long a move takes.",
        app_ja: Some("アプリでは「拍」"),
        app_en: Some("In the app: counts"),
        frequent: true,
        sort_key: "かうんと",
    },
    Term {
        name_ja: "間隔",
        name_en: "Interval",
        category: Category::Field,
        body_ja: "人と人のあいだの距離です。並びのすきまをそろえるときに使います。",
        body_en: "The space between people. Used to keep a line even.",
        app_ja: None,
        app_en: None,
        frequent: false,
        sort_key: "かんかく",
    },
    Term {
        name_ja: "ドリル",
        name_en: "Drill",
        category: Category::Other,
        body_ja: "隊形と動きをまとめた作品のことです。見せる内容の全体を指します。",
        body_en: "The whole piece of formations and moves you are making.",
        app_ja: Some("アプリでは「作品」"),
        app_en: Some("In the app: Show"),
        frequent: true,
        sort_key: "どりる",
    },
    Term {
        name_ja: "フィールド",
        name_en: "Field",
        category: Category::Field,
        body_ja: "練習や本番で並ぶ場所です。グランドと同じ意味で使います。",
        body_en: "The ground you stand on in rehearsal and performance.",
        app_ja: Some("アプリでは「タップして置く」"),
        app_en: Some("In the app: Tap to place"),
        frequent: true,
        sort_key: "ふぃーるど",
    },
    Term {
        name_ja: "ヤードライン",
        name_en: "Yard line",
        category: Category::Field,
        body_ja: "フィールドに引かれた線です。位置の目安になります。",
        body_en: "A painted line on the field. It helps you judge position.",
        app_ja: None,
        app_en: None,
        frequent: false,
        sort_key: "やーどらいん",
    },
    Term {
        name_ja: "ハッシュ",
        name_en: "Hash",
        category: Category::Field,
        body_ja: "フィールド上のしるしです。左右の位置をそろえるときに使います。",
        body_en: "Marks on the field used to line up left and right.",
        app_ja: None,
        app_en: None,
        frequent: false,
        sort_key: "はっしゅ",
    },
    Term {
        name_ja: "フォーメーション",
        name_en: "Formation",
        category: Category::Formation,
        body_ja: "メンバーの並び方です。隊形と同じ意味で使います。",
        body_en: "How people are arranged. Another word for the shape on the field.",
        app_ja: None,
        app_en: None,
        frequent: true,
        sort_key: "ふぉーめーしょん",
    },
    Term {
        name_ja: "スライド",
        name_en: "Slide",
        category: Category::Motion,
        body_ja: "向きを変えずに、横や斜めへ動くことです。",
        body_en: "Moving sideways or diagonally without turning to face that way.",
        app_ja: None,
        app_en: None,
        frequent: false,
        sort_key: "すらいど",
    },
    Term {
        name_ja: "ステップサイズ",
        name_en: "Step size",
        category: Category::Motion,
        body_ja: "一歩の大きさです。歩幅のことです。",
        body_en: "How big one step is. Another word for stride length.",
        app_ja: None,
        app_en: None,
        frequent: false,
        sort_key: "すてっぷさいず",
    },
    Term {
        name_ja: "アテンション",
        name_en: "Attention",
        category: Category::Other,
        body_ja: "気をつけの姿勢です。動きの前にそろえます。",
        body_en: "The standing-ready posture. Everyone matches it before moving.",
        app_ja: None,
        app_en: None,
        frequent: false,
        sort_key: "あてんしょん",
    },
    Term {
        name_ja: "パレードレスト",
        name_en: "Parade rest",
        category: Category::Other,
        body_ja: "やすめの姿勢です。待つときの形です。",
        body_en: "A resting stance used while waiting.",
        app_ja: None,
        app_en: None,
        frequent: false,
        sort_key: "ぱれーどれすと",
    },
];

#[derive(Default)]
pub(crate) struct GlossaryState {
    pub open: bool,
    search: String,
    filter: Filter,
}

impl Term {
    fn name(self, locale: Locale) -> &'static str {
        match locale {
            Locale::Ja => self.name_ja,
            Locale::En => self.name_en,
        }
    }

    fn other_name(self, locale: Locale) -> &'static str {
        match locale {
            Locale::Ja => self.name_en,
            Locale::En => self.name_ja,
        }
    }

    fn body(self, locale: Locale) -> &'static str {
        match locale {
            Locale::Ja => self.body_ja,
            Locale::En => self.body_en,
        }
    }

    fn app_wording(self, locale: Locale) -> Option<&'static str> {
        match locale {
            Locale::Ja => self.app_ja,
            Locale::En => self.app_en,
        }
    }

    fn matches(self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.name_ja.contains(query)
            || self.name_en.to_lowercase().contains(&q)
            || self.body_ja.contains(query)
            || self.body_en.to_lowercase().contains(&q)
            || self.sort_key.contains(&q)
            || self.app_ja.is_some_and(|text| text.contains(query))
            || self
                .app_en
                .is_some_and(|text| text.to_lowercase().contains(&q))
    }
}

impl GlossaryState {
    pub fn open(&mut self) {
        self.open = true;
    }

    pub fn show(&mut self, context: &egui::Context, locale: Locale) {
        if !self.open {
            return;
        }
        let title = super::i18n::registered(locale, "glossary.002");
        let mut open = self.open;
        egui::Window::new(title)
            .id(egui::Id::new("marching-glossary"))
            .open(&mut open)
            .default_width(420.0)
            .default_height(520.0)
            .resizable(true)
            .show(context, |ui| {
                let search_hint = super::i18n::registered(locale, "glossary.003");
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .desired_width(f32::INFINITY)
                        .hint_text(search_hint),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    let frequent = super::i18n::registered(locale, "glossary.004");
                    let field = super::i18n::registered(locale, "glossary.005");
                    let motion = super::i18n::registered(locale, "glossary.006");
                    let formation = super::i18n::registered(locale, "glossary.007");
                    let music = super::i18n::registered(locale, "glossary.008");
                    let other = super::i18n::registered(locale, "glossary.009");
                    self.filter_chip(ui, Filter::Frequent, frequent);
                    self.filter_chip(ui, Filter::Category(Category::Field), field);
                    self.filter_chip(ui, Filter::Category(Category::Motion), motion);
                    self.filter_chip(ui, Filter::Category(Category::Formation), formation);
                    self.filter_chip(ui, Filter::Category(Category::Music), music);
                    self.filter_chip(ui, Filter::Category(Category::Other), other);
                });
                ui.add_space(8.0);
                let terms = self.visible_terms();
                if terms.is_empty() {
                    ui.label(
                        egui::RichText::new(super::i18n::registered(locale, "glossary.010"))
                            .color(super::app_theme::SECONDARY_TEXT),
                    );
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("glossary-cards")
                    .show(ui, |ui| {
                        for term in terms {
                            super::app_theme::surface_frame(ui).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    egui::RichText::new(term.name(locale)).strong().size(16.0),
                                );
                                ui.label(
                                    egui::RichText::new(term.other_name(locale))
                                        .small()
                                        .color(super::app_theme::SECONDARY_TEXT),
                                );
                                ui.add_space(4.0);
                                ui.label(term.body(locale));
                                if let Some(app) = term.app_wording(locale) {
                                    ui.small(
                                        egui::RichText::new(app)
                                            .color(super::app_theme::SECONDARY_TEXT),
                                    );
                                }
                            });
                            ui.add_space(8.0);
                        }
                    });
            });
        self.open = open;
    }

    fn filter_chip(&mut self, ui: &mut egui::Ui, filter: Filter, label: &'static str) {
        let selected = self.filter == filter;
        if ui.selectable_label(selected, label).clicked() {
            self.filter = filter;
        }
    }

    fn visible_terms(&self) -> Vec<&'static Term> {
        let query = self.search.trim();
        let mut terms: Vec<&'static Term> = TERMS
            .iter()
            .filter(|term| {
                let in_filter = match self.filter {
                    Filter::Frequent if query.is_empty() => term.frequent,
                    Filter::Frequent => true,
                    Filter::Category(category) => term.category == category,
                };
                in_filter && (query.is_empty() || term.matches(query))
            })
            .collect();
        if matches!(self.filter, Filter::Frequent) && query.is_empty() {
            terms.sort_by_key(|term| {
                TERMS
                    .iter()
                    .position(|seed| seed.name_ja == term.name_ja)
                    .unwrap_or(usize::MAX)
            });
        } else {
            terms.sort_by_key(|term| term.sort_key);
        }
        terms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_terms_are_present_and_searchable() {
        let names: Vec<_> = TERMS.iter().map(|term| term.name_ja).collect();
        for expected in [
            "セット",
            "カウント",
            "間隔",
            "ドリル",
            "フィールド",
            "ヤードライン",
            "ハッシュ",
            "フォーメーション",
            "スライド",
            "ステップサイズ",
            "アテンション",
            "パレードレスト",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
            assert!(
                TERMS.iter().any(|term| term.matches(expected)),
                "not searchable: {expected}"
            );
        }
        assert!(TERMS.iter().any(|term| term.matches("Set")));
        assert!(TERMS.iter().any(|term| term.matches("count")));
        assert!(TERMS.iter().any(|term| term.matches("場面")));
    }

    #[test]
    fn first_open_shows_frequent_terms_only() {
        let state = GlossaryState::default();
        let names: Vec<_> = state
            .visible_terms()
            .iter()
            .map(|term| term.name_ja)
            .collect();
        assert!(names.contains(&"セット"));
        assert!(names.contains(&"カウント"));
        assert!(names.contains(&"フィールド"));
        assert!(names.len() < TERMS.len());
        assert!(names.iter().all(|name| {
            TERMS
                .iter()
                .find(|term| term.name_ja == *name)
                .is_some_and(|term| term.frequent)
        }));
    }

    #[test]
    fn unknown_query_is_empty() {
        let state = GlossaryState {
            search: "zzzz-not-a-term".into(),
            ..GlossaryState::default()
        };
        assert!(state.visible_terms().is_empty());
    }

    #[test]
    fn category_lists_are_kana_sorted() {
        let state = GlossaryState {
            filter: Filter::Category(Category::Field),
            ..GlossaryState::default()
        };
        let keys: Vec<_> = state
            .visible_terms()
            .iter()
            .map(|term| term.sort_key)
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert!(
            state
                .visible_terms()
                .iter()
                .any(|term| term.name_ja == "フィールド")
        );
    }
}
