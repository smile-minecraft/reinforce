//! The GPUI desktop workbench: a three-column tabbed workspace matching the
//! cross-platform design (collapsible & drag-resizable left and right sidebars,
//! top multi-tab bar, vertical chapter notes with animated check-off collapse,
//! nested subject groups, and grouped quiz view with a fixed bottom bar).
//!
//! State transitions live in [`crate::model`] so business logic remains covered
//! by `cargo test` without needing a GPU context.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyDownEvent, MouseButton, MouseMoveEvent, MouseUpEvent, Render, Rgba, SharedString, Window,
    WindowBounds, WindowOptions, div, img, point, px, relative, rgb, rgba, size,
};

use reinforce_core::content::{ContentBlock, KnowledgeCard, OutlineNode, QuestionCard};
use reinforce_core::db::catalog::NoteOutline;
use reinforce_core::quiz::Score;
use reinforce_core::{KnowledgeCardId, NoteId, SubjectGroupId, Timestamp, User};

use crate::markdown::{Block as MarkdownBlock, parse};
use crate::model::{
    ConfidenceLevel5, Library, NoteCheckState, PracticalCountdown, QuestionUnit, Quiz, QuizMode,
    QuizTargets, Summary, TabManager, WorkspaceTab, WorkspaceTabKind, accepts_text,
    build_subject_group_tree, desktop_options, flatten_subject_group_tree, is_descendant_or_self,
};
use crate::render::{Figure, Renderer};
use crate::store::Store;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const MIN_LEFT_WIDTH: f32 = 180.0;
const MAX_LEFT_WIDTH: f32 = 340.0;
const MIN_RIGHT_WIDTH: f32 = 220.0;
const MAX_RIGHT_WIDTH: f32 = 380.0;
const DISPLAY_NAME_SETTING: &str = "desktop.display_name";
const PRACTICAL_SETTING: &str = "desktop.practical_exam_at";

fn paper() -> Rgba {
    rgb(0xf7f6f3)
}
fn panel() -> Rgba {
    rgb(0xf0eeea)
}
fn surface() -> Rgba {
    rgb(0xffffff)
}
fn line() -> Rgba {
    rgb(0xe2dfd8)
}
fn ink() -> Rgba {
    rgb(0x1f2428)
}
fn muted() -> Rgba {
    rgb(0x6b7280)
}
fn accent() -> Rgba {
    rgb(0x1e6b58)
}
fn accent_soft() -> Rgba {
    rgb(0xe3f1ec)
}
fn good() -> Rgba {
    rgb(0x237a4b)
}
fn good_soft() -> Rgba {
    rgb(0xe4f3ea)
}
fn bad() -> Rgba {
    rgb(0xb4432a)
}
fn bad_soft() -> Rgba {
    rgb(0xf9e7e3)
}
fn warn() -> Rgba {
    rgb(0xb46916)
}
fn warn_soft() -> Rgba {
    rgb(0xfdf3e3)
}

/// Which pane the window opens with. Used by scripts and screenshots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StartScreen {
    #[default]
    Reading,
    Card(usize),
    Quiz(QuizMode),
}

/// Startup knobs that do not belong to the database.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    pub start: StartScreen,
    /// Option keys to preselect on the first question, for screenshots and demos.
    pub preselect: Vec<String>,
}

