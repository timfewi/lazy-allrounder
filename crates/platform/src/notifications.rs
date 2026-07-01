//! Desktop notifications — the feedback channel for tray mode, where there
//! is no badge to animate.

/// Best-effort desktop notification; failures are logged, never fatal, since
/// a missing notification daemon should not break the action itself.
pub fn notify(summary: &str, body: &str) {
    let result = notify_rust::Notification::new()
        .appname("Lazy Allrounder")
        .summary(summary)
        .body(body)
        .show();

    if let Err(error) = result {
        tracing::warn!("could not show a desktop notification: {error}");
    }
}
