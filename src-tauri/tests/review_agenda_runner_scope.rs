//! Architecture review: agenda runner must re-resolve scope every tick.
//! This locks in the fix that prevents scope-switch bug (the runner shouldn't
//! cache the AgendaStore across ticks).

#[test]
fn runner_module_re_resolves_scope_in_loop() {
    let source = std::fs::read_to_string("src/runtime/agenda/runner.rs").unwrap();
    assert!(
        source.contains("path_resolver.resolve_paths()"),
        "spawn_agenda_runner must call resolve_paths() inside the loop"
    );
    let lines: Vec<&str> = source.lines().collect();
    let resolve_idx = lines
        .iter()
        .position(|l| l.contains("path_resolver.resolve_paths()"))
        .expect("resolve_paths call not found");
    let loop_idx = lines
        .iter()
        .position(|l| l.contains("loop {"))
        .expect("loop block not found");
    let store_idx = lines
        .iter()
        .position(|l| l.contains("AgendaStore::new(paths.base_dir())"))
        .expect("AgendaStore::new(paths.base_dir()) call not found");
    assert!(
        resolve_idx > loop_idx,
        "resolve_paths must be inside the tick loop"
    );
    assert!(
        store_idx > loop_idx,
        "AgendaStore::new(paths.base_dir()) must be inside the tick loop"
    );
    assert!(
        lines[..loop_idx]
            .iter()
            .all(|l| !l.contains("AgendaStore::new(")),
        "spawn_agenda_runner must not cache AgendaStore before the tick loop"
    );
}