impl StartScreen {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "reading" | "note" => Some(StartScreen::Reading),
            "card" => Some(StartScreen::Card(0)),
            value if value.starts_with("card:") => {
                let index = value["card:".len()..].parse().ok()?;
                Some(StartScreen::Card(index))
            }
            "daily" => Some(StartScreen::Quiz(QuizMode::DailyDue)),
            "exam" | "full" | "quiz" => Some(StartScreen::Quiz(QuizMode::FullExam)),
            "topic" => Some(StartScreen::Quiz(QuizMode::TopicDrill)),
            "practical" => Some(StartScreen::Quiz(QuizMode::PracticalExam)),
            "weakness" => Some(StartScreen::Quiz(QuizMode::WeaknessDrill)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResizeHandle {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TextFocus {
    None,
    GroupName,
    DisplayName,
    PracticalDate,
    QuizAnswer { session_key: u64, index: usize },
}

#[derive(Debug, Clone)]
struct GroupEditorState {
    editing_id: Option<SubjectGroupId>,
    parent_id: Option<SubjectGroupId>,
    name: String,
    subjects: BTreeSet<String>,
}

impl GroupEditorState {
    fn new_child(parent_id: Option<SubjectGroupId>) -> Self {
        Self {
            editing_id: None,
            parent_id,
            name: String::new(),
            subjects: BTreeSet::new(),
        }
    }

    fn from_group(group: &reinforce_core::SubjectGroup) -> Self {
        Self {
            editing_id: Some(group.id.clone()),
            parent_id: group.parent_id.clone(),
            name: group.name.clone(),
            subjects: group.subjects.iter().cloned().collect(),
        }
    }
}

/// The window root.
pub struct DesktopApp {
    store: Store,
    renderer: Renderer,
    library: Library,
    tabs: TabManager,
    next_quiz_key: u64,
    quizzes: HashMap<u64, Quiz>,
    summaries: HashMap<u64, Summary>,
    note_checks: NoteCheckState,
    anim_running: bool,
    left_open: bool,
    right_open: bool,
    left_width: f32,
    right_width: f32,
    dragging: Option<ResizeHandle>,
    group_editor: Option<GroupEditorState>,
    text_focus: TextFocus,
    display_name: String,
    practical_date_draft: String,
    practical: Option<PracticalCountdown>,
    user: User,
    attempts: usize,
    status_notice: Option<String>,
    error: Option<String>,
    focus: FocusHandle,
}

impl DesktopApp {
    pub fn new(
        store: Store,
        renderer: Renderer,
        practical: Option<PracticalCountdown>,
        launch: LaunchOptions,
        cx: &mut Context<Self>,
    ) -> Self {
        let user = match store.local_user() {
            Ok(user) => user,
            Err(error) => User {
                id: reinforce_core::UserId::generate(),
                display_name: format!("Local Learner ({error})"),
                created_at: Timestamp::now(),
            },
        };
        let display_name = store
            .setting(DISPLAY_NAME_SETTING)
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| user.display_name.clone());
        let practical_date_draft = store
            .setting(PRACTICAL_SETTING)
            .ok()
            .flatten()
            .unwrap_or_default();

        let mut app = Self {
            store,
            renderer,
            library: Library::default(),
            tabs: TabManager::default(),
            next_quiz_key: 1,
            quizzes: HashMap::new(),
            summaries: HashMap::new(),
            note_checks: NoteCheckState::default(),
            anim_running: false,
            left_open: true,
            right_open: true,
            left_width: 224.0,
            right_width: 276.0,
            dragging: None,
            group_editor: None,
            text_focus: TextFocus::None,
            display_name,
            practical_date_draft,
            practical,
            user,
            attempts: 0,
            status_notice: None,
            error: None,
            focus: cx.focus_handle(),
        };
        app.reload();

        match launch.start {
            StartScreen::Reading => {
                if let Some(first_note) = app.library.notes.first().cloned() {
                    app.open_note_tab(first_note.id);
                }
            }
            StartScreen::Card(index) => {
                if let Some(card) = app.library.cards.get(index).cloned() {
                    app.open_card_tab(card.id, card.title);
                }
            }
            StartScreen::Quiz(mode) => {
                if let Some(session_key) = app.start_quiz_in_new_tab(mode, None, None)
                    && let Some(quiz) = app.quizzes.get_mut(&session_key)
                {
                    let keys = launch.preselect.clone();
                    quiz.select_many(&keys);
                }
            }
        }
        app.spawn_clock(cx);
        app
    }

    fn reload(&mut self) {
        let prev_selected_group = self.library.selected_group_id.clone();
        match Library::load(&self.store) {
            Ok(mut library) => {
                if let Some(gid) = prev_selected_group
                    && library.subject_groups.iter().any(|g| g.id == gid)
                {
                    library.selected_group_id = Some(gid);
                }
                self.library = library;
                if let Some(first) = self.library.notes.first().cloned() {
                    let _ = self.library.load_outline(&self.store, &first.id);
                }
                let open_note_ids: Vec<NoteId> = self
                    .tabs
                    .tabs()
                    .iter()
                    .filter_map(|tab| match &tab.kind {
                        WorkspaceTabKind::Note(id) => Some(id.clone()),
                        _ => None,
                    })
                    .collect();
                for note_id in open_note_ids {
                    let _ = self.library.load_outline(&self.store, &note_id);
                }
            }
            Err(error) => self.error = Some(format!("無法讀取資料庫：{error}")),
        }
        self.attempts = self.store.attempt_count(&self.user.id).unwrap_or(0);
    }

    fn open_note_tab(&mut self, note_id: NoteId) {
        let title = self
            .library
            .notes
            .iter()
            .find(|note| note.id == note_id)
            .map(|note| note.title.clone())
            .unwrap_or_else(|| "筆記".to_owned());
        if let Err(error) = self.library.load_outline(&self.store, &note_id) {
            self.error = Some(format!("無法載入筆記：{error}"));
            return;
        }
        self.error = None;
        self.status_notice = None;
        self.tabs.open_or_focus(WorkspaceTab::note(note_id, title));
    }

    fn open_card_tab(&mut self, card_id: KnowledgeCardId, title: String) {
        self.error = None;
        self.tabs.open_or_focus(WorkspaceTab::card(card_id, title));
    }

    fn close_tab(&mut self, tab_id: &str) {
        if let Some(WorkspaceTabKind::Quiz(session_key)) = self.tabs.close(tab_id) {
            self.quizzes.remove(&session_key);
            self.summaries.remove(&session_key);
            if matches!(
                self.text_focus,
                TextFocus::QuizAnswer { session_key: k, .. } if k == session_key
            ) {
                self.text_focus = TextFocus::None;
            }
        }
    }

    fn start_quiz_in_new_tab(
        &mut self,
        mode: QuizMode,
        explicit_targets: Option<QuizTargets>,
        custom_title: Option<String>,
    ) -> Option<u64> {
        let targets = explicit_targets.unwrap_or_else(|| self.library.targets());
        let now = Timestamp::now();
        match Quiz::start(
            &self.store,
            mode,
            &targets,
            &desktop_options(),
            &self.user.id,
            now,
        ) {
            Ok(quiz) if quiz.is_empty() => {
                self.error = Some(format!("{}：目前範圍內沒有可出的題目", mode.title()));
                None
            }
            Ok(quiz) => {
                let key = self.next_quiz_key;
                self.next_quiz_key += 1;
                let title = custom_title.unwrap_or_else(|| {
                    if let Some(group) = self.library.selected_group() {
                        format!("複習 {}", group.name)
                    } else {
                        format!("複習 {}", mode.title())
                    }
                });
                self.quizzes.insert(key, quiz);
                self.summaries.remove(&key);
                self.error = None;
                self.status_notice = None;
                self.tabs.open_or_focus(WorkspaceTab::quiz(key, title));
                Some(key)
            }
            Err(error) => {
                self.error = Some(format!("{}：{error}", mode.title()));
                None
            }
        }
    }

    fn start_card_review_tab(&mut self, card: &KnowledgeCard) {
        let targets = QuizTargets {
            exam_set: None,
            cards: vec![card.id.clone()],
            subjects: None,
        };
        let title = format!("複習 {}", card.title);
        self.start_quiz_in_new_tab(QuizMode::TopicDrill, Some(targets), Some(title));
    }

    fn start_exam_set_tab(&mut self, exam_index: usize) {
        let Some(summary) = self.library.exam_sets.get(exam_index).cloned() else {
            return;
        };
        let targets = QuizTargets {
            exam_set: Some(summary.exam_set.id.clone()),
            cards: Vec::new(),
            subjects: None,
        };
        let title = format!("複習 {}", summary.exam_set.title);
        self.start_quiz_in_new_tab(QuizMode::FullExam, Some(targets), Some(title));
    }

    fn active_quiz_key(&self) -> Option<u64> {
        match self.tabs.active().kind {
            WorkspaceTabKind::Quiz(key) => Some(key),
            _ => None,
        }
    }

    fn submit_active_quiz_unit(&mut self) {
        let Some(key) = self.active_quiz_key() else {
            return;
        };
        let now = Timestamp::now();
        let Some(quiz) = self.quizzes.get_mut(&key) else {
            return;
        };
        let deferred = quiz.mode().deferred();
        match quiz.submit_current_unit(now) {
            Ok(0) => {}
            Ok(count) => {
                self.error = None;
                if deferred {
                    self.status_notice = Some(format!("已暫存 {count} 題作答，交卷後統一計分"));
                } else {
                    self.status_notice = None;
                }
            }
            Err(error) => self.error = Some(format!("送出作答失敗：{error}")),
        }
    }

    fn finish_quiz(&mut self, session_key: u64) {
        let Some(mut quiz) = self.quizzes.remove(&session_key) else {
            return;
        };
        let now = Timestamp::now();
        let mode = quiz.mode();
        let written = match quiz.commit(&mut self.store, &self.user.id, now) {
            Ok(written) => written,
            Err(error) => {
                self.error = Some(format!("寫入作答紀錄失敗：{error}"));
                0
            }
        };
        let score = quiz.score().unwrap_or(Score {
            correct: 0,
            scoreable: 0,
            voided: 0,
            ratio: 0.0,
        });
        self.summaries
            .insert(session_key, Summary::new(mode, &score, written));
        self.attempts = self.store.attempt_count(&self.user.id).unwrap_or(0);
        self.status_notice = None;
    }

    fn toggle_note_card_check(
        &mut self,
        note_id: NoteId,
        card_id: KnowledgeCardId,
        cx: &mut Context<Self>,
    ) {
        self.note_checks.toggle(&note_id, &card_id);
        self.ensure_animation_loop(cx);
        cx.notify();
    }

    fn reset_note_checks(&mut self, note_id: NoteId, cx: &mut Context<Self>) {
        self.note_checks.reset_note(&note_id);
        self.ensure_animation_loop(cx);
        cx.notify();
    }

    fn ensure_animation_loop(&mut self, cx: &mut Context<Self>) {
        if self.anim_running {
            return;
        }
        self.anim_running = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let keep_going = this
                    .update(cx, |app, cx| {
                        let still_animating = app.note_checks.step_animations(0.016);
                        if !still_animating {
                            app.anim_running = false;
                        }
                        cx.notify();
                        still_animating
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        })
        .detach();
    }

    fn spawn_clock(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let alive = this.update(cx, |app, cx| {
                    let Some(active_key) = app.active_quiz_key() else {
                        return;
                    };
                    let timed = app
                        .quizzes
                        .get(&active_key)
                        .is_some_and(|quiz| quiz.seconds_left(Timestamp::now()).is_some());
                    if !timed {
                        return;
                    }
                    let now = Timestamp::now();
                    let finished = if let Some(quiz) = app.quizzes.get_mut(&active_key) {
                        let _ = quiz.tick(now);
                        quiz.is_finished()
                    } else {
                        false
                    };
                    if finished {
                        app.finish_quiz(active_key);
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn save_group_editor(&mut self) {
        let Some(editor) = self.group_editor.clone() else {
            return;
        };
        let name = editor.name.trim();
        if name.is_empty() {
            self.error = Some("分組名稱不能空白".to_owned());
            return;
        }
        let subjects: Vec<String> = editor.subjects.into_iter().collect();
        let now = Timestamp::now();
        let result = match editor.editing_id {
            Some(id) => self.store.update_subject_group(
                &id,
                editor.parent_id.as_ref(),
                name,
                &subjects,
                0,
                now,
            ),
            None => {
                self.store
                    .create_subject_group(editor.parent_id.as_ref(), name, &subjects, 0, now)
            }
        };
        match result {
            Ok(saved) => {
                self.error = None;
                self.group_editor = None;
                self.text_focus = TextFocus::None;
                let _ = self.library.reload_subject_groups(&self.store);
                self.library.selected_group_id = Some(saved.id);
            }
            Err(error) => {
                self.error = Some(format!("儲存科目分組失敗：{error}"));
            }
        }
    }

    fn delete_subject_group(&mut self, id: &SubjectGroupId) {
        let should_clear_selected = self
            .library
            .selected_group_id
            .as_ref()
            .is_some_and(|sel| is_descendant_or_self(&self.library.subject_groups, sel, id));
        match self.store.delete_subject_group(id) {
            Ok(_) => {
                if should_clear_selected {
                    self.library.selected_group_id = None;
                }
                if self
                    .group_editor
                    .as_ref()
                    .and_then(|e| e.editing_id.as_ref())
                    == Some(id)
                {
                    self.group_editor = None;
                }
                self.error = None;
                let _ = self.library.reload_subject_groups(&self.store);
            }
            Err(error) => {
                self.error = Some(format!("刪除科目分組失敗：{error}"));
            }
        }
    }

    fn save_settings(&mut self) {
        let trimmed_name = self.display_name.trim();
        if !trimmed_name.is_empty() {
            let _ = self.store.set_setting(DISPLAY_NAME_SETTING, trimmed_name);
        }
        let date_trimmed = self.practical_date_draft.trim();
        if date_trimmed.is_empty() {
            self.practical = None;
            let _ = self.store.set_setting(PRACTICAL_SETTING, "");
            self.status_notice = Some("設定已儲存".to_owned());
            self.error = None;
        } else {
            match PracticalCountdown::parse(date_trimmed) {
                Ok(parsed) => {
                    self.practical = Some(parsed);
                    let _ = self.store.set_setting(PRACTICAL_SETTING, date_trimmed);
                    self.status_notice = Some("設定已儲存".to_owned());
                    self.error = None;
                }
                Err(error) => {
                    self.error = Some(format!("日期格式錯誤（需 YYYY-MM-DD）：{error}"));
                }
            }
        }
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self.dragging else {
            return;
        };
        let mouse_x: f32 = event.position.x.into();
        let viewport_w: f32 = window.viewport_size().width.into();
        match handle {
            ResizeHandle::Left => {
                self.left_width = mouse_x.clamp(MIN_LEFT_WIDTH, MAX_LEFT_WIDTH);
            }
            ResizeHandle::Right => {
                self.right_width = (viewport_w - mouse_x).clamp(MIN_RIGHT_WIDTH, MAX_RIGHT_WIDTH);
            }
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.dragging.take().is_some() {
            cx.notify();
        }
    }

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        if keystroke.modifiers.platform || keystroke.modifiers.control {
            return;
        }

        // Handle focused text inputs first (group name, settings, short-answer quiz field)
        match self.text_focus.clone() {
            TextFocus::GroupName => {
                match key {
                    "escape" => {
                        self.text_focus = TextFocus::None;
                    }
                    "enter" => self.save_group_editor(),
                    "backspace" => {
                        if let Some(editor) = self.group_editor.as_mut() {
                            editor.name.pop();
                        }
                    }
                    _ => {
                        if let Some(ch) =
                            keystroke.key_char.as_deref().and_then(|s| s.chars().next())
                            && !ch.is_control()
                            && let Some(editor) = self.group_editor.as_mut()
                        {
                            editor.name.push(ch);
                        }
                    }
                }
                cx.notify();
                return;
            }
            TextFocus::DisplayName => {
                match key {
                    "escape" => self.text_focus = TextFocus::None,
                    "enter" => self.save_settings(),
                    "backspace" => {
                        self.display_name.pop();
                    }
                    _ => {
                        if let Some(ch) =
                            keystroke.key_char.as_deref().and_then(|s| s.chars().next())
                            && !ch.is_control()
                        {
                            self.display_name.push(ch);
                        }
                    }
                }
                cx.notify();
                return;
            }
            TextFocus::PracticalDate => {
                match key {
                    "escape" => self.text_focus = TextFocus::None,
                    "enter" => self.save_settings(),
                    "backspace" => {
                        self.practical_date_draft.pop();
                    }
                    _ => {
                        if let Some(ch) =
                            keystroke.key_char.as_deref().and_then(|s| s.chars().next())
                            && (ch.is_ascii_digit() || ch == '-')
                        {
                            self.practical_date_draft.push(ch);
                        }
                    }
                }
                cx.notify();
                return;
            }
            TextFocus::QuizAnswer { session_key, index } => {
                match key {
                    "escape" => self.text_focus = TextFocus::None,
                    "enter" => self.submit_active_quiz_unit(),
                    "backspace" => {
                        if let Some(quiz) = self.quizzes.get_mut(&session_key) {
                            quiz.backspace_at(index);
                        }
                    }
                    _ => {
                        if let Some(ch) =
                            keystroke.key_char.as_deref().and_then(|s| s.chars().next())
                            && !ch.is_control()
                            && let Some(quiz) = self.quizzes.get_mut(&session_key)
                        {
                            quiz.push_char_at(index, ch);
                        }
                    }
                }
                cx.notify();
                return;
            }
            TextFocus::None => {}
        }

        // Global shortcuts when no text field is focused
        match self.tabs.active().kind.clone() {
            WorkspaceTabKind::Quiz(session_key) => {
                self.on_quiz_key(session_key, key, keystroke.key_char.as_deref());
            }
            _ => match key {
                "[" => self.left_open = !self.left_open,
                "]" => self.right_open = !self.right_open,
                "r" => self.reload(),
                other => {
                    if let Some(idx) = digit(other)
                        && let Some(mode) = QuizMode::ALL.get(idx)
                    {
                        self.start_quiz_in_new_tab(*mode, None, None);
                    }
                }
            },
        }
        cx.notify();
    }

    fn on_quiz_key(&mut self, session_key: u64, key: &str, key_char: Option<&str>) {
        if self.summaries.contains_key(&session_key) {
            if matches!(key, "escape" | "enter") {
                self.tabs.open_or_focus(WorkspaceTab::home());
            }
            return;
        }
        let now = Timestamp::now();
        match key {
            "left" => {
                if let Some(quiz) = self.quizzes.get_mut(&session_key) {
                    let _ = quiz.prev_unit(now);
                }
            }
            "right" => {
                if let Some(quiz) = self.quizzes.get_mut(&session_key) {
                    let _ = quiz.next_unit(now);
                }
            }
            "enter" => {
                let finished = self
                    .quizzes
                    .get(&session_key)
                    .is_some_and(Quiz::is_finished);
                if finished {
                    self.finish_quiz(session_key);
                } else {
                    self.submit_active_quiz_unit();
                }
            }
            "1" | "2" | "3" | "4" | "5" => {
                if let Some(idx) = digit(key)
                    && let Some(&level) = ConfidenceLevel5::ALL.get(idx)
                    && let Some(quiz) = self.quizzes.get_mut(&session_key)
                {
                    quiz.set_confidence5(level);
                }
            }
            _ => {
                let Some(quiz) = self.quizzes.get_mut(&session_key) else {
                    return;
                };
                if let Some(&first_idx) = quiz
                    .current_unit()
                    .and_then(|unit| unit.session_indices.first())
                    && let Some(ch) = key_char.and_then(|s| s.chars().next())
                {
                    if quiz
                        .questions()
                        .get(first_idx)
                        .is_some_and(|q| accepts_text(q.question_type))
                    {
                        if !ch.is_control() {
                            quiz.push_char_at(first_idx, ch);
                        }
                    } else if ch.is_ascii_alphabetic() {
                        let option = ch.to_ascii_uppercase().to_string();
                        quiz.toggle_option_at(first_idx, &option);
                    }
                }
            }
        }
    }

    // =========================================================================
    // Top 3-Segment Tabbed Header (Sketches 1 & 2)
    // =========================================================================

    fn top_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let active_id = self.tabs.active_id().to_owned();
        let left_open = self.left_open;
        let right_open = self.right_open;

        let mut tab_strip = div()
            .id("tab-strip")
            .flex()
            .flex_row()
            .items_center()
            .gap_1p5()
            .flex_1()
            .min_w_0()
            .h_full()
            .px_2()
            .overflow_x_scroll();

        for tab in self.tabs.tabs() {
            let is_active = tab.id == active_id;
            let tab_id_activate = tab.id.clone();
            let tab_id_close = tab.id.clone();
            let prefix = match tab.kind {
                WorkspaceTabKind::Home => "⌂",
                WorkspaceTabKind::Note(_) => "▤",
                WorkspaceTabKind::Card(_) => "◈",
                WorkspaceTabKind::StudyHub => "✎",
                WorkspaceTabKind::Quiz(_) => "▣",
                WorkspaceTabKind::Analytics => "◎",
                WorkspaceTabKind::Settings => "⚙",
            };

            tab_strip = tab_strip.child(
                div()
                    .id(SharedString::from(format!("tab-{}", tab.id)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1p5()
                    .h(px(32.))
                    .px_3()
                    .rounded_xl()
                    .border_1()
                    .border_color(if is_active { accent() } else { line() })
                    .bg(if is_active { surface() } else { panel() })
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            app.tabs.activate(&tab_id_activate);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if is_active { accent() } else { muted() })
                            .child(prefix),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(if is_active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if is_active { ink() } else { muted() })
                            .child(tab.title.clone()),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("tab-close-{}", tab.id)))
                            .ml_1()
                            .px_1()
                            .rounded_sm()
                            .text_size(px(11.))
                            .text_color(muted())
                            .hover(|this| this.bg(bad_soft()).text_color(bad()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                    cx.stop_propagation();
                                    app.close_tab(&tab_id_close);
                                    cx.notify();
                                }),
                            )
                            .child("×"),
                    ),
            );
        }

        tab_strip = tab_strip.child(
            div()
                .id("tab-add-study")
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(line())
                .bg(surface())
                .cursor_pointer()
                .text_size(px(14.))
                .text_color(muted())
                .hover(|this| this.border_color(accent()).text_color(accent()))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                        app.tabs.open_or_focus(WorkspaceTab::study_hub());
                        cx.notify();
                    }),
                )
                .child("+"),
        );

