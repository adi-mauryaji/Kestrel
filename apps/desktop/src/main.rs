//! Kestrel's desktop shell (v4 A3, blueprint §4.10): the service's page in a window, a tray icon,
//! start at login, one instance, and reminders as notifications from `GET /api/events`.
//!
//! The service: a running one is used (on Linux `kestrel open` also starts the installed systemd
//! one); else the app starts its own, `kestrel serve --app`, from Kestrel's files (`KESTREL_HOME`,
//! else the app's resources) with the notes folder saved by `kestrel settings notes DIR`, and
//! stops it when the app quits. A service it did not start is never stopped.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod link;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_notification::NotificationExt as _;

/// How often the shell asks the service for events.
const POLL: Duration = Duration::from_secs(5);
/// How long the app's own service may take to show its page.
const START_WAIT: Duration = Duration::from_secs(60);
/// Started at login: the service and the tray icon, no window.
const HIDDEN: &str = "--hidden";
/// `kestrel-desktop --quit`: the running app quits (and stops its own service).
const QUIT: &str = "--quit";
/// The service's config, under Kestrel's files.
const CONFIG: &str = "experiments/configs/router_intents_v0.json";

/// The service this app started, if any.
#[derive(Default)]
struct Own(Mutex<Option<Child>>);

/// The `kestrel` program: beside this app (where the installers put it), else on PATH.
fn kestrel() -> Command {
    let name = if cfg!(windows) {
        "kestrel.exe"
    } else {
        "kestrel"
    };
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file());
    Command::new(beside.unwrap_or_else(|| name.into()))
}

/// The running service's page, if it answers; `start`: on Linux a stopped installed service is
/// started first (`kestrel open`), else never (`--no-start`, so `kestrel stop` stays stopped).
fn find_page(start: bool) -> Option<link::Page> {
    let mut c = kestrel();
    c.args(["open", "--print"]);
    if !start {
        c.arg("--no-start");
    }
    let out = c.stderr(Stdio::null()).output().ok()?;
    if !out.status.success() {
        return None;
    }
    // The address file outlives a service that was killed: only a page that answers counts.
    link::page(&String::from_utf8_lossy(&out.stdout)).filter(|p| connect(p).is_some())
}

fn connect(page: &link::Page) -> Option<std::net::TcpStream> {
    let at = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, page.port));
    std::net::TcpStream::connect_timeout(&at, Duration::from_secs(2)).ok()
}

/// The service's page, starting the app's own service when none runs. None when it cannot start
/// (no notes folder saved yet, or Kestrel's files are missing: its log says why).
fn service_page(app: &AppHandle) -> Option<link::Page> {
    if let Some(page) = find_page(true) {
        return Some(page);
    }
    let own = app.state::<Own>();
    {
        let mut own = own.0.lock().ok()?;
        let running = own
            .as_mut()
            .is_some_and(|c| c.try_wait().is_ok_and(|e| e.is_none()));
        if !running {
            *own = Some(start_service(app)?);
        }
    }
    let until = Instant::now() + START_WAIT;
    while Instant::now() < until {
        if let Some(page) = find_page(false) {
            return Some(page);
        }
        let exited = own
            .0
            .lock()
            .ok()?
            .as_mut()
            .is_none_or(|c| c.try_wait().is_ok_and(|e| e.is_some()));
        if exited {
            return None;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    None
}

/// `kestrel serve --app` from Kestrel's files, its output in the app's log folder.
fn start_service(app: &AppHandle) -> Option<Child> {
    let home = std::env::var_os("KESTREL_HOME")
        .map(PathBuf::from)
        .or_else(|| app.path().resource_dir().ok())?;
    let logs = app.path().app_log_dir().ok()?;
    std::fs::create_dir_all(&logs).ok()?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("service.log"))
        .ok()?;
    let started = kestrel()
        // Any free port: 7811 may be another program's (or another Kestrel's).
        .args(["serve", "--app", "--web-port", "0", CONFIG])
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(log.try_clone().ok()?)
        .stderr(log)
        .spawn();
    match started {
        Ok(child) => Some(child),
        Err(e) => {
            eprintln!("Kestrel could not be started: {e}");
            None
        }
    }
}

/// Stops the service this app started: asked to end first (Unix), ended after 5 s.
fn stop_service(app: &AppHandle) {
    let Some(mut child) = app.state::<Own>().0.lock().ok().and_then(|mut o| o.take()) else {
        return;
    };
    if child.try_wait().is_ok_and(|e| e.is_some()) {
        return;
    }
    if cfg!(unix) {
        let _ = Command::new("kill").arg(child.id().to_string()).status();
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until && child.try_wait().is_ok_and(|e| e.is_none()) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// One ask for the events after `after`, on 127.0.0.1 only: None when the service cannot be
/// reached, Some(None) when it answers without events (a service from before v4 A3).
#[allow(clippy::type_complexity)]
fn ask_events(page: &link::Page, after: u64) -> Option<Option<(Vec<(String, String)>, u64)>> {
    let mut s = connect(page)?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok()?;
    s.write_all(link::events_request(page, after).as_bytes())
        .ok()?;
    let mut response = String::new();
    s.read_to_string(&mut response).ok()?;
    Some(link::body(&response).and_then(link::notifications))
}

/// Reminders as notifications, for as long as the app runs. It never starts a service.
fn hear_events(app: AppHandle) {
    let (mut page, mut after) = (None, 0);
    loop {
        if page.is_none() {
            page = find_page(false);
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

/// Brings the window forward, off the main thread (a start takes a moment): the service's page,
/// or the shell's own page that says how to start it.
fn show(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let page = service_page(&app);
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
        let built = WebviewWindowBuilder::new(&app, "main", url)
            .title("Kestrel")
            .inner_size(1000.0, 760.0)
            .on_navigation(move |u| {
                let ok = link::may_open(u.as_str(), port);
                if !ok {
                    eprintln!("the window may not go to {u}");
                }
                ok
            })
            .build();
        if let Err(e) = built {
            eprintln!("the window could not open: {e}");
        }
    });
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
        // First, so a second start only brings this one forward (or quits it).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            if argv.iter().any(|a| a == QUIT) {
                app.exit(0);
            } else {
                show(app);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN]),
        ))
        .plugin(tauri_plugin_notification::init())
        .manage(Own::default())
        .setup(move |app| {
            tray(app)?;
            if hidden {
                // At login: the service for reminders, no window.
                let handle = app.handle().clone();
                std::thread::spawn(move || service_page(&handle));
            } else {
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
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } if code.is_none() => {
                api.prevent_exit();
            }
            tauri::RunEvent::Exit => stop_service(app),
            _ => {}
        });
}
