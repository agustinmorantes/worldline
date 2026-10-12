//! The app's real capabilities and ACL manifest (`build.rs`): the game overlay may only use
//! events, `main` may call app commands. `delete_game` here is a stand-in with the real name, so
//! nothing is deleted; the ACL checks the name, the window and the origin.

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

#[tauri::command]
fn delete_game(id: i32) -> String {
    format!("deleted {id}")
}

#[tauri::command]
fn purge_deleted(uuid: String) -> String {
    format!("purged {uuid}")
}

fn invoke(window: &WebviewWindow<MockRuntime>, cmd: &str, body: Value) -> Result<Value, Value> {
    get_ipc_response(
        window,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            // The app's own origin, which differs by platform.
            url: if cfg!(windows) {
                "http://tauri.localhost"
            } else {
                "tauri://localhost"
            }
            .parse()
            .unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|b| b.deserialize().unwrap())
}

fn window(app: &tauri::App<MockRuntime>, label: &str) -> WebviewWindow<MockRuntime> {
    app.get_webview_window(label).unwrap_or_else(|| {
        WebviewWindowBuilder::new(app, label, WebviewUrl::default())
            .build()
            .unwrap()
    })
}

#[test]
fn only_main_may_call_app_commands() {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![delete_game, purge_deleted])
        .build(tauri::generate_context!())
        .unwrap();
    let main = window(&app, "main");
    let overlay = window(&app, "overlay");

    let denied = invoke(&overlay, "delete_game", json!({ "id": 7 })).unwrap_err();
    assert!(denied.to_string().contains("not allowed"), "{denied}");
    assert_eq!(
        invoke(&main, "delete_game", json!({ "id": 7 })),
        Ok(json!("deleted 7"))
    );
    // Ending a recovery window is the one delete that can't be undone.
    let purge = json!({ "uuid": "u" });
    assert!(invoke(&overlay, "purge_deleted", purge.clone()).is_err());
    assert_eq!(invoke(&main, "purge_deleted", purge), Ok(json!("purged u")));

    // The overlay's handshake: it listens for popups and says when it's ready and done.
    let listen = json!({ "event": "overlay-show", "target": { "kind": "Any" }, "handler": 2 });
    assert!(invoke(&overlay, "plugin:event|listen", listen).is_ok());
    let ready = json!({ "event": "overlay-ready", "payload": null });
    assert_eq!(
        invoke(&overlay, "plugin:event|emit", ready),
        Ok(json!(null))
    );
}

/// A stand-in with the real name: Name it's card in the app closing.
#[tauri::command]
fn name_it_closed(session: u64) -> bool {
    session == 3
}

#[test]
fn only_main_may_close_a_naming_card_and_the_overlay_does_it_with_events() {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![name_it_closed])
        .build(tauri::generate_context!())
        .unwrap();
    let main = window(&app, "main");
    let overlay = window(&app, "overlay");

    let body = json!({ "session": 3, "how": "save", "name": "Boss" });
    let denied = invoke(&overlay, "name_it_closed", body.clone()).unwrap_err();
    assert!(denied.to_string().contains("not allowed"), "{denied}");
    assert_eq!(invoke(&main, "name_it_closed", body), Ok(json!(true)));

    // The overlay's card hears it opens and loses the focus, and says it closed and landed.
    for event in ["naming-open", "naming-leave"] {
        let listen = json!({ "event": event, "target": { "kind": "Any" }, "handler": 2 });
        assert!(invoke(&overlay, "plugin:event|listen", listen).is_ok());
    }
    let closed = json!({
        "event": "naming-closed",
        "payload": { "session": 3, "how": "escape", "name": "" },
    });
    assert_eq!(
        invoke(&overlay, "plugin:event|emit", closed),
        Ok(json!(null))
    );
    let landed = json!({ "event": "naming-landed", "payload": 3 });
    assert_eq!(
        invoke(&overlay, "plugin:event|emit", landed),
        Ok(json!(null))
    );
}