        let initial = self
            .display_name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "R".to_owned());

        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(48.))
            .flex_none()
            .bg(surface())
            .border_b_1()
            .border_color(line())
            // Left Brand Segment (aligned with left sidebar width)
            .child(
                div()
                    .w(px(if left_open { self.left_width } else { 156.0 }))
                    .flex_none()
                    .h_full()
                    .px_3()
                    .border_r_1()
                    .border_color(line())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_baseline()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_size(px(17.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(accent())
                                    .child("Reinforce"),
                            )
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(muted())
                                    .child(APP_VERSION),
                            ),
                    )
                    .child(
                        div()
                            .id("toggle-left-sidebar")
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(if left_open { accent_soft() } else { panel() })
                            .text_size(px(12.))
                            .text_color(if left_open { accent() } else { muted() })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                    app.left_open = !app.left_open;
                                    cx.notify();
                                }),
                            )
                            .child("◧"),
                    ),
            )
            // Center Tab Strip
            .child(tab_strip)
            // Right User Segment (aligned with right sidebar width)
            .child(
                div()
                    .w(px(if right_open { self.right_width } else { 156.0 }))
                    .flex_none()
                    .h_full()
                    .px_3()
                    .border_l_1()
                    .border_color(line())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(ink())
                                    .child(self.display_name.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(28.))
                                    .rounded_full()
                                    .bg(accent_soft())
                                    .border_1()
                                    .border_color(accent())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(accent())
                                    .child(initial),
                            )
                            .child(
                                div()
                                    .id("toggle-right-sidebar")
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(if right_open { accent_soft() } else { panel() })
                                    .text_size(px(12.))
                                    .text_color(if right_open { accent() } else { muted() })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.right_open = !app.right_open;
                                            cx.notify();
                                        }),
                                    )
                                    .child("◨"),
                            ),
                    ),
            )
            .into_any_element()
    }

    // =========================================================================
    // Left Collapsible & Resizable Navigation Sidebar (Sketches 1 & 2)
    // =========================================================================

    fn left_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let active_kind = &self.tabs.active().kind;
        let nav_items: [(&str, &str, bool, WorkspaceTab); 4] = [
            (
                "nav-notes",
                "▤  知識筆記",
                matches!(
                    active_kind,
                    WorkspaceTabKind::Home | WorkspaceTabKind::Note(_) | WorkspaceTabKind::Card(_)
                ),
                WorkspaceTab::home(),
            ),
            (
                "nav-study",
                "✎  考試",
                matches!(
                    active_kind,
                    WorkspaceTabKind::StudyHub | WorkspaceTabKind::Quiz(_)
                ),
                WorkspaceTab::study_hub(),
            ),
            (
                "nav-analytics",
                "◎  分析",
                matches!(active_kind, WorkspaceTabKind::Analytics),
                WorkspaceTab::analytics(),
            ),
            (
                "nav-settings",
                "⚙  設定",
                matches!(active_kind, WorkspaceTabKind::Settings),
                WorkspaceTab::settings(),
            ),
        ];

        let mut nav_box = div().flex().flex_col().gap_1();
        for (id, label, active, target) in nav_items {
            nav_box = nav_box.child(
                div()
                    .id(SharedString::from(id))
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(if active { accent_soft() } else { panel() })
                    .hover(|this| this.bg(surface()))
                    .text_size(px(13.))
                    .font_weight(if active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if active { accent() } else { ink() })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            app.tabs.open_or_focus(target.clone());
                            cx.notify();
                        }),
                    )
                    .child(label),
            );
        }

        let filtered_notes = self.library.filtered_notes();
        let mut notes_list = div().flex().flex_col().gap_1();
        for note in filtered_notes {
            let note_id = note.id.clone();
            let is_open = matches!(active_kind, WorkspaceTabKind::Note(id) if id == &note.id);
            notes_list = notes_list.child(
                div()
                    .id(SharedString::from(format!(
                        "left-note-{}",
                        note.id.as_str()
                    )))
                    .px_2p5()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .when(is_open, |this| this.bg(surface()))
                    .hover(|this| this.bg(surface()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            app.open_note_tab(note_id.clone());
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(if is_open {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if is_open { accent() } else { ink() })
                            .child(note.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(muted())
                            .child(note.subject.clone()),
                    ),
            );
        }

        let countdown = self.practical.map(|c| c.label(Timestamp::now()));

        div()
            .w(px(self.left_width))
            .flex_none()
            .h_full()
            .bg(panel())
            .flex()
            .flex_col()
            .child(
                div()
                    .id("left-sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(nav_box)
                    .child(div().h(px(1.)).w_full().bg(line()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(section_label("筆記目錄"))
                            .child(notes_list),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(line())
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .children(countdown.map(|label| badge(label, bad_soft(), bad())))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(muted())
                            .child(format!("累計作答 {} 筆", self.attempts)),
                    ),
            )
            .into_any_element()
    }

    // =========================================================================
    // Right Context-Aware Collapsible & Resizable Sidebar
    // =========================================================================

    fn right_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let body = match &self.tabs.active().kind {
            WorkspaceTabKind::Note(note_id) => self.right_note_outline_sidebar(note_id, cx),
            WorkspaceTabKind::Quiz(session_key) => self.right_quiz_sidebar(*session_key, cx),
            _ => self.right_subject_group_sidebar(cx),
        };
        div()
            .w(px(self.right_width))
            .flex_none()
            .h_full()
            .bg(panel())
            .flex()
            .flex_col()
            .child(body)
            .into_any_element()
    }

    /// Sketch 2 right sidebar: 文件列表 with circular checkboxes (○ / ✓).
    fn right_note_outline_sidebar(&self, note_id: &NoteId, cx: &mut Context<Self>) -> AnyElement {
        let Some(outline) = self.library.outline_by_id(note_id) else {
            return div()
                .p_4()
                .text_size(px(12.))
                .text_color(muted())
                .child("尚未載入筆記大綱")
                .into_any_element();
        };

        let ordered_cards = collect_outline_cards(&outline.note.outline, outline);
        let total = ordered_cards.len();
        let checked = self.note_checks.checked_count(note_id);
        let show_checked = self.note_checks.show_checked;
        let reset_note_id = note_id.clone();

        let mut list = div().flex().flex_col().gap_1();
        for card in ordered_cards {
            let is_checked = self.note_checks.is_checked(note_id, &card.id);
            let toggle_note_id = note_id.clone();
            let toggle_card_id = card.id.clone();
            let review_card = card.clone();
            let q_count = self
                .library
                .card_question_counts
                .get(&card.id)
                .copied()
                .unwrap_or(0);

            list = list.child(
                div()
                    .id(SharedString::from(format!("doc-item-{}", card.id.as_str())))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2p5()
                    .py_2()
                    .rounded_lg()
                    .bg(if is_checked { panel() } else { surface() })
                    .border_1()
                    .border_color(line())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .flex_1()
                            .min_w_0()
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                    app.toggle_note_card_check(
                                        toggle_note_id.clone(),
                                        toggle_card_id.clone(),
                                        cx,
                                    );
                                }),
                            )
                            .child(
                                div()
                                    .size(px(18.))
                                    .flex_none()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if is_checked { accent() } else { muted() })
                                    .bg(if is_checked { accent() } else { surface() })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(10.))
                                    .text_color(rgb(0xffffff))
                                    .child(if is_checked { "✓" } else { "" }),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(if is_checked { muted() } else { ink() })
                                    .child(card.title.clone()),
                            ),
                    )
                    .when(q_count > 0, |this| {
                        this.child(
                            div()
                                .id(SharedString::from(format!(
                                    "doc-drill-{}",
                                    review_card.id.as_str()
                                )))
                                .px_1p5()
                                .py_0p5()
                                .rounded_full()
                                .bg(accent_soft())
                                .cursor_pointer()
                                .text_size(px(10.))
                                .text_color(accent())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                        app.start_card_review_tab(&review_card);
                                        cx.notify();
                                    }),
                                )
                                .child(format!("{q_count} 題")),
                        )
                    }),
            );
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_b_1()
                    .border_color(line())
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(ink())
                                    .child("文件列表"),
                            )
                            .child(badge(
                                format!("{checked} / {total}"),
                                accent_soft(),
                                accent(),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("toggle-show-checked")
                                    .px_2()
                                    .py_1()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if show_checked { accent() } else { line() })
                                    .bg(if show_checked {
                                        accent_soft()
                                    } else {
                                        surface()
                                    })
                                    .cursor_pointer()
                                    .text_size(px(11.))
                                    .text_color(if show_checked { accent() } else { muted() })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.note_checks.show_checked =
                                                !app.note_checks.show_checked;
                                            cx.notify();
                                        }),
                                    )
                                    .child(if show_checked {
                                        "隱藏已勾選"
                                    } else {
                                        "顯示已勾選"
                                    }),
                            )
                            .when(checked > 0, |this| {
                                this.child(
                                    div()
                                        .id("reset-note-checks")
                                        .px_2()
                                        .py_1()
                                        .rounded_full()
                                        .border_1()
                                        .border_color(line())
                                        .bg(surface())
                                        .cursor_pointer()
                                        .text_size(px(11.))
                                        .text_color(muted())
                                        .hover(|el| el.text_color(bad()))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                move |app, _: &gpui::MouseDownEvent, _, cx| {
                                                    app.reset_note_checks(
                                                        reset_note_id.clone(),
                                                        cx,
                                                    );
                                                },
                                            ),
                                        )
                                        .child("重置"),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .id("right-note-list-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .child(list),
            )
            .into_any_element()
    }

    /// Sketch 1 right sidebar: 分組題目列表 (Subject -> Year -> Question / Group).
    fn right_quiz_sidebar(&self, session_key: u64, cx: &mut Context<Self>) -> AnyElement {
        let Some(quiz) = self.quizzes.get(&session_key) else {
            return div()
                .p_4()
                .text_size(px(12.))
                .text_color(muted())
                .child("測驗已交卷，可在主畫面檢視成績")
                .into_any_element();
        };

        let buckets = quiz.sidebar_buckets();
        let active_unit_idx = quiz.active_unit_index();
        let mut tree = div().flex().flex_col().gap_3();

        for subject_bucket in buckets {
            let mut subject_el = div().flex().flex_col().gap_1p5().child(
                div()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(ink())
                    .child(subject_bucket.subject),
            );

            for exam_bucket in subject_bucket.exams {
                let mut exam_el = div().pl_2().flex().flex_col().gap_1().child(
                    div()
                        .text_size(px(11.))
                        .text_color(muted())
                        .child(format!("◦ {}", exam_bucket.label)),
                );

                for unit_idx in exam_bucket.unit_indices {
                    let Some(unit) = quiz.units().get(unit_idx) else {
                        continue;
                    };
                    let is_active = unit_idx == active_unit_idx;
                    let label = format_unit_label(unit, quiz.questions());
                    let all_answered = unit
                        .session_indices
                        .iter()
                        .all(|&idx| quiz.answer_for(idx).is_some());
                    let any_wrong = unit.session_indices.iter().any(|&idx| {
                        quiz.graded_for(idx)
                            .is_some_and(|g| !g.outcome.voided && !g.outcome.is_correct)
                    });
                    let all_correct = all_answered
                        && unit.session_indices.iter().all(|&idx| {
                            quiz.graded_for(idx)
                                .is_some_and(|g| g.outcome.is_correct || g.outcome.voided)
                        });
                    let dot_color = if all_correct {
                        good()
                    } else if any_wrong {
                        bad()
                    } else if unit
                        .session_indices
                        .iter()
                        .any(|&idx| quiz.answer_for(idx).is_some())
                    {
                        accent()
                    } else {
                        muted()
                    };

                    exam_el = exam_el.child(
                        div()
                            .id(SharedString::from(format!(
                                "quiz-unit-{session_key}-{unit_idx}"
                            )))
                            .ml_2()
                            .px_2p5()
                            .py_1p5()
                            .rounded_lg()
                            .cursor_pointer()
                            .bg(if is_active { accent_soft() } else { panel() })
                            .hover(|this| this.bg(surface()))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                    if let Some(quiz) = app.quizzes.get_mut(&session_key) {
                                        let _ = quiz.select_unit(unit_idx, Timestamp::now());
                                    }
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(div().size(px(8.)).rounded_full().bg(dot_color))
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(if is_active {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::NORMAL
                                            })
                                            .text_color(if is_active { accent() } else { ink() })
                                            .child(label),
                                    ),
                            )
                            .when(unit.session_indices.len() > 1, |this| {
                                this.child(badge(
                                    format!("{} 題組", unit.session_indices.len()),
                                    surface(),
                                    muted(),
                                ))
                            }),
                    );
                }
                subject_el = subject_el.child(exam_el);
            }
            tree = tree.child(subject_el);
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_b_1()
                    .border_color(line())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ink())
                            .child("分組題目列表"),
                    )
                    .child(badge(
                        format!("{} / {}", quiz.answered_count(), quiz.total()),
                        accent_soft(),
                        accent(),
                    )),
            )
            .child(
                div()
                    .id("right-quiz-list-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .child(tree),
            )
            .into_any_element()
    }

    /// Right sidebar for Home / StudyHub / Analytics: Nested Subject Groups CRUD & selection.
    fn right_subject_group_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let tree = build_subject_group_tree(&self.library.subject_groups);
        let flat = flatten_subject_group_tree(&tree);
        let selected_id = self.library.selected_group_id.clone();

        let mut list = div().flex().flex_col().gap_1p5();

        // "All subjects" root selector
        list = list.child(
            div()
                .id("sg-all-subjects")
                .px_3()
                .py_2()
                .rounded_lg()
                .cursor_pointer()
                .bg(if selected_id.is_none() {
                    accent_soft()
                } else {
                    surface()
                })
                .border_1()
                .border_color(if selected_id.is_none() {
                    accent()
                } else {
                    line()
                })
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                        app.library.selected_group_id = None;
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if selected_id.is_none() {
                            accent()
                        } else {
                            ink()
                        })
                        .child("全部科目"),
                )
                .child(badge(
                    format!("{} 科", self.library.subjects.len()),
                    panel(),
                    muted(),
                )),
        );

        for node in flat {
            let group = node.group;
            let is_selected = selected_id.as_ref() == Some(&group.id);
            let select_id = group.id.clone();
            let child_parent_id = group.id.clone();
            let edit_group = group.clone();
            let delete_id = group.id.clone();
            let indent = (node.depth as f32 * 14.0).min(70.0);

            list = list.child(
                div()
                    .pl(px(indent))
                    .child(
                        div()
                            .id(SharedString::from(format!("sg-node-{}", group.id.as_str())))
                            .px_2p5()
                            .py_2()
                            .rounded_lg()
                            .bg(if is_selected {
                                accent_soft()
                            } else {
                                surface()
                            })
                            .border_1()
                            .border_color(if is_selected { accent() } else { line() })
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex_1()
                                            .cursor_pointer()
                                            .text_size(px(12.))
                                            .font_weight(if is_selected {
                                                FontWeight::SEMIBOLD
                                            } else {
                                                FontWeight::MEDIUM
                                            })
                                            .text_color(if is_selected { accent() } else { ink() })
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |app, _: &gpui::MouseDownEvent, _, cx| {
                                                        app.library.selected_group_id =
                                                            Some(select_id.clone());
                                                        cx.notify();
                                                    },
                                                ),
                                            )
                                            .child(format!("▸ {}", group.name)),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "sg-add-child-{}",
                                                        group.id.as_str()
                                                    )))
                                                    .px_1p5()
                                                    .rounded_sm()
                                                    .bg(panel())
                                                    .cursor_pointer()
                                                    .text_size(px(10.))
                                                    .text_color(muted())
                                                    .hover(|el| el.text_color(accent()))
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            move |app,
                                                                  _: &gpui::MouseDownEvent,
                                                                  _,
                                                                  cx| {
                                                                app.group_editor = Some(
                                                                    GroupEditorState::new_child(
                                                                        Some(child_parent_id.clone()),
                                                                    ),
                                                                );
                                                                app.text_focus =
                                                                    TextFocus::GroupName;
                                                                cx.notify();
                                                            },
                                                        ),
                                                    )
                                                    .child("+子"),
                                            )
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "sg-edit-{}",
                                                        group.id.as_str()
                                                    )))
                                                    .px_1p5()
                                                    .rounded_sm()
                                                    .bg(panel())
                                                    .cursor_pointer()
                                                    .text_size(px(10.))
                                                    .text_color(muted())
                                                    .hover(|el| el.text_color(accent()))
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            move |app,
                                                                  _: &gpui::MouseDownEvent,
                                                                  _,
                                                                  cx| {
                                                                app.group_editor = Some(
                                                                    GroupEditorState::from_group(
                                                                        &edit_group,
                                                                    ),
                                                                );
                                                                app.text_focus =
                                                                    TextFocus::GroupName;
                                                                cx.notify();
                                                            },
                                                        ),
                                                    )
                                                    .child("編"),
                                            )
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "sg-del-{}",
                                                        group.id.as_str()
                                                    )))
                                                    .px_1p5()
                                                    .rounded_sm()
                                                    .bg(panel())
                                                    .cursor_pointer()
                                                    .text_size(px(10.))
                                                    .text_color(muted())
                                                    .hover(|el| el.text_color(bad()))
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            move |app,
                                                                  _: &gpui::MouseDownEvent,
                                                                  _,
                                                                  cx| {
                                                                app.delete_subject_group(
                                                                    &delete_id,
                                                                );
                                                                cx.notify();
                                                            },
                                                        ),
                                                    )
                                                    .child("×"),
                                            ),
                                    ),
                            )
                            .when(!node.resolved_subjects.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(muted())
                                        .child(node.resolved_subjects.join(" · ")),
                                )
                            }),
                    ),
            );
        }

        let editor_view = self.group_editor.as_ref().map(|editor| {
            let name_focused = self.text_focus == TextFocus::GroupName;
            let mut parent_chips = div().flex().flex_row().flex_wrap().gap_1().child(
                div()
                    .id("sg-parent-none")
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .border_1()
                    .border_color(if editor.parent_id.is_none() {
                        accent()
                    } else {
                        line()
                    })
                    .bg(if editor.parent_id.is_none() {
                        accent_soft()
                    } else {
                        panel()
                    })
                    .cursor_pointer()
                    .text_size(px(10.))
                    .text_color(if editor.parent_id.is_none() {
                        accent()
                    } else {
                        muted()
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                            if let Some(ed) = app.group_editor.as_mut() {
                                ed.parent_id = None;
                            }
                            cx.notify();
                        }),
                    )
                    .child("頂層"),
            );
            for candidate in &self.library.subject_groups {
                if let Some(editing_id) = &editor.editing_id
                    && is_descendant_or_self(
                        &self.library.subject_groups,
                        &candidate.id,
                        editing_id,
                    )
                {
                    continue;
                }
                let picked = editor.parent_id.as_ref() == Some(&candidate.id);
                let cand_id = candidate.id.clone();
                parent_chips = parent_chips.child(
                    div()
                        .id(SharedString::from(format!(
                            "sg-parent-{}",
                            candidate.id.as_str()
                        )))
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .border_1()
                        .border_color(if picked { accent() } else { line() })
                        .bg(if picked { accent_soft() } else { panel() })
                        .cursor_pointer()
                        .text_size(px(10.))
                        .text_color(if picked { accent() } else { ink() })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                if let Some(ed) = app.group_editor.as_mut() {
                                    ed.parent_id = Some(cand_id.clone());
                                }
                                cx.notify();
                            }),
                        )
                        .child(candidate.name.clone()),
                );
            }
            let mut subject_chips = div().flex().flex_row().flex_wrap().gap_1();
            for summary in &self.library.subjects {
                let subj = summary.subject.clone();
                let picked = editor.subjects.contains(&subj);
                let toggle_subj = subj.clone();
                subject_chips = subject_chips.child(
                    div()
                        .id(SharedString::from(format!("sg-subj-chip-{subj}")))
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .border_1()
                        .border_color(if picked { accent() } else { line() })
                        .bg(if picked { accent_soft() } else { panel() })
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(if picked { accent() } else { ink() })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                if let Some(ed) = app.group_editor.as_mut()
                                    && !ed.subjects.remove(&toggle_subj)
                                {
                                    ed.subjects.insert(toggle_subj.clone());
                                }
                                cx.notify();
                            }),
                        )
                        .child(subj),
                );
            }

            div()
                .p_3()
                .rounded_xl()
                .bg(surface())
                .border_1()
                .border_color(accent())
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(accent())
                        .child(if editor.editing_id.is_some() {
                            "編輯科目分組"
                        } else if editor.parent_id.is_some() {
                            "新增子分組"
                        } else {
                            "新增頂層分組"
                        }),
                )
                .child(
                    div()
                        .id("sg-name-input")
                        .px_2p5()
                        .py_1p5()
                        .rounded_md()
                        .border_1()
                        .border_color(if name_focused { accent() } else { line() })
                        .bg(paper())
                        .cursor_pointer()
                        .text_size(px(12.))
                        .text_color(if editor.name.is_empty() {
                            muted()
                        } else {
                            ink()
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                app.text_focus = TextFocus::GroupName;
                                cx.notify();
                            }),
                        )
                        .child(if editor.name.is_empty() {
                            if name_focused {
                                "▌".to_owned()
                            } else {
                                "輸入分組名稱...".to_owned()
                            }
                        } else if name_focused {
                            format!("{}▌", editor.name)
                        } else {
                            editor.name.clone()
                        }),
                )
                .child(section_label("上層分組"))
                .child(parent_chips)
                .child(section_label("包含科目（可複選）"))
                .child(subject_chips)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            div()
                                .id("sg-save-btn")
                                .flex_1()
                                .py_1p5()
                                .rounded_lg()
                                .bg(accent())
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(0xffffff))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                        app.save_group_editor();
                                        cx.notify();
                                    }),
                                )
                                .child("儲存"),
                        )
                        .child(
                            div()
                                .id("sg-cancel-btn")
                                .px_3()
                                .py_1p5()
                                .rounded_lg()
                                .border_1()
                                .border_color(line())
                                .cursor_pointer()
                                .text_size(px(12.))
                                .text_color(muted())
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                        app.group_editor = None;
                                        app.text_focus = TextFocus::None;
                                        cx.notify();
                                    }),
                                )
                                .child("取消"),
                        ),
                )
        });

        div()
            .flex()
            .flex_col()
            .size_full()
            .child(
                div()
                    .flex_none()
                    .p_3()
                    .border_b_1()
                    .border_color(line())
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ink())
                            .child("巢狀科目分組"),
                    )
                    .child(
                        div()
                            .id("sg-new-root-btn")
                            .px_2p5()
                            .py_1()
                            .rounded_full()
                            .bg(accent_soft())
                            .cursor_pointer()
                            .text_size(px(11.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(accent())
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                    app.group_editor = Some(GroupEditorState::new_child(None));
                                    app.text_focus = TextFocus::GroupName;
                                    cx.notify();
                                }),
                            )
                            .child("+ 新增分組"),
                    ),
            )
            .child(
                div()
                    .id("right-sg-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .children(editor_view)
                    .child(list),
            )
            .into_any_element()
    }

    // =========================================================================
    // Center Main Frame Views
    // =========================================================================

    fn center_frame(&self, cx: &mut Context<Self>) -> AnyElement {
        match &self.tabs.active().kind {
            WorkspaceTabKind::Home => self.home_view(cx),
            WorkspaceTabKind::Note(note_id) => self.note_view(note_id, cx),
            WorkspaceTabKind::Card(card_id) => self.card_view_page(card_id, cx),
            WorkspaceTabKind::StudyHub => self.study_hub_view(cx),
            WorkspaceTabKind::Quiz(session_key) => self.quiz_view(*session_key, cx),
            WorkspaceTabKind::Analytics => self.analytics_view(cx),
            WorkspaceTabKind::Settings => self.settings_view(cx),
        }
    }

    fn home_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let notes = self.library.filtered_notes();
        let active_group = self.library.selected_group();

        let mut cards_grid = div().flex().flex_col().gap_3();
        if notes.is_empty() {
            cards_grid = cards_grid.child(
                div()
                    .p_6()
                    .rounded_xl()
                    .bg(surface())
                    .border_1()
                    .border_color(line())
                    .text_size(px(13.))
                    .text_color(muted())
                    .child("此範圍內沒有筆記。請匯入 bundle 或於右側切換科目分組。"),
            );
        } else {
            for note in notes {
                let note_id = note.id.clone();
                let checked = self.note_checks.checked_count(&note.id);
                cards_grid = cards_grid.child(
                    div()
                        .id(SharedString::from(format!(
                            "home-note-{}",
                            note.id.as_str()
                        )))
                        .p_4()
                        .rounded_xl()
                        .bg(surface())
                        .border_1()
                        .border_color(line())
                        .cursor_pointer()
                        .hover(|this| this.border_color(accent()))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                app.open_note_tab(note_id.clone());
                                cx.notify();
                            }),
                        )
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(ink())
                                        .child(note.title.clone()),
                                )
                                .child(div().text_size(px(12.)).text_color(muted()).child(
                                    format!(
                                        "{} · 更新於 {}",
                                        note.subject,
                                        note.updated_at.as_str()
                                    ),
                                )),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .when(checked > 0, |this| {
                                    this.child(badge(
                                        format!("已勾選 {checked}"),
                                        good_soft(),
                                        good(),
                                    ))
                                })
                                .child(badge("開啟筆記 →", accent_soft(), accent())),
                        ),
                );
            }
        }

        div()
            .id("home-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_8()
            .py_6()
            .child(
                div()
                    .max_w(px(860.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(24.))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(ink())
                                            .child("知識筆記工作台"),
                                    )
                                    .child(
                                        div().text_size(px(13.)).text_color(muted()).child(
                                            "從右側選擇巢狀科目分組，或點擊筆記於上方分頁開啟",
                                        ),
                                    ),
                            )
                            .children(active_group.map(|group| {
                                div()
                                    .id("home-group-quick-exam")
                                    .px_3p5()
                                    .py_2()
                                    .rounded_full()
                                    .bg(accent())
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.start_quiz_in_new_tab(
                                                QuizMode::FullExam,
                                                None,
                                                None,
                                            );
                                            cx.notify();
                                        }),
                                    )
                                    .child(format!("以「{}」開始模擬考", group.name))
                            })),
                    )
                    .children(self.error.clone().map(|e| self.notice_box(e)))
                    .child(cards_grid),
            )
            .into_any_element()
    }

    /// Sketch 2: Vertical ordered chapter notes with animated check-off collapse.
    fn note_view(&self, note_id: &NoteId, cx: &mut Context<Self>) -> AnyElement {
        let Some(outline) = self.library.outline_by_id(note_id) else {
            return div()
                .p_8()
                .text_size(px(14.))
                .text_color(muted())
                .child("找不到此筆記")
                .into_any_element();
        };

        let note = &outline.note;
        let ordered_cards = collect_outline_cards(&note.outline, outline);
        let all_tags: BTreeSet<String> = ordered_cards
            .iter()
            .flat_map(|card| card.topics.iter().cloned())
            .collect();
        let all_checked = !ordered_cards.is_empty()
            && !self.note_checks.show_checked
            && ordered_cards
                .iter()
                .all(|card| self.note_checks.factor(note_id, &card.id) < 0.03);

        let mut tags_row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1p5()
            .child(badge(
                format!("# {}", note.subject),
                accent_soft(),
                accent(),
            ));
        for tag in all_tags.into_iter().take(6) {
            tags_row = tags_row.child(badge(format!("# {tag}"), paper(), muted()));
        }

        let mut sections: Vec<AnyElement> = Vec::new();
        self.render_linear_outline(note_id, &note.outline, outline, 0, &mut sections, cx);
        let reset_id = note_id.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(surface())
            // Note Header (Sketch 2: 標題XXX + 標籤 pills)
            .child(
                div()
                    .flex_none()
                    .px_8()
                    .py_4()
                    .border_b_1()
                    .border_color(line())
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(ink())
                                    .child(note.title.clone()),
                            )
                            .child(tags_row),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "note-drill-all-{}",
                                note.id.as_str()
                            )))
                            .px_3()
                            .py_1p5()
                            .rounded_full()
                            .border_1()
                            .border_color(accent())
                            .bg(accent_soft())
                            .cursor_pointer()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(accent())
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener({
                                    let card_ids: Vec<KnowledgeCardId> =
                                        ordered_cards.iter().map(|c| c.id.clone()).collect();
                                    let note_title = note.title.clone();
                                    move |app, _: &gpui::MouseDownEvent, _, cx| {
                                        let targets = QuizTargets {
                                            exam_set: None,
                                            cards: card_ids.clone(),
                                            subjects: None,
                                        };
                                        app.start_quiz_in_new_tab(
                                            QuizMode::TopicDrill,
                                            Some(targets),
                                            Some(format!("複習 {note_title}")),
                                        );
                                        cx.notify();
                                    }
                                }),
                            )
                            .child("整篇相關題目複習 ↗"),
                    ),
            )
            .child(
                div()
                    .id("note-linear-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .py_6()
                    .child(
                        div()
                            .max_w(px(820.))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .children(self.error.clone().map(|e| self.notice_box(e)))
                            .children(sections)
                            .when(all_checked, |this| {
                                this.child(
                                    div()
                                        .mt_6()
                                        .p_8()
                                        .rounded_2xl()
                                        .bg(paper())
                                        .border_1()
                                        .border_color(line())
                                        .flex()
                                        .flex_col()
                                        .items_center()
                                        .gap_3()
                                        .child(
                                            div()
                                                .text_size(px(18.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(good())
                                                .child("本篇全部知識點已複習完成！"),
                                        )
                                        .child(
                                            div()
                                                .id("note-reset-all-done")
                                                .px_4()
                                                .py_2()
                                                .rounded_full()
                                                .bg(accent())
                                                .cursor_pointer()
                                                .text_size(px(12.))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(0xffffff))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(
                                                        move |app,
                                                              _: &gpui::MouseDownEvent,
                                                              _,
                                                              cx| {
                                                            app.reset_note_checks(
                                                                reset_id.clone(),
                                                                cx,
                                                            );
                                                        },
                                                    ),
                                                )
                                                .child("重新顯示全部知識點"),
                                        ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    fn render_linear_outline(
        &self,
        note_id: &NoteId,
        nodes: &[OutlineNode],
        outline: &NoteOutline,
        depth: usize,
        output: &mut Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) {
        for node in nodes {
            match node {
                OutlineNode::Section {
                    heading,
                    intro_markdown,
                    children,
                } => {
                    let mut child_elements = Vec::new();
                    self.render_linear_outline(
                        note_id,
                        children,
                        outline,
                        depth + 1,
                        &mut child_elements,
                        cx,
                    );
                    if child_elements.is_empty() && intro_markdown.is_none() {
                        continue;
                    }
                    output.push(
                        div()
                            .mt_4()
                            .pb_1()
                            .text_size(px(if depth == 0 { 19. } else { 16. }))
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent())
                            .child(heading.clone())
                            .into_any_element(),
                    );
                    if let Some(intro) = intro_markdown {
                        output.push(
                            div()
                                .mb_2()
                                .flex()
                                .flex_col()
                                .gap_1p5()
                                .children(self.markdown_elements(&parse(intro)))
                                .into_any_element(),
                        );
                    }
                    output.extend(child_elements);
                }
                OutlineNode::CardRef { card_id } => {
                    let factor = self.note_checks.factor(note_id, card_id);
                    if factor < 0.03 {
                        continue;
                    }
                    if let Some(card) = outline.cards.iter().find(|c| &c.id == card_id) {
                        let is_checked = self.note_checks.is_checked(note_id, card_id);
                        let card_el = self.linear_knowledge_point(note_id, card, is_checked, cx);
                        if factor < 0.995 {
                            output.push(
                                div()
                                    .overflow_hidden()
                                    .max_h(px(factor * 460.0))
                                    .opacity(factor)
                                    .child(card_el)
                                    .into_any_element(),
                            );
                        } else {
                            output.push(card_el);
                        }
                    }
                }
            }
        }
    }

    fn linear_knowledge_point(
        &self,
        note_id: &NoteId,
        card: &KnowledgeCard,
        is_checked: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let key = card.id.as_str();
        let mut body: Vec<AnyElement> = Vec::new();
        for (index, block) in card.blocks.iter().enumerate() {
            if let Some(element) = self.block_element(&key, index, block, cx) {
                body.push(element);
            }
        }
        let q_count = self
            .library
            .card_question_counts
            .get(&card.id)
            .copied()
            .unwrap_or(0);
        let toggle_note_id = note_id.clone();
        let toggle_card_id = card.id.clone();
        let review_card = card.clone();

        div()
            .py_5()
            .border_b_1()
            .border_color(line())
            .opacity(if is_checked { 0.55 } else { 1.0 })
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2p5()
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "kp-check-{}",
                                        card.id.as_str()
                                    )))
                                    .size(px(20.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if is_checked { accent() } else { muted() })
                                    .bg(if is_checked { accent() } else { surface() })
                                    .cursor_pointer()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(11.))
                                    .text_color(rgb(0xffffff))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.toggle_note_card_check(
                                                toggle_note_id.clone(),
                                                toggle_card_id.clone(),
                                                cx,
                                            );
                                        }),
                                    )
                                    .child(if is_checked { "✓" } else { "" }),
                            )
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(ink())
                                    .child(card.title.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(muted())
                                    .child(card.topics.join(" · ")),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "kp-review-{}",
                                        card.id.as_str()
                                    )))
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(if q_count > 0 { accent() } else { line() })
                                    .bg(if q_count > 0 { accent_soft() } else { paper() })
                                    .cursor_pointer()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if q_count > 0 { accent() } else { muted() })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.start_card_review_tab(&review_card);
                                            cx.notify();
                                        }),
                                    )
                                    .child(if q_count > 0 {
                                        format!("相關題目 ({q_count}) ↗")
                                    } else {
                                        "相關題目複習 ↗".to_owned()
                                    }),
                            ),
                    ),
            )
            .children(body)
            .into_any_element()
    }

    fn card_view_page(&self, card_id: &KnowledgeCardId, cx: &mut Context<Self>) -> AnyElement {
        let Some(card) = self.library.cards.iter().find(|c| &c.id == card_id) else {
            return div().p_8().child("找不到知識卡").into_any_element();
        };
        let key = card.id.as_str();
        let mut body = Vec::new();
        for (idx, block) in card.blocks.iter().enumerate() {
            if let Some(el) = self.block_element(&key, idx, block, cx) {
                body.push(el);
            }
        }
        let review_card = card.clone();
        div()
            .id("card-page-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_8()
            .py_6()
            .child(
                div()
                    .max_w(px(820.))
                    .mx_auto()
                    .p_6()
                    .rounded_2xl()
                    .bg(surface())
                    .border_1()
                    .border_color(line())
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::BOLD)
                                    .text_color(ink())
                                    .child(card.title.clone()),
                            )
                            .child(
                                div()
                                    .id("card-page-review")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_full()
                                    .bg(accent())
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.start_card_review_tab(&review_card);
                                            cx.notify();
                                        }),
                                    )
                                    .child("開新分頁複習相關題目 ↗"),
                            ),
                    )
                    .children(body),
            )
            .into_any_element()
    }

    fn study_hub_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let active_group = self.library.selected_group();
        let resolved_subjects = self.library.resolved_selected_subjects();
        let filtered_exams = self.library.filtered_exam_sets();

        let mut mode_cards = div().flex().flex_row().flex_wrap().gap_3();
        for mode in QuizMode::ALL {
            mode_cards = mode_cards.child(
                div()
                    .id(SharedString::from(format!("hub-mode-{mode:?}")))
                    .w(px(230.))
                    .p_4()
                    .rounded_xl()
                    .bg(surface())
                    .border_1()
                    .border_color(line())
                    .cursor_pointer()
                    .hover(|this| this.border_color(accent()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            app.start_quiz_in_new_tab(mode, None, None);
                            cx.notify();
                        }),
                    )
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(accent())
                            .child(mode.title()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted())
                            .child(mode.description()),
                    ),
            );
        }

        let mut exam_list = div().flex().flex_col().gap_2();
        for summary in filtered_exams {
            let exam_idx = self
                .library
                .exam_sets
                .iter()
                .position(|item| item.exam_set.id == summary.exam_set.id);
            if let Some(idx) = exam_idx {
                exam_list = exam_list.child(
                    div()
                        .id(SharedString::from(format!(
                            "hub-exam-{}",
                            summary.exam_set.id.as_str()
                        )))
                        .p_3p5()
                        .rounded_xl()
                        .bg(surface())
                        .border_1()
                        .border_color(line())
                        .cursor_pointer()
                        .hover(|this| this.border_color(accent()))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                app.start_exam_set_tab(idx);
                                cx.notify();
                            }),
                        )
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(ink())
                                        .child(summary.exam_set.title.clone()),
                                )
                                .child(div().text_size(px(11.)).text_color(muted()).child(
                                    format!(
                                        "{} · {} 題",
                                        summary.exam_set.subject, summary.published_question_count
                                    ),
                                )),
                        )
                        .child(badge("開啟試卷 →", accent_soft(), accent())),
                );
            }
        }

        div()
            .id("study-hub-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_8()
            .py_6()
            .child(
                div()
                    .max_w(px(860.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(24.))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(ink())
                                            .child("考試與複習中心"),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(muted())
                                            .child("可在右側選取單一巢狀科目分組，所有模式皆會自動篩選該分組科目出題"),
                                    ),
                            )
                            .children(active_group.map(|group| {
                                let count = resolved_subjects.as_ref().map_or(0, Vec::len);
                                badge(
                                    format!("目前分組：{}（{count} 科）", group.name),
                                    accent_soft(),
                                    accent(),
                                )
                            })),
                    )
                    .children(self.error.clone().map(|e| self.notice_box(e)))
                    .child(section_label("選擇練習模式（開新分頁進入題目視圖）"))
                    .child(mode_cards)
                    .child(section_label("試卷清單"))
                    .child(exam_list),
            )
            .into_any_element()
    }

    // =========================================================================
    // Sketch 1: Grouped Question View + FIXED Bottom Navigation Bar
    // =========================================================================

    fn quiz_view(&self, session_key: u64, cx: &mut Context<Self>) -> AnyElement {
        if let Some(summary) = self.summaries.get(&session_key) {
            return self.summary_view(summary, cx);
        }
        let Some(quiz) = self.quizzes.get(&session_key) else {
            return div().p_8().child("沒有進行中的測驗").into_any_element();
        };
        let Some(unit) = quiz.current_unit() else {
            return div().p_8().child("此測驗沒有題目").into_any_element();
        };

        let now = Timestamp::now();
        let timer = quiz.seconds_left(now);
        let breadcrumb = format_unit_breadcrumb(unit, quiz.questions());
        let unit_tags: BTreeSet<String> = unit
            .session_indices
            .iter()
            .filter_map(|&idx| quiz.questions().get(idx))
            .flat_map(|q| q.topics.iter().cloned())
            .collect();

        let mut tags_row = div().flex().flex_row().flex_wrap().items_center().gap_1p5();
        for tag in unit_tags.into_iter().take(5) {
            tags_row = tags_row.child(badge(format!("# {tag}"), paper(), muted()));
        }

        // Scrollable middle content: Shared QuestionGroup Vignette + ALL child questions
        let mut stage = div().max_w(px(800.)).mx_auto().flex().flex_col().gap_5();

        if let Some(err) = self.error.clone() {
            stage = stage.child(self.notice_box(err));
        }
        if let Some(notice) = self.status_notice.clone() {
            stage = stage.child(
                div()
                    .px_4()
                    .py_2()
                    .rounded_xl()
                    .bg(good_soft())
                    .text_size(px(12.))
                    .text_color(good())
                    .child(notice),
            );
        }

        if let Some(group) = &unit.group {
            let mut shared_blocks = Vec::new();
            for (b_idx, block) in group.shared_stem_blocks.iter().enumerate() {
                if let Some(el) =
                    self.block_element(&format!("grp-{}", group.id.as_str()), b_idx, block, cx)
                {
                    shared_blocks.push(el);
                }
            }
            stage = stage.child(
                div()
                    .p_5()
                    .rounded_2xl()
                    .bg(paper())
                    .border_1()
                    .border_color(accent())
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(accent())
                            .child(group.title.clone().unwrap_or_else(|| {
                                format!("題組（{} 題）", unit.session_indices.len())
                            })),
                    )
                    .children(shared_blocks),
            );
        }

        let is_group = unit.session_indices.len() > 1;
        for &session_idx in &unit.session_indices {
            if let Some(question) = quiz.questions().get(session_idx) {
                stage = stage.child(self.render_quiz_question(
                    session_key,
                    quiz,
                    session_idx,
                    question,
                    is_group,
                    cx,
                ));
            }
        }

        // FIXED Bottom Bar: 5 Confidence Pills + Prev / Submit / Next (or Finish)
        let current_conf = quiz.confidence5();
        let mut conf_row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .justify_center()
            .gap_2();
        for level in ConfidenceLevel5::ALL {
            let active = current_conf == level;
            conf_row = conf_row.child(
                div()
                    .id(SharedString::from(format!("conf-{session_key}-{level:?}")))
                    .px_4()
                    .py_1p5()
                    .rounded_full()
                    .border_1()
                    .border_color(if active { accent() } else { line() })
                    .bg(if active { accent_soft() } else { paper() })
                    .cursor_pointer()
                    .text_size(px(12.))
                    .font_weight(if active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if active { accent() } else { muted() })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            if let Some(q) = app.quizzes.get_mut(&session_key) {
                                q.set_confidence5(level);
                            }
                            cx.notify();
                        }),
                    )
                    .child(level.label()),
            );
        }

        let has_prev = quiz.has_prev_unit();
        let has_next = quiz.has_next_unit();
        let can_submit = quiz.can_submit_current_unit();
        let submit_label = if is_group {
            "送出本組答案"
        } else {
            "送出答案"
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(surface())
            // Top Meta Bar (Sketch 1: XX科目/XX年/第XX題 + #標籤 + Finish)
            .child(
                div()
                    .flex_none()
                    .px_8()
                    .py_3()
                    .border_b_1()
                    .border_color(line())
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .items_center()
                            .gap_2p5()
                            .child(
                                div()
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(ink())
                                    .child(breadcrumb),
                            )
                            .child(tags_row),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .children(timer.map(|secs| {
                                let urgent = secs <= 5;
                                badge(
                                    format!("{:02}:{:02}", secs / 60, secs % 60),
                                    if urgent { bad_soft() } else { paper() },
                                    if urgent { bad() } else { ink() },
                                )
                            }))
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "quiz-finish-top-{session_key}"
                                    )))
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(accent())
                                    .bg(accent_soft())
                                    .cursor_pointer()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(accent())
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.finish_quiz(session_key);
                                            cx.notify();
                                        }),
                                    )
                                    .child("交卷計分"),
                            ),
                    ),
            )
            // Scrollable Question Stage
            .child(
                div()
                    .id(SharedString::from(format!(
                        "quiz-stage-scroll-{session_key}"
                    )))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_8()
                    .py_6()
                    .child(stage),
            )
            // FIXED Bottom Bar (pinned at bottom of center frame!)
            .child(
                div()
                    .flex_none()
                    .border_t_1()
                    .border_color(line())
                    .bg(surface())
                    .px_6()
                    .py_3()
                    .child(
                        div()
                            .max_w(px(800.))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_2p5()
                            .child(conf_row)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .gap_3()
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "quiz-prev-{session_key}"
                                            )))
                                            .px_4()
                                            .py_2()
                                            .rounded_full()
                                            .border_1()
                                            .border_color(line())
                                            .bg(paper())
                                            .opacity(if has_prev { 1.0 } else { 0.4 })
                                            .when(has_prev, |this| {
                                                this.cursor_pointer().on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(
                                                        move |app,
                                                              _: &gpui::MouseDownEvent,
                                                              _,
                                                              cx| {
                                                            if let Some(q) =
                                                                app.quizzes.get_mut(&session_key)
                                                            {
                                                                let _ =
                                                                    q.prev_unit(Timestamp::now());
                                                            }
                                                            cx.notify();
                                                        },
                                                    ),
                                                )
                                            })
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(ink())
                                            .child("← 上一題"),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!(
                                                "quiz-submit-{session_key}"
                                            )))
                                            .flex_1()
                                            .max_w(px(280.))
                                            .py_2()
                                            .rounded_full()
                                            .bg(accent())
                                            .opacity(if can_submit { 1.0 } else { 0.4 })
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .when(can_submit, |this| {
                                                this.cursor_pointer().on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(
                                                        |app, _: &gpui::MouseDownEvent, _, cx| {
                                                            app.submit_active_quiz_unit();
                                                            cx.notify();
                                                        },
                                                    ),
                                                )
                                            })
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(0xffffff))
                                            .child(submit_label),
                                    )
                                    .child(if has_next {
                                        div()
                                            .id(SharedString::from(format!(
                                                "quiz-next-{session_key}"
                                            )))
                                            .px_4()
                                            .py_2()
                                            .rounded_full()
                                            .border_1()
                                            .border_color(line())
                                            .bg(paper())
                                            .cursor_pointer()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(ink())
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |app, _: &gpui::MouseDownEvent, _, cx| {
                                                        if let Some(q) =
                                                            app.quizzes.get_mut(&session_key)
                                                        {
                                                            let _ = q.next_unit(Timestamp::now());
                                                        }
                                                        cx.notify();
                                                    },
                                                ),
                                            )
                                            .child("下一題 →")
                                            .into_any_element()
                                    } else {
                                        div()
                                            .id(SharedString::from(format!(
                                                "quiz-finish-bot-{session_key}"
                                            )))
                                            .px_4()
                                            .py_2()
                                            .rounded_full()
                                            .border_1()
                                            .border_color(accent())
                                            .bg(accent_soft())
                                            .cursor_pointer()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(accent())
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |app, _: &gpui::MouseDownEvent, _, cx| {
                                                        app.finish_quiz(session_key);
                                                        cx.notify();
                                                    },
                                                ),
                                            )
                                            .child("交卷計分")
                                            .into_any_element()
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_quiz_question(
        &self,
        session_key: u64,
        quiz: &Quiz,
        session_idx: usize,
        question: &QuestionCard,
        in_group: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let graded = quiz.graded_for(session_idx);
        let locked = graded.is_some() && !quiz.mode().deferred();
        let selection = quiz.draft_selection(session_idx);
        let typed = quiz.draft_typed(session_idx);

        let mut card = div().flex().flex_col().gap_3p5().when(in_group, |this| {
            this.p_5()
                .rounded_2xl()
                .bg(paper())
                .border_1()
                .border_color(line())
        });

        // Question badges
        let q_no = question.question_no.unwrap_or(session_idx as u32 + 1);
        card = card.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .when(in_group, |this| {
                    this.child(badge(format!("第 {q_no} 題"), accent_soft(), accent()))
                })
                .child(badge(question.question_type.as_str(), panel(), muted()))
                .child(badge(question.grading_mode.as_str(), panel(), muted())),
        );

        // Stem blocks
        for (idx, block) in question.stem_blocks.iter().enumerate() {
            if let Some(el) = self.block_element(&question.id.as_str(), idx, block, cx) {
                card = card.child(el);
            }
        }

        // Pill-shaped Choice Options (Sketch 1: wide rounded pills A. / B. / C. / D.)
        if !question.options.is_empty() {
            let mut options_col = div().flex().flex_col().gap_2p5();
            for opt in &question.options {
                let is_selected = selection.contains(&opt.key);
                let is_correct_opt = graded.is_some()
                    && (question.answer.contains(&opt.key)
                        || question
                            .accepted_answers
                            .iter()
                            .any(|set| set.contains(&opt.key)));
                let is_wrong_selected = graded.is_some()
                    && is_selected
                    && !is_correct_opt
                    && !graded.is_some_and(|g| g.outcome.voided);

                let (border_col, bg_col, badge_bg, badge_fg) = if is_correct_opt {
                    (good(), good_soft(), good(), rgb(0xffffff))
                } else if is_wrong_selected {
                    (bad(), bad_soft(), bad(), rgb(0xffffff))
                } else if is_selected {
                    (accent(), accent_soft(), accent(), rgb(0xffffff))
                } else {
                    (line(), surface(), paper(), muted())
                };
                let opt_key = opt.key.clone();

                options_col = options_col.child(
                    div()
                        .id(SharedString::from(format!(
                            "opt-{session_key}-{session_idx}-{}",
                            opt.key
                        )))
                        .px_5()
                        .py_3p5()
                        .rounded_3xl()
                        .border_1()
                        .border_color(border_col)
                        .bg(bg_col)
                        .when(!locked, |this| {
                            this.cursor_pointer().on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                    if let Some(q) = app.quizzes.get_mut(&session_key) {
                                        q.toggle_option_at(session_idx, &opt_key);
                                    }
                                    cx.notify();
                                }),
                            )
                        })
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .size(px(28.))
                                        .flex_none()
                                        .rounded_full()
                                        .bg(badge_bg)
                                        .border_1()
                                        .border_color(border_col)
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(badge_fg)
                                        .child(opt.key.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .text_color(ink())
                                        .child(opt.content.clone()),
                                ),
                        )
                        .when(graded.is_some() && opt.explanation.is_some(), |this| {
                            this.child(
                                div()
                                    .pl_10()
                                    .text_size(px(12.))
                                    .text_color(muted())
                                    .child(opt.explanation.clone().unwrap_or_default()),
                            )
                        }),
                );
            }
            card = card.child(options_col);
        } else if accepts_text(question.question_type) {
            let is_focused = self.text_focus
                == TextFocus::QuizAnswer {
                    session_key,
                    index: session_idx,
                };
            card = card.child(
                div()
                    .id(SharedString::from(format!(
                        "text-ans-{session_key}-{session_idx}"
                    )))
                    .px_4()
                    .py_3()
                    .rounded_2xl()
                    .border_1()
                    .border_color(if is_focused { accent() } else { line() })
                    .bg(surface())
                    .when(!locked, |this| {
                        this.cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                app.text_focus = TextFocus::QuizAnswer {
                                    session_key,
                                    index: session_idx,
                                };
                                cx.notify();
                            }),
                        )
                    })
                    .text_size(px(15.))
                    .text_color(if typed.is_empty() { muted() } else { ink() })
                    .child(if typed.is_empty() {
                        if is_focused {
                            "▌".to_owned()
                        } else {
                            "點擊輸入答案後按 Enter...".to_owned()
                        }
                    } else if is_focused {
                        format!("{typed}▌")
                    } else {
                        typed.to_owned()
                    }),
            );
        }

        // Immediate grading feedback banner
        if let Some(graded_answer) = graded {
            let outcome = &graded_answer.outcome;
            let (bg, fg, label) = if graded_answer.record.timed_out {
                (warn_soft(), warn(), "時間到未作答")
            } else if outcome.voided {
                (panel(), muted(), "作廢題（不計分）")
            } else if outcome.is_correct {
                (good_soft(), good(), "答對了")
            } else {
                (bad_soft(), bad(), "答錯了")
            };
            let mut fb = div()
                .p_4()
                .rounded_2xl()
                .bg(bg)
                .border_1()
                .border_color(fg)
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(fg)
                        .child(format!("{label} · {}", outcome.note)),
                );
            if !question.answer.is_empty() {
                fb = fb.child(
                    div()
                        .text_size(px(12.))
                        .text_color(ink())
                        .child(format!("參考答案：{}", question.answer.join(", "))),
                );
            }
            for (e_idx, block) in question.explanation_blocks.iter().enumerate() {
                if let Some(el) =
                    self.block_element(&format!("exp-{}", question.id.as_str()), e_idx, block, cx)
                {
                    fb = fb.child(el);
                }
            }
            card = card.child(fb);
        }

        card.into_any_element()
    }

    fn summary_view(&self, summary: &Summary, cx: &mut Context<Self>) -> AnyElement {
        let percent = (summary.ratio * 100.0).round() as i64;
        let rerun_mode = summary.mode;
        div()
            .id("summary-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_8()
            .py_10()
            .child(
                div()
                    .max_w(px(640.))
                    .mx_auto()
                    .p_8()
                    .rounded_2xl()
                    .bg(surface())
                    .border_1()
                    .border_color(line())
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(badge(summary.mode.title(), accent_soft(), accent()))
                    .child(
                        div()
                            .text_size(px(54.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent())
                            .child(format!("{percent}%")),
                    )
                    .child(div().text_size(px(15.)).text_color(ink()).child(format!(
                        "答對 {} / {} 題，作廢 {} 題",
                        summary.correct, summary.scoreable, summary.voided
                    )))
                    .child(div().text_size(px(12.)).text_color(muted()).child(format!(
                        "已寫入 {} 筆作答紀錄並更新 FSRS 排程",
                        summary.written
                    )))
                    .child(
                        div()
                            .mt_2()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .child(
                                div()
                                    .id("summary-home-btn")
                                    .px_5()
                                    .py_2()
                                    .rounded_full()
                                    .bg(accent())
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.tabs.open_or_focus(WorkspaceTab::home());
                                            cx.notify();
                                        }),
                                    )
                                    .child("回到首頁"),
                            )
                            .child(
                                div()
                                    .id("summary-rerun-btn")
                                    .px_5()
                                    .py_2()
                                    .rounded_full()
                                    .border_1()
                                    .border_color(line())
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .text_color(ink())
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.start_quiz_in_new_tab(rerun_mode, None, None);
                                            cx.notify();
                                        }),
                                    )
                                    .child("開新分頁再測一輪"),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn analytics_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let subjects = self.library.resolved_selected_subjects();
        let report = self
            .store
            .weakness_report(&self.user.id, 12, subjects.as_deref())
            .ok();

        let mut weak_cards = div().flex().flex_col().gap_2();
        if let Some(ref rep) = report {
            if rep.knowledge_cards.is_empty() {
                weak_cards = weak_cards.child(
                    div()
                        .p_4()
                        .rounded_xl()
                        .bg(surface())
                        .border_1()
                        .border_color(line())
                        .text_size(px(13.))
                        .text_color(muted())
                        .child("目前此科目範圍內尚無弱點紀錄，多練習幾次後會顯示在此。"),
                );
            } else {
                for item in &rep.knowledge_cards {
                    let card_title = self
                        .library
                        .cards
                        .iter()
                        .find(|c| c.id == item.card_id)
                        .map(|c| c.title.clone())
                        .unwrap_or_else(|| item.card_id.as_str());
                    let card_id = item.card_id.clone();
                    let drill_title = card_title.clone();
                    weak_cards = weak_cards.child(
                        div()
                            .id(SharedString::from(format!(
                                "weak-card-{}",
                                item.card_id.as_str()
                            )))
                            .p_3p5()
                            .rounded_xl()
                            .bg(surface())
                            .border_1()
                            .border_color(line())
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(px(14.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(ink())
                                            .child(card_title),
                                    )
                                    .child(div().text_size(px(11.)).text_color(muted()).child(
                                        format!(
                                            "作答 {} 次 · 錯誤率 {:.0}% · 猶豫率 {:.0}%",
                                            item.attempts,
                                            item.error_rate * 100.0,
                                            item.hesitant_rate * 100.0
                                        ),
                                    )),
                            )
                            .child(
                                div()
                                    .px_3()
                                    .py_1()
                                    .rounded_full()
                                    .bg(accent_soft())
                                    .cursor_pointer()
                                    .text_size(px(11.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(accent())
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                            let targets = QuizTargets {
                                                exam_set: None,
                                                cards: vec![card_id.clone()],
                                                subjects: None,
                                            };
                                            app.start_quiz_in_new_tab(
                                                QuizMode::TopicDrill,
                                                Some(targets),
                                                Some(format!("複習 {drill_title}")),
                                            );
                                            cx.notify();
                                        }),
                                    )
                                    .child("針對複習 ↗"),
                            ),
                    );
                }
            }
        }

        div()
            .id("analytics-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_8()
            .py_6()
            .child(
                div()
                    .max_w(px(820.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(ink())
                            .child("學習與弱點分析"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .child(stat_card("累計作答", format!("{} 筆", self.attempts)))
                            .child(stat_card(
                                "筆記總數",
                                format!("{} 篇", self.library.notes.len()),
                            ))
                            .child(stat_card(
                                "科目分組",
                                format!("{} 組", self.library.subject_groups.len()),
                            )),
                    )
                    .child(section_label("弱點知識卡（點按直接開新分頁複習）"))
                    .child(weak_cards),
            )
            .into_any_element()
    }

    fn settings_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let name_focused = self.text_focus == TextFocus::DisplayName;
        let date_focused = self.text_focus == TextFocus::PracticalDate;
        let db_path = self.store.db_path().display().to_string();

        div()
            .id("settings-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_8()
            .py_6()
            .child(
                div()
                    .max_w(px(640.))
                    .mx_auto()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(ink())
                            .child("工作台設定"),
                    )
                    .children(self.error.clone().map(|e| self.notice_box(e)))
                    .children(self.status_notice.clone().map(|n| {
                        div()
                            .px_4()
                            .py_2()
                            .rounded_xl()
                            .bg(good_soft())
                            .text_size(px(12.))
                            .text_color(good())
                            .child(n)
                    }))
                    .child(
                        div()
                            .p_5()
                            .rounded_2xl()
                            .bg(surface())
                            .border_1()
                            .border_color(line())
                            .flex()
                            .flex_col()
                            .gap_3()
                            .child(section_label("右上角顯示名稱"))
                            .child(
                                div()
                                    .id("settings-name-input")
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(if name_focused { accent() } else { line() })
                                    .bg(paper())
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .text_color(ink())
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.text_focus = TextFocus::DisplayName;
                                            cx.notify();
                                        }),
                                    )
                                    .child(if name_focused {
                                        format!("{}▌", self.display_name)
                                    } else {
                                        self.display_name.clone()
                                    }),
                            )
                            .child(section_label("實務考日期（YYYY-MM-DD，可留空）"))
                            .child(
                                div()
                                    .id("settings-date-input")
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .border_1()
                                    .border_color(if date_focused { accent() } else { line() })
                                    .bg(paper())
                                    .cursor_pointer()
                                    .text_size(px(14.))
                                    .text_color(if self.practical_date_draft.is_empty() {
                                        muted()
                                    } else {
                                        ink()
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.text_focus = TextFocus::PracticalDate;
                                            cx.notify();
                                        }),
                                    )
                                    .child(if self.practical_date_draft.is_empty() {
                                        if date_focused {
                                            "▌".to_owned()
                                        } else {
                                            "2026-06-15".to_owned()
                                        }
                                    } else if date_focused {
                                        format!("{}▌", self.practical_date_draft)
                                    } else {
                                        self.practical_date_draft.clone()
                                    }),
                            )
                            .child(
                                div()
                                    .id("settings-save-btn")
                                    .mt_1()
                                    .px_5()
                                    .py_2()
                                    .rounded_full()
                                    .bg(accent())
                                    .cursor_pointer()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(0xffffff))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                            app.save_settings();
                                            cx.notify();
                                        }),
                                    )
                                    .child("儲存設定"),
                            ),
                    )
                    .child(
                        div()
                            .p_4()
                            .rounded_xl()
                            .bg(surface())
                            .border_1()
                            .border_color(line())
                            .text_size(px(12.))
                            .text_color(muted())
                            .child(format!("本機資料庫路徑：{db_path}")),
                    ),
            )
            .into_any_element()
    }

    // =========================================================================
    // Shared Content Block Rendering
    // =========================================================================

    fn markdown_elements(&self, blocks: &[MarkdownBlock]) -> Vec<AnyElement> {
        blocks
            .iter()
            .map(|block| match block {
                MarkdownBlock::Heading { level, text } => div()
                    .mt_2()
                    .text_size(px(match level {
                        1 => 20.,
                        2 => 17.,
                        3 => 15.,
                        _ => 14.,
                    }))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(text.clone())
                    .into_any_element(),
                MarkdownBlock::Paragraph(text) => div()
                    .text_size(px(15.))
                    .line_height(px(25.))
                    .child(text.clone())
                    .into_any_element(),
                MarkdownBlock::List { ordered, items } => {
                    let mut list = div().flex().flex_col().gap_1();
                    for (index, item) in items.iter().enumerate() {
                        list = list.child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_2()
                                .child(div().text_size(px(14.)).text_color(muted()).child(
                                    if *ordered {
                                        format!("{}.", index + 1)
                                    } else {
                                        "•".to_owned()
                                    },
                                ))
                                .child(div().text_size(px(15.)).child(item.clone())),
                        );
                    }
                    list.into_any_element()
                }
                MarkdownBlock::Table { head, rows } => {
                    let mut table = div()
                        .flex()
                        .flex_col()
                        .border_1()
                        .border_color(line())
                        .rounded_md()
                        .overflow_hidden();
                    let mut header = div().flex().flex_row().bg(accent_soft());
                    for cell in head {
                        header = header.child(
                            div()
                                .flex_1()
                                .px_3()
                                .py_2()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(cell.clone()),
                        );
                    }
                    table = table.child(header);
                    for row in rows {
                        let mut row_element =
                            div().flex().flex_row().border_t_1().border_color(line());
                        for cell in row {
                            row_element = row_element.child(
                                div()
                                    .flex_1()
                                    .px_3()
                                    .py_2()
                                    .text_size(px(13.))
                                    .child(cell.clone()),
                            );
                        }
                        table = table.child(row_element);
                    }
                    table.into_any_element()
                }
                MarkdownBlock::Code { language, text } => {
                    let caption = language.clone().unwrap_or_else(|| "code".to_owned());
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_size(px(11.)).text_color(muted()).child(caption))
                        .child(
                            div()
                                .p_3()
                                .rounded_md()
                                .bg(paper())
                                .text_size(px(13.))
                                .child(text.clone()),
                        )
                        .into_any_element()
                }
                MarkdownBlock::Quote(text) => div()
                    .pl_3()
                    .border_l_2()
                    .border_color(accent())
                    .text_size(px(14.))
                    .text_color(muted())
                    .child(text.clone())
                    .into_any_element(),
                MarkdownBlock::Rule => div().h(px(1.)).w_full().bg(line()).into_any_element(),
            })
            .collect()
    }

    fn block_element(
        &self,
        key: &str,
        index: usize,
        block: &ContentBlock,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        match block {
            ContentBlock::Markdown { text } => Some(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(self.markdown_elements(&parse(text)))
                    .into_any_element(),
            ),
            ContentBlock::Math { latex, display } => {
                Some(match self.renderer.math(latex, *display) {
                    Ok(Some(path)) => self.image_element(path, 560.0),
                    Ok(None) => self.notice_box(format!("無法轉換的數學式：{latex}")),
                    Err(error) => self.notice_box(format!("數學式渲染失敗：{error}")),
                })
            }
            ContentBlock::Diagram { .. } => Some(match self.renderer.diagram(key, index, block) {
                Ok(Figure::Image(path)) => self.image_element(path, 620.0),
                Ok(Figure::Unsupported(note)) => self.notice_box(note),
                Err(error) => self.notice_box(format!("圖表渲染失敗：{error}")),
            }),
            ContentBlock::InteractivePins {
                resource_id,
                overlay_resource_id,
                pins,
            } => Some(self.pins_figure(resource_id, overlay_resource_id.as_ref(), pins, cx)),
            ContentBlock::ImageOcclusion { resource_id, masks } => {
                let mut container = div()
                    .relative()
                    .w(px(520.))
                    .h(px(325.))
                    .rounded_md()
                    .overflow_hidden()
                    .bg(paper());
                match self.renderer.resource(&self.store, resource_id) {
                    Ok(Figure::Image(path)) => {
                        container =
                            container.child(img(path).absolute().left_0().top_0().size_full());
                    }
                    Ok(Figure::Unsupported(note)) => {
                        container = container
                            .child(div().p_3().text_size(px(12.)).text_color(bad()).child(note));
                    }
                    Err(error) => {
                        container = container.child(
                            div()
                                .p_3()
                                .text_size(px(12.))
                                .text_color(bad())
                                .child(format!("載入圖片失敗：{error}")),
                        );
                    }
                }
                for mask in masks {
                    container = container.child(
                        div()
                            .absolute()
                            .left(relative(mask.rect.x as f32))
                            .top(relative(mask.rect.y as f32))
                            .w(relative(mask.rect.w as f32))
                            .h(relative(mask.rect.h as f32))
                            .bg(rgba(0x1f2428d1)),
                    );
                }
                Some(container.into_any_element())
            }
        }
    }

    fn pins_figure(
        &self,
        resource_id: &reinforce_core::ResourceId,
        overlay: Option<&reinforce_core::ResourceId>,
        pins: &[reinforce_core::content::Pin],
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut container = div()
            .relative()
            .w(px(520.))
            .h(px(325.))
            .rounded_md()
            .overflow_hidden()
            .bg(surface())
            .border_1()
            .border_color(line());
        match self.renderer.resource(&self.store, resource_id) {
            Ok(Figure::Image(path)) => {
                container = container.child(img(path).absolute().left_0().top_0().size_full());
            }
            Ok(Figure::Unsupported(note)) => {
                container =
                    container.child(div().p_3().text_size(px(12.)).text_color(bad()).child(note));
            }
            Err(error) => {
                container = container.child(
                    div()
                        .p_3()
                        .text_size(px(12.))
                        .text_color(bad())
                        .child(format!("載入圖片失敗：{error}")),
                );
            }
        }
        if let Some(overlay_id) = overlay
            && let Ok(Figure::Image(path)) = self.renderer.resource(&self.store, overlay_id)
        {
            container = container.child(img(path).absolute().left_0().top_0().size_full());
        }
        for pin in pins {
            container = container
                .child(
                    div()
                        .absolute()
                        .left(relative(pin.x as f32 - 0.012))
                        .top(relative(pin.y as f32 - 0.019))
                        .size_4()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(bad())
                        .text_size(px(10.))
                        .text_color(rgb(0xffffff))
                        .child(format!("{}", pin.no)),
                )
                .child(
                    div()
                        .absolute()
                        .left(relative(pin.label_x as f32))
                        .top(relative(pin.label_y as f32))
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .bg(surface())
                        .border_1()
                        .border_color(line())
                        .text_size(px(11.))
                        .child(pin.term_primary.clone()),
                );
        }
        container.into_any_element()
    }

    fn image_element(&self, path: PathBuf, max_width: f32) -> AnyElement {
        let (width, height) = crate::render::svg_size(&path).unwrap_or((320.0, 80.0));
        let scale = (max_width / width).min(1.4);
        div()
            .py_2()
            .child(img(path).w(px(width * scale)).h(px(height * scale)))
            .into_any_element()
    }

    fn notice_box(&self, text: String) -> AnyElement {
        div()
            .px_4()
            .py_2()
            .rounded_xl()
            .bg(bad_soft())
            .text_size(px(13.))
            .text_color(bad())
            .child(text)
            .into_any_element()
    }
}

