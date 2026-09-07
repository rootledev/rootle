use super::*;

/// 0017 M3 + 0018 M2: a newer release lands the `↑` chip; the toast
/// rides the event (once-a-day, worker-side) and never fires without
/// the flag.
/// 0022 M1+M3: fallback degradation is sticky (it outlives a cleared
/// status) and tints the forge chip warning.
#[test]
fn degraded_notice_is_sticky_and_tints_the_chip() {
    let mut app = browsing_app();
    // Simulate a fallback outcome (what App::new does on Warn).
    app.set_degraded_for_test("gitlab failed to start (ENOENT) — browsing github".into());

    // The notice shows immediately…
    let screen = render(&mut app, 120, 30).join("\n");
    assert!(
        screen.contains("gitlab failed to start"),
        "degraded shows:\n{screen}"
    );

    // …and it survives the transient status clearing (the stickiness
    // is the whole point — 0018's race erased fallback notices).
    app.clear_status_for_test();
    let screen = render(&mut app, 120, 30).join("\n");
    assert!(
        screen.contains("gitlab failed to start"),
        "degraded persists:\n{screen}"
    );

    // M3: the forge chip tints warning while degraded.
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| app.render(f, f.area())).unwrap();
    let buf = terminal.backend().buffer();
    let modeline_y = buf.area.height - 1;
    let sem = rootle::theme::Theme::catppuccin_mocha().semantic;
    let tinted = (0..buf.area.width).any(|x| buf[(x, modeline_y)].bg == sem.warning);
    assert!(tinted, "forge chip carries the warning tint while degraded");
}

#[test]
fn update_notice_chips_in_the_modeline() {
    let mut app = browsing_app();
    app.handle_app_event(rootle::event::AppEvent::UpdateAvailable {
        tag: "v9.9.9".into(),
        toast: true,
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("↑ v9.9.9"),
        "modeline chip missing:\n{screen}"
    );
    assert!(
        screen.contains("rootle v9.9.9 is out"),
        "status line missing:\n{screen}"
    );
}

/// 0018 M2: the spent toast quota leaves the chip, drops the status.
#[test]
fn update_notice_chip_persists_without_the_toast() {
    let mut app = browsing_app();
    app.handle_app_event(rootle::event::AppEvent::UpdateAvailable {
        tag: "v9.9.9".into(),
        toast: false,
    });
    let screen = render(&mut app, 100, 30).join("\n");
    assert!(
        screen.contains("↑ v9.9.9"),
        "modeline chip missing:\n{screen}"
    );
    assert!(
        !screen.contains("is out"),
        "toast should be spent:\n{screen}"
    );
}
