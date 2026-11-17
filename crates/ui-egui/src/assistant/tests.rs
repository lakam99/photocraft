use super::*;
use crate::Services;

fn new_app() -> PhotocraftApp {
    PhotocraftApp::new(photocraft_engine::Session::new(), Services::default())
}
fn command(method: &str, params: Value) -> ToolCall {
    ToolCall::new("photocraft", json!({"method":method,"params_json":params.to_string()}))
}

#[test]
fn shared_plugin_contract_matches_preferences_and_rejects_private_targets() {
    info().validate().unwrap();
    let mut app = new_app();
    assert_eq!(Adapter { app: &mut app }.preferences(), Preferences::default());
    for call in [
        command("engine.execute", json!({"command":"prefs.set","params":{"path":"ai.model","value":"bad"}})),
        command("engine.execute", json!({"command":null,"id":"prefs.set","params":{"path":"ai.model","value":"bad"}})),
        command("engine.execute", json!({"command":"prefs.reset","params":{}})),
        command("engine.execute", json!({"command":"prefs.reset","params":{"section":"ai"}})),
        command("engine.execute", json!({"command":"prefs.set","params":{"values":{"ai.model":"bad"}}})),
        command("ui.menu.invoke", json!({"id":"edit.preferences.general"})),
        command("ui.menu.invoke", json!({"id":"edit.preferences.ai"})),
        command("ui.dialog.open", json!({"kind":"command","command":"prefs.set"})),
        command("ui.dialog.open", json!({"kind":"command","command":"prefs.reset"})),
        command("ui.dialog.open", json!({"kind":"command","command":"edit.preferences.ai"})),
        command("engine.execute", json!({"command":"ai.send"})),
        command("app.quit", json!({})),
        ToolCall::new("video_frame", json!({"monitor":"Desktop"})),
        ToolCall::new("photocraft", json!({"method":"engine.execute","params_json":"[]"})),
    ] {
        assert!(tools::start(&mut app, &call).is_err(), "{}", call.arguments);
    }
    assert!(tools::start(&mut app, &ToolCall::new("video_frame", json!({"monitor":"Canvas"}))).is_err());
    assert!(tools::start(&mut app, &command("engine.execute", json!({"command":"prefs.set","params":{"path":"interface.language","value":"en-US"}}))).is_ok());
}

#[test]
fn generic_preferences_dialog_cannot_bypass_the_private_settings_guard() {
    let mut app = new_app();
    let dialog = crate::dialogs::open_command_dialog(&mut app, "prefs.set", "Preferences");
    let fields = &mut app.ui.dialog_mut(dialog).unwrap().fields;
    fields.insert("path".into(), json!("ai.model"));
    fields.insert("value".into(), json!("unwanted-change"));
    for call in [
        command("ui.dialog.confirm", json!({"dialog":dialog})),
        command("ui.dialog.set", json!({"dialog":dialog,"field":"value","value":"unwanted-change"})),
        command("ui.key", json!({"key":"Enter"})),
    ] {
        assert!(tools::start(&mut app, &call).is_err());
    }
    assert_eq!(app.session.prefs().ai, photocraft_engine::prefs::Ai::default());
}

#[test]
fn preferences_guard_is_rechecked_before_dispatch() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    let mut pending = tools::start(&mut app, &command("ui.key", json!({"key":"Escape"}))).unwrap();
    route(&mut app, &ctx, "ai.settings", &json!({})).unwrap().unwrap();
    assert!(tools::poll(&mut app, &mut pending).is_err());
    assert!(tools::start(&mut app, &command("ui.type", json!({"text":"unwanted input"}))).is_err());
    let dialog = app.ui.dialogs.last().unwrap().id;
    assert!(tools::start(&mut app, &command("ui.dialog.apply", json!({"dialog":dialog}))).is_err());
    assert_eq!(app.session.prefs().ai, photocraft_engine::prefs::Ai::default());
}

#[test]
fn chat_opens_in_its_own_column_and_restores_a_user_tab_placement() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    let panel = "aiChat".to_owned();
    route(&mut app, &ctx, "window.panel.aiChat", &json!({})).unwrap().unwrap();
    let location = app.ui.docking.as_ref().unwrap().layout.location(&panel).unwrap();
    assert!(matches!(location.placement, craft_ui::docking::Placement::Column { .. }), "{location:?}");
    app.run("window.panel.move", json!({"panel":panel,"anchor":"color"})).unwrap();
    app.run("window.panel.close", json!({"panel":panel})).unwrap();
    route(&mut app, &ctx, "window.panel.aiChat", &json!({})).unwrap().unwrap();
    let location = app.ui.docking.as_ref().unwrap().layout.location(&panel).unwrap();
    assert!(matches!(location.placement, craft_ui::docking::Placement::Tab { .. }), "{location:?}");
}

