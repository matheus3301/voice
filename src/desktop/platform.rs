//! Native document sheets and microphone authorization for the desktop shell.

use anyhow::Result;
use futures_channel::oneshot;
use gpui::{App, Window};
use std::path::{Path, PathBuf};

pub type FileDialogResult = oneshot::Receiver<Result<Option<PathBuf>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MicrophoneAuthorization {
    NotDetermined,
    Authorized,
    Denied,
    Restricted,
}

pub fn open_microphone_settings(cx: &mut App) {
    cx.open_url("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone");
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use anyhow::{Context, anyhow, bail};
    use block2::RcBlock;
    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
        rc::Retained,
        runtime::{AnyObject, Bool, ProtocolObject, Sel},
    };
    use objc2_app_kit::{
        NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSDocumentController,
        NSModalResponse, NSModalResponseCancel, NSModalResponseOK, NSOpenPanel, NSSavePanel,
        NSView, NSWindow,
    };
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};
    use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSString, NSURL};
    use objc2_uniform_type_identifiers::UTType;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    static TERMINATION_REQUESTED: AtomicBool = AtomicBool::new(false);
    static TERMINATION_ALLOWED: AtomicBool = AtomicBool::new(false);

    struct TerminationDelegateIvars {
        original: Option<Retained<ProtocolObject<dyn NSApplicationDelegate>>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements. All delegate methods
        // run on the main thread, and the original delegate remains retained.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[ivars = TerminationDelegateIvars]
        struct TerminationDelegate;

        // SAFETY: NSObjectProtocol has no additional implementation requirements.
        unsafe impl NSObjectProtocol for TerminationDelegate {
            #[unsafe(method(respondsToSelector:))]
            fn responds_to_selector(&self, selector: Sel) -> bool {
                // SAFETY: This calls NSObject's implementation with its declared
                // selector/signature and therefore includes our own methods.
                let implemented: bool = unsafe {
                    msg_send![super(self), respondsToSelector: selector]
                };
                implemented || self.ivars().original.as_ref().is_some_and(|original| {
                    original.respondsToSelector(selector)
                })
            }
        }

        impl TerminationDelegate {
            #[unsafe(method(forwardingTargetForSelector:))]
            fn forwarding_target(&self, selector: Sel) -> Option<&AnyObject> {
                self.ivars().original.as_deref().and_then(|original| {
                    original.respondsToSelector(selector).then(|| {
                        AsRef::<AnyObject>::as_ref(original)
                    })
                })
            }
        }

        // SAFETY: The proxy uses AppKit's delegate protocol on the main thread.
        unsafe impl NSApplicationDelegate for TerminationDelegate {
            #[unsafe(method(applicationShouldTerminate:))]
            fn application_should_terminate(
                &self,
                _application: &NSApplication,
            ) -> NSApplicationTerminateReply {
                if TERMINATION_ALLOWED.swap(false, Ordering::AcqRel) {
                    TERMINATION_REQUESTED.store(false, Ordering::Release);
                    NSApplicationTerminateReply::TerminateNow
                } else {
                    // Do not borrow GPUI or present alerts during AppKit's
                    // synchronous delegate call. The application polls this flag.
                    TERMINATION_REQUESTED.store(true, Ordering::Release);
                    NSApplicationTerminateReply::TerminateCancel
                }
            }
        }
    );

    impl TerminationDelegate {
        fn new(
            mtm: MainThreadMarker,
            original: Option<Retained<ProtocolObject<dyn NSApplicationDelegate>>>,
        ) -> Retained<Self> {
            let delegate = Self::alloc(mtm).set_ivars(TerminationDelegateIvars { original });
            // SAFETY: NSObject's init has this signature and initializes the
            // superclass after the proxy's Rust ivars have been installed.
            unsafe { msg_send![super(delegate), init] }
        }
    }

    thread_local! {
        // NSApplication's delegate is weak. Keep the proxy alive for the process;
        // the proxy in turn keeps GPUI's original delegate alive for forwarding.
        static TERMINATION_DELEGATE: RefCell<Option<Retained<TerminationDelegate>>> =
            const { RefCell::new(None) };
    }

    /// Install after GPUI has assigned its application delegate, on the main thread.
    pub fn install_termination_guard() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        TERMINATION_DELEGATE.with(|installed| {
            if installed.borrow().is_some() {
                return;
            }
            let application = NSApplication::sharedApplication(mtm);
            let delegate = TerminationDelegate::new(mtm, application.delegate());
            application.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            *installed.borrow_mut() = Some(delegate);
        });
    }

    /// Consume one pending external quit request from the application's UI timer.
    pub fn take_termination_request() -> bool {
        TERMINATION_REQUESTED.swap(false, Ordering::AcqRel)
    }

    /// Permit one final native termination after the document save flow succeeds.
    pub fn allow_termination() {
        TERMINATION_ALLOWED.store(true, Ordering::Release);
    }

    pub fn microphone_authorization() -> MicrophoneAuthorization {
        // AVMediaTypeAudio is a framework constant on every supported macOS release.
        let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
            return MicrophoneAuthorization::Restricted;
        };
        // SAFETY: Audio is one of the two media types accepted by this API.
        match unsafe { AVCaptureDevice::authorizationStatusForMediaType(audio) } {
            AVAuthorizationStatus::NotDetermined => MicrophoneAuthorization::NotDetermined,
            AVAuthorizationStatus::Authorized => MicrophoneAuthorization::Authorized,
            AVAuthorizationStatus::Denied => MicrophoneAuthorization::Denied,
            _ => MicrophoneAuthorization::Restricted,
        }
    }

    pub fn request_microphone_authorization() -> oneshot::Receiver<bool> {
        let (sender, receiver) = oneshot::channel();
        match microphone_authorization() {
            MicrophoneAuthorization::Authorized => {
                let _ = sender.send(true);
            }
            MicrophoneAuthorization::Denied | MicrophoneAuthorization::Restricted => {
                let _ = sender.send(false);
            }
            MicrophoneAuthorization::NotDetermined => {
                let Some(audio) = (unsafe { AVMediaTypeAudio }) else {
                    let _ = sender.send(false);
                    return receiver;
                };
                // AVFoundation may invoke this completion on any dispatch queue.
                let pending = Arc::new(Mutex::new(Some(sender)));
                let completion = RcBlock::new(move |granted: Bool| {
                    if let Ok(mut sender) = pending.lock()
                        && let Some(sender) = sender.take()
                    {
                        let _ = sender.send(granted.as_bool());
                    }
                });
                // SAFETY: Audio is valid. AVFoundation copies the escaping block;
                // its captured sender is thread-safe and owns no UI references.
                unsafe {
                    AVCaptureDevice::requestAccessForMediaType_completionHandler(
                        audio,
                        &completion,
                    );
                }
            }
        }
        receiver
    }

    fn native_window(window: &Window) -> Result<Retained<NSWindow>> {
        MainThreadMarker::new().context("Native document windows require the main thread")?;
        let handle = HasWindowHandle::window_handle(window)
            .map_err(|error| anyhow!("The application window is not available: {error}"))?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            bail!("The application window is not an AppKit window");
        };
        // SAFETY: GPUI's borrowed AppKit handle identifies its live NSView. It is
        // only accessed on the main thread, and window() returns a retained object.
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        view.window()
            .context("The application window is not attached")
    }

    fn content_types(extensions: &[&str]) -> Result<Retained<NSArray<UTType>>> {
        let types: Vec<_> = extensions
            .iter()
            .filter_map(|extension| {
                UTType::typeWithFilenameExtension(&NSString::from_str(extension))
            })
            .collect();
        if types.is_empty() {
            bail!("macOS could not determine the supported document types");
        }
        Ok(NSArray::from_retained_slice(&types))
    }

    fn project_content_types() -> Result<Retained<NSArray<UTType>>> {
        // The .voice extension is also used by system speech resources. Resolve
        // our exported identifier explicitly instead of accepting that ambiguity.
        let project =
            UTType::typeWithIdentifier(&NSString::from_str("dev.mmonteiro.voice-project"))
                .context("The Voice project document type is not registered with macOS")?;
        Ok(NSArray::from_retained_slice(&[project]))
    }

    struct PendingSheet {
        panel: Retained<NSSavePanel>,
        // Retain the native parent while AppKit owns the asynchronous sheet.
        _window: Retained<NSWindow>,
        sender: oneshot::Sender<Result<Option<PathBuf>>>,
    }

    fn present_sheet(
        panel: Retained<NSSavePanel>,
        window: Retained<NSWindow>,
        sender: oneshot::Sender<Result<Option<PathBuf>>>,
    ) {
        if window.attachedSheet().is_some() {
            let _ = sender.send(Err(anyhow!(
                "Close the current sheet before opening another"
            )));
            return;
        }
        let pending = Rc::new(RefCell::new(Some(PendingSheet {
            panel: panel.clone(),
            _window: window.clone(),
            sender,
        })));
        let completion = RcBlock::new(move |response: NSModalResponse| {
            // Taking the state releases our panel ownership when the copied block
            // finishes, breaking the panel/block retention cycle on every response.
            let Some(pending) = pending.borrow_mut().take() else {
                return;
            };
            pending.panel.orderOut(None);
            let result = if response == NSModalResponseOK {
                pending
                    .panel
                    .URL()
                    .and_then(|url| url.to_file_path())
                    .context("The selected document does not have a local file path")
                    .map(Some)
            } else if response == NSModalResponseCancel {
                Ok(None)
            } else {
                Err(anyhow!("macOS could not present the document sheet"))
            };
            let _ = pending.sender.send(result);
        });
        // AppKit copies this completion block until the sheet finishes.
        panel.beginSheetModalForWindow_completionHandler(&window, &completion);
    }

    fn open_dialog(
        window: &Window,
        title: &str,
        types: Result<Retained<NSArray<UTType>>>,
    ) -> FileDialogResult {
        let (sender, receiver) = oneshot::channel();
        let configured = (|| -> Result<_> {
            let mtm = MainThreadMarker::new().context("Document sheets require the main thread")?;
            let parent = native_window(window)?;
            let types = types?;
            let panel = NSOpenPanel::openPanel(mtm);
            panel.setTitle(Some(&NSString::from_str(title)));
            panel.setCanChooseFiles(true);
            panel.setCanChooseDirectories(false);
            panel.setAllowsMultipleSelection(false);
            panel.setResolvesAliases(true);
            panel.setAllowedContentTypes(&types);
            panel.setAllowsOtherFileTypes(false);
            Ok((panel.into_super(), parent))
        })();
        match configured {
            Ok((panel, parent)) => present_sheet(panel, parent, sender),
            Err(error) => {
                let _ = sender.send(Err(error));
            }
        }
        receiver
    }

    pub fn open_audio_dialog(window: &Window, title: &str) -> FileDialogResult {
        open_dialog(
            window,
            title,
            content_types(&[
                "wav", "wave", "flac", "mp3", "m4a", "aac", "ogg", "opus", "aif", "aiff", "caf",
                "mp4",
            ]),
        )
    }

    pub fn open_project_dialog(window: &Window) -> FileDialogResult {
        open_dialog(window, "Open Voice Project", project_content_types())
    }

    pub fn save_project_dialog(
        window: &Window,
        directory: &Path,
        suggested_name: &str,
    ) -> FileDialogResult {
        let (sender, receiver) = oneshot::channel();
        let configured = (|| -> Result<_> {
            let mtm = MainThreadMarker::new().context("Document sheets require the main thread")?;
            let parent = native_window(window)?;
            let types = project_content_types()?;
            let directory = NSURL::from_file_path(directory)
                .context("The save directory does not have a valid file URL")?;
            let panel = NSSavePanel::savePanel(mtm);
            panel.setTitle(Some(&NSString::from_str("Save Voice Project")));
            panel.setPrompt(Some(&NSString::from_str("Save")));
            panel.setAllowedContentTypes(&types);
            panel.setAllowsOtherFileTypes(false);
            panel.setCanCreateDirectories(true);
            panel.setCanSelectHiddenExtension(false);
            panel.setExtensionHidden(false);
            panel.setDirectoryURL(Some(&directory));
            panel.setNameFieldStringValue(&NSString::from_str(&project_filename(suggested_name)));
            Ok((panel, parent))
        })();
        match configured {
            Ok((panel, parent)) => present_sheet(panel, parent, sender),
            Err(error) => {
                let _ = sender.send(Err(error));
            }
        }
        receiver
    }

    fn project_filename(suggested_name: &str) -> String {
        let name = Path::new(suggested_name)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("Untitled");
        if Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("voice"))
        {
            name.to_owned()
        } else {
            format!("{name}.voice")
        }
    }

    pub fn update_document_state(window: &Window, path: Option<&Path>, edited: bool) {
        let Ok(window) = native_window(window) else {
            return;
        };
        let url = path.and_then(NSURL::from_file_path);
        window.setRepresentedURL(url.as_deref());
        window.setDocumentEdited(edited);
    }

    pub fn note_recent_document(path: &Path) {
        let (Some(mtm), Some(url)) = (MainThreadMarker::new(), NSURL::from_file_path(path)) else {
            return;
        };
        NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url);
    }

    pub fn recent_documents() -> Vec<PathBuf> {
        let Some(mtm) = MainThreadMarker::new() else {
            return Vec::new();
        };
        NSDocumentController::sharedDocumentController(mtm)
            .recentDocumentURLs()
            .iter()
            .filter_map(|url| url.to_file_path())
            .collect()
    }

    pub fn show_about_panel() {
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm).orderFrontStandardAboutPanel(None);
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(target_os = "macos"))]
mod unsupported {
    use super::*;

    pub fn microphone_authorization() -> MicrophoneAuthorization {
        MicrophoneAuthorization::Restricted
    }

    pub fn request_microphone_authorization() -> oneshot::Receiver<bool> {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(false);
        receiver
    }

    fn unavailable() -> FileDialogResult {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(Err(anyhow::anyhow!("Native document sheets require macOS")));
        receiver
    }

    pub fn open_audio_dialog(_: &Window, _: &str) -> FileDialogResult {
        unavailable()
    }

    pub fn open_project_dialog(_: &Window) -> FileDialogResult {
        unavailable()
    }

    pub fn save_project_dialog(_: &Window, _: &Path, _: &str) -> FileDialogResult {
        unavailable()
    }

    pub fn update_document_state(_: &Window, _: Option<&Path>, _: bool) {}

    pub fn note_recent_document(_: &Path) {}

    pub fn recent_documents() -> Vec<PathBuf> {
        Vec::new()
    }

    pub fn show_about_panel() {}

    pub fn install_termination_guard() {}

    pub fn take_termination_request() -> bool {
        false
    }

    pub fn allow_termination() {}
}

#[cfg(not(target_os = "macos"))]
pub use unsupported::*;
