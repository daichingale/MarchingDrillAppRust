use drill_core::Locale;
use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WelcomeAction {
    OpenJson,
    OpenProject,
    NewShow,
    SimpleMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OnboardingState {
    pub welcome_seen: bool,
    pub coach_dismissed: bool,
    pub coach_step: u8,
    pub simple_drag_tip_seen: bool,
    pub simple_steps_dismissed: bool,
    /// Existing preference files omit this field. Missing means the person
    /// already had the full editor, so they stay there. A brand-new install
    /// uses [`Default`], which opens the simple field.
    #[serde(default = "existing_user_prefers_full_editor")]
    pub prefer_simple: bool,
    #[serde(skip)]
    pub show_welcome: bool,
    #[serde(skip)]
    pub show_help: bool,
    #[serde(skip)]
    persisted: String,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            welcome_seen: false,
            coach_dismissed: false,
            coach_step: 0,
            simple_drag_tip_seen: false,
            simple_steps_dismissed: false,
            prefer_simple: true,
            show_welcome: true,
            show_help: false,
            persisted: String::new(),
        }
    }
}

impl OnboardingState {
    pub fn load() -> Self {
        let mut state = preferences_path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .unwrap_or_default();
        state.show_welcome = !state.welcome_seen;
        state.show_help = false;
        state.persisted = serde_json::to_string(&state).unwrap_or_default();
        state
    }
    pub fn persist_if_changed(&mut self) {
        let Ok(json) = serde_json::to_string(self) else {
            return;
        };
        if json == self.persisted {
            return;
        }
        let Some(path) = preferences_path() else {
            return;
        };
        if path
            .parent()
            .is_none_or(|parent| std::fs::create_dir_all(parent).is_err())
        {
            return;
        }
        if drill_project::atomic_write(&path, json.as_bytes(), None).is_ok() {
            self.persisted = json;
        }
    }
    pub fn observe(&mut self, selected: bool, edited: bool, played: bool) {
        if self.coach_dismissed {
            return;
        }
        if self.coach_step == 0 && selected {
            self.coach_step = 1;
        }
        if self.coach_step == 1 && edited {
            self.coach_step = 2;
        }
        if self.coach_step == 2 && played {
            self.coach_step = 3;
        }
    }
    pub fn coach_message(&self, locale: Locale) -> Option<(&'static str, &'static str)> {
        if self.coach_dismissed || self.coach_step >= 3 {
            return None;
        }
        Some(match (locale, self.coach_step) {
            (Locale::Ja, 0) => (
                "ステップ 1 / 3",
                "フィールドの黄色い演者をクリックして選択します",
            ),
            (Locale::Ja, 1) => (
                "ステップ 2 / 3",
                "選択した演者をドラッグして隊形を編集します",
            ),
            (Locale::Ja, _) => (
                "ステップ 3 / 3",
                "上部の『再生』、または Space で動きを確認します",
            ),
            (Locale::En, 0) => (
                "STEP 1 OF 3",
                "Click a yellow performer on the field to select them",
            ),
            (Locale::En, 1) => (
                "STEP 2 OF 3",
                "Drag the selected performers to edit the formation",
            ),
            (Locale::En, _) => (
                "STEP 3 OF 3",
                "Choose Play above, or press Space, to preview the move",
            ),
        })
    }
    pub fn welcome_ui(&mut self, context: &egui::Context, locale: Locale) -> Option<WelcomeAction> {
        if !self.show_welcome {
            return None;
        }
        let mut action = None;
        egui::Modal::new(egui::Id::new("first_run_welcome")).show(context, |ui| {
            ui.set_max_width(570.0_f32.min(ui.available_width()));
            ui.heading(super::i18n::registered(locale, "onboarding.001"));
            ui.label(super::i18n::registered(locale, "onboarding.002"));
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(ui.visuals().faint_bg_color)
                .inner_margin(12)
                .corner_radius(6)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(super::i18n::registered(locale, "onboarding.003"))
                            .strong(),
                    );
                    ui.label(super::i18n::registered(locale, "onboarding.004"));
                    ui.small(super::i18n::registered(locale, "onboarding.005"));
                });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(super::i18n::registered(locale, "onboarding.025"))
                    .on_hover_text(super::i18n::registered(locale, "onboarding.026"))
                    .clicked()
                {
                    self.welcome_seen = true;
                    self.show_welcome = false;
                    action = Some(WelcomeAction::NewShow);
                }
                if ui
                    .button(super::i18n::registered(locale, "onboarding.027"))
                    .on_hover_text(super::i18n::registered(locale, "onboarding.028"))
                    .clicked()
                {
                    self.welcome_seen = true;
                    self.show_welcome = false;
                    action = Some(WelcomeAction::SimpleMode);
                }
                if ui
                    .button(super::i18n::registered(locale, "onboarding.006"))
                    .on_hover_text(super::i18n::registered(locale, "onboarding.007"))
                    .clicked()
                {
                    self.welcome_seen = true;
                    self.show_welcome = false;
                }
                if ui
                    .button(super::i18n::registered(locale, "onboarding.008"))
                    .on_hover_text(super::i18n::registered(locale, "onboarding.009"))
                    .clicked()
                {
                    self.welcome_seen = true;
                    self.show_welcome = false;
                    action = Some(WelcomeAction::OpenJson);
                }
                if ui
                    .button(super::i18n::registered(locale, "onboarding.010"))
                    .on_hover_text(super::i18n::registered(locale, "onboarding.011"))
                    .clicked()
                {
                    self.welcome_seen = true;
                    self.show_welcome = false;
                    action = Some(WelcomeAction::OpenProject);
                }
            });
            ui.separator();
            ui.small(super::i18n::registered(locale, "onboarding.012"));
        });
        action
    }
    pub fn help_ui(&mut self, context: &egui::Context, locale: Locale) {
        if !self.show_help {
            return;
        }
        egui::Window::new(super::i18n::registered(locale, "onboarding.013"))
            .open(&mut self.show_help)
            .default_width(520.0)
            .resizable(true)
            .scroll(true)
            .show(context, |ui| {
                ui.set_min_width(280.0_f32.min(ui.available_width()));
                ui.heading(super::i18n::registered(locale, "onboarding.014"));
                ui.label(super::i18n::registered(locale, "onboarding.015"));
                ui.separator();
                ui.label(
                    egui::RichText::new(super::i18n::registered(locale, "onboarding.016")).strong(),
                );
                ui.label(super::i18n::registered(locale, "onboarding.017"));
                ui.label(super::i18n::registered(locale, "onboarding.018"));
                ui.separator();
                ui.label(
                    egui::RichText::new(super::i18n::registered(locale, "onboarding.029")).strong(),
                );
                ui.label(super::i18n::registered(locale, "onboarding.030"));
                ui.separator();
                ui.label(
                    egui::RichText::new(super::i18n::registered(locale, "onboarding.019")).strong(),
                );
                ui.label(super::i18n::registered(locale, "onboarding.020"));
                ui.label(super::i18n::registered(locale, "onboarding.021"));
                ui.separator();
                ui.label(
                    egui::RichText::new(super::i18n::registered(locale, "onboarding.022")).strong(),
                );
                ui.label(super::i18n::registered(locale, "onboarding.023"));
                ui.separator();
                ui.small(super::i18n::registered(locale, "onboarding.024"));
            });
    }
}

