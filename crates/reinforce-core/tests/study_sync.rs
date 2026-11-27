use reinforce_core::db::{auth, study as study_store, sync as sync_store};
use reinforce_core::primitives::{AttemptDirection, AttemptId, CardId, Confidence, SessionMode};
use reinforce_core::study::{AttemptLog, UserCardMeta};
use reinforce_core::{Database, Timestamp};
use serde_json::json;

const CARD_ONE: &str = "01a21714-c98d-70f7-90ba-1bced11dd072";
const CARD_TWO: &str = "01a21714-c98d-716e-9b53-ead7c8ee810f";

fn stamp(value: &str) -> Timestamp {
    value.parse().expect("timestamp")
}

fn attempt(
    user_id: &reinforce_core::UserId,
    card: &str,
    created_at: &str,
    is_correct: bool,
    cursor: i64,
) -> AttemptLog {
    AttemptLog {
        id: AttemptId::generate(),
        user_id: user_id.clone(),
        card_id: card.parse::<CardId>().expect("card id"),
        direction: AttemptDirection::Default,
        session_mode: SessionMode::DailyDue,
        submitted_payload: json!({"keys": ["A"]}),
        is_correct,
        duration_ms: 1_500,
        confidence: Confidence::Certain,
        error_tags: Vec::new(),
        created_at: stamp(created_at),
        sync_cursor: cursor,
        affects_scheduling: true,
    }
}

fn user(database: &mut Database, name: &str) -> reinforce_core::UserId {
    auth::create_user(database.connection(), name, Timestamp::now()).expect("user")
}

#[test]
fn per_user_cursor_is_monotonic_and_pages_in_order() {
    let mut database = Database::open_in_memory().expect("database");
    let first = user(&mut database, "first");
    let second = user(&mut database, "second");
    let connection = database.connection();

    let mut first_cursors = Vec::new();
    for index in 0..3 {
        let cursor = study_store::next_user_attempt_cursor(connection, &first).expect("cursor");
        study_store::append_attempt(
            connection,
            &attempt(
                &first,
                CARD_ONE,
                "2026-01-01T00:00:00.000000Z",
                index % 2 == 0,
                cursor,
            ),
        )
        .expect("append");
        first_cursors.push(cursor);
    }
    // Another user's inserts cannot advance the first user's sequence.
    for _ in 0..5 {
        let cursor = study_store::next_user_attempt_cursor(connection, &second).expect("cursor");
        study_store::append_attempt(
            connection,
            &attempt(
                &second,
                CARD_TWO,
                "2026-01-01T00:00:00.000000Z",
                true,
                cursor,
            ),
        )
        .expect("append");
    }
    assert_eq!(first_cursors, vec![1, 2, 3]);
    assert_eq!(
        study_store::next_user_attempt_cursor(connection, &first).expect("cursor"),
        4
    );
    assert_eq!(
        study_store::max_attempt_cursor(connection, &first).expect("cursor"),
        3
    );

    let page = study_store::list_attempts_after(connection, &first, 0, 2).expect("page");
    assert_eq!(page.len(), 2);
    assert_eq!(page[0].sync_cursor, 1);
    assert_eq!(page[1].sync_cursor, 2);
    let next = study_store::list_attempts_after(connection, &first, 2, 2).expect("page");
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].sync_cursor, 3);
    assert!(
        study_store::list_attempts_after(connection, &second, 3, 10)
            .expect("page")
            .len()
            == 2,
        "the second user keeps its own sequence (4 and 5)"
    );
}

