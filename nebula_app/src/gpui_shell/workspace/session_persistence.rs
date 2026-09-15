use super::*;
use crate::session::Session;

impl NebulaWorkspace {
    /// 启动恢复：断路器跳闸就隔离现场并走干净路径；恢复成功弹一条
    /// 自动消失的提示（崩溃现场多一句来源说明）。返回是否恢复出了 tab。
    pub(super) fn try_restore_session(
        &mut self,
        resume_ai: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::display::ToastKind;

        let Some(mut session) = crate::session::load() else { return false };
        if !crate::session::should_restore(&session) {
            if !session.tabs.is_empty() {
                // 连续几次启动都没活到第一次自动保存：把「一恢复就崩」的
                // 现场挪去隔离文件（唯一的诊断材料），本次干净启动。
                if let Some(path) = crate::session::quarantine() {
                    crate::gpui_shell::toast::banner(
                        window,
                        cx,
                        ToastKind::Warning,
                        format!("连续多次启动未完成恢复，已跳过；现场保存在 {}", path.display()),
                    );
                }
            }
            return false;
        }
        let crashed = crate::session::was_crash(&session);
        crate::session::mark_boot_attempt(&mut session);
        let mut restored = 0usize;
        for tab in &session.tabs {
            if self.restore_tab(tab, resume_ai, window, cx) {
                restored += 1;
            }
        }
        if restored == 0 {
            return false;
        }
        self.active = session.active_tab.min(self.tabs.len().saturating_sub(1));
        self.focus_active(window, cx);
        let text = if crashed {
            format!("上次未正常退出，已恢复 {restored} 个标签")
        } else {
            format!("已恢复 {restored} 个标签")
        };
        crate::gpui_shell::toast::toast(window, cx, ToastKind::Success, text);
        cx.notify();
        true
    }

    /// 恢复一个 Terminal tab：DFS 逐叶 spawn（消失目录回退默认 cwd、AI 会话
    /// 以安全 resume 命令接续、SSH launch 只作用于首 pane——launch 描述的
    /// 是「首 pane 怎么启动」，旧壳同义），再按持久化树的形状重建分屏树。
    pub(super) fn restore_tab(
        &mut self,
        tab: &crate::session::TabSession,
        resume_ai: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::session::{LaunchSession, LayoutSession};

        let layout =
            tab.layout.clone().unwrap_or(LayoutSession::unnamed_pane(tab.cwd.clone(), None));
        // v1-v3 / 早期 GPUI 快照没有 launch，按共享 schema 回退 Default；
        // v4 的 Shell/Profile/Ssh 必须原样用于首 Pane，不能再次读取当前默认。
        let saved_launch = tab.launch.clone().unwrap_or(LaunchSession::Default);
        let grid = self.initial_grid;
        let mut panes: Vec<TerminalPane> = Vec::new();
        for (index, leaf) in layout.leaves().into_iter().enumerate() {
            let LayoutSession::Pane { cwd, agent, custom_name } = leaf else { continue };
            let launch = if index == 0 {
                Self::terminal_launch_from_session(&saved_launch, crate::session::valid_dir(cwd))
            } else {
                // 共享 v4 与旧壳只把 Tab 的 launch 赋给首 Pane；其它叶子没有
                // 独立启动身份，保持既有 Default 恢复语义。
                crate::gpui_shell::terminal::view::TerminalLaunch::Local {
                    cwd: crate::session::valid_dir(cwd),
                    shell: None,
                    shell_name: None,
                }
            };
            let command = restored_agent_command(resume_ai, agent.as_ref());
            let mut pane = self.new_pane(grid, launch, command, window, cx);
            pane.custom_name = custom_name.as_deref().and_then(pane_rename::normalize_name);
            // 冷恢复已经知道这段对话的 hook 身份：种回 view，右键「分叉
            // AI 会话」不必再等下一条带 session_id 的 hook。
            if let Some((source, session_id)) = agent
                .as_ref()
                .and_then(|agent| Some((agent.source.clone(), agent.session_id.clone()?)))
            {
                pane.view.update(cx, |view, cx| view.seed_ai_session(source, session_id, cx));
            }
            panes.push(pane);
        }
        if panes.is_empty() {
            return false;
        }
        let mut ids = panes.iter().map(|pane| pane.id).collect::<Vec<_>>().into_iter();
        let (tree, _) = crate::gpui_shell::session_restore::tree_from_layout(&layout, &mut || {
            ids.next().unwrap_or(0)
        });
        let focused =
            panes.get(tab.active_pane).or_else(|| panes.first()).map(|pane| pane.id).unwrap_or(0);
        // 恢复期保持文件里的既有次序，不套「新标签插入位置」策略。
        // 重命名与色标随会话一起回来（旧壳同合同）。
        let at = self.tabs.len();
        self.insert_tab_at(
            at,
            WorkspaceTab::Terminal { panes, tree, focused, zoomed: false, broadcast: false },
            TabMeta {
                custom_name: tab.custom_name.clone(),
                color: tab.color,
                shell_tag: Self::launch_shell_tag(&saved_launch),
                launch: Some(saved_launch),
                has_bell: false,
            },
        );
        true
    }

