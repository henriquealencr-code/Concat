// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! CI harness, never packaged. It can die natively inside the real protocol.
fn main() {
    concat_host::crash::catch();
    let path = std::path::PathBuf::from(std::env::args_os().nth(2).expect("worker directory"));
    let text = std::fs::read_to_string(path.join("request.json")).unwrap();
    let forced_cpu = std::env::var("BERSEKER_ENHANCE_DEVICE").is_ok_and(|value| value == "cpu");
    if text.contains("readback-source.png") && !forced_cpu {
        std::fs::write(path.join("output.png"), b"incomplete").unwrap();
        std::fs::write(path.join("reply.json"), r#"{"error":"upscaler: the answer would not map: Error occurred when trying to async map a buffer","preview":null}"#).unwrap();
        return;
    }
    if text.contains("abort-source.mp4") || (text.contains("fallback-source.png") && !forced_cpu) {
        std::fs::write(path.join("output.mp4"), b"incomplete video").unwrap();
        eprintln!("test: native abort after partial output");
        std::process::abort();
    }
    if text.contains("sleep-source.mp4") {
        std::fs::write(path.join("started"), b"ready").unwrap();
        std::thread::sleep(std::time::Duration::from_secs(120));
    }
    if let Err(error) = concat_host::enhance_worker::run(&path) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
