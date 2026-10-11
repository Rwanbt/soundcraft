//! The JSON control channel's UI side. The app's TCP server turns each request line into a
//! [`ControlRequest`]; they are handled on the UI thread here, so every command an agent sends
//! goes through exactly the same code as a click.
//!
//! Methods:
//! - `engine.execute {command, params}` (alias `command`): run any command programmatically (no dialogs)
//! - `engine.commands {filter?}`, `engine.parity`, `session.inspect {detail?}`, `ui.inspect`
//! - `ui.menu.list`, `ui.menu.invoke {path: "Track > New..." | id}` (menu-style: may open dialogs)
//! - `ui.click {x,y,button?,count?,mods?}`, `ui.move {x,y}`, `ui.drag {x,y,to_x,to_y,steps?}`
//! - `ui.key {key, cmd?, shift?, alt?, ctrl?}`, `ui.text {text}`
//! - `ui.screenshot {path?}` (PNG; base64 when no path), `ui.set {...UiState fields}`
//! - `app.quit`

use crate::SoundApp;
use serde_json::{Value, json};
use std::sync::mpsc::{Receiver, Sender, channel};

pub struct ControlRequest {
    pub method: String,
    pub params: Value,
    pub reply: Sender<Value>,
}

impl ControlRequest {
    pub fn new(method: impl Into<String>, params: Value) -> (Self, Receiver<Value>) {
        let (tx, rx) = channel();
        (ControlRequest { method: method.into(), params, reply: tx }, rx)
    }
}

pub struct PendingShot {
    token: u64,
    reply: Sender<Value>,
    path: Option<String>,
    frames: u32,
    sent: bool,
    deadline_frames: u32,
}

fn ok(v: Value) -> Value {
    json!({"ok": true, "result": v})
}

fn err(e: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": e.to_string()})
}

pub fn drain(app: &mut SoundApp, ctx: &egui::Context) {
    let Some(rx) = app.control_rx.take() else { return };
    let mut n = 0;
    while let Ok(req) = rx.try_recv() {
        n += 1;
        if let Some(v) = handle(app, ctx, &req) {
            let _ = req.reply.send(v);
        }
        if n > 64 {
            break;
        }
    }
    app.control_rx = Some(rx);
    if n > 0 {
        ctx.request_repaint();
    }
}

fn key_from(name: &str) -> Option<egui::Key> {
    egui::Key::from_name(name).or(match name.to_ascii_lowercase().as_str() {
        "space" => Some(egui::Key::Space),
        "enter" | "return" => Some(egui::Key::Enter),
        "escape" | "esc" => Some(egui::Key::Escape),
        "tab" => Some(egui::Key::Tab),
        "home" => Some(egui::Key::Home),
        "end" => Some(egui::Key::End),
        "=" => Some(egui::Key::Equals),
        _ => None,
    })
}

fn mods(p: &Value) -> egui::Modifiers {
    let b = |k: &str| p.get(k).and_then(Value::as_bool).unwrap_or(false);
    let cmd = b("cmd") || b("command");
    egui::Modifiers {
        alt: b("alt"),
        ctrl: b("ctrl") || (cmd && !cfg!(target_os = "macos")),
        shift: b("shift"),
        mac_cmd: cmd && cfg!(target_os = "macos"),
        command: cmd,
    }
}