impl Focusable for DesktopApp {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DesktopApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut row = div().flex().flex_row().flex_1().min_h_0();

        if self.left_open {
            row = row.child(self.left_sidebar(cx)).child(
                div()
                    .id("resize-left-handle")
                    .w(px(4.))
                    .flex_none()
                    .h_full()
                    .bg(line())
                    .hover(|this| this.bg(accent()))
                    .cursor_col_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                            app.dragging = Some(ResizeHandle::Left);
                            cx.notify();
                        }),
                    ),
            );
        }

        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .bg(paper())
                .flex()
                .flex_col()
                .child(self.center_frame(cx)),
        );

        if self.right_open {
            row = row
                .child(
                    div()
                        .id("resize-right-handle")
                        .w(px(4.))
                        .flex_none()
                        .h_full()
                        .bg(line())
                        .hover(|this| this.bg(accent()))
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                app.dragging = Some(ResizeHandle::Right);
                                cx.notify();
                            }),
                        ),
                )
                .child(self.right_sidebar(cx));
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(paper())
            .text_color(ink())
            .text_size(px(14.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(self.top_bar(cx))
            .child(row)
    }
}

fn collect_outline_cards<'a>(
    nodes: &[OutlineNode],
    outline: &'a NoteOutline,
) -> Vec<&'a KnowledgeCard> {
    let mut out = Vec::new();
    fn walk<'a>(nodes: &[OutlineNode], outline: &'a NoteOutline, out: &mut Vec<&'a KnowledgeCard>) {
        for node in nodes {
            match node {
                OutlineNode::Section { children, .. } => walk(children, outline, out),
                OutlineNode::CardRef { card_id } => {
                    if let Some(card) = outline.cards.iter().find(|c| &c.id == card_id)
                        && !out
                            .iter()
                            .any(|existing: &&KnowledgeCard| existing.id == card.id)
                    {
                        out.push(card);
                    }
                }
            }
        }
    }
    walk(nodes, outline, &mut out);
    out
}

