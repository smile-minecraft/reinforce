//! View state for the desktop client.
//!
//! Everything in this module is plain Rust: it owns no window, no GPU context
//! and no GPUI types. The window layer drives it from keyboard and mouse events
//! and renders whatever it exposes, which keeps the behaviour that matters —
//! multi-tab navigation, nested subject groups, linear note check-off progress,
//! grouped question units, answer composition and attempt commits — covered by
//! ordinary `cargo test`.

use std::collections::{BTreeSet, HashMap, HashSet};

use reinforce_core::content::{ExamSet, KnowledgeCard, QuestionCard, QuestionGroup};
use reinforce_core::db::catalog::{ExamSetSummary, NoteOutline, SubjectSummary};
use reinforce_core::primitives::{Confidence, GradingMode, QuestionType, SessionMode};
use reinforce_core::quiz::build::{
    BuildOptions, build_daily_due, build_full_exam, build_practical_exam, build_subject_exam,
    build_topic_drill, build_weakness_drill,
};
use reinforce_core::quiz::{
    AnswerRecord, AnswerSubmission, GradedAnswer, GradingOutcome, NormalizedPoint, PinClick,
    QuizAnswer, QuizSession, Score, SessionState, TickOutcome,
};
use reinforce_core::{
    ExamSetId, KnowledgeCardId, Note, NoteId, QuestionCardId, QuestionGroupId, SubjectGroup,
    SubjectGroupId, Timestamp, UserId,
};

use crate::store::Store;
use crate::{Error, Result};

/// The five study modes the spec requires from every client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuizMode {
    DailyDue,
    FullExam,
    TopicDrill,
    PracticalExam,
    WeaknessDrill,
}

impl QuizMode {
    pub const ALL: [QuizMode; 5] = [
        QuizMode::DailyDue,
        QuizMode::FullExam,
        QuizMode::TopicDrill,
        QuizMode::PracticalExam,
        QuizMode::WeaknessDrill,
    ];

    pub fn title(self) -> &'static str {
        match self {
            QuizMode::DailyDue => "每日到期",
            QuizMode::FullExam => "整卷模擬",
            QuizMode::TopicDrill => "主題／卡片",
            QuizMode::PracticalExam => "實務考",
            QuizMode::WeaknessDrill => "弱點複習",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            QuizMode::DailyDue => "依 FSRS 到期日排序，含新卡",
            QuizMode::FullExam => "依題號或單一分組作答，交卷後統一評分",
            QuizMode::TopicDrill => "依知識點的 tests 邊抽題複習",
            QuizMode::PracticalExam => "釘選與影像題，逐題倒數",
            QuizMode::WeaknessDrill => "錯誤率高與沒把握的題目優先",
        }
    }

    pub fn session_mode(self) -> SessionMode {
        match self {
            QuizMode::DailyDue => SessionMode::DailyDue,
            QuizMode::FullExam => SessionMode::FullExam,
            QuizMode::TopicDrill => SessionMode::TopicDrill,
            QuizMode::PracticalExam => SessionMode::PracticalExam,
            QuizMode::WeaknessDrill => SessionMode::WeaknessDrill,
        }
    }

    /// Whether grading is deferred until the paper is handed in.
    pub fn deferred(self) -> bool {
        matches!(self, QuizMode::FullExam)
    }
}

/// Five-level confidence selector from the bottom bar sketch, mapped cleanly
/// onto the core 3-level `Confidence` enum without altering DB schema.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConfidenceLevel5 {
    #[default]
    Certain,
    FairlySure,
    Hesitant,
    Unfamiliar,
    Guessed,
}

impl ConfidenceLevel5 {
    pub const ALL: [ConfidenceLevel5; 5] = [
        ConfidenceLevel5::Certain,
        ConfidenceLevel5::FairlySure,
        ConfidenceLevel5::Hesitant,
        ConfidenceLevel5::Unfamiliar,
        ConfidenceLevel5::Guessed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConfidenceLevel5::Certain => "確定",
            ConfidenceLevel5::FairlySure => "還可以",
            ConfidenceLevel5::Hesitant => "猶豫中",
            ConfidenceLevel5::Unfamiliar => "不熟",
            ConfidenceLevel5::Guessed => "亂猜",
        }
    }

    pub fn to_core(self) -> (Confidence, Option<&'static str>) {
        match self {
            ConfidenceLevel5::Certain | ConfidenceLevel5::FairlySure => (Confidence::Certain, None),
            ConfidenceLevel5::Hesitant => (Confidence::Split, None),
            ConfidenceLevel5::Unfamiliar => (Confidence::Split, Some("concept_gap")),
            ConfidenceLevel5::Guessed => (Confidence::Guessed, None),
        }
    }
}

