const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const test = require("node:test");
const { runBrowserFixture } = require("./helpers/runtime-browser-process.cjs");
for (const viewport of [{ width: 1560, height: 900 }, { width: 960, height: 600 }]) {
  test(`Nexus background download, explicit embedded sign-in and local import at ${viewport.width}x${viewport.height}`, { timeout: 90000 }, async () => {
    const output = process.env.LANGAME_NEXUS_MODS_OUTPUT;
    if (output) {
      assert.equal(path.isAbsolute(output), true);
      const relative = path.relative(path.resolve(__dirname, "../../.."), output);
      assert.ok(relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative), "Evidence must remain outside the repository");
      await fs.mkdir(output, { recursive: true });
    }
    const name = `nexus-activity-${viewport.width}x${viewport.height}`;
    let avatarRequests = 0;
    const report = await runBrowserFixture({ fixturePath: "nexus-mods-browser.html", viewport, keyboard: true, pointer: true,
      deviceScaleFactor: 1.5, fixtureCleanup: true,
      bundleWithMiddleware: true,
      fixtureMiddleware(request, response, next) {
        if (request.url?.split("?")[0] !== "/__nexus_mod_avatar.svg") return next();
        avatarRequests++;
        response.statusCode = 200;
        response.setHeader("Content-Type", "image/svg+xml");
        response.setHeader("Cache-Control", "no-store");
        response.end('<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32"><rect width="32" height="32" rx="6" fill="#59331d"/><circle cx="16" cy="16" r="10" fill="#ed9b43"/><path d="M11 22V10L21 22V10" stroke="#442d20" stroke-width="3" fill="none"/></svg>');
      },
      screenshotPath: output ? path.join(output, `${name}.png`) : undefined });
    if (output) await fs.writeFile(path.join(output, `${name}.json`), JSON.stringify(report, null, 2));
    assert.equal(report.status, "passed", report.error);
    assert.deepEqual(report.browser_errors, []); assert.equal(report.browser_exited, true);
    assert.equal(report.local_imports, 2); assert.equal(report.nexus_api_calls, 0);
    assert.equal(report.website_installs, 3);
    assert.deepEqual(report.automatic_installation, { webpage_mounts: 0, native_visibility_commands: 0 });
    assert.deepEqual(report.byte_progress, { real_counters: true, unknown_total: true, retry_sampling_reset: true,
      old_attempt_ignored: true, old_sequence_ignored: true, instance_session_isolation: true, owned_cancel_disabled: true,
      full_bytes_not_installed: true, stages: ["downloading", "verifying", "installing"], subscription_failure_does_not_abort: true,
      async_release_failure_does_not_abort: true,
      subscriptions: 22, unsubscriptions: 22 });
    assert.equal(report.website_references.length, 23);
    assert.equal(report.website_references.filter((reference) => reference === "9559").length, 2);
    assert.ok(report.website_references.every((reference) => reference === "9559"
      || reference === "https://www.nexusmods.com/sevendaystodie/mods/9559?tab=files"));
    assert.deepEqual(report.website_sessions, { opened: 24, closed: 24 }, "All background download sessions close before drag-hint capture");
    assert.deepEqual(report.fixture_cleanup.website_sessions, { opened: 24, closed: 24 });
    assert.deepEqual(report.coordinator, { cancellation_flushes: 8, reference_failure_blocks_flush: true });
    assert.deepEqual(report.loading, { cancel_without_flash: true,
      failure_without_flash: true, late_open_without_flash: true, browser_preview_has_no_native_commands: true });
    assert.equal(report.screenshot_state, "nexus-drag-hint-in-activity");
    assert.deepEqual(report.guidance, { hover_without_bubble: true, focus_without_bubble: true, activity_store: true,
      store_link_drag: true, my_mods_link_drag: true, local_file_drag: true, leave_clears_hint: true,
      website_button_click: true, specific_errors_in_activity: true });
    assert.deepEqual(report.login_sessions, { attempts: 10, opened: 9, closed: 9 });
    assert.deepEqual(report.fixture_cleanup.login_sessions, { opened: 9, closed: 9 });
    assert.ok(report.auth_cache_reads > 0);
    assert.deepEqual(report.authentication, { cache_read_without_page: true, unknown_is_neutral: true, signed_out_is_explicit: true,
      first_sign_in_returns_to_store: true, known_account_stays_open: true, close_is_awaited: true,
      closing_drop_has_feedback: true, no_automatic_retry: true, same_mount_retry_installs: true,
      stale_events_ignored: true, download_completion_refreshes_cache: true, store_return_refreshes_cache: true });
    assert.deepEqual(report.login, { explicit_only: true, mutually_exclusive: true, dpi: 1.5, physical_bounds: true,
      resize: true, movement: true, offscreen_hidden: true, modal_hidden: true, tab_cleanup: true,
      instance_cleanup: true, unmount_cleanup: true, pending_cancel_cleanup: true, late_open_cleanup: true,
      failure_without_page: true, retry_background_only: true, no_authenticated_claim: true });
    assert.ok(report.login_bounds?.width > 200 && report.login_bounds?.height > 100,
      "Explicit login retains its measured embedded slot; native website pixels are not mocked");
    assert.deepEqual(report.background, {
      phases: ["installing", "opening", "selecting", "verifying", "waiting"], webpage_mounts: 0, native_visibility_commands: 0,
      blocked_errors: ["nexus_website_login_required", "nexus_website_verification_required", "nexus_file_selection_required",
        "nexus_website_action_required", "nexus_website_controls_unavailable", "nexus_website_file_quarantined", "nexus_browser_page_unavailable", "nexus_browser_timed_out"],
      same_instance_tabs_continue: true, unmount_cleanup: true, late_open_cleanup: true, instance_switch_isolated: true,
      owned_installation_survives_detach: true, my_mods_link_drop: true, my_mods_id_entry: true, installing_phase_retained: true,
    });
    assert.ok(avatarRequests > 0, "The website Mod avatar is fetched from the fixture server");
    assert.deepEqual(report.presentation, { title: "Nexus server Mod with artwork", source_name: "nexus-sevendaystodie-9559",
      decoded_stages: ["installed", "tab-return", "remounted", "automatic", "authenticated", "final"] });
    assert.ok(report.success_auto_dismiss_ms >= 4800 && report.success_auto_dismiss_ms < 6500);
    assert.equal(report.success_manual_dismiss, true); assert.equal(report.inventory_retained_after_feedback, true);
    assert.equal(report.browser_processes_remaining, 0); assert.equal(report.scratch_removed, true);
    console.log(JSON.stringify({ viewport, checks: report.assertions, local_imports: report.local_imports,
      website_installs: report.website_installs, nexus_api_calls: report.nexus_api_calls, evidence_scope: report.evidence_scope }));
  });
}
