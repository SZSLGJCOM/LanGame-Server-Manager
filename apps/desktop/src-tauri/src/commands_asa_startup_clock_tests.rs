use super::*;

fn normal() -> Duration {
    Duration::from_secs(300)
}

fn clock() -> PhaseClock {
    PhaseClock::new(normal(), Duration::ZERO, Duration::ZERO)
}

fn cold() -> Observation {
    Observation {
        cold_download: true,
        initialized: false,
    }
}

fn loaded() -> Observation {
    Observation {
        cold_download: false,
        initialized: true,
    }
}

#[test]
fn modless_clock_retains_warm_and_cold_offset_contracts() {
    let mut warm = clock();
    assert!(warm.mods_ready());
    assert!(!warm.mods_validated);
    warm.observe(&loaded(), Duration::from_secs(100), normal())
        .unwrap();
    warm.observe_mods(true, Duration::from_secs(150), normal())
        .unwrap();
    assert_eq!(warm.deadline(), normal());
    assert!(!warm.mods_validated);
    let mut cold_clock = clock();
    cold_clock
        .observe(&cold(), Duration::from_secs(100), normal())
        .unwrap();
    assert_eq!(cold_clock.deadline(), Duration::from_secs(900));
    cold_clock
        .observe(&loaded(), Duration::from_secs(800), normal())
        .unwrap();
    assert_eq!(cold_clock.deadline(), Duration::from_secs(1100));
}

#[test]
fn offset_completion_does_not_release_the_required_mod_budget_or_listener_gate() {
    let mut clock = clock();
    clock.require_mods();
    clock
        .observe(&cold(), Duration::from_secs(100), normal())
        .unwrap();
    clock
        .observe(&loaded(), Duration::from_secs(200), normal())
        .unwrap();
    assert!(clock.initialized);
    assert!(!clock.mods_ready());
    for seconds in [300, 600, 899] {
        clock
            .observe_mods(false, Duration::from_secs(seconds), normal())
            .unwrap();
        assert_eq!(clock.deadline(), Duration::from_secs(900));
    }
    assert!(
        clock
            .timeout_message()
            .contains("CurseForge Mod validation")
    );
    assert!(clock.timeout_message().contains("native log"));
}

#[test]
fn first_mod_completion_starts_normal_binding_budget_without_repeated_renewal() {
    let mut clock = clock();
    clock.require_mods();
    clock
        .observe_mods(true, Duration::from_secs(800), normal())
        .unwrap();
    assert!(clock.mods_ready());
    assert_eq!(clock.deadline(), Duration::from_secs(1100));
    clock
        .observe_mods(true, Duration::from_secs(1000), normal())
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(1100));
    assert!(
        clock
            .observe_mods(true, Duration::from_secs(1100), normal())
            .is_err()
    );
}

#[test]
fn late_mod_completion_cannot_revive_the_absolute_mod_phase() {
    let mut clock = clock();
    clock.require_mods();
    assert!(
        clock
            .observe_mods(true, Duration::from_secs(900), normal())
            .is_err()
    );
    assert!(!clock.mods_validated);
    assert!(!clock.mods_ready());
    assert_eq!(clock.deadline(), Duration::from_secs(900));
}

#[test]
fn an_unfinished_log_tail_cannot_renew_the_listener_deadline() {
    let mut clock = clock();
    clock.require_mods();
    clock
        .observe_mods(true, Duration::from_secs(10), normal())
        .unwrap();
    clock
        .observe_mods(false, Duration::from_secs(11), normal())
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(310));
    clock
        .observe_mods(true, Duration::from_secs(100), normal())
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(310));
}

#[test]
fn mod_budget_counts_process_age_and_caps_binding_at_the_total_hard_deadline() {
    let mut clock = PhaseClock::new(normal(), Duration::from_secs(40), Duration::from_secs(30));
    clock.require_mods();
    assert_eq!(clock.deadline(), Duration::from_secs(890));
    clock
        .observe_mods(true, Duration::from_secs(880), Duration::from_secs(600))
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(1190));
    assert_eq!(clock.deadline(), clock.hard_deadline);
}

#[test]
fn mod_completion_does_not_bypass_an_unfinished_offset_cache() {
    let mut clock = clock();
    clock.require_mods();
    clock
        .observe(&cold(), Duration::from_secs(100), normal())
        .unwrap();
    clock
        .observe_mods(true, Duration::from_secs(700), normal())
        .unwrap();
    assert!(clock.mods_ready());
    assert_eq!(clock.deadline(), Duration::from_secs(900));
    clock
        .observe(&loaded(), Duration::from_secs(800), normal())
        .unwrap();
    assert_eq!(clock.deadline(), Duration::from_secs(1100));
}
