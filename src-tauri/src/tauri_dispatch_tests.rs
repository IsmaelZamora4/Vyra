use super::*;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{
    Manager,
    ipc::{CallbackFn, InvokeBody},
    test::{INVOKE_KEY, mock_builder, mock_context, noop_assets},
    webview::{InvokeRequest, WebviewWindowBuilder},
};

fn isolated_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("vyra-tauri-ipc-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn invoke(
    window: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: command.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: "http://tauri.localhost".parse().unwrap(),
        body: InvokeBody::Json(body),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.into(),
    };
    tauri::test::get_ipc_response(window, request)
        .map(|response| response.deserialize::<Value>().unwrap())
}

#[test]
fn registered_tauri_ipc_routes_names_arguments_errors_and_closed_apply_gates() {
    let root = isolated_root();
    let app = mock_builder()
        .invoke_handler(app_invoke_handler!())
        .build(mock_context(noop_assets()))
        .unwrap();
    app.manage(ProfileService::open(root.join("profiles.json")).unwrap());
    app.manage(Mutex::new(ColorEngineManager::new(
        models::BackendPreference::Windows,
    )));
    app.manage(Mutex::new(diagnostic::DiagnosticService::new(
        root.join("diagnostic-events.jsonl"),
    )));
    let window = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let status = invoke(&window, "get_engine_status", json!({})).unwrap();
    assert_eq!(status["enabled"], false);

    let initial_diagnostic = invoke(&window, "get_diagnostic", json!({})).unwrap();
    assert_eq!(initial_diagnostic["stage"], "notStarted");
    let cancelled = invoke(&window, "cancel_diagnostic", json!({})).unwrap();
    assert_eq!(cancelled["stage"], "cancelled");
    for _ in 0..3 {
        let polled = invoke(&window, "get_diagnostic", json!({})).unwrap();
        assert_eq!(polled["stage"], "cancelled");
        assert_eq!(polled["applicationAccepted"], Value::Null);
    }

    // This traverses Tauri's registered command dispatcher and production guard.
    // ColorEngineManager::new is a fake engine in tests; no native API is constructed.
    let apply_error =
        invoke(&window, "set_filters_enabled", json!({ "enabled": true })).unwrap_err();
    assert!(
        apply_error
            .as_str()
            .unwrap()
            .contains("no se han validado escrituras")
    );
    assert_eq!(
        invoke(&window, "get_engine_status", json!({})).unwrap()["enabled"],
        false
    );

    // start_diagnostic is registered with the frontend's exact snake_case name and camelCase arg.
    let start_error = invoke(
        &window,
        "start_diagnostic",
        json!({ "attemptId": "ipc-test-attempt" }),
    )
    .unwrap_err();
    assert!(
        start_error
            .as_str()
            .unwrap()
            .contains("bloqueado por incidente")
    );

    // Wrong argument type and a frontend-style command name fail at Tauri dispatch.
    assert!(invoke(&window, "start_diagnostic", json!({ "attemptId": 42 }),).is_err());
    assert!(invoke(&window, "startDiagnostic", json!({ "attemptId": "x" })).is_err());

    drop(window);
    drop(app);
    fs::remove_dir_all(root).unwrap();
}