#[test]
fn pages_never_split_a_shared_cursor_group() {
    let mut database = Database::open_in_memory().expect("database");
    let learner = user(&mut database, "learner");
    let connection = database.connection();

    // One quiz session commit writes several attempts under a single cursor.
    let session_cursor =
        study_store::next_user_attempt_cursor(connection, &learner).expect("cursor");
    for _ in 0..3 {
        study_store::append_attempt(
            connection,
            &attempt(
                &learner,
                CARD_ONE,
                "2026-01-01T00:00:00.000000Z",
                true,
                session_cursor,
            ),
        )
        .expect("append");
    }
    let later = study_store::next_user_attempt_cursor(connection, &learner).expect("cursor");
    study_store::append_attempt(
        connection,
        &attempt(
            &learner,
            CARD_TWO,
            "2026-01-01T00:05:00.000000Z",
            true,
            later,
        ),
    )
    .expect("append");

    // A limit smaller than the group must return the whole group, or a client
    // that advances to `last.sync_cursor` would skip the rest forever.
    let first_page = study_store::list_attempts_after(connection, &learner, 0, 1).expect("page");
    assert_eq!(first_page.len(), 3, "the shared cursor group is complete");
    assert!(
        first_page
            .iter()
            .all(|entry| entry.sync_cursor == session_cursor)
    );
    assert!(
        study_store::has_attempts_after(connection, &learner, session_cursor).expect("has more"),
        "the later attempt is still waiting"
    );

    let second_page =
        study_store::list_attempts_after(connection, &learner, session_cursor, 1).expect("page");
    assert_eq!(second_page.len(), 1);
    assert_eq!(second_page[0].sync_cursor, later);
    assert!(!study_store::has_attempts_after(connection, &learner, later).expect("has more"));

    // Paging with a limit larger than the remaining rows is still complete and
    // never repeats a row.
    let all = study_store::list_attempts_after(connection, &learner, 0, 100).expect("page");
    assert_eq!(all.len(), 4);
    let ids: std::collections::BTreeSet<String> =
        all.iter().map(|entry| entry.id.as_str()).collect();
    assert_eq!(ids.len(), 4, "no duplicate rows across the page");
}

#[test]
fn pending_uploads_disappear_only_after_acknowledgement() {
    let mut database = Database::open_in_memory().expect("database");
    let learner = user(&mut database, "learner");
    let connection = database.connection();
    let mut ids = Vec::new();
    for index in 0..3 {
        let cursor = study_store::next_attempt_cursor(connection).expect("cursor");
        assert!(cursor > 0);
        let entry = attempt(
            &learner,
            CARD_ONE,
            "2026-01-01T00:00:00.000000Z",
            index == 0,
            cursor,
        );
        ids.push(entry.id.clone());
        study_store::append_attempt(connection, &entry).expect("append");
    }

    let pending = sync_store::pending_attempts(connection, &learner, 2).expect("pending");
    assert_eq!(pending.len(), 2, "the page is capped by the limit");
    assert_eq!(pending[0].id, ids[0]);

    sync_store::mark_attempts_uploaded(connection, &learner, &[ids[0].clone()], Timestamp::now())
        .expect("ack");
    // Acknowledging twice is harmless and an unknown id is ignored.
    sync_store::mark_attempts_uploaded(
        connection,
        &learner,
        &[ids[0].clone(), AttemptId::generate()],
        Timestamp::now(),
    )
    .expect("ack twice");
    let pending = sync_store::pending_attempts(connection, &learner, 10).expect("pending");
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].id, ids[1]);
    assert_eq!(pending[1].id, ids[2]);
}

#[test]
fn acknowledgements_only_mark_the_owning_profile() {
    let mut database = Database::open_in_memory().expect("database");
    let first = user(&mut database, "first profile");
    let second = user(&mut database, "second profile");
    let connection = database.connection();
    let mut ids = Vec::new();
    for owner in [&first, &second] {
        let cursor = study_store::next_attempt_cursor(connection).expect("cursor");
        let entry = attempt(owner, CARD_ONE, "2026-01-01T00:00:00.000000Z", true, cursor);
        ids.push(entry.id.clone());
        study_store::append_attempt(connection, &entry).expect("append");
    }

    // Acknowledging the second profile's attempt under the first profile must
    // not mark it, or one device profile would silently drop another's upload.
    sync_store::mark_attempts_uploaded(connection, &first, &[ids[1].clone()], Timestamp::now())
        .expect("ack");
    let pending_first = sync_store::pending_attempts(connection, &first, 10).expect("pending");
    assert_eq!(pending_first.len(), 1);
    assert_eq!(pending_first[0].id, ids[0]);
    let pending_second = sync_store::pending_attempts(connection, &second, 10).expect("pending");
    assert_eq!(
        pending_second.len(),
        1,
        "the other profile still owes its row"
    );
    assert_eq!(pending_second[0].id, ids[1]);
}

