use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};
pub fn install(app: &tauri::App) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "necko7 CS2 Integration", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, "exit", "Exit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&status, &PredefinedMenuItem::separator(app)?, &open, &exit],
    )?;
    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("necko7 CS2 Integration")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => crate::app::show(app),
            "exit" => {
                crate::app::shutdown(app);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                crate::app::show(tray.app_handle());
            }
        })
        .build(app)?;
    let state = app.state::<crate::app::Shared>().inner().clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! { _ = state.cancel.cancelled() => break, _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {} }
            let inner = state.inner.lock().unwrap();
            let text = inner
                .settings
                .pairing
                .as_ref()
                .map(|p| format!("@{} — {}", p.channel.username, inner.cloud))
                .unwrap_or_else(|| "Unpaired".into());
            let _ = status.set_text(text);
        }
    });
    Ok(())
}
