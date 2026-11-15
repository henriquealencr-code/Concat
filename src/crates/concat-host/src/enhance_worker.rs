// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Concat contributors
//! Native-failure boundary: the worker writes private staging files, the parent commits after exit 0.
use crate::{
    enhance::{EnhanceRequest, Enhancers, Preview, PreviewRequest, Progress},
    jobs::Job,
};
use concat_core::Frame;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
#[derive(Clone, Serialize, Deserialize)]
enum Operation {
    Enhance(EnhanceRequest),
    Preview(PreviewRequest),
}
#[derive(Serialize, Deserialize)]
struct Request {
    data: PathBuf,
    operation: Operation,
}
#[derive(Serialize, Deserialize)]
struct PreviewInfo {
    width: u32,
    height: u32,
    tile_seconds: f64,
    tiles_per_frame: usize,
}
#[derive(Serialize, Deserialize)]
struct Reply {
    error: Option<String>,
    preview: Option<PreviewInfo>,
}
struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Process(Child, bool);
impl Drop for Process {
    fn drop(&mut self) {
        if !self.1 {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
static NEXT: AtomicU64 = AtomicU64::new(0);
fn directory(base: &Path) -> Result<Directory, String> {
    fs::create_dir_all(base).map_err(|e| e.to_string())?;
    for _ in 0..100 {
        let path = base.join(format!(
            ".berseker-enhance-{}-{}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(Directory(path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Não foi possível reservar a pasta temporária do Enhance.".into())
}
fn execute_attempt(
    exe: &Path,
    data: &Path,
    dir: &Directory,
    operation: Operation,
    job: &Job,
    progress: &mut dyn FnMut(Progress),
    cpu: bool,
    crashed: &mut bool,
) -> Result<Reply, String> {
    if job.cancelled() {
        return Err("Aprimoramento cancelado.".into());
    }
    fs::write(
        dir.0.join("request.json"),
        serde_json::to_vec(&Request {
            data: data.into(),
            operation,
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let logs = data.join("logs");
    fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let name = dir
        .0
        .file_name()
        .ok_or("invalid worker directory")?
        .to_string_lossy();
    let log_path = logs.join(format!("{name}-{}.log", if cpu { "cpu" } else { "gpu" }));
    let mut diagnostic = File::create(&log_path).map_err(|e| e.to_string())?;
    writeln!(
        diagnostic,
        "Berseker Enhance worker; parent={}; request={}",
        std::process::id(),
        dir.0.display()
    )
    .map_err(|e| e.to_string())?;
    diagnostic.flush().map_err(|e| e.to_string())?;
    File::create(dir.0.join("progress.jsonl")).map_err(|e| e.to_string())?;
    let mut command = Command::new(exe);
    command
        .arg("--enhance-worker")
        .arg(&dir.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(diagnostic));
    if cpu {
        command.env("BERSEKER_ENHANCE_DEVICE", "cpu");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    log::info!(
        "enhance: isolated worker; diagnostics {}",
        log_path.display()
    );
    let mut child = Process(
        command
            .spawn()
            .map_err(|e| format!("Não foi possível iniciar a IA: {e}"))?,
        false,
    );
    let mut events =
        BufReader::new(File::open(dir.0.join("progress.jsonl")).map_err(|e| e.to_string())?);
    #[cfg(windows)]
    let _limits = match windows_limits::Limit::attach(&child.0) {
        Ok(limit) => Some(limit),
        Err(_) if child.0.try_wait().map_err(|e| e.to_string())?.is_some() => None,
        Err(error) => return Err(error),
    };
    let mut last_progress = Instant::now();
    let mut checkpoint = -1.0f32;
    let mut line = String::new();
    let status = loop {
        if job.cancelled() {
            return Err("Aprimoramento cancelado; o original foi preservado.".into());
        }
        while (&mut events)
            .take(4096)
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            > 0
        {
            if line.len() > 4096 {
                return Err("Mensagem de progresso inválida.".into());
            }
            if !line.ends_with('\n') {
                break;
            }
            if let Ok(value) = serde_json::from_str::<Progress>(&line) {
                if let Progress::Analysing(fraction) = value
                    && fraction - checkpoint >= 0.05
                {
                    log::info!(
                        "enhance: isolated checkpoint {:.0}%; cpu={cpu}; {}",
                        fraction * 100.0,
                        log_path.display()
                    );
                    checkpoint = fraction;
                }
                progress(value);
                last_progress = Instant::now();
            }
            line.clear();
        }
        if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
            child.1 = true;
            break status;
        }
        if last_progress.elapsed() > Duration::from_secs(if cpu { 1800 } else { 300 }) {
            *crashed = true;
            return Err(format!(
                "A IA ficou sem responder e foi interrompida. Diagnóstico: {}",
                log_path.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    log::info!(
        "enhance: worker exited {status}; diagnostics {}",
        log_path.display()
    );
    if !status.success() {
        *crashed = true;
        let cause = match status.code().map(|n| n as u32) {
            Some(0xc0000005) => "falha nativa de acesso à memória (GPU, driver ou runtime)",
            Some(0xc0000017) => "memória insuficiente",
            Some(0xc0000135) | Some(0xc0000139) => "biblioteca nativa ausente ou incompatível",
            Some(0xc0000409) => "falha nativa ou panic com abort",
            _ => "falha no processo de IA",
        };
        return Err(format!(
            "O aprimoramento foi interrompido: {cause}; {status}. O editor e o original foram preservados. Log: {}",
            log_path.display()
        ));
    }
    let path = dir.0.join("reply.json");
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 65536 {
        return Err("Resposta de IA inválida.".into());
    }
    let reply: Reply = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if let Some(error) = &reply.error {
        *crashed = error.contains("GPU") || error.contains("wgpu") || error.contains("device lost");
        return Err(format!("{error}\nLog: {}", log_path.display()));
    }
    if job.cancelled() {
        return Err("Aprimoramento cancelado.".into());
    }
    Ok(reply)
}
fn execute(
    exe: &Path,
    data: &Path,
    dir: &Directory,
    operation: Operation,
    job: &Job,
    progress: &mut dyn FnMut(Progress),
) -> Result<Reply, String> {
    let cpu = std::env::var("BERSEKER_ENHANCE_DEVICE")
        .is_ok_and(|v| v.trim().eq_ignore_ascii_case("cpu"))
        || std::env::var("CONCAT_GPU").is_ok_and(|v| v.trim().eq_ignore_ascii_case("software"));
    let mut crashed = false;
    let first = execute_attempt(
        exe,
        data,
        dir,
        operation.clone(),
        job,
        progress,
        cpu,
        &mut crashed,
    );
    if first.is_ok() || !crashed || cpu || job.cancelled() {
        return first;
    }
    log::warn!(
        "enhance: isolated GPU failed; retrying once with ONNX CPU; {}",
        first.as_ref().err().unwrap()
    );
    // A child may have died after creating or even closing its output.
    // No partial GPU product is accepted by the CPU attempt's cache fast path.
    for entry in fs::read_dir(&dir.0).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_file() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
    }
    execute_attempt(exe, data, dir, operation, job, progress, true, &mut crashed).map_err(
        |cpu_error| format!("A GPU falhou e a tentativa por CPU também falhou. {cpu_error}"),
    )
}

/// Enhances outside the editor, publishing only a complete successful result.
pub fn enhance(
    exe: &Path,
    data: &Path,
    request: &EnhanceRequest,
    job: &Job,
    progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, String> {
    if request.target.is_file() {
        return Ok(request.target.clone());
    }
    let original = fs::metadata(&request.media_path).map_err(|e| e.to_string())?;
    let stamp = (
        original.len(),
        original.modified().map_err(|e| e.to_string())?,
    );
    let dir = directory(request.target.parent().ok_or("Destino inválido.")?)?;
    let mut isolated = request.clone();
    isolated.target = dir.0.join(if request.still {
        "output.png"
    } else {
        "output.mp4"
    });
    execute(
        exe,
        data,
        &dir,
        Operation::Enhance(isolated.clone()),
        job,
        progress,
    )?;
    if fs::metadata(&isolated.target)
        .map_err(|e| e.to_string())?
        .len()
        == 0
    {
        return Err("A IA não produziu um arquivo completo.".into());
    }
    let original = fs::metadata(&request.media_path).map_err(|e| e.to_string())?;
    if (
        original.len(),
        original.modified().map_err(|e| e.to_string())?,
    ) != stamp
    {
        return Err("A origem mudou durante o aprimoramento; a cópia foi descartada.".into());
    }
    // Atomic publication without replacing an existing destination, including a source alias.
    fs::hard_link(&isolated.target, &request.target).map_err(|e| e.to_string())?;
    Ok(request.target.clone())
}
/// The comparison preview shares the full job's native-crash boundary.
pub fn preview(
    exe: &Path,
    data: &Path,
    request: &PreviewRequest,
    job: &Job,
    progress: &mut dyn FnMut(Progress),
) -> Result<Preview, String> {
    let dir = directory(&data.join("cache"))?;
    let reply = execute(
        exe,
        data,
        &dir,
        Operation::Preview(request.clone()),
        job,
        progress,
    )?;
    let info = reply.preview.ok_or("A IA não devolveu a prévia.")?;
    if info.width == 0 || info.height == 0 || info.width > 640 || info.height > 360 {
        return Err("Dimensões de prévia inválidas.".into());
    }
    let size = u64::from(info.width) * u64::from(info.height) * 4;
    let read = |name| -> Result<Frame, String> {
        let path = dir.0.join(name);
        if fs::metadata(&path).map_err(|e| e.to_string())?.len() != size {
            return Err("Buffer de prévia incompleto.".into());
        }
        Frame::from_rgba(
            info.width,
            info.height,
            fs::read(path).map_err(|e| e.to_string())?,
        )
        .ok_or("Buffer de prévia inválido.".into())
    };
    Ok(Preview {
        plain: read("plain.rgba")?,
        enhanced: read("enhanced.rgba")?,
        tile_seconds: info.tile_seconds,
        tiles_per_frame: info.tiles_per_frame,
    })
}
/// Internal entry point, before the desktop initialises its window or GPU.
pub fn run(directory: &Path) -> Result<(), String> {
    let path = directory.join("request.json");
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > 65536 {
        return Err("Worker request too large".into());
    }
    let request: Request = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let _ = crate::logs::open_console(None);
    let engine = Enhancers::new(&request.data);
    let mut events = fs::OpenOptions::new()
        .append(true)
        .open(directory.join("progress.jsonl"))
        .map_err(|e| e.to_string())?;
    let mut progress = |value| {
        let _ = serde_json::to_writer(&mut events, &value);
        let _ = events.write_all(b"\n");
        let _ = events.flush();
    };
    let result = match request.operation {
        Operation::Enhance(request) => engine.enhance(&request, &mut progress).map(|_| None),
        Operation::Preview(request) => {
            engine.preview(&request, &mut progress).and_then(|preview| {
                fs::write(directory.join("plain.rgba"), preview.plain.pixels())
                    .map_err(|e| e.to_string())?;
                fs::write(directory.join("enhanced.rgba"), preview.enhanced.pixels())
                    .map_err(|e| e.to_string())?;
                Ok(Some(PreviewInfo {
                    width: preview.plain.width(),
                    height: preview.plain.height(),
                    tile_seconds: preview.tile_seconds,
                    tiles_per_frame: preview.tiles_per_frame,
                }))
            })
        }
    };
    let reply = match result {
        Ok(preview) => Reply {
            error: None,
            preview,
        },
        Err(error) => {
            log::error!("enhance worker: {error}");
            Reply {
                error: Some(error),
                preview: None,
            }
        }
    };
    fs::write(
        directory.join("reply.json"),
        serde_json::to_vec(&reply).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn executable() -> PathBuf {
        std::env::var_os("BERSEKER_TEST_WORKER")
            .expect("build --example enhance-worker and set BERSEKER_TEST_WORKER")
            .into()
    }
    #[test]
    fn cancellation_before_worker_start_does_not_open_source_or_load_model() {
        let root = tempfile::tempdir().unwrap();
        let service = Enhancers::new(root.path());
        let job = service.reserve().unwrap();
        service.cancel();
        let request = PreviewRequest {
            media_path: "does-not-exist".into(),
            seconds: 0.0,
            scale: 2.0,
        };
        assert!(
            service
                .preview_reserved(job, &request, &mut |_| {})
                .unwrap_err()
                .contains("cancelado")
        );
        assert!(!service.is_busy());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }
    #[test]
    #[ignore = "requires BERSEKER_TEST_WORKER and CONCAT_ENHANCE_TEST_MODEL"]
    fn real_model_comparison_preview_crosses_the_process_boundary() {
        let root = tempfile::tempdir().unwrap();
        let model = concat_vision::models::model_file(root.path(), concat_vision::ModelId::Enhance);
        fs::create_dir_all(model.parent().unwrap()).unwrap();
        fs::copy(
            std::env::var_os("CONCAT_ENHANCE_TEST_MODEL").expect("verified model"),
            model,
        )
        .unwrap();
        let source = root.path().join("original.png");
        {
            let mut encoder = png::Encoder::new(File::create(&source).unwrap(), 16, 16);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[180, 60, 100, 255].repeat(256))
                .unwrap();
        }
        let original = fs::read(&source).unwrap();
        let service = Enhancers::new(root.path());
        service.isolate_in(executable());
        let preview = service
            .preview(
                &PreviewRequest {
                    media_path: source.to_string_lossy().into(),
                    seconds: 0.0,
                    scale: 2.0,
                },
                &mut |_| {},
            )
            .unwrap();
        assert_eq!((preview.plain.width(), preview.plain.height()), (32, 32));
        assert_eq!(
            (preview.enhanced.width(), preview.enhanced.height()),
            (32, 32)
        );
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(!service.is_busy());
    }
    #[test]
    #[ignore = "requires BERSEKER_TEST_WORKER and CONCAT_ENHANCE_TEST_MODEL"]
    fn a_native_gpu_abort_retries_real_cpu_without_accepting_partial_output() {
        let root = tempfile::tempdir().unwrap();
        let model = concat_vision::models::model_file(root.path(), concat_vision::ModelId::Enhance);
        fs::create_dir_all(model.parent().unwrap()).unwrap();
        fs::copy(
            std::env::var_os("CONCAT_ENHANCE_TEST_MODEL").expect("verified model"),
            model,
        )
        .unwrap();
        let source = root.path().join("fallback-source.png");
        {
            let mut encoder = png::Encoder::new(File::create(&source).unwrap(), 16, 16);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[180, 60, 100, 255].repeat(256))
                .unwrap();
        }
        let original = fs::read(&source).unwrap();
        let service = Enhancers::new(root.path());
        service.isolate_in(executable());
        let target = root.path().join("completed.png");
        service
            .enhance(
                &EnhanceRequest {
                    media_path: source.to_string_lossy().into(),
                    target: target.clone(),
                    scale: 2.0,
                    strength: 1.0,
                    still: true,
                    span: None,
                },
                &mut |_| {},
            )
            .unwrap();
        let info = concat_media::probe(&target).unwrap().video.unwrap();
        assert_eq!((info.width, info.height), (32, 32));
        assert_eq!(fs::read(&source).unwrap(), original);
        let logs: Vec<_> = fs::read_dir(root.path().join("logs"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(logs.len(), 2);
        assert!(logs.iter().any(|p| {
            fs::read_to_string(p)
                .unwrap()
                .contains("explicit CPU inference")
        }));
        assert!(!service.is_busy());
    }
    #[test]
    #[ignore = "requires BERSEKER_TEST_WORKER"]
    fn native_abort_does_not_publish_output_or_change_source_and_retry_is_possible() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("abort-source.mp4");
        fs::write(&source, b"original bytes").unwrap();
        let service = Enhancers::new(root.path());
        service.isolate_in(executable());
        let request = EnhanceRequest {
            media_path: source.to_string_lossy().into(),
            target: root.path().join("result.mp4"),
            scale: 2.0,
            strength: 1.0,
            still: false,
            span: None,
        };
        for _ in 0..2 {
            let error = service.enhance(&request, &mut |_| {}).unwrap_err();
            assert!(
                error.contains("processo de IA") || error.contains("falha nativa"),
                "{error}"
            );
            assert!(!service.is_busy());
            assert!(!request.target.exists());
            assert_eq!(fs::read(&source).unwrap(), b"original bytes");
            assert!(!fs::read_dir(root.path()).unwrap().any(|p| {
                p.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".berseker-enhance-")
            }));
        }
        let logs: Vec<_> = fs::read_dir(root.path().join("logs")).unwrap().collect();
        assert_eq!(logs.len(), 4);
        assert!(logs.iter().any(|p| {
            fs::read_to_string(p.as_ref().unwrap().path())
                .unwrap()
                .contains("native abort")
        }));
    }
    #[test]
    #[ignore = "requires BERSEKER_TEST_WORKER"]
    fn cancelling_a_hung_native_worker_reaps_it_and_discards_staging() {
        let root = tempfile::tempdir().unwrap();
        let service = std::sync::Arc::new(Enhancers::new(root.path()));
        service.isolate_in(executable());
        let source = root.path().join("sleep-source.mp4");
        fs::write(&source, b"original").unwrap();
        let request = EnhanceRequest {
            media_path: source.to_string_lossy().into(),
            target: root.path().join("result.mp4"),
            scale: 2.0,
            strength: 1.0,
            still: false,
            span: None,
        };
        let other = service.clone();
        let start = Instant::now();
        let thread = std::thread::spawn(move || other.enhance(&request, &mut |_| {}));
        while !service.is_busy() {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::yield_now();
        }
        while !fs::read_dir(root.path())
            .unwrap()
            .any(|p| p.unwrap().path().join("started").exists())
        {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        service.cancel();
        assert!(thread.join().unwrap().unwrap_err().contains("cancelado"));
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(!service.is_busy());
        assert_eq!(fs::read(&source).unwrap(), b"original");
        assert!(!root.path().join("result.mp4").exists());
    }
}

#[cfg(windows)]
mod windows_limits {
    use std::{os::windows::io::AsRawHandle, process::Child};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::JobObjects::*,
    };
    pub(super) struct Limit(HANDLE);
    impl Drop for Limit {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    impl Limit {
        pub(super) fn attach(child: &Child) -> Result<Self, String> {
            // An unnamed, non-inheritable job. Closing it kills a worker left
            // behind by an editor crash; its commit limit bounds host RAM.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                let job = Self(handle);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_PROCESS_MEMORY
                    | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
                info.ProcessMemoryLimit =
                    (crate::memory::available().unwrap_or(2 * 1024 * 1024 * 1024) / 2)
                        .min(4 * 1024 * 1024 * 1024) as usize;
                log::info!(
                    "enhance: worker Windows RAM commit limit {} MiB",
                    info.ProcessMemoryLimit / 1_048_576
                );
                if SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of_val(&info) as u32,
                ) == 0
                    || AssignProcessToJobObject(handle, child.as_raw_handle() as HANDLE) == 0
                {
                    return Err(format!(
                        "Não foi possível proteger o processo de IA: {}",
                        std::io::Error::last_os_error()
                    ));
                }
                Ok(job)
            }
        }
    }
}