#[test]
fn merge_is_idempotent_and_replays_scheduling_locally() {
    let mut database = Database::open_in_memory().expect("database");
    let local = user(&mut database, "device profile");
    let remote = user(&mut database, "server account");
    let connection = database.connection();
    sync_store::bind_remote_user(connection, &local, &remote, Timestamp::now()).expect("bind");

    let first = attempt(&remote, CARD_ONE, "2026-01-01T10:00:00.000000Z", false, 7);
    let second = attempt(&remote, CARD_ONE, "2026-01-01T10:05:00.000000Z", true, 8);
    let other_card = attempt(&remote, CARD_TWO, "2026-01-01T10:06:00.000000Z", true, 9);

    let outcome = sync_store::merge_remote_batch(
        database.connection_mut(),
        &local,
        &[first.clone(), second.clone(), other_card.clone()],
        &[],
        9,
        0.9,
    )
    .expect("merge");
    assert_eq!(outcome.inserted_attempts, 3);
    assert_eq!(outcome.skipped_attempts, 0);
    assert_eq!(outcome.pull_cursor, 9);
    assert!(
        sync_store::pending_attempts(database.connection(), &local, 10)
            .expect("pending")
            .is_empty(),
        "pulled attempts came from the server and are not pushed back"
    );

    let stored = study_store::list_attempts(
        database.connection(),
        &local,
        &CARD_ONE.parse().expect("card"),
        AttemptDirection::Default,
    )
    .expect("attempts");
    assert_eq!(stored.len(), 2);
    assert!(
        stored.iter().all(|entry| entry.user_id == local),
        "pulled rows are stored under the local profile"
    );
    let scheduling = study_store::get_scheduling(
        database.connection(),
        &local,
        &CARD_ONE.parse().expect("card"),
        AttemptDirection::Default,
    )
    .expect("scheduling")
    .expect("rows");
    assert_eq!(scheduling.reps, 2);
    assert_eq!(scheduling.lapses, 1);
    assert_eq!(
        scheduling.last_review_at,
        Some(stamp("2026-01-01T10:05:00.000000Z"))
    );

    // Replaying the same batch changes nothing and does not move the cursor back.
    let outcome = sync_store::merge_remote_batch(
        database.connection_mut(),
        &local,
        &[first, second, other_card],
        &[],
        4,
        0.9,
    )
    .expect("replay");
    assert_eq!(outcome.inserted_attempts, 0);
    assert_eq!(outcome.skipped_attempts, 3);
    assert_eq!(outcome.pull_cursor, 9, "the cursor never regresses");
    let scheduling_again = study_store::get_scheduling(
        database.connection(),
        &local,
        &CARD_ONE.parse().expect("card"),
        AttemptDirection::Default,
    )
    .expect("scheduling")
    .expect("rows");
    assert_eq!(scheduling, scheduling_again);
}