#[test]
fn window_menu_toggles_chat_visibility() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    for shown in [true, false, true] {
        route(&mut app, &ctx, "window.panel.aiChat", &json!({})).unwrap().unwrap();
        assert_eq!(app.ui.panels.ai_chat, shown);
    }
}

#[test]
fn dock_menu_settings_and_private_prompt_are_drivable() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    route(&mut app, &ctx, "window.panel.aiChat", &json!({})).unwrap().unwrap();
    assert!(app.ui.panels.ai_chat);
    let saved = crate::dock::snapshot(&app);
    let mut reopened = new_app();
    crate::dock::apply(&mut reopened, &saved);
    assert!(reopened.ui.panels.ai_chat);
    route(&mut app, &ctx, "ai.prompt", &json!({"text":"private draft"})).unwrap().unwrap();
    assert!(!serde_json::to_string(&app.ui).unwrap().contains("private draft"));
    assert!(!app.session.prefs_value().to_string().contains("private draft"));
    assert!(route(&mut app, &ctx, "ai.prompt", &json!({"text":7})).unwrap().is_err());
    assert!(route(&mut app, &ctx, "ai.prompt", &json!({"text":"x".repeat(32769)})).unwrap().is_err());
    route(&mut app, &ctx, "ai.settings", &json!({})).unwrap().unwrap();
    assert!(app.ui.dialogs.iter().any(|d| d.fields.get("section") == Some(&json!("ai"))));
    crate::menus::invoke(&mut reopened, &ctx, "edit.preferences.ai", json!({})).unwrap();
    assert!(reopened.ui.dialogs.iter().any(|d| d.fields.get("section") == Some(&json!("ai"))));
}

#[test]
fn tools_use_editor_dispatch_and_existing_undo() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    app.frame_tool_ctx = Some(ctx);
    let mut create = tools::start(&mut app, &command("engine.execute", json!({"command":"file.new","params":{"width":32,"height":24}}))).unwrap();
    let reply = tools::poll(&mut app, &mut create).unwrap().unwrap();
    let reply: Value = serde_json::from_str(reply.as_str().unwrap()).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(app.session.active().is_some());
    let mut edit = tools::start(&mut app, &command("engine.execute", json!({"command":"layer.new.layer","params":{"name":"Assistant layer"}}))).unwrap();
    let reply = tools::poll(&mut app, &mut edit).unwrap().unwrap();
    let reply: Value = serde_json::from_str(reply.as_str().unwrap()).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    assert!(app.session.active().unwrap().doc.layers.iter().any(|l| l.name == "Assistant layer"));
    app.session.execute("edit.undo", json!({})).unwrap();
    assert!(!app.session.active().unwrap().doc.layers.iter().any(|l| l.name == "Assistant layer"));
}