fn handle(app: &mut SoundApp, ctx: &egui::Context, req: &ControlRequest) -> Option<Value> {
    let p = &req.params;
    let f = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
    Some(match req.method.as_str() {
        "engine.execute" | "command" => {
            let id = p.get("command").or_else(|| p.get("id")).and_then(Value::as_str).unwrap_or("");
            let params = p.get("params").cloned().unwrap_or(json!({}));
            match app.run(id, params) {
                Ok(v) => ok(v),
                Err(e) => err(e),
            }
        }
        "engine.commands" => {
            let mut list = match app.engine.execute("engine.commands", p) {
                Ok(v) => v,
                Err(e) => return Some(err(e)),
            };
            if let Some(arr) = list.as_array_mut() {
                for (id, label, path, sc) in crate::menus::UI_COMMANDS {
                    arr.push(json!({"id": id, "label": label, "menu": path, "shortcut": sc, "enabled": true, "params": "{}"}));
                }
            }
            ok(list)
        }
        "engine.parity" => ok(crate::menus::parity()),
        "session.inspect" | "document.inspect" => {
            ok(soundcraft_engine::inspect::session(&app.engine, p.get("detail").and_then(Value::as_str) == Some("full")))
        }
        "ui.inspect" => {
            let mut v = app.inspect(ctx);
            v["edit_layout"] = json!(app.edit_layout);
            ok(v)
        }
        "ui.menu.list" => ok(json!(
            soundcraft_engine::catalog::catalog()
                .iter()
                .map(|e| json!({"path": e, "command": soundcraft_engine::catalog::implemented_by(e, &crate::menus::ui_aliases())}))
                .collect::<Vec<_>>()
        )),
        "ui.menu.invoke" => {
            let path = p.get("path").and_then(Value::as_str).unwrap_or("");
            let id = p
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| soundcraft_engine::catalog::implemented_by(path, &crate::menus::ui_aliases()));
            match id {
                Some(id) => {
                    crate::menus::invoke_menu(app, &id, path);
                    ok(json!({"invoked": id, "dialog": app.dialogs.open_name()}))
                }
                None => err(format!("menu item `{path}` is not implemented yet")),
            }
        }
        "ui.set" => match app.run("ui.set", p.clone()) {
            Ok(v) => ok(v),
            Err(e) => err(e),
        },
        "ui.move" => {
            app.synthetic.push(egui::Event::PointerMoved(egui::pos2(f("x"), f("y"))));
            ok(json!({}))
        }
        "ui.click" => {
            let pos = egui::pos2(f("x"), f("y"));
            let button = match p.get("button").and_then(Value::as_str) {
                Some("right") | Some("secondary") => egui::PointerButton::Secondary,
                _ => egui::PointerButton::Primary,
            };
            let m = mods(p);
            let count = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 3);
            app.synthetic.push(egui::Event::PointerMoved(pos));
            for _ in 0..count {
                app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: true, modifiers: m });
                app.synthetic.push(egui::Event::PointerButton { pos, button, pressed: false, modifiers: m });
            }
            ok(json!({}))
        }
        "ui.drag" => {
            let a = egui::pos2(f("x"), f("y"));
            let b = egui::pos2(f("to_x"), f("to_y"));
            let steps = p.get("steps").and_then(Value::as_u64).unwrap_or(8).clamp(1, 200);
            let m = mods(p);
            app.synthetic.push(egui::Event::PointerMoved(a));
            app.synthetic.push(egui::Event::PointerButton { pos: a, button: egui::PointerButton::Primary, pressed: true, modifiers: m });
            for i in 1..=steps {
                let k = i as f32 / steps as f32;
                app.synthetic.push(egui::Event::PointerMoved(a + (b - a) * k));
            }
            app.synthetic.push(egui::Event::PointerButton { pos: b, button: egui::PointerButton::Primary, pressed: false, modifiers: m });
            ok(json!({}))
        }
        "ui.key" => {
            let name = p.get("key").and_then(Value::as_str).unwrap_or("");
            let Some(key) = key_from(name) else { return Some(err(format!("unknown key `{name}`"))) };
            let m = mods(p);
            app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m });
            app.synthetic.push(egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: m });
            ok(json!({}))
        }
        "ui.text" => {
            let t = p.get("text").and_then(Value::as_str).unwrap_or("").to_string();
            app.synthetic.push(egui::Event::Text(t));
            ok(json!({}))
        }
        "ui.screenshot" => {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let token = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            app.pending_shots.push(PendingShot {
                token,
                reply: req.reply.clone(),
                path: p.get("path").and_then(Value::as_str).map(str::to_string),
                frames: 0,
                sent: false,
                deadline_frames: 600,
            });
            ctx.request_repaint();
            return None;
        }
        "app.quit" => {
            app.quit_requested = true;
            ok(json!({}))
        }
        other => err(format!("unknown method `{other}`")),
    })
}

