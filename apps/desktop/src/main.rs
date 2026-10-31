//! Kestrel's desktop shell (v4 A3, blueprint §4.10): the running service's page in a window, a tray
//! icon, start at login, one instance, and reminders as notifications from `GET /api/events`.
//! It finds the service through `kestrel open --print`; starting the service itself (the sidecar,
//! `kestrel serve --app`) comes with onboarding.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod link;

use std::io::{Read, Write};
use std::time::Duration;

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_notification::NotificationExt as _;

/// How often the shell asks the service for events.
const POLL: Duration = Duration::from_secs(5);
/// Started at login: the tray icon only, no window.
const HIDDEN: &str = "--hidden";

/// The service's page, from `kestrel open --print` (the `kestrel` beside this app, else on PATH).
fn find_page() -> Option<link::Page> {
    let name = if cfg!(windows) {
        "kestrel.exe"
    } else {
        "kestrel"
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file());
    let out = std::process::Command::new(beside.unwrap_or_else(|| name.into()))
        .args(["open", "--print"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    link::page(&String::from_utf8_lossy(&out.stdout))
}

/// One ask for the events after `after`, on 127.0.0.1 only: None when the service cannot be
/// reached, Some(None) when it answers without events (a service from before v4 A3).
#[allow(clippy::type_complexity)]
fn ask_events(page: &link::Page, after: u64) -> Option<Option<(Vec<(String, String)>, u64)>> {
    let at = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, page.port));
    let mut s = std::net::TcpStream::connect_timeout(&at, Duration::from_secs(2)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    s.write_all(link::events_request(page, after).as_bytes())
        .ok()?;
    let mut response = String::new();
    s.read_to_string(&mut response).ok()?;
    Some(link::body(&response).and_then(link::notifications))
}

/// Reminders as notifications, for as long as the app runs.
fn hear_events(app: AppHandle) {
    let (mut page, mut after) = (None, 0);
    loop {
        if page.is_none() {
            page = find_page();
        }
        match page.as_ref().map(|p| ask_events(p, after)) {
            Some(Some(Some((shown, next)))) => {
                for (title, body) in shown {
                    let note = app.notification().builder().title(title).body(body);
                    if let Err(e) = note.show() {
                        eprintln!("a notification could not be shown: {e}");
                    }
                }
                after = next;
            }
            Some(Some(None)) => {}
            // Not running (any more): look for it again next time.
            _ => page = None,
        }
        std::thread::sleep(POLL);
    }
}

/// Brings the window forward: the service's page when it runs, else the shell's own page that says
/// how to start it.
fn show(app: &AppHandle) {
    let page = find_page();
    if let Some(w) = app.get_webview_window("main") {
        let on_page = w.url().is_ok_and(|u| u.host_str() == Some("127.0.0.1"));
        if on_page || page.is_none() {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }
        // The service started since: a new window, allowed to go to its page.
        let _ = w.destroy();
    }
    let port = page.as_ref().map_or(0, |p| p.port);
    let url = match page.map(|p| p.url().parse()) {
        Some(Ok(u)) => WebviewUrl::External(u),
        _ => WebviewUrl::App("index.html".into()),
    };
    let built = WebviewWindowBuilder::new(app, "main", url)
        .title("Kestrel")
        .inner_size(1000.0, 760.0)
        .on_navigation(move |u| link::may_open(u.as_str(), port))
        .build();
    if let Err(e) = built {
        eprintln!("the window could not open: {e}");
    }
}

fn tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Kestrel", true, None::<&str>)?;
    let at_login = app.autolaunch().is_enabled().unwrap_or(false);
    let login =
        CheckMenuItem::with_id(app, "login", "Start at login", true, at_login, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Kestrel", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &login, &quit])?;
    let mut tray = TrayIconBuilder::with_id("kestrel")
        .tooltip("Kestrel")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show(app),
            "login" => {
                let auto = app.autolaunch();
                let done = if auto.is_enabled().unwrap_or(false) {
                    auto.disable()
                } else {
                    auto.enable()
                };
                if let Err(e) = done {
                    eprintln!("start at login could not be changed: {e}");
                }
                let _ = login.set_checked(auto.is_enabled().unwrap_or(false));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn main() {
    let hidden = std::env::args().any(|a| a == HIDDEN);
    tauri::Builder::default()
        // First, so a second start only brings this one forward.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show(app)))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN]),
        ))
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            tray(app)?;
            if !hidden {
                show(app.handle());
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || hear_events(handle));
            Ok(())
        })
        // Closing the window keeps Kestrel in the tray; Quit ends it.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("Kestrel could not start")
        .run(|_, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event
                && code.is_none()
            {
                api.prevent_exit();
            }
        });
}
