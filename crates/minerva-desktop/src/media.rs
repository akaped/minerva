//! Microphone access on Linux, for recording audio into notes. WebKitGTK turns
//! media capture off and refuses permission requests nobody answers, so pages
//! asking for the microphone are asked about here: a native dialog names the
//! page, and an allowed site is not asked again until the app restarts. The
//! camera and every other permission stay refused.
use gtk::prelude::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use webkit2gtk::{
    PermissionRequestExt, SettingsExt, UserMediaPermissionRequest, UserMediaPermissionRequestExt,
    WebViewExt,
};
use wry::WebViewExtUnix;

pub fn install(webview: &wry::WebView) {
    let view = webview.webview();
    if let Some(settings) = WebViewExt::settings(&view) {
        settings.set_enable_media_stream(true);
    }
    let allowed: Rc<RefCell<HashSet<String>>> = Rc::default();
    view.connect_permission_request(move |view, request| {
        let Some(media) = request.downcast_ref::<UserMediaPermissionRequest>() else {
            return false;
        };
        if !media.is_for_audio_device() || media.is_for_video_device() {
            request.deny();
            return true;
        }
        let site = view
            .uri()
            .and_then(|u| url::Url::parse(&u).ok())
            .map(|u| u.origin().ascii_serialization())
            .unwrap_or_default();
        if allowed.borrow().contains(&site) {
            request.allow();
            return true;
        }
        let window = view
            .toplevel()
            .and_then(|w| w.downcast::<gtk::Window>().ok());
        let dialog = gtk::MessageDialog::new(
            window.as_ref(),
            gtk::DialogFlags::MODAL | gtk::DialogFlags::DESTROY_WITH_PARENT,
            gtk::MessageType::Question,
            gtk::ButtonsType::None,
            "Allow Minerva to use your microphone?",
        );
        dialog.set_secondary_text(Some(&format!(
            "{} wants to record audio. You can change your mind the next time Minerva starts.",
            if site.is_empty() { "This page" } else { &site }
        )));
        dialog.add_button("Don't allow", gtk::ResponseType::Reject);
        dialog.add_button("Allow", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Reject);
        let answer = dialog.run();
        dialog.close();
        if answer == gtk::ResponseType::Accept {
            allowed.borrow_mut().insert(site);
            request.allow();
        } else {
            request.deny();
        }
        true
    });
}