#[test]
fn merge_keeps_non_scheduling_attempts_out_of_fsrs() {
    let mut database = Database::open_in_memory().expect("database");
    let local = user(&mut database, "device profile");
    let remote = user(&mut database, "server account");
    let connection = database.connection();
    sync_store::bind_remote_user(connection, &local, &remote, Timestamp::now()).expect("bind");

    let mut voided = attempt(&remote, CARD_ONE, "2026-01-01T10:00:00.000000Z", false, 1);
    voided.affects_scheduling = false;
    voided.submitted_payload = json!({"voided": true, "grading_mode": "voided"});
    sync_store::merge_remote_batch(database.connection_mut(), &local, &[voided], &[], 1, 0.9)
        .expect("merge");

    assert_eq!(
        study_store::list_attempts(
            database.connection(),
            &local,
            &CARD_ONE.parse().expect("card"),
            AttemptDirection::Default,
        )
        .expect("attempts")
        .len(),
        1,
        "the attempt stays in the append-only log"
    );
    assert!(
        study_store::get_scheduling(
            database.connection(),
            &local,
            &CARD_ONE.parse().expect("card"),
            AttemptDirection::Default,
        )
        .expect("scheduling")
        .is_none(),
        "voided attempts never create FSRS state"
    );
}

#[test]
fn voided_attempts_never_write_an_empty_schedule() {
    let mut database = Database::open_in_memory().expect("database");
    let local = user(&mut database, "device profile");
    let remote = user(&mut database, "server account");
    let connection = database.connection();
    sync_store::bind_remote_user(connection, &local, &remote, Timestamp::now()).expect("bind");

    // A client may mark a voided attempt as affecting scheduling; the server
    // and the merge must still keep it out of FSRS and not store the empty
    // replay (reps = 0, updated_at = 1970) as a schedule row.
    let mut voided = attempt(&remote, CARD_ONE, "2026-01-01T10:00:00.000000Z", false, 1);
    voided.affects_scheduling = true;
    voided.submitted_payload = json!({"voided": true, "grading_mode": "voided"});
    let outcome =
        sync_store::merge_remote_batch(database.connection_mut(), &local, &[voided], &[], 1, 0.9)
            .expect("merge");
    assert_eq!(outcome.inserted_attempts, 1);
    assert!(
        study_store::get_scheduling(
            database.connection(),
            &local,
            &CARD_ONE.parse().expect("card"),
            AttemptDirection::Default,
        )
        .expect("scheduling")
        .is_none(),
        "a voided attempt must not create a schedule row"
    );
}

#[test]
fn pulled_personal_rows_are_not_echoed_back() {
    let mut database = Database::open_in_memory().expect("database");
    let local = user(&mut database, "device profile");
    let remote = user(&mut database, "server account");
    let connection = database.connection();
    sync_store::bind_remote_user(connection, &local, &remote, Timestamp::now()).expect("bind");

    let pulled = UserCardMeta {
        user_id: remote,
        card_id: CARD_ONE.parse().expect("card"),
        starred: true,
        personal_tags: vec!["hard".to_owned()],
        personal_note: Some("from the server".to_owned()),
        updated_at: stamp("2026-01-02T00:00:00.000000Z"),
    };
    sync_store::merge_remote_batch(
        database.connection_mut(),
        &local,
        &[],
        std::slice::from_ref(&pulled),
        0,
        0.9,
    )
    .expect("merge");

    assert!(
        sync_store::pending_user_card_meta(database.connection(), &local, 10)
            .expect("pending")
            .is_empty(),
        "a row that came from the server is already stored there"
    );
}

