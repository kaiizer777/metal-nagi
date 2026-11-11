use std::fs;
use std::path::PathBuf;

#[test]
fn test_seed_migration_contains_valid_models_and_safe_conflict_handling() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let up_migration_path = manifest_dir.join("../../migrations/20261111000001_seed_opencode_provider.sql");
    let content = fs::read_to_string(&up_migration_path)
        .unwrap_or_else(|_| panic!("Failed to read up migration file at {:?}", up_migration_path));

    // Verify Issue 3: Exact valid model names
    let expected_models = [
        "beam-free",
        "iceman-contributor-free",
        "muse-spark-1.3-contributor",
        "mimo-v2.6-flash",
    ];
    for model in &expected_models {
        assert!(
            content.contains(model),
            "Up migration missing expected model {}",
            model
        );
    }

    // Verify Issue 9 & 10: No collision risk on priority unique constraints
    // ON CONFLICT must NOT overwrite priority to avoid unique constraint violations
    assert!(
        !content.contains("priority = EXCLUDED.priority"),
        "Up migration must not update priority on conflict to avoid unique constraint collisions"
    );
    assert!(
        content.contains("COALESCE(MAX(priority)"),
        "Up migration must calculate dynamic/safe priority using COALESCE(MAX(priority))"
    );
}

#[test]
fn test_down_migration_restricts_scope_to_seeded_models() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let down_migration_path = manifest_dir.join("../../migrations/20261111000001_seed_opencode_provider.down.sql");
    let content = fs::read_to_string(&down_migration_path)
        .unwrap_or_else(|_| panic!("Failed to read down migration file at {:?}", down_migration_path));

    // Verify Issue 8: Narrow down migration scope to exact seeded model names
    let expected_models = [
        "beam-free",
        "iceman-contributor-free",
        "muse-spark-1.3-contributor",
        "mimo-v2.6-flash",
    ];
    for model in &expected_models {
        assert!(
            content.contains(model),
            "Down migration missing scoped model {}",
            model
        );
    }

    assert!(
        content.contains("AND model_name IN"),
        "Down migration must explicitly filter deletions with AND model_name IN (...)"
    );
}