    /// 当前工作区 → 共享 v4 快照。设置/文档/图片 tab 不进会话（旧壳同
    /// 合同）；AI 会话身份优先取 hook 直报的精确 id，退而取可解析的前台
    /// 程序名（claude 无 id 恢复成 `--continue`，安全判定在 schema 层）。
    pub(crate) fn snapshot_session(&self, cx: &App) -> crate::session::Session {
        use crate::session::{AgentSession, LaunchSession, Session, TabSession};

        let mut tabs = Vec::new();
        let mut active_out = 0usize;
        for (ix, tab) in self.tabs.iter().enumerate() {
            let WorkspaceTab::Terminal { panes, tree, focused, .. } = tab else { continue };
            if ix == self.active {
                active_out = tabs.len();
            }
            let leaf_data = |id: u64| -> (String, Option<AgentSession>, Option<String>) {
                let Some(pane) = panes.iter().find(|pane| pane.id == id) else {
                    return (String::new(), None, None);
                };
                let view = pane.view.read(cx);
                let agent = view
                    .ai_session
                    .as_ref()
                    .map(|identity| AgentSession {
                        source: identity.source.clone(),
                        session_id: Some(identity.session_id.clone()),
                    })
                    .or_else(|| {
                        view.running_program
                            .as_deref()
                            .filter(|program| crate::ai_agents::AgentKind::parse(program).is_some())
                            .map(|program| AgentSession {
                                source: program.to_owned(),
                                session_id: None,
                            })
                    });
                (view.cwd.clone(), agent, pane.custom_name.clone())
            };
            let layout = crate::gpui_shell::session_restore::layout_from_tree(tree, &leaf_data);
            let cwd = panes
                .iter()
                .find(|pane| pane.id == *focused)
                .map(|pane| pane.view.read(cx).cwd.clone())
                .unwrap_or_default();
            let meta = self.meta(ix);
            let first_leaf = tree.first_leaf();
            let launch = meta.launch.clone().unwrap_or_else(|| {
                // 兼容本次修复前已经在内存中的 Tab：SSH 仍可从首 Pane 取回；
                // 旧本地 Tab 已经没有身份信息，只能诚实落为 Default。
                panes
                    .iter()
                    .find(|pane| pane.id == first_leaf)
                    .and_then(|pane| pane.view.read(cx).ssh_destination.clone())
                    .map(|host| LaunchSession::Ssh { host })
                    .unwrap_or(LaunchSession::Default)
            });
            let active_pane = tree.leaves().iter().position(|id| id == focused).unwrap_or(0);
            tabs.push(TabSession {
                cwd,
                custom_name: meta.custom_name,
                color: meta.color,
                launch: Some(launch),
                layout: Some(layout),
                active_pane,
            });
        }
        Session::new(active_out, tabs)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SaveReason {
    Checkpoint,
    WindowClose,
    TabsClosed,
    Quit,
}

pub(super) struct SessionPersistence {
    latest: Option<Session>,
    saved: Option<Session>,
    quitting: bool,
    isolated: bool,
}

impl Default for SessionPersistence {
    fn default() -> Self {
        Self {
            latest: None,
            saved: None,
            quitting: false,
            isolated: crate::platform::elevation::requires_isolation(),
        }
    }
}

impl SessionPersistence {
    pub(super) fn save(&mut self, current: Option<Session>, reason: SaveReason) {
        self.save_with(current, reason, crate::session::try_save);
    }

    fn save_with(
        &mut self,
        current: Option<Session>,
        reason: SaveReason,
        write: impl FnOnce(&Session) -> std::io::Result<()>,
    ) {
        // An administrator window must not overwrite the ordinary workspace or
        // cause its privileged shell command to be restored in a later session.
        if self.isolated {
            return;
        }
        let retry_checkpoint = reason == SaveReason::Checkpoint
            && current.as_ref().is_none_or(|session| session.tabs.is_empty());
        let candidate = if self.quitting {
            if reason != SaveReason::Quit {
                return;
            }
            self.latest.clone()
        } else {
            match reason {
                SaveReason::Checkpoint => {
                    current.filter(|session| !session.tabs.is_empty()).or_else(|| {
                        self.latest
                            .as_ref()
                            .filter(|latest| self.saved.as_ref() != Some(latest))
                            .cloned()
                    })
                },
                SaveReason::WindowClose | SaveReason::TabsClosed => current,
                SaveReason::Quit => current
                    .filter(|session| !session.tabs.is_empty())
                    .or_else(|| self.latest.clone()),
            }
        };
        self.quitting |= reason == SaveReason::Quit;
        let Some(mut session) = candidate else { return };
        if !retry_checkpoint {
            session.clean_exit = matches!(reason, SaveReason::WindowClose | SaveReason::Quit);
        }
        self.latest = Some(session.clone());
        if self.saved.as_ref() == Some(&session) {
            return;
        }
        match write(&session) {
            Ok(()) => self.saved = Some(session),
            Err(error) => log::warn!("Could not persist terminal session: {error}"),
        }
    }
}

pub(super) fn combine_sessions(
    sessions: impl IntoIterator<Item = (bool, Session)>,
) -> Option<Session> {
    let mut combined = None;
    for (active, session) in sessions {
        let combined = combined.get_or_insert_with(|| Session::new(0, Vec::new()));
        if active {
            combined.active_tab = combined.tabs.len().saturating_add(session.active_tab);
        }
        combined.tabs.extend(session.tabs);
    }
    if let Some(session) = combined.as_mut() {
        session.active_tab = session.active_tab.min(session.tabs.len().saturating_sub(1));
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isolated_windows_never_write_shared_session_storage() {
        let mut state = SessionPersistence { isolated: true, ..SessionPersistence::default() };
        for reason in [
            SaveReason::Checkpoint,
            SaveReason::TabsClosed,
            SaveReason::WindowClose,
            SaveReason::Quit,
        ] {
            state.save_with(Some(sample_session()), reason, |_| {
                panic!("privileged session reached shared storage")
            });
        }
        assert!(state.latest.is_none());
    }
    use crate::session::{AgentSession, LayoutSession, TabSession};

    fn ordinary_window() -> SessionPersistence {
        // Hosted Windows runners can be elevated. These tests exercise ordinary
        // window persistence; privileged isolation has its own negative test.
        SessionPersistence { isolated: false, ..SessionPersistence::default() }
    }

    fn sample_session() -> Session {
        let mut tab = TabSession::single("D:/work".into(), Some("Workspace".into()), None);
        tab.layout = Some(LayoutSession::Pane {
            cwd: tab.cwd.clone(),
            custom_name: Some("后端 API".into()),
            agent: Some(AgentSession {
                source: "claude".into(),
                session_id: Some("saved-42".into()),
            }),
        });
        Session::new(0, vec![tab])
    }

    fn save_to(
        state: &mut SessionPersistence,
        path: &std::path::Path,
        current: Option<Session>,
        reason: SaveReason,
    ) {
        state.save_with(current, reason, |session| crate::session::save_to(path, session));
    }

    #[test]
    fn closing_last_window_then_quitting_preserves_tabs_and_ai_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let expected = sample_session();
        let mut state = ordinary_window();
        save_to(&mut state, &path, Some(expected.clone()), SaveReason::WindowClose);
        save_to(&mut state, &path, None, SaveReason::Checkpoint);
        save_to(&mut state, &path, None, SaveReason::Quit);
        let restored = crate::session::load_from(&path).unwrap();
        assert!(crate::session::should_restore(&restored));
        assert!(restored.clean_exit);
        assert_eq!(restored.tabs, expected.tabs);
    }

    #[test]
    fn process_kill_restores_the_last_checkpoint_without_an_exit_callback() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let expected = sample_session();
        let mut state = ordinary_window();
        save_to(&mut state, &path, Some(expected.clone()), SaveReason::Checkpoint);
        drop(state);
        let restored = crate::session::load_from(&path).unwrap();
        assert!(crate::session::should_restore(&restored));
        assert!(crate::session::was_crash(&restored));
        assert_eq!(restored.tabs, expected.tabs);
    }

    #[test]
    fn teardown_events_cannot_overwrite_the_final_snapshot() {
        let mut state = ordinary_window();
        let expected = sample_session();
        state.save_with(Some(expected.clone()), SaveReason::Quit, |_| Ok(()));
        for reason in [SaveReason::Checkpoint, SaveReason::TabsClosed, SaveReason::WindowClose] {
            state.save_with(Some(Session::new(0, vec![])), reason, |_| {
                panic!("teardown must not write a second snapshot")
            });
        }
        state.save_with(Some(Session::new(0, vec![])), SaveReason::Quit, |_| {
            panic!("repeated quit must retain the frozen snapshot")
        });
        assert_eq!(state.saved.unwrap().tabs, expected.tabs);
    }

    #[test]
    fn explicitly_closing_all_tabs_does_not_resurrect_them() {
        let mut state = ordinary_window();
        state.save_with(Some(sample_session()), SaveReason::Checkpoint, |_| Ok(()));
        state.save_with(Some(Session::new(0, vec![])), SaveReason::TabsClosed, |_| Ok(()));
        state.save_with(None, SaveReason::Quit, |_| Ok(()));
        let saved = state.saved.unwrap();
        assert!(!crate::session::should_restore(&saved));
        assert!(saved.clean_exit);
    }

    #[test]
    fn an_empty_startup_or_auxiliary_window_cannot_erase_a_saved_session() {
        let mut state = ordinary_window();
        for current in [None, Some(Session::new(0, vec![]))] {
            state.save_with(current, SaveReason::Checkpoint, |_| {
                panic!("initial empty state must not reach storage")
            });
        }
        state.save_with(None, SaveReason::Quit, |_| panic!("no snapshot to write"));
    }

    #[test]
    fn failed_checkpoint_and_final_writes_are_retried() {
        let mut state = ordinary_window();
        let session = sample_session();
        let fail = |_: &Session| Err(std::io::Error::other("storage unavailable"));
        state.save_with(Some(session.clone()), SaveReason::Checkpoint, fail);
        assert!(state.saved.is_none());
        state.save_with(Some(session.clone()), SaveReason::Checkpoint, |_| Ok(()));
        assert_eq!(state.saved.as_ref(), Some(&session));
        state.save_with(Some(session.clone()), SaveReason::Quit, fail);
        state.save_with(None, SaveReason::Quit, |_| Ok(()));
        let saved = state.saved.unwrap();
        assert_eq!(saved.tabs, session.tabs);
        assert!(saved.clean_exit);
    }

    #[test]
    fn unchanged_checkpoints_do_not_rewrite_storage() {
        let mut state = ordinary_window();
        state.save_with(Some(sample_session()), SaveReason::Checkpoint, |_| Ok(()));
        state.save_with(Some(sample_session()), SaveReason::Checkpoint, |_| {
            panic!("unchanged checkpoint must not write")
        });
    }

    #[test]
    fn failed_window_close_is_retried_even_without_remaining_windows() {
        let mut state = ordinary_window();
        state.save_with(Some(sample_session()), SaveReason::WindowClose, |_| {
            Err(std::io::Error::other("temporary write failure"))
        });
        state.save_with(None, SaveReason::Checkpoint, |session| {
            assert!(session.clean_exit);
            assert_eq!(session.tabs, sample_session().tabs);
            Ok(())
        });
        assert!(state.saved.unwrap().clean_exit);
    }

    #[test]
    fn failed_explicit_empty_snapshot_is_retried_without_resurrecting_tabs() {
        let mut state = ordinary_window();
        state.save_with(Some(sample_session()), SaveReason::Checkpoint, |_| Ok(()));
        state.save_with(Some(Session::new(0, vec![])), SaveReason::WindowClose, |_| {
            Err(std::io::Error::other("temporary write failure"))
        });
        state.save_with(None, SaveReason::Checkpoint, |session| {
            assert!(session.tabs.is_empty());
            Ok(())
        });
        assert!(state.saved.unwrap().tabs.is_empty());
    }

    #[test]
    fn all_windows_are_combined_with_the_active_tab_offset() {
        let first = sample_session();
        let mut second = sample_session();
        second.tabs.push(TabSession::single("D:/other".into(), None, None));
        second.active_tab = 1;
        let combined = combine_sessions([(false, first), (true, second)]).unwrap();
        assert_eq!(combined.tabs.len(), 3);
        assert_eq!(combined.active_tab, 2);
        assert!(combine_sessions([]).is_none());
    }
}
