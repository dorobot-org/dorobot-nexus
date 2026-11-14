//! Opt-in, read-only observations for real-input UI acceptance tests.
//! No commands are accepted and no user data is sent over the network.
use makepad_widgets::*;

pub fn write(cx: &mut Cx, ui: &WidgetRef, state: serde_json::Value, paths: &[&str]) {
    let Some(path) = std::env::var_os("DOROBOT_UX_REPORT") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    let mut controls = serde_json::Map::new();
    for path in paths {
        let ids: Vec<_> = path.split('.').map(LiveId::from_str).collect();
        let widget = ui.widget(cx, &ids);
        let rect = widget.area().rect(cx);
        controls.insert(
            (*path).into(),
            serde_json::json!({
                "visible": widget.visible(),
                "rect": [rect.pos.x, rect.pos.y, rect.size.x, rect.size.y]
            }),
        );
    }
    let report = serde_json::json!({"state": state, "controls": controls});
    if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
        if std::fs::read(&path).is_ok_and(|old| old == bytes) {
            return;
        }
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        if std::fs::write(&temporary, bytes).is_ok() {
            let _ = std::fs::rename(temporary, path);
        }
    }
}
