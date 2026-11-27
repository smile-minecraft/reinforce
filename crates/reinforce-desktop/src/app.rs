//! The GPUI window: a three-band layout with a library rail, a reading room and
//! a quiz runner.
//!
//! Rendering is deliberately straightforward: the state lives in [`crate::model`]
//! and this module only turns it into elements and keyboard actions.

use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Bounds, Context, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyDownEvent, MouseButton, Render, Rgba, SharedString, Window, WindowBounds, WindowOptions,
    div, img, point, px, relative, rgb, rgba, size,
};

use reinforce_core::content::{ContentBlock, KnowledgeCard, OutlineNode};
use reinforce_core::db::catalog::NoteOutline;
use reinforce_core::primitives::QuestionType;
use reinforce_core::quiz::Score;
use reinforce_core::{Timestamp, User};

use crate::markdown::{Block as MarkdownBlock, parse};
use crate::model::{
    Library, PracticalCountdown, Quiz, QuizMode, QuizTargets, Summary, desktop_options,
};
use crate::render::{Figure, Renderer};
use crate::store::Store;

fn paper() -> Rgba {
    rgb(0xfaf7f2)
}
fn panel() -> Rgba {
    rgb(0xf0eae1)
}
fn surface() -> Rgba {
    rgb(0xfffdfa)
}
fn line() -> Rgba {
    rgb(0xe2dbcf)
}
fn ink() -> Rgba {
    rgb(0x2a2622)
}
fn muted() -> Rgba {
    rgb(0x8b8178)
}
fn accent() -> Rgba {
    rgb(0x1e6b58)
}
fn accent_soft() -> Rgba {
    rgb(0xe3efe9)
}
fn good() -> Rgba {
    rgb(0x2f7a4f)
}
fn good_soft() -> Rgba {
    rgb(0xe4f1e8)
}
fn bad() -> Rgba {
    rgb(0xa94a2c)
}
fn bad_soft() -> Rgba {
    rgb(0xf7e7e0)
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
enum View {
    Reading,
    Card,
    Quiz,
    Summary,
}

/// The window root.
pub struct DesktopApp {
    store: Store,
    renderer: Renderer,
    library: Library,
    note_index: usize,
    card_index: usize,
    quiz: Option<Quiz>,
    summary: Option<Summary>,
    view: View,
    practical: Option<PracticalCountdown>,
    user: User,
    attempts: usize,
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
            Err(error) => {
                // A store without a writable users table still renders; the
                // error is shown in the footer instead of crashing the window.
                User {
                    id: reinforce_core::UserId::generate(),
                    display_name: format!("Local Learner ({error})"),
                    created_at: Timestamp::now(),
                }
            }
        };
        let mut app = Self {
            store,
            renderer,
            library: Library::default(),
            note_index: 0,
            card_index: 0,
            quiz: None,
            summary: None,
            view: View::Reading,
            practical,
            user,
            attempts: 0,
            error: None,
            focus: cx.focus_handle(),
        };
        app.reload();
        if let StartScreen::Card(index) = launch.start {
            app.open_card(index);
        }
        if let StartScreen::Quiz(mode) = launch.start {
            app.start_quiz(mode);
            if let Some(quiz) = app.quiz.as_mut() {
                let keys = launch.preselect.clone();
                quiz.select_many(&keys);
            }
        }
        app.spawn_clock(cx);
        app
    }

    fn reload(&mut self) {
        match Library::load(&self.store) {
            Ok(library) => {
                self.library = library;
                if !self.library.notes.is_empty() {
                    let index = self.note_index.min(self.library.notes.len() - 1);
                    self.note_index = index;
                    if let Err(error) = self.library.select(&self.store, index) {
                        self.error = Some(error.to_string());
                    }
                }
            }
            Err(error) => self.error = Some(format!("無法讀取資料庫：{error}")),
        }
        self.attempts = self.store.attempt_count(&self.user.id).unwrap_or(0);
    }

    fn open_note(&mut self, index: usize) {
        self.note_index = index;
        match self.library.select(&self.store, index) {
            Ok(()) => {
                self.view = View::Reading;
                self.quiz = None;
                self.summary = None;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("無法開啟筆記：{error}")),
        }
    }

    fn open_card(&mut self, index: usize) {
        if index >= self.library.cards.len() {
            return;
        }
        self.card_index = index;
        self.view = View::Card;
        self.quiz = None;
        self.summary = None;
        self.error = None;
    }

    fn start_quiz(&mut self, mode: QuizMode) {
        let targets = self.library.targets();
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
                self.error = Some(format!("{}：這個資料庫沒有可出的題目", mode.title()));
                self.view = View::Reading;
                self.quiz = None;
            }
            Ok(quiz) => {
                self.quiz = Some(quiz);
                self.summary = None;
                self.error = None;
                self.view = View::Quiz;
            }
            Err(error) => {
                self.error = Some(format!("{}：{error}", mode.title()));
                self.view = View::Reading;
            }
        }
    }

    fn open_exam(&mut self, index: usize) {
        let targets = QuizTargets {
            exam_set: self
                .library
                .exam_sets
                .get(index)
                .map(|summary| summary.exam_set.id.clone()),
            cards: self
                .library
                .outline()
                .map(|outline| outline.cards.iter().map(|card| card.id.clone()).collect())
                .unwrap_or_default(),
        };
        let now = Timestamp::now();
        match Quiz::start(
            &self.store,
            QuizMode::FullExam,
            &targets,
            &desktop_options(),
            &self.user.id,
            now,
        ) {
            Ok(quiz) if quiz.is_empty() => {
                self.error = Some("這份試卷沒有已發布的題目".to_owned());
            }
            Ok(quiz) => {
                self.quiz = Some(quiz);
                self.summary = None;
                self.error = None;
                self.view = View::Quiz;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn submit_answer(&mut self) {
        let now = Timestamp::now();
        let Some(quiz) = self.quiz.as_mut() else {
            return;
        };
        match quiz.submit(now) {
            Ok(Some(outcome)) => {
                if outcome.voided {
                    self.error = Some("此題為作廢題，不計分".to_owned());
                }
            }
            Ok(None) => {}
            Err(error) => self.error = Some(format!("送出作答失敗：{error}")),
        }
        if self.quiz.as_ref().is_some_and(Quiz::is_finished) {
            self.finish_quiz();
        }
    }

    fn finish_quiz(&mut self) {
        let Some(mut quiz) = self.quiz.take() else {
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
        self.summary = Some(Summary::new(mode, &score, written));
        self.attempts = self.store.attempt_count(&self.user.id).unwrap_or(0);
        self.view = View::Summary;
    }

    fn spawn_clock(&self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let alive = this.update(cx, |app, cx| {
                    let timed = app
                        .quiz
                        .as_ref()
                        .is_some_and(|quiz| quiz.seconds_left(Timestamp::now()).is_some());
                    if !timed {
                        return;
                    }
                    let now = Timestamp::now();
                    if let Some(quiz) = app.quiz.as_mut() {
                        let _ = quiz.tick(now);
                        let finished = quiz.is_finished();
                        if finished {
                            app.finish_quiz();
                        }
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

    fn on_key(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let key = keystroke.key.as_str();
        if keystroke.modifiers.platform || keystroke.modifiers.control {
            return;
        }
        match self.view {
            View::Quiz => self.on_quiz_key(key, keystroke.key_char.as_deref()),
            View::Summary => {
                if matches!(key, "enter" | "escape" | "space") {
                    self.view = View::Reading;
                } else if let Some(index) = digit(key)
                    && let Some(mode) = QuizMode::ALL.get(index)
                {
                    self.start_quiz(*mode);
                }
            }
            View::Card => {
                if key == "escape" {
                    self.view = View::Reading;
                } else {
                    self.on_reading_key(key);
                }
            }
            View::Reading => self.on_reading_key(key),
        }
        cx.notify();
    }

    fn on_reading_key(&mut self, key: &str) {
        match key {
            "j" | "down" => {
                if self.note_index + 1 < self.library.notes.len() {
                    let index = self.note_index + 1;
                    self.open_note(index);
                }
            }
            "k" | "up" => {
                let index = self.note_index.saturating_sub(1);
                self.open_note(index);
            }
            "r" => self.reload(),
            "escape" => {
                self.summary = None;
                self.quiz = None;
                self.view = View::Reading;
            }
            other => {
                if let Some(index) = digit(other)
                    && let Some(mode) = QuizMode::ALL.get(index)
                {
                    self.start_quiz(*mode);
                }
            }
        }
    }

    fn on_quiz_key(&mut self, key: &str, key_char: Option<&str>) {
        match key {
            "escape" => {
                self.quiz = None;
                self.view = View::Reading;
            }
            "enter" => {
                if self.quiz.as_ref().is_some_and(Quiz::is_finished) {
                    self.finish_quiz();
                } else {
                    self.submit_answer();
                }
            }
            "backspace" => {
                if let Some(quiz) = self.quiz.as_mut() {
                    quiz.backspace();
                }
            }
            _ => {
                let Some(quiz) = self.quiz.as_mut() else {
                    return;
                };
                if quiz.accepts_text() {
                    if let Some(character) = key_char.and_then(|value| value.chars().next()) {
                        quiz.push_char(character);
                    }
                } else if let Some(character) = key_char.and_then(|value| value.chars().next())
                    && character.is_ascii_alphanumeric()
                {
                    let option = character.to_ascii_uppercase().to_string();
                    quiz.toggle_option(&option);
                }
            }
        }
    }

    // ---- Rendering -------------------------------------------------------

    fn header(&self) -> AnyElement {
        let countdown = self
            .practical
            .map(|countdown| countdown.label(Timestamp::now()));
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_6()
            .h(px(66.))
            .bg(rgb(0xfffdf9))
            .border_b_1()
            .border_color(line())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(21.))
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent())
                            .child("Reinforce"),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(muted())
                            .child("GPUI 閱讀室 · 本機資料庫"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .children(countdown.map(|label| badge(label, bad_soft(), bad())))
                    .child(badge(
                        format!("已記錄作答 {}", self.attempts),
                        accent_soft(),
                        accent(),
                    )),
            )
            .into_any_element()
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut notes = div().flex().flex_col().gap_1();
        for (index, note) in self.library.notes.iter().enumerate() {
            let selected = index == self.note_index && self.view == View::Reading;
            notes = notes.child(
                div()
                    .id(SharedString::from(format!("note-{index}")))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |this| this.bg(accent_soft()))
                    .hover(|this| this.bg(panel()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            app.open_note(index);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(if selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if selected { accent() } else { ink() })
                            .child(note.title.clone()),
                    )
                    .child(div().text_size(px(11.)).text_color(muted()).child(format!(
                        "{} · {}",
                        note.subject,
                        note.status.as_str()
                    ))),
            );
        }

        let mut cards = div().flex().flex_col().gap_1();
        for (index, card) in self.library.cards.iter().enumerate() {
            let selected = index == self.card_index && self.view == View::Card;
            cards = cards.child(
                div()
                    .id(SharedString::from(format!("card-{index}")))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(selected, |this| this.bg(accent_soft()))
                    .hover(|this| this.bg(panel()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            app.open_card(index);
                            cx.notify();
                        }),
                    )
                    .child(div().text_size(px(14.)).child(card.title.clone()))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(muted())
                            .child(card.topics.join(" · ")),
                    ),
            );
        }

        let mut exams = div().flex().flex_col().gap_1();
        for (index, summary) in self.library.exam_sets.iter().enumerate() {
            exams = exams.child(
                div()
                    .id(SharedString::from(format!("exam-{index}")))
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|this| this.bg(panel()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            app.open_exam(index);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .child(summary.exam_set.title.clone()),
                    )
                    .child(div().text_size(px(11.)).text_color(muted()).child(format!(
                        "{} 題 · {}",
                        summary.published_question_count,
                        summary.exam_set.source_kind.as_str()
                    ))),
            );
        }

        let mut modes = div().flex().flex_col().gap_1();
        for (index, mode) in QuizMode::ALL.iter().enumerate() {
            let mode = *mode;
            modes = modes.child(
                div()
                    .id(SharedString::from(format!("mode-{index}")))
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|this| this.bg(panel()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                            app.start_quiz(mode);
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .mt_0p5()
                            .size_5()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .bg(accent_soft())
                            .text_size(px(11.))
                            .text_color(accent())
                            .child(format!("{}", index + 1)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(div().text_size(px(14.)).child(mode.title()))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(muted())
                                    .child(mode.description()),
                            ),
                    ),
            );
        }

        div()
            .w(px(304.))
            .flex_none()
            .h_full()
            .bg(panel())
            .border_r_1()
            .border_color(line())
            .id("sidebar")
            .overflow_y_scroll()
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(section_label("筆記"))
            .child(notes)
            .child(section_label("知識卡"))
            .child(cards)
            .child(section_label("試卷"))
            .child(exams)
            .child(section_label("刷題模式（按 1–5 開始）"))
            .child(modes)
            .into_any_element()
    }

    fn main_pane(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.view {
            View::Reading => self.reading_room(cx),
            View::Card => self.card_room(cx),
            View::Quiz => self.quiz_room(cx),
            View::Summary => self.summary_room(cx),
        }
    }

    fn reading_room(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(outline) = self.library.outline() else {
            return div()
                .p_10()
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("這個資料庫還沒有內容"),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(muted())
                        .child("用 --import fixtures/demo 匯入示範資料，或先以 CLI 發布筆記。"),
                )
                .into_any_element();
        };
        let note = &outline.note;
        let mut sections: Vec<AnyElement> = Vec::new();
        self.render_outline(&outline.note.outline, outline, 0, &mut sections, cx);
        div()
            .px_10()
            .py_6()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(28.))
                            .font_weight(FontWeight::BOLD)
                            .child(note.title.clone()),
                    )
                    .child(badge(note.subject.clone(), accent_soft(), accent())),
            )
            .child(div().text_size(px(12.)).text_color(muted()).child(format!(
                "{} 張知識卡 · 更新於 {}",
                outline.cards.len(),
                note.updated_at.as_str()
            )))
            .children(sections)
            .into_any_element()
    }

    fn card_room(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(card) = self.library.cards.get(self.card_index) else {
            return div().p_10().child("沒有知識卡").into_any_element();
        };
        div()
            .px_10()
            .py_8()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_size(px(12.)).text_color(muted()).child(format!(
                "知識卡 · {} · {}",
                card.subject,
                card.verification.as_str()
            )))
            .child(self.card_view(card, cx))
            .child(
                div()
                    .mt_2()
                    .text_size(px(12.))
                    .text_color(muted())
                    .child("Esc 回筆記 · 1–5 開始刷題"),
            )
            .into_any_element()
    }

    fn render_outline(
        &self,
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
                    output.push(
                        div()
                            .mt_4()
                            .text_size(px(if depth == 0 { 21. } else { 17. }))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(accent())
                            .child(heading.clone())
                            .into_any_element(),
                    );
                    if let Some(intro) = intro_markdown {
                        output.push(
                            div()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .children(self.markdown_elements(&parse(intro)))
                                .into_any_element(),
                        );
                    }
                    self.render_outline(children, outline, depth + 1, output, cx);
                }
                OutlineNode::CardRef { card_id } => {
                    if let Some(card) = outline.cards.iter().find(|card| &card.id == card_id) {
                        output.push(self.card_view(card, cx));
                    } else {
                        output.push(
                            div()
                                .p_4()
                                .rounded_md()
                                .bg(bad_soft())
                                .text_size(px(13.))
                                .text_color(bad())
                                .child(format!("缺少知識卡 {card_id}"))
                                .into_any_element(),
                        );
                    }
                }
            }
        }
    }

    fn card_view(&self, card: &KnowledgeCard, cx: &mut Context<Self>) -> AnyElement {
        let key = card.id.as_str();
        let mut body: Vec<AnyElement> = Vec::new();
        for (index, block) in card.blocks.iter().enumerate() {
            if let Some(element) = self.block_element(&key, index, block, cx) {
                body.push(element);
            }
        }
        div()
            .bg(surface())
            .border_1()
            .border_color(line())
            .rounded_lg()
            .p_5()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(card.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(muted())
                            .child(card.topics.join(" · ")),
                    ),
            )
            .children(body)
            .into_any_element()
    }

    fn markdown_elements(&self, blocks: &[MarkdownBlock]) -> Vec<AnyElement> {
        blocks
            .iter()
            .map(|block| match block {
                MarkdownBlock::Heading { level, text } => div()
                    .mt_2()
                    .text_size(px(match level {
                        1 => 22.,
                        2 => 18.,
                        3 => 16.,
                        _ => 14.,
                    }))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(text.clone())
                    .into_any_element(),
                MarkdownBlock::Paragraph(text) => div()
                    .text_size(px(15.))
                    .line_height(px(26.))
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
                                .bg(rgb(0xf4efe7))
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
                    .bg(rgb(0xf4efe7));
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
                            .bg(rgba(0x2a2622d1)),
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
                        .bg(rgb(0xfffdf9))
                        .border_1()
                        .border_color(line())
                        .text_size(px(11.))
                        .child(pin.term_primary.clone()),
                );
        }
        container.into_any_element()
    }

    fn quiz_room(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(quiz) = self.quiz.as_ref() else {
            return div().p_10().child("沒有進行中的測驗").into_any_element();
        };
        let mode = quiz.mode();
        let total = quiz.total();
        let index = quiz.index();
        let now = Timestamp::now();
        let timer = quiz.seconds_left(now);
        let mut body: Vec<AnyElement> = Vec::new();

        if let Some(question) = quiz.current() {
            body.push(
                div()
                    .p_5()
                    .bg(surface())
                    .border_1()
                    .border_color(line())
                    .rounded_lg()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_size(px(11.)).text_color(muted()).child(format!(
                        "第 {} 題 · {} · {}",
                        question.question_no.unwrap_or(index as u32 + 1),
                        question.question_type.as_str(),
                        question.grading_mode.as_str()
                    )))
                    .children(self.question_blocks(question, cx))
                    .into_any_element(),
            );

            if question.options.is_empty() {
                let pins = self.question_pins(question);
                if question.question_type == QuestionType::PinLocate {
                    body.push(
                        self.notice_box(
                            "這題需要在圖片上自由定位；桌面版先以空白作答，位置輸入留給 WebApp。"
                                .to_owned(),
                        ),
                    );
                } else if !pins.is_empty() {
                    let mut chips = div().flex().flex_row().flex_wrap().gap_2();
                    for pin in pins {
                        let key = format!("{}", pin.no);
                        let selected = quiz.selection().contains(&key);
                        let number = key.clone();
                        let quiz_key = key.clone();
                        chips = chips.child(
                            div()
                                .id(SharedString::from(format!("pin-option-{number}")))
                                .px_4()
                                .py_2()
                                .rounded_md()
                                .cursor_pointer()
                                .border_1()
                                .border_color(if selected { accent() } else { line() })
                                .bg(if selected { accent_soft() } else { surface() })
                                .text_size(px(14.))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                        if let Some(quiz) = app.quiz.as_mut() {
                                            quiz.toggle_option(&quiz_key);
                                        }
                                        cx.notify();
                                    }),
                                )
                                .child(format!("釘點 {number}")),
                        );
                    }
                    body.push(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(section_label("選出正確的釘點"))
                            .child(chips)
                            .into_any_element(),
                    );
                }
            } else {
                let mut options = div().flex().flex_col().gap_2();
                for option in &question.options {
                    let selected = quiz.selection().contains(&option.key);
                    let key = option.key.clone();
                    options = options.child(
                        div()
                            .id(SharedString::from(format!("option-{}", option.key)))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .px_4()
                            .py_3()
                            .rounded_md()
                            .cursor_pointer()
                            .border_1()
                            .border_color(if selected { accent() } else { line() })
                            .bg(if selected { accent_soft() } else { surface() })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                                    if let Some(quiz) = app.quiz.as_mut() {
                                        quiz.toggle_option(&key);
                                    }
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .size_6()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(if selected { accent() } else { line() })
                                    .text_size(px(12.))
                                    .text_color(if selected { accent() } else { muted() })
                                    .child(option.key.clone()),
                            )
                            .child(div().text_size(px(15.)).child(option.content.clone())),
                    );
                }
                body.push(options.into_any_element());
            }

            if quiz.accepts_text() {
                body.push(
                    div()
                        .px_4()
                        .py_3()
                        .rounded_md()
                        .bg(surface())
                        .border_1()
                        .border_color(accent())
                        .text_size(px(16.))
                        .child(if quiz.typed().is_empty() {
                            "輸入答案後按 Enter".to_owned()
                        } else {
                            format!("{}▌", quiz.typed())
                        })
                        .into_any_element(),
                );
            }
        } else {
            body.push(
                div()
                    .p_5()
                    .rounded_lg()
                    .bg(surface())
                    .border_1()
                    .border_color(line())
                    .child("所有題目都已作答，按 Enter 交卷。")
                    .into_any_element(),
            );
        }

        if let Some(outcome) = quiz.feedback() {
            let (background, color, label) = if outcome.voided {
                (rgb(0xf3efe6), muted(), "作廢題，不計分")
            } else if outcome.is_correct {
                (good_soft(), good(), "答對了")
            } else {
                (bad_soft(), bad(), "答錯了")
            };
            if !mode.deferred() {
                body.push(
                    div()
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(background)
                        .text_size(px(13.))
                        .text_color(color)
                        .child(format!("{label} · {}", outcome.note))
                        .into_any_element(),
                );
            }
        }

        div()
            .px_10()
            .py_8()
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
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(mode.title()),
                            )
                            .child(div().text_size(px(12.)).text_color(muted()).child(format!(
                                "第 {} / {} 題 · 已作答 {}",
                                (index + 1).min(total),
                                total,
                                quiz.submitted()
                            ))),
                    )
                    .children(
                        timer.map(|seconds| badge(format!("{seconds} 秒"), bad_soft(), bad())),
                    ),
            )
            .children(body)
            .child(div().text_size(px(12.)).text_color(muted()).child(
                "A/B/C 選擇 · Enter 送出 · Backspace 刪字 · Esc 離開 · 完成後自動寫入 AttemptLog",
            ))
            .into_any_element()
    }

    fn question_blocks(
        &self,
        question: &reinforce_core::content::QuestionCard,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut blocks = Vec::new();
        for (index, block) in question.stem_blocks.iter().enumerate() {
            // Question blocks have no card key of their own; the question id
            // keeps diagram cache names unique.
            if let Some(element) = self.block_element(&question.id.as_str(), index, block, cx) {
                blocks.push(element);
            }
        }
        blocks
    }

    fn question_pins(
        &self,
        question: &reinforce_core::content::QuestionCard,
    ) -> Vec<reinforce_core::content::Pin> {
        let mut pins = Vec::new();
        for block in &question.stem_blocks {
            if let ContentBlock::InteractivePins {
                pins: block_pins, ..
            } = block
            {
                pins.extend(block_pins.iter().cloned());
            }
        }
        pins
    }

    fn summary_room(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(summary) = self.summary.as_ref() else {
            return div().p_10().child("沒有成績").into_any_element();
        };
        let percent = (summary.ratio * 100.0).round() as i64;
        div()
            .px_10()
            .py_10()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(muted())
                    .child(summary.mode.title()),
            )
            .child(
                div()
                    .text_size(px(56.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(accent())
                    .child(format!("{percent}%")),
            )
            .child(div().text_size(px(15.)).child(format!(
                "答對 {} / {} 題，作廢 {} 題",
                summary.correct, summary.scoreable, summary.voided
            )))
            .child(div().text_size(px(13.)).text_color(muted()).child(format!(
                "已寫入 {} 筆 AttemptLog，並更新 FSRS 排程",
                summary.written
            )))
            .child(
                div()
                    .mt_4()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .child(
                        div()
                            .id("back-to-reading")
                            .px_5()
                            .py_2()
                            .rounded_md()
                            .bg(accent())
                            .text_color(rgb(0xffffff))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                    app.view = View::Reading;
                                    cx.notify();
                                }),
                            )
                            .child("回閱讀室（Enter）"),
                    )
                    .child(
                        div()
                            .id("rerun")
                            .px_5()
                            .py_2()
                            .rounded_md()
                            .border_1()
                            .border_color(line())
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                                    let mode = app.summary.as_ref().map(|summary| summary.mode);
                                    if let Some(mode) = mode {
                                        app.start_quiz(mode);
                                    }
                                    cx.notify();
                                }),
                            )
                            .child("再來一輪"),
                    ),
            )
            .children(self.error.clone().map(|error| self.notice_box(error)))
            .into_any_element()
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
            .rounded_md()
            .bg(bad_soft())
            .text_size(px(13.))
            .text_color(bad())
            .child(text)
            .into_any_element()
    }

    fn footer(&self) -> AnyElement {
        let db = self.store.db_path().display().to_string();
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px_6()
            .h(px(34.))
            .bg(rgb(0xfffdf9))
            .border_t_1()
            .border_color(line())
            .text_size(px(11.))
            .text_color(muted())
            .child(div().child(format!(
                "資料庫：{db}　·　j/k 切換筆記 · 1–5 刷題 · Enter 送出 · Esc 返回"
            )))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_4()
                    .children(
                        self.error
                            .clone()
                            .map(|error| div().text_color(bad()).child(error)),
                    )
                    .child(format!(
                        "{} · {} 筆作答",
                        self.user.display_name, self.attempts
                    )),
            )
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
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(paper())
            .text_color(ink())
            .text_size(px(15.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .child(self.header())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .child(self.main_pane(cx)),
                    ),
            )
            .child(self.footer())
    }
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
        .px_3()
        .py_1()
        .rounded_full()
        .bg(background)
        .text_size(px(11.))
        .text_color(color)
        .child(text.into())
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
            size: size(px(1400.), px(1060.)),
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
