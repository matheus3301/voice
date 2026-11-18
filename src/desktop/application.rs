//! Native menu bar, application actions and document-window routing.
use super::*;
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = voice, no_json)]
struct OpenRecentProject {
    path: PathBuf,
}

struct DesktopState {
    main_window: Option<WindowHandle<VoiceApp>>,
    open_requests: crossbeam_channel::Receiver<Vec<String>>,
}
impl Global for DesktopState {}

pub(super) fn request_quit(cx: &mut App) {
    if let Some(handle) = cx.global::<DesktopState>().main_window
        && handle
            .update(cx, |app, window, cx| app.quit(window, cx))
            .is_ok()
    {
        return;
    }
    platform::allow_termination();
    cx.quit();
}

fn create_document_window(cx: &mut App) -> anyhow::Result<WindowHandle<VoiceApp>> {
    let receiver = cx.global::<DesktopState>().open_requests.clone();
    let bounds = Bounds::centered(None, size(px(1240.), px(820.)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(1060.), px(680.))),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Untitled — Voice".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| {
            let entity = cx.new(|cx| VoiceApp::new(window, cx, receiver));
            let weak = entity.downgrade();
            window.on_window_should_close(cx, move |window, cx| {
                weak.update(cx, |app, cx| {
                    if app.pending_exit {
                        return true;
                    }
                    app.close_document(window, cx);
                    false
                })
                .unwrap_or(true)
            });
            entity
        },
    )?;
    cx.global_mut::<DesktopState>().main_window = Some(handle);
    Ok(handle)
}
fn ensure_document_window(cx: &mut App) -> Option<WindowHandle<VoiceApp>> {
    if let Some(handle) = cx.global::<DesktopState>().main_window
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return Some(handle);
    }
    match create_document_window(cx) {
        Ok(handle) => Some(handle),
        Err(error) => {
            eprintln!("Couldn't open a Voice window: {error:#}");
            None
        }
    }
}
fn route_document_intent(intent: DocumentIntent, cx: &mut App) {
    if let Some(handle) = ensure_document_window(cx) {
        let _ = handle.update(cx, |app, window, cx| {
            app.request_document_action(intent, window, cx)
        });
    }
}
pub(super) fn install_menus(cx: &mut App) {
    let recents = platform::recent_documents()
        .into_iter()
        .filter(|path| path.extension().is_some_and(|e| e == "voice"))
        .map(|path| {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            MenuItem::action(name, OpenRecentProject { path })
        })
        .collect::<Vec<_>>();
    cx.set_menus(vec![
        Menu {
            name: "Voice".into(),
            items: vec![
                MenuItem::action("About Voice", AboutVoice),
                MenuItem::separator(),
                MenuItem::action("Settings…", ShowAudio),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide Voice", HideVoice),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Voice", Quit),
            ],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("New Project", NewProject),
                MenuItem::action("Open…", OpenProject),
                MenuItem::submenu(Menu {
                    name: "Open Recent".into(),
                    items: recents,
                }),
                MenuItem::separator(),
                MenuItem::action("Import Vocal Reference…", ImportTarget),
                MenuItem::action("Import Accompaniment…", ImportAccompaniment),
                MenuItem::separator(),
                MenuItem::action("Save", SaveProject),
                MenuItem::action("Save As…", SaveProjectAs),
                MenuItem::separator(),
                MenuItem::action("Close", CloseDocument),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Undo", EditUndo, gpui::OsAction::Undo),
                MenuItem::os_action("Redo", EditRedo, gpui::OsAction::Redo),
                MenuItem::separator(),
                MenuItem::os_action("Cut", EditCut, gpui::OsAction::Cut),
                MenuItem::os_action("Copy", EditCopy, gpui::OsAction::Copy),
                MenuItem::os_action("Paste", EditPaste, gpui::OsAction::Paste),
                MenuItem::os_action("Select All", EditSelectAll, gpui::OsAction::SelectAll),
            ],
        },
        Menu {
            name: "Playback".into(),
            items: vec![
                MenuItem::action("Play / Pause", TogglePlayback),
                MenuItem::action("Record Your Voice", ToggleRecording),
                MenuItem::action("Return to Beginning", ReturnToStart),
                MenuItem::separator(),
                MenuItem::action("Move Back One Second", SeekBack),
                MenuItem::action("Move Forward One Second", SeekForward),
                MenuItem::action("Loop", ToggleLoop),
                MenuItem::separator(),
                MenuItem::action("Microphone", ToggleMicrophone),
                MenuItem::action("Monitoring", ToggleMonitoring),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Mix…", ShowMix),
                MenuItem::action("Vocal Reference…", ShowTarget),
                MenuItem::action("Practice History…", ShowHistory),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", MinimizeWindow),
                MenuItem::action("Zoom", ZoomWindow),
            ],
        },
        Menu {
            name: "Help".into(),
            items: vec![MenuItem::action("Voice Help", ShowHelp)],
        },
    ]);
}

