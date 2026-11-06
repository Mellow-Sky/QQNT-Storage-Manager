//! A separate executable: actual embedded frontend + native IPC + temporary data.
//! No smoke commands or configuration are added to the production executable.
use qqnt_desktop::{builder, service::Backend};
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
use tauri::{webview::PageLoadEvent, Manager};

struct Fixture {
    _temp: tempfile::TempDir,
    grant_id: String,
    evaluated: AtomicBool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::Builder::new()
        .prefix("qqnt-NATIVE-SMOKE-")
        .tempdir()?;
    let root = temp.path().join("模拟账号/nt_qq/nt_data");
    for child in ["Pic", "Video", "nt_db"] {
        fs::create_dir_all(root.join(child))?;
    }
    fs::write(
        root.join("Pic/中文.png"),
        include_bytes!("../../../../tests/fixtures/media/image.png"),
    )?;
    fs::write(
        root.join("Video/模拟.mp4"),
        include_bytes!("../../../../tests/fixtures/media/video.mp4"),
    )?;
    fs::write(root.join("nt_db/chat.db"), b"protected database")?; // 18 bytes
    let mut context = tauri::generate_context!();
    if let Some(window) = context.config_mut().app.windows.first_mut() {
        window.title = "QQNT Storage Manager · 原生自检（模拟数据）".into();
        window.data_directory = Some(temp.path().join("webview"));
    }
    let backend = Backend::new(&temp.path().join("index"))?;
    let grant = backend.authorize(&root)?;
    let fixture = Arc::new(Fixture {
        _temp: temp,
        grant_id: grant.id,
        evaluated: AtomicBool::new(false),
    });
    let startup_fixture = Arc::clone(&fixture);
    let success = Arc::new(AtomicBool::new(false));
    let completed = Arc::clone(&success);
    builder()
        .setup(move |app| {
            app.manage(backend);
            app.manage(startup_fixture);
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // A dead WebView or a failed page load must not leave CI waiting forever.
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(60) {
                    std::thread::sleep(Duration::from_millis(100));
                    let Some(window) = handle.get_webview_window("main") else {
                        continue;
                    };
                    let (send, receive) = mpsc::channel();
                    if window
                        .eval_with_callback("window.__qqntNativeSmokeResult ?? null", move |json| {
                            let _ = send.send(json);
                        })
                        .is_err()
                    {
                        continue;
                    }
                    if let Ok(json) = receive.recv_timeout(Duration::from_secs(1)) {
                        if let Ok(result) = serde_json::from_str::<serde_json::Value>(&json) {
                            if result.is_object() {
                                println!("{result}");
                                let passed = result["ok"] == true;
                                completed.store(passed, Ordering::SeqCst);
                                handle.exit(if passed { 0 } else { 1 });
                                return;
                            }
                        }
                    }
                }
                eprintln!("native smoke timed out: page or IPC did not become ready");
                handle.exit(1);
            });
            Ok(())
        })
        .on_page_load(|window, load| {
            if load.event() != PageLoadEvent::Finished {
                return;
            }
            let Some(fixture) = window.try_state::<Arc<Fixture>>() else {
                return;
            };
            if fixture.evaluated.swap(true, Ordering::SeqCst) {
                return;
            }
            let Ok(grant_json) = serde_json::to_string(&fixture.grant_id) else {
                return;
            };
            let script = include_str!("native_smoke.js").replace("__GRANT_ID_JSON__", &grant_json);
            if window.eval(script).is_err() {
                eprintln!("native smoke script evaluation failed");
            }
        })
        .run(context)?;
    // Keep the fixture alive until the WebView, scanner and SQLite have stopped.
    drop(fixture);
    if success.load(Ordering::SeqCst) {
        Ok(())
    } else {
        Err("native WebView smoke failed".into())
    }
}
