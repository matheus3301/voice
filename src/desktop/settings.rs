//! A native utility window for audio preferences, owned by the document controller.
use super::{CloseDocument, ClosePanel, VoiceApp, theme::Theme};
use gpui::{
    App, Bounds, Context, Entity, FocusHandle, IntoElement, Render, Subscription, WeakEntity,
    Window, WindowBounds, WindowHandle, WindowOptions, div, prelude::*, px, rgb, size,
};

pub(super) struct AudioSettingsWindow {
    owner: WeakEntity<VoiceApp>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}
impl AudioSettingsWindow {
    fn new(owner: &Entity<VoiceApp>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        // Register observers only. The document can be borrowed while this window opens.
        let changes = cx.observe_in(owner, window, |_, _, window, cx| {
            window.refresh();
            cx.notify();
        });
        let released =
            cx.observe_release_in(owner, window, |_, _, window, _| window.remove_window());
        let appearance = cx.observe_window_appearance(window, |_, _, cx| cx.notify());
        let permission_owner = owner.downgrade();
        let activation = cx.observe_window_activation(window, move |_, _, cx| {
            let owner = permission_owner.clone();
            cx.defer(move |cx| {
                let _ = owner.update(cx, |app, cx| {
                    app.microphone_authorization = super::platform::microphone_authorization();
                    if app.microphone_authorization
                        != super::platform::MicrophoneAuthorization::Authorized
                        && app.input_running
                    {
                        app.stop_input();
                    }
                    cx.notify();
                });
            });
            cx.notify();
        });
        Self {
            owner: owner.downgrade(),
            focus,
            _subscriptions: vec![changes, released, appearance, activation],
        }
    }
}
impl Render for AudioSettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let Some(owner) = self.owner.upgrade() else {
            window.remove_window();
            return div().into_any_element();
        };
        let content = owner.update(cx, |app, cx| app.audio_settings_view(window, cx));
        div()
            .id("audio-settings-window")
            .key_context("VoiceSettings")
            .track_focus(&self.focus)
            .size_full()
            .bg(rgb(theme.window))
            .text_color(rgb(theme.text))
            .font_family(".SystemUIFont")
            .text_size(px(13.))
            .on_action(cx.listener(|_, _: &CloseDocument, window, cx| {
                window.remove_window();
                cx.stop_propagation();
            }))
            .on_action(cx.listener(|_, _: &ClosePanel, window, cx| {
                window.remove_window();
                cx.stop_propagation();
            }))
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .id("audio-settings-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .p_5()
                    .child(content),
            )
            .into_any_element()
    }
}

pub(super) fn open(
    owner: Entity<VoiceApp>,
    cx: &mut App,
) -> anyhow::Result<WindowHandle<AudioSettingsWindow>> {
    let bounds = Bounds::centered(None, size(px(520.), px(650.)), cx);
    let handle = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(450.), px(450.))),
            titlebar: Some(gpui::TitlebarOptions {
                title: Some("Audio Settings".into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| AudioSettingsWindow::new(&owner, window, cx)),
    )?;
    Ok(handle)
}
