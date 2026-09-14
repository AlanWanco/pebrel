use super::*;
use gpui::{Entity, TestAppContext, VisualTestContext, size};
use gpui_component::Root;
use nebula_terminal::event::Event;
use std::sync::mpsc::Receiver;

// Keep the terminal entity off the layout tree so tests control each viewport
// and PTY event explicitly, while using real GPUI tasks and terminal parsing.
struct Surface;
impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn open(cx: &mut TestAppContext) -> (Entity<TerminalView>, &mut VisualTestContext, Receiver<Msg>) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.set_global(Settings::load(nebula_settings::ThemeName::Nord));
    });
    let mut result = None;
    let (_, window) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            TerminalView::new(
                42,
                (80, 24),
                TerminalLaunch::Local {
                    cwd: None,
                    shell: Some(nebula_terminal::tty::Shell::new(
                        "pebrel-test-missing-shell-executable".into(),
                        vec![],
                    )),
                    shell_name: None,
                },
                window,
                cx,
            )
        });
        let receiver = view.update(cx, |view, _| {
            let (session, receiver) = session::test_session();
            view.session = Some(session);
            view.error = None;
            view.exited = None;
            view.exec_context = None;
            view.suggest.suggest_env = crate::display::SuggestEnv::Wsl { distro: "Debian".into() };
            receiver
        });
        result = Some((view, receiver));
        Root::new(cx.new(|_| Surface), window, cx)
    });
    let (view, receiver) = result.unwrap();
    (view, window, receiver)
}

fn feed(view: &mut TerminalView, bytes: &[u8]) {
    let mut term = view.session.as_ref().unwrap().term.lock();
    let mut parser = nebula_terminal::vte::ansi::Processor::<
        nebula_terminal::vte::ansi::StdSyncHandler,
    >::default();
    parser.advance(&mut *term, bytes);
}

#[gpui::test]
fn review_regression_cold_resume_survives_initial_prompt_and_clears_on_exit(
    cx: &mut TestAppContext,
) {
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| view.update(cx, |view, cx| {
        view.run_command("codex resume saved-42".into(), cx);
        view.seed_ai_session("codex".into(), "saved-42".into(), cx);
        assert!(receiver.try_recv().is_err(), "do not submit into shell initialization");
        assert_eq!(view.session_agent().unwrap().session_id.as_deref(), Some("saved-42"));
        view.process_event(Event::CommandDone { exit_code: Some(0) }, cx);
        assert!(view.pending_shell_command.is_some());
        feed(view, b"\x1b]133;A\x07hello@host:/home/hello$ ");
        view.process_event(Event::Wakeup, cx);
        assert!(view.pending_shell_command.is_none());
        assert_eq!(view.running_program.as_deref(), Some("codex"));
        assert_eq!(view.ai_session.as_ref().unwrap().session_id, "saved-42");
        assert!(matches!(receiver.try_recv().unwrap(), Msg::Input(bytes) if bytes.as_ref() == b"codex resume saved-42"));
        // The initial shell edge cannot consume the pending Enter or identity.
        view.process_event(Event::CommandDone { exit_code: Some(0) }, cx);
        assert!(view.ai_session.is_some());
        feed(view, b"codex resume saved-42");
        view.flush_pending_runtime_submit(cx);
        assert!(matches!(receiver.try_recv().unwrap(), Msg::Input(bytes) if bytes.as_ref() == b"\r"));
        view.process_event(Event::CommandStart, cx);
        assert_eq!(view.runtime_agent().unwrap().kind, "codex");
        assert_eq!(view.ai_session.as_ref().unwrap().session_id, "saved-42");
        view.process_event(Event::CommandDone { exit_code: Some(0) }, cx);
        assert!(view.running_program.is_none());
        assert!(view.ai_session.is_none(), "both foreground fields must clear together");
        assert!(view.runtime_agent().is_none());
    }));
}

#[gpui::test]
fn review_regression_saved_identity_without_a_resume_command_is_not_live(cx: &mut TestAppContext) {
    let (view, window, _) = open(cx);
    window.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.seed_ai_session("codex".into(), "saved-42".into(), cx);
            assert!(view.ai_session.is_none());
            view.run_command("codex resume saved-42".into(), cx);
            feed(view, b"Password: ");
            view.process_event(Event::Wakeup, cx);
            assert!(
                view.pending_shell_command.is_some(),
                "never send a command into authentication"
            );
            assert!(view.running_program.is_none());
        })
    });
}

#[gpui::test]
fn review_regression_quiet_startup_and_maximize_deliver_the_latest_pty_size(
    cx: &mut TestAppContext,
) {
    let (view, window, receiver) = open(cx);
    window.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.set_layout(
                point(px(0.0), px(0.0)),
                px(10.0),
                px(20.0),
                size(px(1200.0), px(700.0)),
                1.0,
                cx,
            );
            view.set_layout(
                point(px(0.0), px(0.0)),
                px(10.0),
                px(20.0),
                size(px(1600.0), px(900.0)),
                1.0,
                cx,
            );
            assert!(receiver.try_recv().is_err(), "startup waits for final layout");
        })
    });
    window.run_until_parked();
    window.executor().advance_clock(TerminalView::STARTUP_GRID_GRACE);
    window.run_until_parked();
    let sizes: Vec<_> = receiver
        .try_iter()
        .filter_map(|message| match message {
            Msg::Resize(size) => Some((size.num_cols, size.num_lines)),
            _ => None,
        })
        .collect();
    assert_eq!(sizes, [(160, 45)], "one final resize even without a new terminal frame");
    window.update(|_, cx| {
        view.update(cx, |view, cx| {
            view.mark_structural_resize();
            view.set_layout(
                point(px(0.0), px(0.0)),
                px(10.0),
                px(20.0),
                size(px(800.0), px(500.0)),
                1.0,
                cx,
            );
        })
    });
    assert!(receiver.try_iter().any(|message| matches!(message, Msg::Resize(size) if size.num_cols == 80 && size.num_lines == 25)));
}