/// Request and collect viewport screenshots for pending `ui.screenshot` calls.
pub fn collect_screenshots(app: &mut SoundApp, ctx: &egui::Context) {
    if app.pending_shots.is_empty() {
        return;
    }
    let images: Vec<(u64, std::sync::Arc<egui::ColorImage>)> = ctx.input(|i| {
        i.raw
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Screenshot { user_data, image, .. } => {
                    user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).map(|t| (*t, image.clone()))
                }
                _ => None,
            })
            .collect()
    });
    let mut keep = Vec::new();
    for mut s in std::mem::take(&mut app.pending_shots) {
        if let Some((_, img)) = images.iter().find(|(t, _)| *t == s.token) {
            let _ = s.reply.send(encode(img, s.path.as_deref()));
            continue;
        }
        s.frames += 1;
        if !s.sent && s.frames >= 3 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(s.token)));
            s.sent = true;
        }
        if s.frames > s.deadline_frames {
            let _ = s.reply.send(err("no frame was presented (is the window visible?)"));
            continue;
        }
        keep.push(s);
    }
    app.pending_shots = keep;
    ctx.request_repaint();
}

pub fn png_bytes(img: &egui::ColorImage) -> Result<Vec<u8>, String> {
    let [w, h] = img.size;
    let mut raw = Vec::with_capacity(w * h * 4);
    for c in &img.pixels {
        raw.extend_from_slice(&c.to_array());
    }
    let buf = image::RgbaImage::from_raw(u32::try_from(w).map_err(|e| e.to_string())?, u32::try_from(h).map_err(|e| e.to_string())?, raw)
        .ok_or("bad image size")?;
    let mut out = std::io::Cursor::new(Vec::new());
    buf.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

fn encode(img: &egui::ColorImage, path: Option<&str>) -> Value {
    let bytes = match png_bytes(img) {
        Ok(b) => b,
        Err(e) => return err(e),
    };
    match path {
        Some(p) => match std::fs::write(p, &bytes) {
            Ok(()) => ok(json!({"path": p, "width": img.size[0], "height": img.size[1]})),
            Err(e) => err(format!("{p}: {e}")),
        },
        None => ok(json!({"png_base64": base64(&bytes), "width": img.size[0], "height": img.size[1]})),
    }
}

/// Minimal base64 (standard alphabet) to avoid another dependency.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                let idx = ((n >> (18 - 6 * i)) & 63) as usize;
                out.push(char::from(T.get(idx).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_known() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
    }
}

#[cfg(test)]
mod protocol_regressions {
    use super::*;
    use crate::Services;

    fn app() -> SoundApp {
        SoundApp::new(soundcraft_engine::demo::demo_engine(), None, Services::default())
    }

    fn call(app: &mut SoundApp, ctx: &egui::Context, method: &str, params: Value) -> Value {
        let (request, _receiver) = ControlRequest::new(method, params);
        handle(app, ctx, &request).unwrap()
    }

    #[test]
    fn key_aliases_and_unknown_keys_are_distinguished() {
        for (name, key) in [
            ("space", egui::Key::Space),
            ("enter", egui::Key::Enter),
            ("return", egui::Key::Enter),
            ("escape", egui::Key::Escape),
            ("esc", egui::Key::Escape),
            ("tab", egui::Key::Tab),
            ("home", egui::Key::Home),
            ("end", egui::Key::End),
            ("=", egui::Key::Equals),
        ] {
            assert_eq!(key_from(name), Some(key), "{name}");
        }
        assert_eq!(key_from("not-a-key"), None);
    }

    #[test]
    fn modifier_aliases_respect_host_command_mapping() {
        let m = mods(&json!({"command": true, "shift": true, "alt": true}));
        assert!(m.command && m.shift && m.alt);
        assert_eq!(m.mac_cmd, cfg!(target_os = "macos"));
        assert_eq!(m.ctrl, !cfg!(target_os = "macos"));
        let m = mods(&json!({"ctrl": true, "cmd": false}));
        assert!(m.ctrl);
        assert!(!m.command && !m.mac_cmd);
        assert_eq!(mods(&json!({})), egui::Modifiers::NONE);
    }

    #[test]
    fn command_queries_and_unknown_methods_return_structured_results() {
        let mut app = app();
        let ctx = egui::Context::default();
        let s = call(&mut app, &ctx, "session.inspect", json!({}));
        let a = call(&mut app, &ctx, "document.inspect", json!({}));
        assert_eq!(s, a);
        assert_eq!(s["ok"], true);
        assert_eq!(call(&mut app, &ctx, "engine.parity", json!({}))["ok"], true);
        assert_eq!(call(&mut app, &ctx, "engine.commands", json!({}))["ok"], true);
        assert_eq!(call(&mut app, &ctx, "ui.menu.list", json!({}))["ok"], true);
        let invalid = call(&mut app, &ctx, "not.a.command", json!({}));
        assert_eq!(invalid["ok"], false);
        assert!(invalid["error"].as_str().unwrap().contains("unknown method"));
        assert_eq!(call(&mut app, &ctx, "ui.menu.invoke", json!({"path": "__invalid__"}))["ok"], false);
    }

    #[test]
    fn engine_aliases_execute_commands_and_surface_errors() {
        let mut app = app();
        let ctx = egui::Context::default();
        let before = app.engine.session().tracks.len();
        let out = call(&mut app, &ctx, "engine.execute", json!({"command": "track.new", "params": {"count": 1}}));
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(app.engine.session().tracks.len(), before + 1);
        let out = call(&mut app, &ctx, "command", json!({"id": "track.new", "params": {"count": 1}}));
        assert_eq!(out["ok"], true, "{out}");
        assert_eq!(app.engine.session().tracks.len(), before + 2);
        let out = call(&mut app, &ctx, "engine.execute", json!({"command": "__invalid__"}));
        assert_eq!(out["ok"], false);
    }

    #[test]
    fn synthetic_clicks_have_exact_press_release_pairs_and_bounded_counts() {
        let mut app = app();
        let ctx = egui::Context::default();
        assert_eq!(call(&mut app, &ctx, "ui.click", json!({"x": 12.0, "y": 28.0, "count": 2, "button": "right", "shift": true}))["ok"], true);
        assert_eq!(app.synthetic.len(), 5);
        assert!(matches!(app.synthetic[0], egui::Event::PointerMoved(p) if p == egui::pos2(12.0, 28.0)));
        for (i, pressed) in [(1, true), (2, false), (3, true), (4, false)] {
            assert!(matches!(&app.synthetic[i],
                egui::Event::PointerButton { button: egui::PointerButton::Secondary, pressed: v, modifiers, .. }
                    if *v == pressed && modifiers.shift));
        }
        app.synthetic.clear();
        let _ = call(&mut app, &ctx, "ui.click", json!({"count": 1000000}));
        assert_eq!(app.synthetic.len(), 7, "click count is capped at three");
        app.synthetic.clear();
        let _ = call(&mut app, &ctx, "ui.click", json!({"count": 0}));
        assert_eq!(app.synthetic.len(), 3, "a zero-count request is normalized to one click");
    }

    #[test]
    fn synthetic_drag_moves_in_order_and_caps_path_steps() {
        let mut app = app();
        let ctx = egui::Context::default();
        assert_eq!(call(&mut app, &ctx, "ui.drag", json!({"x": 0.0, "y": 0.0, "to_x": 8.0, "to_y": 4.0, "steps": 2}))["ok"], true);
        assert_eq!(app.synthetic.len(), 5);
        assert!(matches!(app.synthetic[2], egui::Event::PointerMoved(p) if p == egui::pos2(4.0, 2.0)));
        assert!(matches!(app.synthetic[3], egui::Event::PointerMoved(p) if p == egui::pos2(8.0, 4.0)));
        assert!(matches!(app.synthetic[4], egui::Event::PointerButton { pressed: false, .. }));
        app.synthetic.clear();
        let _ = call(&mut app, &ctx, "ui.drag", json!({"steps": 1000000}));
        assert_eq!(app.synthetic.len(), 203, "drag steps are bounded to 200");
        app.synthetic.clear();
        let _ = call(&mut app, &ctx, "ui.drag", json!({"steps": 0}));
        assert_eq!(app.synthetic.len(), 4, "zero steps are normalized to one");
    }

    #[test]
    fn key_text_move_and_ui_inspect_work_without_a_native_window() {
        let mut app = app();
        let ctx = egui::Context::default();
        assert_eq!(call(&mut app, &ctx, "ui.move", json!({"x": 3, "y": 4}))["ok"], true);
        assert_eq!(call(&mut app, &ctx, "ui.text", json!({"text": "é漢🙂"}))["ok"], true);
        assert_eq!(call(&mut app, &ctx, "ui.key", json!({"key": "space", "shift": true}))["ok"], true);
        assert!(matches!(app.synthetic[0], egui::Event::PointerMoved(p) if p == egui::pos2(3.0,4.0)));
        assert!(matches!(&app.synthetic[1], egui::Event::Text(t) if t == "é漢🙂"));
        assert!(matches!(&app.synthetic[2], egui::Event::Key { key: egui::Key::Space, pressed: true, modifiers, .. } if modifiers.shift));
        assert!(matches!(&app.synthetic[3], egui::Event::Key { key: egui::Key::Space, pressed: false, .. }));
        let count = app.synthetic.len();
        let invalid = call(&mut app, &ctx, "ui.key", json!({"key": "invalid-key-name"}));
        assert_eq!(invalid["ok"], false);
        assert_eq!(app.synthetic.len(), count, "invalid keys cannot enqueue input");
        let inspect = call(&mut app, &ctx, "ui.inspect", json!({}));
        assert_eq!(inspect["ok"], true);
        assert!(inspect["result"].get("edit_layout").is_some());
    }

    #[test]
    fn requests_are_drained_in_bounded_batches_without_losses() {
        let (tx, rx) = channel();
        let mut app = app().with_control(rx);
        let ctx = egui::Context::default();
        let mut responses = Vec::new();
        for i in 0..80 {
            let (req, reply) = ControlRequest::new("ui.move", json!({"x": i, "y": 1}));
            tx.send(req).unwrap();
            responses.push(reply);
        }
        drain(&mut app, &ctx);
        assert_eq!(app.synthetic.len(), 65, "at most 65 requests are drained per frame");
        for (i, reply) in responses.iter().enumerate() {
            assert_eq!(reply.try_recv().is_ok(), i < 65, "batch boundary request {i}");
        }
        drain(&mut app, &ctx);
        assert_eq!(app.synthetic.len(), 80, "remaining requests are not lost");
        for reply in responses.iter().skip(65) {
            assert_eq!(reply.try_recv().unwrap()["ok"], true);
        }
    }

    #[test]
    fn screenshot_requests_are_deferred_and_fail_cleanly_without_frames() {
        let (tx, rx) = channel();
        let mut app = app().with_control(rx);
        let ctx = egui::Context::default();
        let (req, reply) = ControlRequest::new("ui.screenshot", json!({}));
        tx.send(req).unwrap();
        drain(&mut app, &ctx);
        assert_eq!(app.pending_shots.len(), 1);
        assert!(reply.try_recv().is_err(), "screenshot cannot respond before a frame exists");
        collect_screenshots(&mut app, &ctx);
        assert_eq!(app.pending_shots.len(), 1);
        app.pending_shots[0].frames = 600;
        collect_screenshots(&mut app, &ctx);
        assert!(app.pending_shots.is_empty());
        let response = reply.try_recv().unwrap();
        assert_eq!(response["ok"], false);
        assert!(response["error"].as_str().unwrap().contains("no frame was presented"));
    }

    #[test]
    fn tiny_rgba_screenshots_are_valid_pngs_and_base64() {
        let img = egui::ColorImage::from_rgba_unmultiplied([2, 1], &[255, 0, 0, 255, 0, 255, 0, 255]);
        let bytes = png_bytes(&img).unwrap();
        assert!(bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
        let output = encode(&img, None);
        assert_eq!(output["ok"], true);
        assert_eq!(output["result"]["width"], 2);
        assert_eq!(output["result"]["height"], 1);
        assert!(output["result"]["png_base64"].as_str().unwrap().starts_with("iVBORw0KGgo"));
        let fail = encode(&img, Some("/__soundcraft_missing_parent__/invalid.png"));
        assert_eq!(fail["ok"], false, "invalid paths must fail without crashing");
    }

    #[test]
    fn quit_is_explicit_and_does_not_require_a_ui_event_loop() {
        let mut app = app();
        let ctx = egui::Context::default();
        assert!(!app.quit_requested);
        assert_eq!(call(&mut app, &ctx, "app.quit", json!({}))["ok"], true);
        assert!(app.quit_requested);
    }
}
