//! Printing on Linux. WebKitGTK ignores CSS `@page` margins, so documents would
//! print against the edge of the paper; print jobs get real paper margins
//! instead, matching the PDF templates' usual ones.
use gtk::prelude::*;
use webkit2gtk::{PrintOperationExt, WebViewExt};
use wry::WebViewExtUnix;

/// Top, right, bottom, and left margins in millimetres.
const MARGINS: [f64; 4] = [18.0, 17.0, 20.0, 17.0];

pub fn install(webview: &wry::WebView) {
    webview.webview().connect_print(|view, operation| {
        let setup = operation.page_setup().unwrap_or_default();
        setup.set_top_margin(MARGINS[0], gtk::Unit::Mm);
        setup.set_right_margin(MARGINS[1], gtk::Unit::Mm);
        setup.set_bottom_margin(MARGINS[2], gtk::Unit::Mm);
        setup.set_left_margin(MARGINS[3], gtk::Unit::Mm);
        operation.set_page_setup(&setup);
        let window = view
            .toplevel()
            .and_then(|w| w.downcast::<gtk::Window>().ok());
        operation.run_dialog(window.as_ref());
        true
    });
}