#[test]
fn personal_rows_follow_last_writer_wins() {
    let mut database = Database::open_in_memory().expect("database");
    let local = user(&mut database, "device profile");
    let remote = user(&mut database, "server account");
    let connection = database.connection();
    sync_store::bind_remote_user(connection, &local, &remote, Timestamp::now()).expect("bind");

    let newer = UserCardMeta {
        user_id: remote.clone(),
        card_id: CARD_ONE.parse().expect("card"),
        starred: true,
        personal_tags: vec!["hard".to_owned()],
        personal_note: Some("newer".to_owned()),
        updated_at: stamp("2026-01-02T00:00:00.000000Z"),
    };
    let outcome = sync_store::merge_remote_batch(
        database.connection_mut(),
        &local,
        &[],
        std::slice::from_ref(&newer),
        0,
        0.9,
    )
    .expect("merge");
    assert_eq!(outcome.applied_meta, 1);

    let older = UserCardMeta {
        updated_at: stamp("2026-01-01T00:00:00.000000Z"),
        personal_note: Some("older".to_owned()),
        ..newer.clone()
    };
    let outcome =
        sync_store::merge_remote_batch(database.connection_mut(), &local, &[], &[older], 0, 0.9)
            .expect("merge");
    assert_eq!(outcome.applied_meta, 0);
    assert_eq!(outcome.skipped_meta, 1);
    let stored = study_store::get_user_card_meta(
        database.connection(),
        &local,
        &CARD_ONE.parse().expect("card"),
    )
    .expect("meta")
    .expect("row");
    assert_eq!(stored.personal_note.as_deref(), Some("newer"));
    assert!(stored.starred);

    // A row written after the pull is pending again.
    study_store::save_user_card_meta(
        database.connection(),
        &UserCardMeta {
            user_id: local.clone(),
            updated_at: Timestamp::now(),
            ..stored
        },
    )
    .expect("save");
    assert_eq!(
        sync_store::pending_user_card_meta(database.connection(), &local, 10)
            .expect("pending")
            .len(),
        1
    );
}

#[test]
fn rows_from_another_account_are_rejected_before_writing() {
    let mut database = Database::open_in_memory().expect("database");
    let local = user(&mut database, "device profile");
    let remote = user(&mut database, "server account");
    let stranger = user(&mut database, "someone else");
    let connection = database.connection();
    sync_store::bind_remote_user(connection, &local, &remote, Timestamp::now()).expect("bind");

    let foreign = attempt(&stranger, CARD_ONE, "2026-01-01T10:00:00.000000Z", true, 1);
    let error =
        sync_store::merge_remote_batch(database.connection_mut(), &local, &[foreign], &[], 1, 0.9)
            .expect_err("a foreign attempt must be rejected");
    assert!(error.to_string().contains("another server account"));
    assert!(
        study_store::list_attempts(
            database.connection(),
            &local,
            &CARD_ONE.parse().expect("card"),
            AttemptDirection::Default,
        )
        .expect("attempts")
        .is_empty()
    );

    // Merging without a binding is rejected instead of guessing an identity.
    let unbound = user(&mut database, "unbound");
    let error =
        sync_store::merge_remote_batch(database.connection_mut(), &unbound, &[], &[], 1, 0.9)
            .expect_err("an unbound profile cannot merge");
    assert!(error.to_string().contains("not bound"));
}

#[test]
fn binding_again_to_another_account_restarts_the_pull() {
    let mut database = Database::open_in_memory().expect("database");
    let local = user(&mut database, "device profile");
    let first = user(&mut database, "first account");
    let second = user(&mut database, "second account");
    let connection = database.connection();

    sync_store::bind_remote_user(connection, &local, &first, Timestamp::now()).expect("bind");
    sync_store::set_pull_cursor(connection, &local, 42, Timestamp::now()).expect("cursor");
    let state = sync_store::state(connection, &local)
        .expect("state")
        .expect("binding");
    assert_eq!(state.pull_cursor, 42);

    let state = sync_store::bind_remote_user(connection, &local, &first, Timestamp::now())
        .expect("rebind same");
    assert_eq!(
        state.pull_cursor, 42,
        "rebinding the same account keeps progress"
    );

    let state = sync_store::bind_remote_user(connection, &local, &second, Timestamp::now())
        .expect("rebind other");
    assert_eq!(state.pull_cursor, 0, "a different account starts from zero");

    sync_store::set_pull_cursor(connection, &local, 5, Timestamp::now()).expect("cursor");
    sync_store::set_pull_cursor(connection, &local, 2, Timestamp::now()).expect("cursor");
    assert_eq!(
        sync_store::state(connection, &local)
            .expect("state")
            .expect("binding")
            .pull_cursor,
        5,
        "the cursor never moves backwards"
    );
}