// ---------------------------------------------------------------------------
// Multi-tab workspace manager
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorkspaceTabKind {
    Home,
    Note(NoteId),
    Card(KnowledgeCardId),
    StudyHub,
    Quiz(u64),
    Analytics,
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTab {
    pub id: String,
    pub kind: WorkspaceTabKind,
    pub title: String,
}

impl WorkspaceTab {
    pub fn home() -> Self {
        Self {
            id: "home".to_owned(),
            kind: WorkspaceTabKind::Home,
            title: "首頁".to_owned(),
        }
    }

    pub fn study_hub() -> Self {
        Self {
            id: "study".to_owned(),
            kind: WorkspaceTabKind::StudyHub,
            title: "考試".to_owned(),
        }
    }

    pub fn analytics() -> Self {
        Self {
            id: "analytics".to_owned(),
            kind: WorkspaceTabKind::Analytics,
            title: "分析".to_owned(),
        }
    }

    pub fn settings() -> Self {
        Self {
            id: "settings".to_owned(),
            kind: WorkspaceTabKind::Settings,
            title: "設定".to_owned(),
        }
    }

    pub fn note(id: NoteId, title: impl Into<String>) -> Self {
        Self {
            id: format!("note:{}", id.as_str()),
            kind: WorkspaceTabKind::Note(id),
            title: title.into(),
        }
    }

    pub fn card(id: KnowledgeCardId, title: impl Into<String>) -> Self {
        Self {
            id: format!("card:{}", id.as_str()),
            kind: WorkspaceTabKind::Card(id),
            title: title.into(),
        }
    }

    pub fn quiz(session_key: u64, title: impl Into<String>) -> Self {
        Self {
            id: format!("quiz:{session_key}"),
            kind: WorkspaceTabKind::Quiz(session_key),
            title: title.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TabManager {
    tabs: Vec<WorkspaceTab>,
    active_id: String,
}

impl Default for TabManager {
    fn default() -> Self {
        let home = WorkspaceTab::home();
        let active_id = home.id.clone();
        Self {
            tabs: vec![home],
            active_id,
        }
    }
}

impl TabManager {
    pub fn tabs(&self) -> &[WorkspaceTab] {
        &self.tabs
    }

    pub fn active_id(&self) -> &str {
        &self.active_id
    }

    pub fn active(&self) -> &WorkspaceTab {
        self.tabs
            .iter()
            .find(|tab| tab.id == self.active_id)
            .unwrap_or(&self.tabs[0])
    }

    pub fn open_or_focus(&mut self, incoming: WorkspaceTab) {
        if let Some(existing) = self.tabs.iter_mut().find(|tab| tab.id == incoming.id) {
            if !incoming.title.trim().is_empty() {
                existing.title = incoming.title;
            }
            self.active_id = existing.id.clone();
            return;
        }
        self.active_id = incoming.id.clone();
        self.tabs.push(incoming);
    }

    pub fn activate(&mut self, id: &str) {
        if self.tabs.iter().any(|tab| tab.id == id) {
            self.active_id = id.to_owned();
        }
    }

    /// Closes `target_id` and returns its `WorkspaceTabKind` if it existed.
    pub fn close(&mut self, target_id: &str) -> Option<WorkspaceTabKind> {
        let idx = self.tabs.iter().position(|tab| tab.id == target_id)?;
        let removed = self.tabs.remove(idx);
        if self.tabs.is_empty() {
            let home = WorkspaceTab::home();
            self.active_id = home.id.clone();
            self.tabs.push(home);
        } else if self.active_id == target_id {
            let next_idx = idx.saturating_sub(1).min(self.tabs.len() - 1);
            self.active_id = self.tabs[next_idx].id.clone();
        }
        Some(removed.kind)
    }
}

// ---------------------------------------------------------------------------
// Nested Subject Groups tree
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectGroupNode {
    pub group: SubjectGroup,
    pub depth: usize,
    pub children: Vec<SubjectGroupNode>,
    pub resolved_subjects: Vec<String>,
}

pub fn build_subject_group_tree(groups: &[SubjectGroup]) -> Vec<SubjectGroupNode> {
    let mut sorted = groups.to_vec();
    sorted.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.id.as_str().cmp(&b.id.as_str()))
    });
    let ids: HashSet<SubjectGroupId> = sorted.iter().map(|g| g.id.clone()).collect();
    let mut children_map: HashMap<Option<SubjectGroupId>, Vec<SubjectGroup>> = HashMap::new();
    for group in sorted {
        let parent_key = group
            .parent_id
            .as_ref()
            .filter(|pid| ids.contains(*pid))
            .cloned();
        children_map.entry(parent_key).or_default().push(group);
    }

    fn build(
        parent: Option<SubjectGroupId>,
        depth: usize,
        children_map: &HashMap<Option<SubjectGroupId>, Vec<SubjectGroup>>,
        trail: &mut HashSet<SubjectGroupId>,
    ) -> Vec<SubjectGroupNode> {
        let Some(list) = children_map.get(&parent) else {
            return Vec::new();
        };
        let mut nodes = Vec::new();
        for group in list {
            if !trail.insert(group.id.clone()) {
                continue;
            }
            let children = build(Some(group.id.clone()), depth + 1, children_map, trail);
            let mut resolved: BTreeSet<String> = group.subjects.iter().cloned().collect();
            for child in &children {
                for subject in &child.resolved_subjects {
                    resolved.insert(subject.clone());
                }
            }
            nodes.push(SubjectGroupNode {
                group: group.clone(),
                depth,
                children,
                resolved_subjects: resolved.into_iter().collect(),
            });
            trail.remove(&group.id);
        }
        nodes
    }

    let mut trail = HashSet::new();
    build(None, 0, &children_map, &mut trail)
}

pub fn flatten_subject_group_tree(nodes: &[SubjectGroupNode]) -> Vec<SubjectGroupNode> {
    fn walk(nodes: &[SubjectGroupNode], out: &mut Vec<SubjectGroupNode>) {
        for node in nodes {
            out.push(node.clone());
            walk(&node.children, out);
        }
    }
    let mut out = Vec::new();
    walk(nodes, &mut out);
    out
}

pub fn is_descendant_or_self(
    groups: &[SubjectGroup],
    candidate_id: &SubjectGroupId,
    ancestor_id: &SubjectGroupId,
) -> bool {
    if candidate_id == ancestor_id {
        return true;
    }
    let parent_of: HashMap<&SubjectGroupId, Option<&SubjectGroupId>> = groups
        .iter()
        .map(|g| (&g.id, g.parent_id.as_ref()))
        .collect();
    let mut visited = HashSet::new();
    let mut cursor = Some(candidate_id);
    while let Some(current) = cursor {
        if current == ancestor_id {
            return true;
        }
        if !visited.insert(current) {
            break;
        }
        cursor = parent_of.get(current).copied().flatten();
    }
    false
}

pub fn resolve_group_subjects_in_memory(
    groups: &[SubjectGroup],
    group_id: &SubjectGroupId,
) -> Vec<String> {
    let tree = build_subject_group_tree(groups);
    flatten_subject_group_tree(&tree)
        .into_iter()
        .find(|node| &node.group.id == group_id)
        .map(|node| node.resolved_subjects)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Linear Note check-off & smooth collapse animation state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct NoteCheckState {
    /// Checked knowledge cards per note.
    pub checked_by_note: HashMap<NoteId, HashSet<KnowledgeCardId>>,
    /// Per-card visibility factor in `0.0..=1.0` while collapsing/expanding.
    pub visibility: HashMap<(NoteId, KnowledgeCardId), f32>,
    /// Whether already-checked cards remain visible in dimmed state.
    pub show_checked: bool,
}

impl NoteCheckState {
    pub fn is_checked(&self, note_id: &NoteId, card_id: &KnowledgeCardId) -> bool {
        self.checked_by_note
            .get(note_id)
            .is_some_and(|set| set.contains(card_id))
    }

    pub fn checked_count(&self, note_id: &NoteId) -> usize {
        self.checked_by_note.get(note_id).map_or(0, HashSet::len)
    }

    pub fn toggle(&mut self, note_id: &NoteId, card_id: &KnowledgeCardId) -> bool {
        let entry = self.checked_by_note.entry(note_id.clone()).or_default();
        let now_checked = if entry.contains(card_id) {
            entry.remove(card_id);
            false
        } else {
            entry.insert(card_id.clone());
            true
        };
        let key = (note_id.clone(), card_id.clone());
        let current = self
            .visibility
            .get(&key)
            .copied()
            .unwrap_or(if now_checked { 1.0 } else { 0.0 });
        self.visibility.insert(key, current);
        now_checked
    }

    pub fn reset_note(&mut self, note_id: &NoteId) {
        if let Some(removed) = self.checked_by_note.remove(note_id) {
            for card_id in removed {
                self.visibility.insert((note_id.clone(), card_id), 0.15);
            }
        }
    }

    /// Visibility factor (`0.0` = fully collapsed/hidden, `1.0` = fully expanded).
    pub fn factor(&self, note_id: &NoteId, card_id: &KnowledgeCardId) -> f32 {
        if self.show_checked {
            return 1.0;
        }
        let target = if self.is_checked(note_id, card_id) {
            0.0
        } else {
            1.0
        };
        self.visibility
            .get(&(note_id.clone(), card_id.clone()))
            .copied()
            .unwrap_or(target)
    }

    /// Advances all running collapse/expand animations by `dt_secs`.
    /// Returns `true` while any card is still animating.
    pub fn step_animations(&mut self, dt_secs: f32) -> bool {
        if self.visibility.is_empty() {
            return false;
        }
        let speed = (dt_secs / 0.24).clamp(0.02, 1.0);
        let mut finished = Vec::new();
        let show_checked = self.show_checked;
        let checked_by_note = &self.checked_by_note;
        for (key, value) in &mut self.visibility {
            let checked = checked_by_note
                .get(&key.0)
                .is_some_and(|set| set.contains(&key.1));
            let target = if show_checked || !checked { 1.0 } else { 0.0 };
            let diff = target - *value;
            if diff.abs() <= 0.02 {
                *value = target;
                finished.push(key.clone());
            } else {
                // Smooth cubic-like exponential interpolation toward target.
                *value += diff * (speed * 2.6).min(1.0);
            }
        }
        for key in finished {
            self.visibility.remove(&key);
        }
        !self.visibility.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Quiz targets, Library & Grouped Question Units
// ---------------------------------------------------------------------------

/// What a mode needs besides the database: the exam set for a full paper,
/// knowledge cards for a targeted drill, and an optional subject scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuizTargets {
    pub exam_set: Option<ExamSetId>,
    pub cards: Vec<KnowledgeCardId>,
    pub subjects: Option<Vec<String>>,
}

impl QuizTargets {
    /// Everything the open note transcludes, which is what a targeted drill
    /// follows through `tests` edges.
    pub fn from_outline(outline: Option<&NoteOutline>) -> Self {
        Self {
            exam_set: None,
            cards: outline
                .map(|outline| outline.cards.iter().map(|card| card.id.clone()).collect())
                .unwrap_or_default(),
            subjects: None,
        }
    }
}

/// The library list, nested subject groups, and currently cached note outlines.
#[derive(Debug, Clone, Default)]
pub struct Library {
    pub subjects: Vec<SubjectSummary>,
    pub subject_groups: Vec<SubjectGroup>,
    pub selected_group_id: Option<SubjectGroupId>,
    pub notes: Vec<Note>,
    pub cards: Vec<KnowledgeCard>,
    pub exam_sets: Vec<ExamSetSummary>,
    outline: Option<NoteOutline>,
    outlines_by_id: HashMap<NoteId, NoteOutline>,
    pub card_question_counts: HashMap<KnowledgeCardId, usize>,
}

impl Library {
    pub fn load(store: &Store) -> Result<Self> {
        let subjects = store.subjects()?;
        let subject_groups = store.subject_groups()?;
        let notes = store.notes(None)?;
        let cards = store.knowledge_cards(None)?;
        let exam_sets = store.exam_sets(None)?;
        let mut card_question_counts = HashMap::new();
        for card in &cards {
            let count = store
                .topic_question_ids(std::slice::from_ref(&card.id), 200)
                .map(|ids| ids.len())
                .unwrap_or(0);
            card_question_counts.insert(card.id.clone(), count);
        }
        Ok(Self {
            subjects,
            subject_groups,
            selected_group_id: None,
            notes,
            cards,
            exam_sets,
            outline: None,
            outlines_by_id: HashMap::new(),
            card_question_counts,
        })
    }

    pub fn reload_subject_groups(&mut self, store: &Store) -> Result<()> {
        self.subject_groups = store.subject_groups()?;
        if let Some(selected) = self.selected_group_id.as_ref()
            && !self.subject_groups.iter().any(|g| &g.id == selected)
        {
            self.selected_group_id = None;
        }
        Ok(())
    }

    pub fn selected_group(&self) -> Option<&SubjectGroup> {
        let id = self.selected_group_id.as_ref()?;
        self.subject_groups.iter().find(|g| &g.id == id)
    }

    pub fn resolved_selected_subjects(&self) -> Option<Vec<String>> {
        let id = self.selected_group_id.as_ref()?;
        Some(resolve_group_subjects_in_memory(&self.subject_groups, id))
    }

    pub fn filtered_notes(&self) -> Vec<&Note> {
        let Some(allowed) = self.resolved_selected_subjects() else {
            return self.notes.iter().collect();
        };
        let set: HashSet<&str> = allowed.iter().map(String::as_str).collect();
        self.notes
            .iter()
            .filter(|note| set.contains(note.subject.as_str()))
            .collect()
    }

    pub fn filtered_exam_sets(&self) -> Vec<&ExamSetSummary> {
        let Some(allowed) = self.resolved_selected_subjects() else {
            return self.exam_sets.iter().collect();
        };
        let set: HashSet<&str> = allowed.iter().map(String::as_str).collect();
        self.exam_sets
            .iter()
            .filter(|summary| set.contains(summary.exam_set.subject.as_str()))
            .collect()
    }

    /// Opens the note at `index` and resolves its transcluded cards.
    pub fn select(&mut self, store: &Store, index: usize) -> Result<()> {
        let Some(note) = self.notes.get(index) else {
            return Ok(());
        };
        let id: NoteId = note.id.clone();
        self.load_outline(store, &id)?;
        Ok(())
    }

    pub fn select_id(&mut self, store: &Store, id: &NoteId) -> Result<()> {
        let index = self.notes.iter().position(|note| &note.id == id);
        match index {
            Some(index) => self.select(store, index),
            None => Ok(()),
        }
    }

    pub fn load_outline(&mut self, store: &Store, id: &NoteId) -> Result<Option<&NoteOutline>> {
        if let Some(loaded) = store.note_outline(id)? {
            self.outline = Some(loaded.clone());
            self.outlines_by_id.insert(id.clone(), loaded);
        }
        Ok(self.outlines_by_id.get(id))
    }

    pub fn outline_by_id(&self, id: &NoteId) -> Option<&NoteOutline> {
        self.outlines_by_id.get(id)
    }

    pub fn outline(&self) -> Option<&NoteOutline> {
        self.outline.as_ref()
    }

    pub fn note(&self) -> Option<&Note> {
        self.outline.as_ref().map(|outline| &outline.note)
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.exam_sets.is_empty()
    }

    pub fn targets(&self) -> QuizTargets {
        let subjects = self.resolved_selected_subjects();
        let exam_set = if subjects.is_some() {
            None
        } else {
            self.exam_sets.first().map(|set| set.exam_set.id.clone())
        };
        QuizTargets {
            exam_set,
            cards: self
                .outline()
                .map(|outline| outline.cards.iter().map(|card| card.id.clone()).collect())
                .unwrap_or_default(),
            subjects,
        }
    }
}

/// Countdown to a practical exam date, shown on every screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PracticalCountdown {
    target: Timestamp,
}

impl PracticalCountdown {
    /// Accepts a `YYYY-MM-DD` date and treats it as midnight UTC.
    pub fn parse(date: &str) -> Result<Self> {
        let date = date.trim();
        let format = time::macros::format_description!("[year]-[month]-[day]");
        let date = time::Date::parse(date, &format).map_err(|_| {
            Error::Invalid("practical exam date", format!("{date} is not YYYY-MM-DD"))
        })?;
        let datetime = date
            .with_hms(0, 0, 0)
            .map_err(|_| Error::Invalid("practical exam date", date.to_string()))?
            .assume_utc();
        Ok(Self {
            target: Timestamp::from_offset(datetime),
        })
    }

    pub fn target(&self) -> Timestamp {
        self.target
    }

    /// Whole days left, rounding up so the last day still reads "1 天".
    pub fn days_left(&self, now: Timestamp) -> i64 {
        let remaining = self.target.days_since(&now);
        if remaining <= 0.0 {
            0
        } else {
            remaining.ceil() as i64
        }
    }

    pub fn label(&self, now: Timestamp) -> String {
        let days = self.days_left(now);
        if days == 0 {
            "實務考：今天".to_owned()
        } else {
            format!("實務考倒數 {days} 天")
        }
    }
}

// ---------------------------------------------------------------------------
// Grouped Question Units & Sidebar Buckets
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct QuestionUnit {
    pub unit_index: usize,
    pub group_id: Option<QuestionGroupId>,
    pub group: Option<QuestionGroup>,
    pub subject: String,
    pub exam_set_id: Option<ExamSetId>,
    pub exam_year: Option<i32>,
    pub exam_title: Option<String>,
    /// Indices into `QuizSession::questions()`.
    pub session_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct SidebarExamBucket {
    pub label: String,
    pub unit_indices: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct SidebarSubjectBucket {
    pub subject: String,
    pub exams: Vec<SidebarExamBucket>,
}

pub fn build_question_units(
    questions: &[QuestionCard],
    groups: &HashMap<QuestionGroupId, QuestionGroup>,
    exam_sets: &HashMap<ExamSetId, ExamSet>,
) -> Vec<QuestionUnit> {
    let mut units: Vec<QuestionUnit> = Vec::new();
    let mut group_to_unit: HashMap<QuestionGroupId, usize> = HashMap::new();

    for (session_idx, question) in questions.iter().enumerate() {
        if let Some(group_id) = &question.question_group_id
            && let Some(&existing_idx) = group_to_unit.get(group_id)
        {
            units[existing_idx].session_indices.push(session_idx);
            continue;
        }
        let unit_index = units.len();
        let exam_meta = question
            .exam_set_id
            .as_ref()
            .and_then(|id| exam_sets.get(id));
        let group = question
            .question_group_id
            .as_ref()
            .and_then(|id| groups.get(id))
            .cloned();
        if let Some(group_id) = &question.question_group_id {
            group_to_unit.insert(group_id.clone(), unit_index);
        }
        units.push(QuestionUnit {
            unit_index,
            group_id: question.question_group_id.clone(),
            group,
            subject: question.subject.clone(),
            exam_set_id: question.exam_set_id.clone(),
            exam_year: exam_meta.and_then(|meta| meta.exam_year),
            exam_title: exam_meta.map(|meta| meta.title.clone()),
            session_indices: vec![session_idx],
        });
    }
    units
}

pub fn build_sidebar_buckets(units: &[QuestionUnit]) -> Vec<SidebarSubjectBucket> {
    let mut subject_order: Vec<String> = Vec::new();
    let mut exam_map: HashMap<String, Vec<(String, String, Vec<usize>)>> = HashMap::new();

    for unit in units {
        if !exam_map.contains_key(&unit.subject) {
            subject_order.push(unit.subject.clone());
            exam_map.insert(unit.subject.clone(), Vec::new());
        }
        let exam_key = unit
            .exam_set_id
            .as_ref()
            .map(ExamSetId::as_str)
            .or_else(|| unit.exam_year.map(|year| format!("year:{year}")))
            .unwrap_or_else(|| "__general__".to_owned());
        let exam_label = unit
            .exam_year
            .map(|year| format!("{year} 年"))
            .or_else(|| unit.exam_title.clone())
            .unwrap_or_else(|| "綜合題目".to_owned());
        let buckets = exam_map.get_mut(&unit.subject).expect("subject inserted");
        if let Some(existing) = buckets.iter_mut().find(|(key, _, _)| key == &exam_key) {
            existing.2.push(unit.unit_index);
        } else {
            buckets.push((exam_key, exam_label, vec![unit.unit_index]));
        }
    }

    subject_order
        .into_iter()
        .map(|subject| {
            let exams = exam_map
                .remove(&subject)
                .unwrap_or_default()
                .into_iter()
                .map(|(_, label, unit_indices)| SidebarExamBucket {
                    label,
                    unit_indices,
                })
                .collect();
            SidebarSubjectBucket { subject, exams }
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
pub struct QuestionDraft {
    pub selection: Vec<String>,
    pub typed: String,
}

/// A running quiz session with grouped question units, per-index drafts,
/// free navigation, and five-level confidence selection.
pub struct Quiz {
    mode: QuizMode,
    session: QuizSession,
    units: Vec<QuestionUnit>,
    active_unit_idx: usize,
    drafts: HashMap<usize, QuestionDraft>,
    confidence5: ConfidenceLevel5,
    feedback: Option<GradingOutcome>,
    submitted: usize,
    question_started_at: Timestamp,
    question_limit_secs: Option<u16>,
}

impl Quiz {
    pub fn start(
        store: &Store,
        mode: QuizMode,
        targets: &QuizTargets,
        options: &BuildOptions,
        user_id: &UserId,
        now: Timestamp,
    ) -> Result<Self> {
        let mut scoped_options = options.clone();
        if scoped_options.subjects.is_none() && targets.subjects.is_some() {
            scoped_options.subjects = targets.subjects.clone();
        }
        let session = match mode {
            QuizMode::DailyDue => {
                build_daily_due(store.connection(), user_id, now, &scoped_options)?
            }
            QuizMode::FullExam => {
                if let Some(exam_set) = targets.exam_set.as_ref() {
                    build_full_exam(store.connection(), exam_set, &scoped_options)?
                } else if let Some(subjects) = scoped_options.subjects.as_ref() {
                    build_subject_exam(store.connection(), subjects, &scoped_options)?
                } else {
                    return Err(Error::Invalid(
                        "exam set",
                        "no exam set to simulate".to_owned(),
                    ));
                }
            }
            QuizMode::TopicDrill => {
                if targets.cards.is_empty() {
                    return Err(Error::Invalid(
                        "topic drill",
                        "open a note with cards first".to_owned(),
                    ));
                }
                build_topic_drill(store.connection(), &targets.cards, &scoped_options)?
            }
            QuizMode::PracticalExam => {
                build_practical_exam(store.connection(), &[], &scoped_options)?
            }
            QuizMode::WeaknessDrill => {
                build_weakness_drill(store.connection(), user_id, &scoped_options)?
            }
        };

        let mut groups: HashMap<QuestionGroupId, QuestionGroup> = HashMap::new();
        let mut exam_sets: HashMap<ExamSetId, ExamSet> = HashMap::new();
        for question in session.questions() {
            if let Some(gid) = &question.question_group_id
                && !groups.contains_key(gid)
                && let Some(group) = store.question_group(gid)?
            {
                groups.insert(gid.clone(), group);
            }
            if let Some(eid) = &question.exam_set_id
                && !exam_sets.contains_key(eid)
                && let Some(exam_set) = store.exam_set(eid)?
            {
                exam_sets.insert(eid.clone(), exam_set);
            }
        }
        let units = build_question_units(session.questions(), &groups, &exam_sets);

        let mut quiz = Self {
            mode,
            session,
            units,
            active_unit_idx: 0,
            drafts: HashMap::new(),
            confidence5: ConfidenceLevel5::Certain,
            feedback: None,
            submitted: 0,
            question_started_at: now,
            question_limit_secs: (mode == QuizMode::PracticalExam)
                .then_some(scoped_options.seconds_per_question),
        };
        quiz.session.start(now);
        Ok(quiz)
    }

    pub fn mode(&self) -> QuizMode {
        self.mode
    }

    pub fn session(&self) -> &QuizSession {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut QuizSession {
        &mut self.session
    }

    pub fn questions(&self) -> &[QuestionCard] {
        self.session.questions()
    }

    pub fn units(&self) -> &[QuestionUnit] {
        &self.units
    }

    pub fn sidebar_buckets(&self) -> Vec<SidebarSubjectBucket> {
        build_sidebar_buckets(&self.units)
    }

    pub fn active_unit_index(&self) -> usize {
        self.active_unit_idx.min(self.units.len().saturating_sub(1))
    }

    pub fn current_unit(&self) -> Option<&QuestionUnit> {
        self.units.get(self.active_unit_index())
    }

    pub fn has_prev_unit(&self) -> bool {
        self.active_unit_index() > 0
    }

    pub fn has_next_unit(&self) -> bool {
        self.active_unit_index() + 1 < self.units.len()
    }

    pub fn select_unit(&mut self, unit_idx: usize, now: Timestamp) -> Result<()> {
        if unit_idx >= self.units.len() {
            return Ok(());
        }
        self.active_unit_idx = unit_idx;
        if let Some(&first_session_idx) = self.units[unit_idx].session_indices.first()
            && matches!(
                self.session.state(),
                SessionState::InProgress { .. } | SessionState::Paused { .. }
            )
        {
            self.session.select_index(first_session_idx, now)?;
            self.question_started_at = now;
        }
        Ok(())
    }

    pub fn prev_unit(&mut self, now: Timestamp) -> Result<()> {
        if self.has_prev_unit() {
            self.select_unit(self.active_unit_index() - 1, now)?;
        }
        Ok(())
    }

    pub fn next_unit(&mut self, now: Timestamp) -> Result<()> {
        if self.has_next_unit() {
            self.select_unit(self.active_unit_index() + 1, now)?;
        }
        Ok(())
    }

    pub fn confidence5(&self) -> ConfidenceLevel5 {
        self.confidence5
    }

    pub fn set_confidence5(&mut self, level: ConfidenceLevel5) {
        self.confidence5 = level;
    }

    pub fn answer_for(&self, session_idx: usize) -> Option<&AnswerRecord> {
        self.session.answer_for(session_idx)
    }

    pub fn graded_for(&self, session_idx: usize) -> Option<&GradedAnswer> {
        self.session.graded_for(session_idx)
    }

    pub fn answered_count(&self) -> usize {
        self.session.answers().len()
    }

    pub fn current_unit_fully_graded(&self) -> bool {
        let Some(unit) = self.current_unit() else {
            return false;
        };
        !unit.session_indices.is_empty()
            && unit
                .session_indices
                .iter()
                .all(|&idx| self.session.graded_for(idx).is_some())
    }

    pub fn draft_selection(&self, session_idx: usize) -> &[String] {
        self.drafts
            .get(&session_idx)
            .map(|draft| draft.selection.as_slice())
            .unwrap_or(&[])
    }

    pub fn draft_typed(&self, session_idx: usize) -> &str {
        self.drafts
            .get(&session_idx)
            .map(|draft| draft.typed.as_str())
            .unwrap_or("")
    }

    pub fn toggle_option_at(&mut self, session_idx: usize, key: &str) {
        let Some(question) = self.questions().get(session_idx) else {
            return;
        };
        if !self.mode.deferred() && self.session.graded_for(session_idx).is_some() {
            return;
        }
        let qtype = question.question_type;
        let draft = self.drafts.entry(session_idx).or_default();
        match qtype {
            QuestionType::MultipleChoice => {
                if let Some(pos) = draft.selection.iter().position(|value| value == key) {
                    draft.selection.remove(pos);
                } else {
                    draft.selection.push(key.to_owned());
                    draft.selection.sort();
                }
            }
            QuestionType::SingleChoice
            | QuestionType::Vocab
            | QuestionType::ImageQa
            | QuestionType::PinIdentify => {
                draft.selection = vec![key.to_owned()];
            }
            _ => {}
        }
    }

    pub fn push_char_at(&mut self, session_idx: usize, character: char) {
        let Some(question) = self.questions().get(session_idx) else {
            return;
        };
        if !self.mode.deferred() && self.session.graded_for(session_idx).is_some() {
            return;
        }
        if accepts_text(question.question_type) && !character.is_control() {
            self.drafts
                .entry(session_idx)
                .or_default()
                .typed
                .push(character);
        }
    }

    pub fn backspace_at(&mut self, session_idx: usize) {
        if !self.mode.deferred() && self.session.graded_for(session_idx).is_some() {
            return;
        }
        if let Some(draft) = self.drafts.get_mut(&session_idx) {
            draft.typed.pop();
        }
    }

    pub fn can_submit_current_unit(&self) -> bool {
        if !matches!(self.session.state(), SessionState::InProgress { .. }) {
            return false;
        }
        let Some(unit) = self.current_unit() else {
            return false;
        };
        unit.session_indices
            .iter()
            .any(|&idx| self.build_submission_at(idx).is_some())
    }

    fn build_submission_at(&self, session_idx: usize) -> Option<AnswerSubmission> {
        let question = self.questions().get(session_idx)?;
        if !self.mode.deferred() && self.session.graded_for(session_idx).is_some() {
            return None;
        }
        let draft = self.drafts.get(&session_idx);
        if accepts_text(question.question_type) {
            let text = draft.map(|d| d.typed.trim()).unwrap_or("");
            if text.is_empty() {
                (question.grading_mode == GradingMode::Voided).then_some(AnswerSubmission::TimedOut)
            } else {
                Some(AnswerSubmission::Text {
                    value: text.to_owned(),
                })
            }
        } else {
            let selection = draft.map(|d| d.selection.as_slice()).unwrap_or(&[]);
            if selection.is_empty() {
                (question.grading_mode == GradingMode::Voided).then_some(AnswerSubmission::TimedOut)
            } else {
                Some(AnswerSubmission::Keys {
                    values: selection.to_vec(),
                })
            }
        }
    }

    /// Submits every submittable question in the currently displayed unit
    /// (single question or entire `QuestionGroup`) via `submit_answer_at`.
    pub fn submit_current_unit(&mut self, now: Timestamp) -> Result<usize> {
        let Some(indices) = self.current_unit().map(|unit| unit.session_indices.clone()) else {
            return Ok(0);
        };
        let (confidence, implied_tag) = self.confidence5.to_core();
        let error_tags: Vec<String> = implied_tag.into_iter().map(str::to_owned).collect();
        let mut count = 0;
        for session_idx in indices {
            let Some(submission) = self.build_submission_at(session_idx) else {
                continue;
            };
            let answer = QuizAnswer {
                submission,
                confidence,
                error_tags: error_tags.clone(),
            };
            let outcome = self.session.submit_answer_at(session_idx, answer, now)?;
            self.submitted = self.session.answers().len();
            self.feedback = outcome;
            count += 1;
        }
        Ok(count)
    }

    pub fn is_empty(&self) -> bool {
        self.questions().is_empty()
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.session.state(),
            SessionState::Submitted | SessionState::Expired { .. }
        )
    }

    pub fn index(&self) -> usize {
        match self.session.state() {
            SessionState::NotStarted => 0,
            SessionState::InProgress { index }
            | SessionState::Paused { index }
            | SessionState::Expired { index } => {
                index.min(self.questions().len().saturating_sub(1))
            }
            SessionState::Submitted => self.questions().len(),
        }
    }

    pub fn total(&self) -> usize {
        self.questions().len()
    }

    pub fn current(&self) -> Option<&QuestionCard> {
        if self.is_finished() {
            return None;
        }
        self.questions().get(self.index())
    }

    pub fn current_question_id(&self) -> Option<QuestionCardId> {
        self.current().map(|question| question.id.clone())
    }

    pub fn selection(&self) -> &[String] {
        self.draft_selection(self.index())
    }

    pub fn typed(&self) -> &str {
        self.draft_typed(self.index())
    }

    pub fn feedback(&self) -> Option<&GradingOutcome> {
        self.feedback.as_ref()
    }

    pub fn submitted(&self) -> usize {
        self.submitted
    }

    /// Toggles an option of the current question (backwards-compatible linear helper).
    pub fn toggle_option(&mut self, key: &str) {
        let idx = self.index();
        self.toggle_option_at(idx, key);
    }

    pub fn select_many(&mut self, keys: &[String]) {
        for key in keys {
            self.toggle_option(key);
        }
    }

    pub fn push_char(&mut self, character: char) {
        let idx = self.index();
        self.push_char_at(idx, character);
    }

    pub fn backspace(&mut self) {
        let idx = self.index();
        self.backspace_at(idx);
    }

    /// Replaces the typed answer for the current question, used by tests and scripted runs.
    pub fn type_text(&mut self, text: &str) {
        let idx = self.index();
        if self.accepts_text() {
            self.drafts.entry(idx).or_default().typed = text.to_owned();
        }
    }

    pub fn accepts_text(&self) -> bool {
        self.current()
            .is_some_and(|question| accepts_text(question.question_type))
    }

    /// Sends the composed answer linearly to the core session (used by existing tests).
    pub fn submit(&mut self, now: Timestamp) -> Result<Option<GradingOutcome>> {
        let idx = self.index();
        let Some(submission) = self.build_submission_at(idx) else {
            return Ok(None);
        };
        let (confidence, implied_tag) = self.confidence5.to_core();
        let answer = QuizAnswer {
            submission,
            confidence,
            error_tags: implied_tag.into_iter().map(str::to_owned).collect(),
        };
        let outcome = self.session.submit_answer(answer, now)?;
        self.submitted = self.session.answers().len();
        self.drafts.remove(&idx);
        self.feedback = outcome.clone();
        self.question_started_at = now;
        self.sync_active_unit_from_session();
        Ok(outcome)
    }

    /// Records a click on a pin-locate question.
    pub fn submit_pin(&mut self, point: NormalizedPoint, now: Timestamp) -> Result<()> {
        let Some(question) = self.current() else {
            return Ok(());
        };
        if question.question_type != QuestionType::PinLocate {
            return Ok(());
        }
        let (confidence, implied_tag) = self.confidence5.to_core();
        let answer = QuizAnswer {
            submission: AnswerSubmission::PinClicks {
                clicks: vec![PinClick {
                    no: u32::try_from(self.submitted + 1).unwrap_or(1),
                    point,
                }],
            },
            confidence,
            error_tags: implied_tag.into_iter().map(str::to_owned).collect(),
        };
        let outcome = self.session.submit_answer(answer, now)?;
        self.submitted = self.session.answers().len();
        self.feedback = outcome;
        self.question_started_at = now;
        self.sync_active_unit_from_session();
        Ok(())
    }

    fn sync_active_unit_from_session(&mut self) {
        let idx = self.index();
        if let Some(pos) = self
            .units
            .iter()
            .position(|unit| unit.session_indices.contains(&idx))
        {
            self.active_unit_idx = pos;
        }
    }

    /// Drives the practical countdown; a timeout is graded by the core session.
    /// Does not tick while the active unit is already fully graded.
    pub fn tick(&mut self, now: Timestamp) -> Result<TickOutcome> {
        if self.mode == QuizMode::PracticalExam && self.current_unit_fully_graded() {
            return Ok(TickOutcome::None);
        }
        let outcome = self.session.tick(now)?;
        if let TickOutcome::QuestionTimedOut { index, graded } = &outcome {
            self.submitted = self.session.answers().len();
            self.feedback = Some(graded.outcome.clone());
            self.drafts.remove(index);
            self.question_started_at = now;
            self.sync_active_unit_from_session();
        }
        Ok(outcome)
    }

    pub fn score(&self) -> Result<Score> {
        Ok(self.session.score()?)
    }

    /// Turns the finished session into attempt rows and derived scheduling.
    pub fn commit(&mut self, store: &mut Store, user_id: &UserId, now: Timestamp) -> Result<u32> {
        if !self.is_finished() {
            self.session.finish(now)?;
        }
        store.commit_session(&mut self.session, user_id, now)
    }

    /// Seconds left on a practical question, if the session is timed and active unit is ungraded.
    pub fn seconds_left(&self, now: Timestamp) -> Option<u64> {
        let limit = self.question_limit_secs?;
        if self.is_finished() || self.current_unit_fully_graded() {
            return None;
        }
        let elapsed_ms = (now.days_since(&self.question_started_at) * 86_400_000.0)
            .round()
            .max(0.0) as u64;
        let limit_ms = u64::from(limit) * 1_000;
        Some(limit_ms.saturating_sub(elapsed_ms) / 1_000)
    }
}

pub fn accepts_text(question_type: QuestionType) -> bool {
    matches!(
        question_type,
        QuestionType::ShortAnswer | QuestionType::Vocab
    )
}

/// Everything the window needs after a session is written.
#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    pub mode: QuizMode,
    pub correct: usize,
    pub scoreable: usize,
    pub voided: usize,
    pub ratio: f64,
    pub written: u32,
}

impl Summary {
    pub fn new(mode: QuizMode, score: &Score, written: u32) -> Self {
        Self {
            mode,
            correct: score.correct,
            scoreable: score.scoreable,
            voided: score.voided,
            ratio: score.ratio,
            written,
        }
    }
}

impl std::fmt::Debug for Quiz {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Quiz")
            .field("mode", &self.mode)
            .field("index", &self.index())
            .field("total", &self.total())
            .field("submitted", &self.submitted)
            .finish()
    }
}

/// Default options for desktop sessions: untimed papers, practical questions
/// keep the spec's 30 second default countdown.
pub fn desktop_options() -> BuildOptions {
    BuildOptions::default()
}
