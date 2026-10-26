//! Window chrome. Linux uses a single GTK header bar: an application menu on the
//! left, the page title, and the window buttons. Windows keeps a native menu bar,
//! and macOS the menu bar at the top of the screen.
use crate::UserEvent;
use tao::event_loop::EventLoopProxy;
use tao::window::Window;

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Servers,
    Reload,
    Quit,
}

/// Keeps platform menu objects alive for the lifetime of the window.
pub struct Chrome {
    #[cfg(any(windows, target_os = "macos"))]
    _menu: muda::Menu,
}

#[cfg(target_os = "linux")]
pub fn install(
    window: &Window,
    proxy: EventLoopProxy<UserEvent>,
) -> Result<Chrome, Box<dyn std::error::Error>> {
    use gtk::prelude::*;
    use tao::platform::unix::WindowExtUnix;

    let gtk_window = window.gtk_window();
    let accelerators = gtk::AccelGroup::new();
    gtk_window.add_accel_group(&accelerators);
    let menu = gtk::Menu::new();
    menu.set_accel_group(Some(&accelerators));
    let items = [
        ("Servers…", Command::Servers, "<Primary><Shift>o"),
        ("Reload", Command::Reload, "<Primary>r"),
        ("Quit", Command::Quit, "<Primary>q"),
    ];
    for (index, (label, command, accelerator)) in items.into_iter().enumerate() {
        if index == 2 {
            menu.append(&gtk::SeparatorMenuItem::new());
        }
        let item = gtk::MenuItem::with_label(label);
        let (key, modifiers) = gtk::accelerator_parse(accelerator);
        item.add_accelerator(
            "activate",
            &accelerators,
            key,
            modifiers,
            gtk::AccelFlags::VISIBLE,
        );
        let proxy = proxy.clone();
        item.connect_activate(move |_| {
            let _ = proxy.send_event(UserEvent::Command(command));
        });
        menu.append(&item);
    }
    menu.show_all();

    let content = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    content.add(&gtk::Label::new(Some("Minerva")));
    content.add(&gtk::Image::from_icon_name(
        Some("pan-down-symbolic"),
        gtk::IconSize::Button,
    ));
    let button = gtk::MenuButton::builder()
        .child(&content)
        .tooltip_text("Servers and application")
        .popup(&menu)
        .build();
    button.style_context().add_class("flat");
    let header = gtk::HeaderBar::new();
    header.set_show_close_button(true);
    header.pack_start(&button);
    // The header bar has no input window of its own, so its presses go to the
    // window, where the window library stops them before GTK can move the
    // window. An event box gives the bar its own input window: drag it to move,
    // double-click to maximize. The menu and window buttons have their own
    // input windows and still get their clicks.
    let bar = gtk::EventBox::new();
    bar.add(&header);
    bar.set_above_child(false);
    let target = gtk_window.clone();
    bar.connect_button_press_event(move |_, event| {
        if event.button() != 1 {
            return gtk::glib::Propagation::Proceed;
        }
        if event.event_type() == gtk::gdk::EventType::DoubleButtonPress {
            if target.is_maximized() {
                target.unmaximize();
            } else {
                target.maximize();
            }
        } else if event.event_type() == gtk::gdk::EventType::ButtonPress {
            let (x, y) = event.root();
            target.begin_move_drag(1, x as i32, y as i32, event.time());
        }
        gtk::glib::Propagation::Stop
    });
    bar.show_all();
    // Must happen before the window is first shown.
    gtk_window.set_titlebar(Some(&bar));
    window.set_visible(true);
    Ok(Chrome {})
}

#[cfg(windows)]
pub fn install(
    window: &Window,
    proxy: EventLoopProxy<UserEvent>,
) -> Result<Chrome, Box<dyn std::error::Error>> {
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
    use tao::platform::windows::WindowExtWindows;

    let shortcut = |modifiers, code| Some(Accelerator::new(Some(modifiers), code));
    let app = Submenu::with_items(
        "&Minerva",
        true,
        &[
            &MenuItem::with_id(
                "servers",
                "&Servers…",
                true,
                shortcut(Modifiers::CONTROL | Modifiers::SHIFT, Code::KeyO),
            ),
            &MenuItem::with_id(
                "reload",
                "&Reload",
                true,
                shortcut(Modifiers::CONTROL, Code::KeyR),
            ),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                "quit",
                "&Quit",
                true,
                shortcut(Modifiers::CONTROL, Code::KeyQ),
            ),
        ],
    )?;
    let menu = Menu::with_items(&[&app])?;
    // SAFETY: the HWND belongs to `window`, which outlives the menu.
    unsafe { menu.init_for_hwnd(window.hwnd() as _)? };
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let command = match e.id.as_ref() {
            "servers" => Command::Servers,
            "reload" => Command::Reload,
            "quit" => Command::Quit,
            _ => return,
        };
        let _ = proxy.send_event(UserEvent::Command(command));
    }));
    window.set_visible(true);
    Ok(Chrome { _menu: menu })
}

/// The application menu, plus Edit and Window menus: without them macOS has no
/// copy, paste or undo shortcuts in the webview.
#[cfg(target_os = "macos")]
pub fn install(
    window: &Window,
    proxy: EventLoopProxy<UserEvent>,
) -> Result<Chrome, Box<dyn std::error::Error>> {
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    let shortcut = |modifiers, code| Some(Accelerator::new(Some(modifiers), code));
    let app = Submenu::with_items(
        "Minerva",
        true,
        &[
            &PredefinedMenuItem::about(None, None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                "servers",
                "Servers…",
                true,
                shortcut(Modifiers::SUPER | Modifiers::SHIFT, Code::KeyO),
            ),
            &MenuItem::with_id(
                "reload",
                "Reload",
                true,
                shortcut(Modifiers::SUPER, Code::KeyR),
            ),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &MenuItem::with_id(
                "quit",
                "Quit Minerva",
                true,
                shortcut(Modifiers::SUPER, Code::KeyQ),
            ),
        ],
    )?;
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )?;
    let windows = Submenu::with_items(
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::fullscreen(None),
            &PredefinedMenuItem::close_window(None),
        ],
    )?;
    let menu = Menu::with_items(&[&app, &edit, &windows])?;
    menu.init_for_nsapp();
    windows.set_as_windows_menu_for_nsapp();
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        let command = match e.id.as_ref() {
            "servers" => Command::Servers,
            "reload" => Command::Reload,
            "quit" => Command::Quit,
            _ => return,
        };
        let _ = proxy.send_event(UserEvent::Command(command));
    }));
    window.set_visible(true);
    Ok(Chrome { _menu: menu })
}
