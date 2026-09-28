mod app;
mod cloud;
mod deep_link;
mod gsi;
mod gsi_config;
mod heartbeat;
mod identity;
mod lifecycle;
mod settings;
mod steam;
mod tray;
use tauri::Manager;
use tauri_plugin_deep_link::DeepLinkExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();
    let cleanup = std::env::args().any(|arg| arg == "--uninstall-cleanup");
    let mut builder = tauri::Builder::default();
    if !cleanup {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            app::show(app)
        }));
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--autostart"])
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            app::status,
            app::pair,
            app::unpair,
            app::preferences,
            app::retry_discovery,
            app::reset_identity,
            app::open_dashboard,
            app::open_channel
        ])
        .setup(move |application| {
            let dir = app::development_dir().unwrap_or(application.path().app_data_dir()?);
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("settings.json");
            if cleanup {
                use tauri_plugin_autostart::ManagerExt;
                let _ = application.autolaunch().disable();
                if let Ok(settings) = settings::Settings::load(&path) {
                    if let Some(path) = settings.config_path {
                        gsi_config::remove_owned(&path, &settings.local_token, 31337);
                    }
                }
                application.handle().exit(0);
                return Ok(());
            }
            let (state, rx) = app::App::new(path).map_err(std::io::Error::other)?;
            application.manage(state.clone());
            tray::install(application)?;
            let discovery = state.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let _ = discovery.discover();
            });
            tauri::async_runtime::spawn(state.tasks.track_future(heartbeat::run(state.clone())));
            tauri::async_runtime::spawn(state.tasks.track_future(gsi::serve(state.clone())));
            tauri::async_runtime::spawn(state.tasks.track_future(gsi::forward(state.clone(), rx)));
            let handle = application.handle().clone();
            application.deep_link().on_open_url(move |event| {
                for url in event.urls() {
                    app::deep_link(&handle, url.as_str());
                }
            });
            if let Ok(Some(urls)) = application.deep_link().get_current() {
                for url in urls {
                    app::deep_link(application.handle(), url.as_str());
                }
            }
            if !std::env::args().any(|arg| arg == "--autostart") {
                app::show(application.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Focused(true) = event {
                if let Some(state) = window.app_handle().try_state::<app::Shared>() {
                    state.heartbeat_requested.notify_one();
                }
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Cancel destruction synchronously, before any lock or deferred work.
                api.prevent_close();
                if let Some(state) = window.app_handle().try_state::<app::Shared>() {
                    let minimize = state.inner.lock().unwrap().settings.minimize_to_tray;
                    match lifecycle::close_action(
                        minimize,
                        state.quitting.load(std::sync::atomic::Ordering::SeqCst),
                    ) {
                        lifecycle::CloseAction::Hide => {
                            // Windows visibility changes may synchronously emit window events.
                            // Leave the close callback/listener locks before calling hide.
                            let window = window.clone();
                            tauri::async_runtime::spawn(async move {
                                let _ = window.hide();
                            });
                        }
                        lifecycle::CloseAction::Shutdown => app::shutdown(window.app_handle()),
                        lifecycle::CloseAction::Quitting => {}
                    }
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("Cannot initialize desktop application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Resumed) {
                if let Some(state) = app.try_state::<app::Shared>() {
                    state.heartbeat_requested.notify_one();
                }
            }
            if matches!(event, tauri::RunEvent::Exit) {
                if let Some(state) = app.try_state::<app::Shared>() {
                    state.cancel.cancel();
                }
            }
        });
}
