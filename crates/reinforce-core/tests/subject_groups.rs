use reinforce_core::db::subject_groups as store;
use reinforce_core::{Database, SubjectGroupId, Timestamp};

fn base() -> Timestamp {
    "2026-01-01T00:00:00.000000Z".parse().unwrap()
}

fn subjects(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn nested_groups_resolve_every_descendant_subject_once() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();

    let root = store::create_subject_group(
        connection,
        None,
        "自然科",
        &subjects(&["physics", "physics"]),
        0,
        base(),
    )
    .unwrap();
    assert_eq!(root.subjects, subjects(&["physics"]));
    let mechanics = store::create_subject_group(
        connection,
        Some(&root.id),
        "力學",
        &subjects(&["mechanics", "physics"]),
        1,
        base(),
    )
    .unwrap();
    assert_eq!(mechanics.parent_id.as_ref(), Some(&root.id));
    let optics = store::create_subject_group(
        connection,
        Some(&mechanics.id),
        "光學",
        &subjects(&["optics"]),
        0,
        base(),
    )
    .unwrap();

    assert_eq!(
        store::resolve_group_subjects(connection, &root.id).unwrap(),
        subjects(&["mechanics", "optics", "physics"])
    );
    assert_eq!(
        store::resolve_group_subjects(connection, &mechanics.id).unwrap(),
        subjects(&["mechanics", "optics", "physics"])
    );
    assert_eq!(
        store::resolve_group_subjects(connection, &optics.id).unwrap(),
        subjects(&["optics"])
    );

    let listed = store::list_subject_groups(connection).unwrap();
    assert_eq!(listed.len(), 3);
    assert_eq!(
        listed
            .iter()
            .map(|group| group.id.clone())
            .collect::<Vec<_>>(),
        vec![optics.id.clone(), root.id.clone(), mechanics.id.clone()],
        "list order is sort_order, then name, then id"
    );
}

#[test]
fn groups_reject_empty_names_missing_parents_and_cycles() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let root = store::create_subject_group(connection, None, "root", &[], 0, base()).unwrap();
    let child = store::create_subject_group(
        connection,
        Some(&root.id),
        "child",
        &subjects(&["physics"]),
        0,
        base(),
    )
    .unwrap();
    let grandchild =
        store::create_subject_group(connection, Some(&child.id), "grandchild", &[], 0, base())
            .unwrap();

    assert!(store::create_subject_group(connection, None, "   ", &[], 0, base()).is_err());
    let missing = SubjectGroupId::generate();
    assert!(
        store::create_subject_group(connection, Some(&missing), "orphan", &[], 0, base()).is_err()
    );

    assert!(
        store::update_subject_group(connection, &root.id, Some(&root.id), "root", &[], 0, base())
            .is_err(),
        "a group cannot be its own parent"
    );
    assert!(
        store::update_subject_group(
            connection,
            &root.id,
            Some(&grandchild.id),
            "root",
            &[],
            0,
            base()
        )
        .is_err(),
        "a group cannot move below its descendant"
    );
    assert!(
        store::update_subject_group(connection, &root.id, Some(&missing), "root", &[], 0, base())
            .is_err(),
        "an update cannot point at a missing group"
    );
    assert!(
        store::update_subject_group(connection, &missing, None, "ghost", &[], 0, base()).is_err(),
        "an update must target an existing group"
    );

    let moved = store::update_subject_group(
        connection,
        &grandchild.id,
        Some(&root.id),
        "光學",
        &subjects(&["optics"]),
        5,
        base(),
    )
    .unwrap();
    assert_eq!(moved.parent_id.as_ref(), Some(&root.id));
    assert_eq!(moved.name, "光學");
    assert_eq!(moved.sort_order, 5);
    assert_eq!(
        store::resolve_group_subjects(connection, &root.id).unwrap(),
        subjects(&["optics", "physics"])
    );
}

#[test]
fn deleting_a_group_cascades_to_descendants_and_subject_links() {
    let database = Database::open_in_memory().unwrap();
    let connection = database.connection();
    let root = store::create_subject_group(connection, None, "root", &[], 0, base()).unwrap();
    let child = store::create_subject_group(
        connection,
        Some(&root.id),
        "child",
        &subjects(&["physics"]),
        0,
        base(),
    )
    .unwrap();

    assert!(store::delete_subject_group(connection, &root.id).unwrap());
    assert!(
        store::get_subject_group(connection, &root.id)
            .unwrap()
            .is_none()
    );
    assert!(
        store::get_subject_group(connection, &child.id)
            .unwrap()
            .is_none(),
        "descendants cascade with their parent"
    );
    let remaining: i64 = connection
        .query_row("SELECT COUNT(*) FROM subject_group_subjects", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(remaining, 0, "subject links cascade with their group");
    assert!(!store::delete_subject_group(connection, &root.id).unwrap());
    assert!(store::resolve_group_subjects(connection, &root.id).is_err());
}