fn format_unit_label(unit: &QuestionUnit, questions: &[QuestionCard]) -> String {
    let nums: Vec<u32> = unit
        .session_indices
        .iter()
        .map(|&idx| {
            questions
                .get(idx)
                .and_then(|q| q.question_no)
                .unwrap_or(idx as u32 + 1)
        })
        .collect();
    if nums.len() > 1 {
        format!("第 {}-{} 題", nums[0], nums[nums.len() - 1])
    } else {
        format!("第 {} 題", nums.first().copied().unwrap_or(1))
    }
}

fn format_unit_breadcrumb(unit: &QuestionUnit, questions: &[QuestionCard]) -> String {
    let mut parts = vec![unit.subject.clone()];
    if let Some(year) = unit.exam_year {
        parts.push(format!("{year} 年"));
    } else if let Some(title) = &unit.exam_title {
        parts.push(title.clone());
    }
    parts.push(format_unit_label(unit, questions));
    parts.join(" / ")
}

fn section_label(text: &str) -> AnyElement {
    div()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(muted())
        .child(text.to_owned())
        .into_any_element()
}

fn badge(text: impl Into<SharedString>, background: Rgba, color: Rgba) -> AnyElement {
    div()
        .px_2p5()
        .py_0p5()
        .rounded_full()
        .bg(background)
        .text_size(px(11.))
        .text_color(color)
        .child(text.into())
        .into_any_element()
}