fn existing_user_prefers_full_editor() -> bool {
    false
}

fn preferences_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA").map(std::path::PathBuf::from);
    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config"))
        });
    base.map(|base| base.join("DrillForge").join("ui-state.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coach_advances_after_actions_in_order() {
        let mut s = OnboardingState::default();
        s.observe(false, true, true);
        assert_eq!(s.coach_step, 0);
        s.observe(true, false, false);
        assert_eq!(s.coach_step, 1);
        s.observe(true, true, false);
        assert_eq!(s.coach_step, 2);
        s.observe(true, true, true);
        assert!(s.coach_message(Locale::Ja).is_none());
    }
    #[test]
    fn dismissal_stops_coaching() {
        let mut s = OnboardingState {
            coach_dismissed: true,
            ..Default::default()
        };
        s.observe(true, true, true);
        assert_eq!(s.coach_step, 0);
        assert!(s.coach_message(Locale::Ja).is_none());
    }
    #[test]
    fn coaching_is_localized() {
        let s = OnboardingState::default();
        assert_ne!(s.coach_message(Locale::Ja), s.coach_message(Locale::En));
    }
    #[test]
    fn fresh_install_opens_the_simple_field() {
        let state = OnboardingState::default();
        assert!(state.prefer_simple);
        assert!(!state.welcome_seen);
    }
    #[test]
    fn saved_preferences_without_the_new_flag_stay_on_the_full_editor() {
        let json = r#"{"welcome_seen":true,"coach_dismissed":true,"coach_step":3,"simple_drag_tip_seen":true,"simple_steps_dismissed":true}"#;
        let state: OnboardingState = serde_json::from_str(json).expect("older prefs still parse");
        assert!(state.welcome_seen);
        assert!(!state.prefer_simple);
    }
}