#[test]
fn active_image_is_png_and_rejects_a_switched_document() {
    let mut app = new_app();
    app.session.execute("file.new", json!({"width":32,"height":24})).unwrap();
    let mut pending = tools::start(&mut app, &ToolCall::new("video_frame", json!({"monitor":"Canvas"}))).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let image = loop {
        if let Some(image) = tools::poll(&mut app, &mut pending).unwrap() {
            break image;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    use base64::Engine;
    let bytes =
        base64::engine::general_purpose::STANDARD.decode(image[1]["image_url"].as_str().unwrap().strip_prefix("data:image/png;base64,").unwrap()).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let mut second = tools::start(&mut app, &ToolCall::new("video_frame", json!({"monitor":"Canvas"}))).unwrap();
    app.session.execute("file.new", json!({"width":16,"height":16})).unwrap();
    assert!(tools::poll(&mut app, &mut second).is_err());
}

#[test]
fn mocked_chat_creates_a_document_and_finishes_without_network_or_credentials() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    *app.assistant.panel.prompt_mut() = "Create a small image".into();
    let mut turn = 0;
    with_panel(&mut app, |panel, host| panel.send_with(host, &ctx, move |body| {
        turn += 1;
        if turn == 1 {
            Ok(json!({"status":"completed","output":[{"type":"function_call","call_id":"new-image","name":"photocraft","arguments":json!({"method":"engine.execute","params_json":json!({"command":"file.new","params":{"width":48,"height":32}}).to_string()}).to_string()}]}))
        } else {
            assert!(body["input"].to_string().contains("function_call_output"));
            Ok(json!({"status":"completed","output":[{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Created the image."}]}]}))
        }
    })).unwrap().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while app.assistant.panel.busy() {
        poll(&mut app, &ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    assert_eq!(app.session.active().unwrap().doc.size.width, 48);
    assert!(app.assistant.panel.transcript().iter().any(|(_, text)| text == "Created the image."), "{:?}", app.assistant.panel.transcript());
}

fn pending_file_open(app: &mut PhotocraftApp, ctx: &egui::Context, path: &std::path::Path) -> tools::Pending {
    app.background_jobs = true;
    app.frame_tool_ctx = Some(ctx.clone());
    let image = photocraft_codecs::Image::from_u8(2, 2, photocraft_codecs::ChannelLayout::Rgba, vec![255; 16]).unwrap();
    let png = photocraft_codecs::encode(&image, photocraft_codecs::Format::Png, &photocraft_codecs::EncodeOptions::default()).unwrap();
    std::fs::write(path, png).unwrap();
    let mut pending = tools::start(app, &command("ui.menu.invoke", json!({"id":"file.open","params":{"path":path}}))).unwrap();
    assert!(tools::poll(app, &mut pending).unwrap().is_none(), "File Open must wait for the import result");
    assert_eq!(app.jobs.opens.len(), 1);
    pending
}

#[test]
fn background_open_retains_its_chat_and_waits_for_the_new_document() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    app.session.execute("file.new", json!({"width":16,"height":16})).unwrap();
    with_panel(&mut app, |_, _| ()).unwrap();
    let instance = app.assistant.instance.clone();
    let dir = tempfile::tempdir().unwrap();
    let mut pending = pending_file_open(&mut app, &ctx, &dir.path().join("open.png"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let result = loop {
        crate::jobs_ui::tick(&mut app, &ctx);
        with_panel(&mut app, |_, _| ()).unwrap();
        if let Some(result) = tools::poll(&mut app, &mut pending).unwrap() {
            break result;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(3));
    };
    let result: Value = serde_json::from_str(result.as_str().unwrap()).unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(app.session.active().unwrap().doc.size.width, 2);
    assert!(Arc::ptr_eq(&instance, &app.assistant.instance));
    let mut edit = tools::start(&mut app, &command("engine.execute", json!({"command":"layer.new.layer","params":{"name":"Opened image edit"}}))).unwrap();
    tools::poll(&mut app, &mut edit).unwrap().unwrap();
    assert!(app.session.active().unwrap().doc.layers.iter().any(|layer| layer.name == "Opened image edit"));
    assert!(app.session.documents()[0].doc.layers.iter().all(|layer| layer.name != "Opened image edit"));
}

#[test]
fn a_user_switch_during_background_open_still_invalidates_pending_edits() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    with_panel(&mut app, |_, _| ()).unwrap();
    let instance = app.assistant.instance.clone();
    let dir = tempfile::tempdir().unwrap();
    let _pending = pending_file_open(&mut app, &ctx, &dir.path().join("open.png"));
    app.session.execute("file.new", json!({"width":16,"height":16})).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !app.jobs.opens.is_empty() {
        crate::jobs_ui::tick(&mut app, &ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    with_panel(&mut app, |_, _| ()).unwrap();
    assert!(!Arc::ptr_eq(&instance, &app.assistant.instance));
}

#[test]
fn editor_tools_obey_the_automation_command_policy() {
    let mut app = new_app();
    app.frame_tool_ctx = Some(egui::Context::default());
    app.services.automation_command = Some(Box::new(|_, _| Err("policy denied".into())));
    let mut tool = tools::start(&mut app, &command("engine.execute", json!({"command":"file.new","params":{"width":32,"height":24}}))).unwrap();
    let result = tools::poll(&mut app, &mut tool).unwrap().unwrap();
    let result: Value = serde_json::from_str(result.as_str().unwrap()).unwrap();
    assert_eq!(result["ok"], false, "{result}");
    assert!(app.session.active().is_none());
    app.automation_input = true;
    assert!(crate::menus::invoke(&mut app, &egui::Context::default(), "window.panel.aiChat", json!({})).is_err());
    assert!(!app.ui.panels.ai_chat);
}

#[test]
fn a_user_document_switch_cancels_an_awaiting_edit() {
    let mut app = new_app();
    let ctx = egui::Context::default();
    app.session.execute("file.new", json!({"width":16,"height":16})).unwrap();
    app.session.edit_prefs(|p| p.ai.attach_frames = false);
    *app.assistant.panel.prompt_mut() = "Add a layer".into();
    let (release, wait) = std::sync::mpsc::channel();
    let (started, request_started) = std::sync::mpsc::channel();
    with_panel(&mut app, |panel, host| panel.send_with(host, &ctx, move |_| {
        started.send(()).unwrap();
        wait.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        Ok(json!({"status":"completed","output":[{"type":"function_call","call_id":"stale","name":"photocraft","arguments":json!({"method":"engine.execute","params_json":json!({"command":"layer.new.layer","params":{"name":"Stale edit"}}).to_string()}).to_string()}]}))
    })).unwrap().unwrap();
    request_started.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    app.session.execute("file.new", json!({"width":24,"height":24})).unwrap();
    poll(&mut app, &ctx);
    release.send(()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while app.assistant.panel.busy() {
        poll(&mut app, &ctx);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(3));
    }
    assert!(app.session.documents().iter().all(|st| st.doc.layers.iter().all(|layer| layer.name != "Stale edit")));
}