pub fn run() {
    let (open_sender, open_receiver) = crossbeam_channel::unbounded();
    let async_app = std::rc::Rc::new(std::cell::RefCell::new(None::<gpui::AsyncApp>));
    let application = Application::new();
    let open_context = async_app.clone();
    application.on_open_urls(move |urls| {
        let _ = open_sender.send(urls);
        if let Some(cx) = open_context.borrow_mut().as_mut() {
            let _ = cx.update(|cx| {
                ensure_document_window(cx);
                cx.activate(true);
            });
        }
    });
    application.on_reopen(|cx| {
        if cx.has_global::<DesktopState>() {
            ensure_document_window(cx);
            cx.activate(true);
        }
    });
    let path = std::env::args_os().nth(1).map(PathBuf::from);
    application.run(move |cx: &mut App| {
        *async_app.borrow_mut() = Some(cx.to_async());
        cx.set_global(DesktopState {
            main_window: None,
            open_requests: open_receiver,
        });
        cx.bind_keys([
            KeyBinding::new(
                "space",
                TogglePlayback,
                Some("Voice && !VoiceControl && !VoiceSheet"),
            ),
            KeyBinding::new(
                "enter",
                ReturnToStart,
                Some("Voice && !VoiceControl && !VoiceSheet"),
            ),
            KeyBinding::new(
                "left",
                SeekBack,
                Some("Voice && !VoiceControl && !VoiceSheet"),
            ),
            KeyBinding::new(
                "right",
                SeekForward,
                Some("Voice && !VoiceControl && !VoiceSheet"),
            ),
            KeyBinding::new(
                "l",
                ToggleLoop,
                Some("Voice && !VoiceControl && !VoiceSheet"),
            ),
            KeyBinding::new(
                "m",
                ToggleMicrophone,
                Some("Voice && !VoiceControl && !VoiceSheet"),
            ),
            KeyBinding::new(
                "alt-m",
                ToggleMonitoring,
                Some("Voice && !VoiceControl && !VoiceSheet"),
            ),
            KeyBinding::new(
                "r",
                ToggleRecording,
                Some("Voice && !VoiceControl && !VoiceSheet"),
            ),
            KeyBinding::new("cmd-o", OpenProject, None),
            KeyBinding::new("cmd-s", SaveProject, None),
            KeyBinding::new("cmd-shift-s", SaveProjectAs, None),
            KeyBinding::new("cmd-n", NewProject, None),
            KeyBinding::new("cmd-i", ImportTarget, None),
            KeyBinding::new("cmd-shift-i", ImportAccompaniment, None),
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-w", CloseDocument, None),
            KeyBinding::new("escape", ClosePanel, None),
            KeyBinding::new("cmd-,", ShowAudio, None),
            KeyBinding::new("cmd-1", ShowMix, None),
            KeyBinding::new("cmd-2", ShowTarget, None),
            KeyBinding::new("cmd-4", ShowHistory, None),
            KeyBinding::new("cmd-/", ShowHelp, None),
            KeyBinding::new("cmd-h", HideVoice, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-m", MinimizeWindow, None),
            KeyBinding::new("cmd-z", EditUndo, None),
            KeyBinding::new("cmd-shift-z", EditRedo, None),
            KeyBinding::new("cmd-x", EditCut, None),
            KeyBinding::new("cmd-c", EditCopy, None),
            KeyBinding::new("cmd-v", EditPaste, None),
            KeyBinding::new("cmd-a", EditSelectAll, None),
        ]);
        cx.on_action(|_: &NewProject, cx| route_document_intent(DocumentIntent::New, cx));
        cx.on_action(|_: &OpenProject, cx| route_document_intent(DocumentIntent::OpenDialog, cx));
        cx.on_action(|action: &OpenRecentProject, cx| {
            route_document_intent(DocumentIntent::Open(action.path.clone()), cx)
        });
        cx.on_action(|_: &ShowAudio, cx| {
            if let Some(handle) = ensure_document_window(cx) {
                let _ = handle.update(cx, |app, window, cx| app.show_audio_settings(window, cx));
            }
        });
        cx.on_action(|_: &Quit, cx| request_quit(cx));
        cx.on_action(|_: &AboutVoice, _| platform::show_about_panel());
        cx.on_action(|_: &HideVoice, cx| cx.hide());
        cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
        cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
        cx.on_action(|_: &MinimizeWindow, cx| {
            cx.defer(|cx| {
                if let Some(handle) = cx.active_window() {
                    let _ = handle.update(cx, |_, window, _| window.minimize_window());
                }
            })
        });
        cx.on_action(|_: &ZoomWindow, cx| {
            cx.defer(|cx| {
                if let Some(handle) = cx.active_window() {
                    let _ = handle.update(cx, |_, window, _| window.zoom_window());
                }
            })
        });
        install_menus(cx);
        platform::install_termination_guard();
        // Keep external quit handling alive after the last document closes.
        // AppKit's synchronous delegate only enqueues; GPUI owns the save flow.
        cx.spawn(async move |cx| {
            loop {
                Timer::after(Duration::from_millis(50)).await;
                if cx
                    .update(|cx| {
                        if platform::take_termination_request() {
                            request_quit(cx);
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        if let Some(handle) = ensure_document_window(cx)
            && let Some(path) = path
        {
            let _ = handle.update(cx, |app, _, _| app.open_path(path));
        }
        cx.activate(true);
    });
}
