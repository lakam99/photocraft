use super::{Arc, PhotocraftApp, ToolCall, Value, editor_tools, json};
use craft_ai_chat_egui::core::editor::EditorCall;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(crate) enum Pending {
    Editor { request: crate::ControlRequest, reply: mpsc::Receiver<Value>, waiting: bool, opening: Option<Arc<Opening>> },
    Image { generation: u64, revision: u64, receive: mpsc::Receiver<Result<Value, String>> },
}

pub(crate) struct Opening {
    job: photocraft_engine::jobs::JobId,
    generation: u64,
    documents: std::collections::BTreeSet<u64>,
}

impl Opening {
    pub(super) fn completed_switch(&self, app: &PhotocraftApp) -> bool {
        // Only this live tool's single new document may retain the conversation. A user switch
        // away and back also advances the generation and must cancel pending editing tools.
        app.session.active_generation() == self.generation.wrapping_add(1)
            && app.session.active().is_some_and(|st| {
                !self.documents.contains(&st.doc.id.0)
                    && app
                        .session
                        .jobs_with_recent()
                        .into_iter()
                        .find(|job| job.id == self.job)
                        .is_some_and(|job| job.state == "done" && job.result.as_ref().and_then(|v| v.get("docId")).and_then(Value::as_u64) == Some(st.doc.id.0))
            })
    }
}

struct Busy(Arc<AtomicBool>);
impl Drop for Busy {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn authorize(app: &PhotocraftApp, method: &str, params: &Value) -> Result<(), String> {
    let preferences_command = |id: &str| matches!(id, "prefs.set" | "prefs.reset") || id.starts_with("edit.preferences.");
    if method == "ui.dialog.open" && params.get("command").and_then(Value::as_str).is_some_and(preferences_command) {
        return Err("Use explicit preference commands for public settings; AI preferences are reserved for the user".into());
    }
    // Preferences dialogs contain the complete settings object, including AI settings. Do not
    // let synthetic input, generic command dialogs or Apply bypass the private-chat guard.
    if method.starts_with("ui.")
        && !matches!(method, "ui.inspect" | "ui.menu.list")
        && app.ui.dialogs.iter().any(|dialog| {
            dialog.fields.get("__prefsui").and_then(Value::as_str) == Some("prefs")
                || dialog.fields.get("__command").and_then(Value::as_str).is_some_and(preferences_command)
        })
    {
        return Err("Close Preferences before the assistant uses UI tools".into());
    }
    if matches!(method, "engine.execute" | "ui.menu.invoke") {
        let id = params.get("command").and_then(Value::as_str).or_else(|| params.get("id").and_then(Value::as_str)).unwrap_or("");
        let fields = params.get("params").unwrap_or(&Value::Null);
        let ai_path = |path: &str| path.is_empty() || path == "ai" || path.starts_with("ai.");
        let private = match id {
            "prefs.set" => {
                fields.get("path").and_then(Value::as_str).is_some_and(ai_path)
                    || fields.get("values").and_then(Value::as_object).is_some_and(|values| values.keys().any(|path| ai_path(path)))
            }
            "prefs.reset" => fields.get("path").or_else(|| fields.get("section")).and_then(Value::as_str).is_none_or(ai_path),
            _ => id.starts_with("edit.preferences."),
        };
        if private {
            return Err("AI preferences and the Preferences dialog are reserved for the user".into());
        }
    }
    Ok(())
}

pub(super) fn start(app: &mut PhotocraftApp, call: &ToolCall) -> Result<Pending, String> {
    match editor_tools().parse(&call.name, &call.arguments)? {
        EditorCall::Command { method, params } => {
            authorize(app, &method, &params)?;
            let (request, reply) = crate::ControlRequest::new(method, params);
            Ok(Pending::Editor { request, reply, waiting: false, opening: None })
        }
        EditorCall::View(_) => {
            let st = app.session.active().ok_or("No document is open")?;
            let (doc, revision, generation) = (st.doc.clone(), st.revision, app.session.active_generation());
            if doc.size.width == 0 || doc.size.height == 0 || doc.size.width > 300_000 || doc.size.height > 300_000 {
                return Err("The document dimensions exceed the image limit".into());
            }
            if app.assistant.image_busy.swap(true, Ordering::AcqRel) {
                return Err("An active-image capture is still running".into());
            }
            let busy = Busy(app.assistant.image_busy.clone());
            let (send, receive) = mpsc::channel();
            std::thread::Builder::new().name("photocraft-ai-image".into()).spawn(move || {
                let busy_guard = busy;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let image = photocraft_compose::thumbnail(&doc, 1280);
                    let expected = (image.width as usize).checked_mul(image.height as usize).and_then(|n| n.checked_mul(4)).ok_or("Invalid image dimensions")?;
                    if image.width == 0 || image.height == 0 || image.width > 1280 || image.height > 1280 || image.pixels.len() != expected { return Err("Invalid active-image buffer".into()); }
                    let input = photocraft_codecs::Image::from_u8(image.width, image.height, photocraft_codecs::ChannelLayout::Rgba, image.pixels).map_err(|e| e.to_string())?;
                    let png = photocraft_codecs::encode(&input, photocraft_codecs::Format::Png, &photocraft_codecs::EncodeOptions::default()).map_err(|e| e.to_string())?;
                    if png.len() > 4 * 1024 * 1024 { return Err("The active-image PNG exceeds 4 MiB".into()); }
                    use base64::Engine;
                    Ok(json!([
                        {"type":"input_text","text":json!({"monitor":"Canvas","document":doc.id.0,"revision":revision,"width":image.width,"height":image.height}).to_string()},
                        {"type":"input_image","image_url":format!("data:image/png;base64,{}",base64::engine::general_purpose::STANDARD.encode(png)),"detail":"auto"}
                    ]))
                })).unwrap_or_else(|_| Err("Active-image capture failed; the document is unchanged".into()));
                drop(busy_guard);
                let _ = send.send(result);
            }).map_err(|error| format!("Cannot start active-image capture: {error}"))?;
            Ok(Pending::Image { generation, revision, receive })
        }
    }
}

