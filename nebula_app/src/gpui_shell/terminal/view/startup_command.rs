//! Resume commands wait for the new shell prompt before publishing live identity.

use super::*;

pub(super) struct PendingShellCommand {
    text: String,
    identity: Option<crate::display::AiSessionIdentity>,
}

impl TerminalView {
    pub(crate) fn seed_restored_cwd(&mut self, cwd: String, cx: &mut Context<Self>) {
        self.process_event(TermEvent::CwdReport(cwd), cx);
    }

    /// Keep a queued cold resume in the next snapshot without presenting it as
    /// a live foreground agent before the shell accepts the command.
    pub(crate) fn session_agent(&self) -> Option<crate::session::AgentSession> {
        let pending = self.pending_shell_command.as_ref();
        let identity =
            pending.and_then(|request| request.identity.as_ref()).or(self.ai_session.as_ref());
        if let Some(identity) = identity {
            return Some(crate::session::AgentSession {
                source: identity.source.clone(),
                session_id: Some(identity.session_id.clone()),
            });
        }
        let kind = pending
            .and_then(|request| crate::ai_agents::AgentKind::parse_command(&request.text))
            .or_else(|| {
                self.running_program.as_deref().and_then(crate::ai_agents::AgentKind::parse)
            })?;
        Some(crate::session::AgentSession { source: kind.slug().to_owned(), session_id: None })
    }

    pub fn run_command(&mut self, text: String, cx: &mut Context<Self>) {
        if text.is_empty() || self.exited.is_some() {
            return;
        }
        self.pending_shell_command = Some(PendingShellCommand { text, identity: None });
        self.flush_pending_shell_command(cx);
    }

    pub fn seed_ai_session(&mut self, source: String, session_id: String, cx: &mut Context<Self>) {
        if session_id.is_empty() {
            return;
        }
        let identity = crate::display::AiSessionIdentity { source, session_id };
        if let Some(pending) = self.pending_shell_command.as_mut() {
            pending.identity = Some(identity);
        } else if self.running_program.as_deref() == Some(identity.source.as_str()) {
            self.ai_session = Some(identity);
            cx.emit(TerminalViewEvent::TitleChanged);
            cx.notify();
        }
    }

    pub(super) fn flush_pending_shell_command(&mut self, cx: &mut Context<Self>) {
        if self.pending_shell_command.is_none()
            || self.pending_runtime_submit.is_some()
            || self.exited.is_some()
        {
            return;
        }
        let Some(session) = &self.session else { return };
        let ready = {
            let term = session.term.lock();
            !term.mode().intersects(TermMode::ALT_SCREEN | TermMode::VI)
                && crate::display::nebula_shell_ready_from_raw_grid(
                    &term,
                    &self.suggest.suggest_env,
                )
        };
        if !ready {
            return;
        }
        let pending = self.pending_shell_command.take().expect("checked above");
        let kind = crate::ai_agents::AgentKind::parse_command(&pending.text);
        if let Err(error) = self.runtime_prompt(pending.text.clone(), true, cx) {
            log::warn!("Could not submit startup command: {error:?}");
            self.pending_shell_command = Some(pending);
            return;
        }
        if let Some(kind) = kind {
            self.running_program = Some(kind.slug().to_owned());
            self.ai_session = pending.identity;
            self.agent_status = crate::ai_agents::AgentStatus::Working;
            self.agent_status_source = crate::ai_agents::AgentStatusSource::Process;
            self.agent_turn_active = false;
        }
        cx.emit(TerminalViewEvent::TitleChanged);
        cx.notify();
    }
}