fn stat_card(label: &str, value: String) -> AnyElement {
    div()
        .flex_1()
        .p_4()
        .rounded_xl()
        .bg(surface())
        .border_1()
        .border_color(line())
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .text_size(px(11.))
                .text_color(muted())
                .child(label.to_owned()),
        )
        .child(
            div()
                .text_size(px(22.))
                .font_weight(FontWeight::BOLD)
                .text_color(ink())
                .child(value),
        )
        .into_any_element()
}

fn digit(key: &str) -> Option<usize> {
    match key {
        "1" => Some(0),
        "2" => Some(1),
        "3" => Some(2),
        "4" => Some(3),
        "5" => Some(4),
        _ => None,
    }
}

/// Opens the desktop window; kept here so the binary stays a thin wrapper.
pub fn run(
    store: Store,
    renderer: Renderer,
    practical: Option<PracticalCountdown>,
    launch: LaunchOptions,
) {
    gpui::Application::new().run(move |cx| {
        let bounds = Bounds {
            origin: point(px(40.), px(30.)),
            size: size(px(1440.), px(960.)),
        };
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| DesktopApp::new(store, renderer, practical, launch, cx));
                let handle = view.read(cx).focus.clone();
                window.focus(&handle);
                view
            },
        )
        .expect("the desktop window should open");
        cx.activate(true);
    });
}

/// Small helper so an error repeated at startup still leaves a usable window.
pub fn with_cache_dir(db_path: &std::path::Path) -> PathBuf {
    db_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("desktop-cache")
}