pub(super) fn poll(app: &mut PhotocraftApp, pending: &mut Pending) -> Result<Option<Value>, String> {
    match pending {
        Pending::Image { generation, revision, receive } => {
            if *generation != app.session.active_generation() || app.session.active().is_none_or(|st| st.revision != *revision) {
                return Err("The active document changed; request another image".into());
            }
            match receive.try_recv() {
                Ok(result) => result.map(Some),
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => Err("The active-image worker stopped".into()),
            }
        }
        Pending::Editor { request, reply, waiting, opening } => {
            authorize(app, &request.method, &request.params)?;
            if *waiting {
                return match reply.try_recv() {
                    Ok(value) => Ok(Some(Value::String(value.to_string()))),
                    Err(mpsc::TryRecvError::Empty) => Ok(None),
                    Err(mpsc::TryRecvError::Disconnected) => Err("The editor tool channel closed".into()),
                };
            }
            let ctx = app.frame_tool_ctx.clone().ok_or("The editor UI is not ready")?;
            let old_opens: Vec<_> = app.jobs.opens.iter().map(|tab| tab.job).collect();
            let documents = (app.session.documents().len() <= 4096).then(|| app.session.documents().iter().map(|st| st.doc.id.0).collect());
            let outcome = crate::control::handle(app, &ctx, request);
            // A synchronous document switch requested by this tool is intentional. User switches
            // between polls still replace the instance token before another tool can start.
            app.assistant.generation = app.session.active_generation();
            if let Some(tab) = app.jobs.opens.iter().find(|tab| !old_opens.contains(&tab.job)) {
                let job = tab.job;
                if let Some(documents) = documents {
                    let owned = Arc::new(Opening { job, generation: app.assistant.generation, documents });
                    app.assistant.opening = Arc::downgrade(&owned);
                    *opening = Some(owned);
                }
                // File Open returns before its background import finishes. Wait for the actual
                // job result before allowing a subsequent assistant edit to choose its document.
                app.jobs.waiters.push((job, request.reply.clone()));
                *waiting = true;
                return Ok(None);
            }
            match outcome {
                crate::control::Outcome::Done(value) => Ok(Some(Value::String(value.to_string()))),
                crate::control::Outcome::AfterInput => {
                    app.input_waiters.push(request.reply.clone());
                    *waiting = true;
                    Ok(None)
                }
                crate::control::Outcome::AfterJob(job) => {
                    app.jobs.waiters.push((job, request.reply.clone()));
                    *waiting = true;
                    Ok(None)
                }
                crate::control::Outcome::Screenshot { .. } => Err("Use active-image capture instead of desktop screenshots".into()),
            }
        }
    }
}
