//
// Minimal CPAL-based audio recorder.
//
// Supported platforms:
// - Windows
// - macOS
//
// Linux support is intentionally not enabled yet because we don't want to introduce
// new platform dependencies without committing to a full Linux UX.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Sample, SampleFormat, SizedSample, Stream};

use crate::resample::resample_mono_f32;

#[derive(Debug, thiserror::Error)]
pub enum AudioCaptureError {
    #[error("no input device found")]
    NoInputDevice,

    #[error("failed to list input devices: {0}")]
    ListDevices(#[from] cpal::DevicesError),

    #[error("failed to query supported configs: {0}")]
    SupportedConfigs(#[from] cpal::SupportedStreamConfigsError),

    #[error("failed to get default config: {0}")]
    DefaultConfig(#[from] cpal::DefaultStreamConfigError),

    #[error("failed to build input stream: {0}")]
    BuildStream(#[from] cpal::BuildStreamError),

    #[error("failed to play stream: {0}")]
    PlayStream(#[from] cpal::PlayStreamError),

    #[error("audio worker failed: {0}")]
    Worker(String),

    #[error("audio worker startup timeout")]
    WorkerTimeout,

    #[error("recording stop timed out")]
    StopTimeout,

    #[error("failed to resample: {0}")]
    Resample(#[from] anyhow::Error),

    #[error("recording not started")]
    NotStarted,

    #[error("internal channel error")]
    Channel,
}

pub struct CapturedAudio {
    pub sample_rate_hz: u32,
    pub samples: Vec<f32>,
}

pub struct AudioRecorder {
    cmd_tx: mpsc::Sender<Cmd>,
    worker_handle: Option<std::thread::JoinHandle<()>>,
    sample_rate_hz: u32,
    level_cb: Arc<Mutex<Option<Arc<dyn Fn(&[f32]) + Send + Sync + 'static>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioInputDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone)]
struct EnumeratedInputDevice {
    id: String,
    name: String,
    is_default: bool,
    device: Device,
}

impl AudioRecorder {
    pub fn set_level_callback<F>(&self, cb: F)
    where
        F: Fn(&[f32]) + Send + Sync + 'static,
    {
        let mut guard = self.level_cb.lock().unwrap();
        *guard = Some(Arc::new(cb));
    }
}

enum Cmd {
    Start,
    Stop(mpsc::Sender<Vec<f32>>),
    Shutdown,
}

enum WorkerMsg {
    Ready,
    Error(String),
}

impl AudioRecorder {
    fn build_input_device_id(name: &str, occurrence: usize) -> String {
        format!("cpal:{occurrence}:{name}")
    }

    fn enumerate_input_devices(
        host: &cpal::Host,
    ) -> Result<Vec<EnumeratedInputDevice>, AudioCaptureError> {
        let default_name = host.default_input_device().and_then(|d| d.name().ok());
        let mut default_marked = false;
        let mut counts_by_name: HashMap<String, usize> = HashMap::new();
        let mut out = Vec::new();

        for dev in host.input_devices()? {
            let Ok(name) = dev.name() else {
                continue;
            };

            let occurrence = counts_by_name
                .entry(name.clone())
                .and_modify(|n| *n += 1)
                .or_insert(1);

            let is_default = match default_name.as_ref() {
                Some(default_name) if !default_marked && default_name == &name => {
                    default_marked = true;
                    true
                }
                _ => false,
            };

            out.push(EnumeratedInputDevice {
                id: Self::build_input_device_id(&name, *occurrence),
                name,
                is_default,
                device: dev,
            });
        }

        Ok(out)
    }

    pub fn list_input_devices() -> Result<Vec<AudioInputDeviceInfo>, AudioCaptureError> {
        let host = cpal::default_host();
        let devices = Self::enumerate_input_devices(&host)?;
        Ok(devices
            .into_iter()
            .map(|d| AudioInputDeviceInfo {
                id: d.id,
                name: d.name,
                is_default: d.is_default,
            })
            .collect())
    }

    pub fn list_input_device_names() -> Result<Vec<String>, AudioCaptureError> {
        let mut out = Self::list_input_devices()?
            .into_iter()
            .map(|d| d.name)
            .collect::<Vec<_>>();
        out.sort();
        out.dedup();
        Ok(out)
    }

    pub fn open_preferred(
        preferred_id: Option<&str>,
        preferred_name: Option<&str>,
    ) -> Result<Self, AudioCaptureError> {
        let host = cpal::default_host();
        let devices = Self::enumerate_input_devices(&host)?;

        if let Some(needle_id) = preferred_id.map(str::trim).filter(|v| !v.is_empty()) {
            if let Some(found) = devices.iter().find(|d| d.id == needle_id) {
                log::info!(
                    "Using preferred input device id: {} ({})",
                    found.id,
                    found.name
                );
                return Self::open(Some(found.device.clone()));
            }

            log::warn!("Preferred input device id not found, falling back: {needle_id}");
        }

        if let Some(needle_name) = preferred_name.map(str::trim).filter(|v| !v.is_empty()) {
            if let Some(found) = devices.iter().find(|d| d.name == needle_name) {
                log::info!(
                    "Using preferred input device name: {} ({})",
                    found.name,
                    found.id
                );
                return Self::open(Some(found.device.clone()));
            }

            log::warn!("Preferred input device name not found, falling back: {needle_name}");
        }

        Self::open_default()
    }

    pub fn open_named(device_name: Option<&str>) -> Result<Self, AudioCaptureError> {
        Self::open_preferred(None, device_name)
    }

    pub fn open_default() -> Result<Self, AudioCaptureError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(AudioCaptureError::NoInputDevice)?;
        Self::open(Some(device))
    }

    pub fn open(device: Option<Device>) -> Result<Self, AudioCaptureError> {
        let host = cpal::default_host();
        let device = match device {
            Some(d) => d,
            None => host
                .default_input_device()
                .ok_or(AudioCaptureError::NoInputDevice)?,
        };

        // Prefer the device's default input config first.
        // We'll resample to 16k later if needed.
        let default_cfg = device.default_input_config()?;
        let sample_rate_hz = default_cfg.sample_rate().0;

        let (sample_tx, sample_rx) = mpsc::channel::<Vec<f32>>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (worker_tx, worker_rx) = mpsc::channel::<WorkerMsg>();

        let level_cb: Arc<Mutex<Option<Arc<dyn Fn(&[f32]) + Send + Sync + 'static>>>> =
            Arc::new(Mutex::new(None));
        let level_cb_worker = level_cb.clone();

        let worker_handle = std::thread::spawn(move || {
            let config = default_cfg;
            let sample_format = config.sample_format();
            let channels = config.channels() as usize;

            let stream = match sample_format {
                SampleFormat::F32 => {
                    build_input_stream::<f32>(&device, &config.clone().into(), channels, sample_tx)
                }
                SampleFormat::I16 => {
                    build_input_stream::<i16>(&device, &config.clone().into(), channels, sample_tx)
                }
                SampleFormat::U16 => {
                    build_input_stream::<u16>(&device, &config.clone().into(), channels, sample_tx)
                }
                SampleFormat::I8 => {
                    build_input_stream::<i8>(&device, &config.clone().into(), channels, sample_tx)
                }
                SampleFormat::U8 => {
                    build_input_stream::<u8>(&device, &config.clone().into(), channels, sample_tx)
                }
                SampleFormat::I32 => {
                    build_input_stream::<i32>(&device, &config.clone().into(), channels, sample_tx)
                }
                SampleFormat::U32 => {
                    build_input_stream::<u32>(&device, &config.clone().into(), channels, sample_tx)
                }
                SampleFormat::F64 => {
                    build_input_stream::<f64>(&device, &config.clone().into(), channels, sample_tx)
                }
                _ => {
                    build_input_stream::<f32>(&device, &config.clone().into(), channels, sample_tx)
                }
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    let _ = worker_tx.send(WorkerMsg::Error(format!("build stream: {e}")));
                    log::error!("Audio stream build failed: {e}");
                    return;
                }
            };

            if let Err(e) = stream.play() {
                let _ = worker_tx.send(WorkerMsg::Error(format!("play stream: {e}")));
                log::error!("Audio stream play failed: {e}");
                return;
            }

            let _ = worker_tx.send(WorkerMsg::Ready);

            run_consumer(sample_rx, cmd_rx, level_cb_worker);
            drop(stream);
        });

        // Block briefly until the worker has either started the stream or failed.
        match worker_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(WorkerMsg::Ready) => {}
            Ok(WorkerMsg::Error(e)) => return Err(AudioCaptureError::Worker(e)),
            Err(mpsc::RecvTimeoutError::Timeout) => return Err(AudioCaptureError::WorkerTimeout),
            Err(_) => return Err(AudioCaptureError::Channel),
        }

        Ok(Self {
            cmd_tx,
            worker_handle: Some(worker_handle),
            sample_rate_hz,
            level_cb,
        })
    }

    pub fn start(&self) -> Result<(), AudioCaptureError> {
        self.cmd_tx
            .send(Cmd::Start)
            .map_err(|_| AudioCaptureError::Channel)
    }

    pub fn stop(&self) -> Result<Vec<f32>, AudioCaptureError> {
        let (resp_tx, resp_rx) = mpsc::channel();
        self.cmd_tx
            .send(Cmd::Stop(resp_tx))
            .map_err(|_| AudioCaptureError::Channel)?;

        resp_rx
            .recv_timeout(Duration::from_secs(3))
            .map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => AudioCaptureError::StopTimeout,
                mpsc::RecvTimeoutError::Disconnected => AudioCaptureError::Channel,
            })
    }

    pub fn close(mut self) -> Result<(), AudioCaptureError> {
        let _ = self.cmd_tx.send(Cmd::Shutdown);
        if let Some(h) = self.worker_handle.take() {
            let _ = h.join();
        }
        Ok(())
    }

    pub fn stop_captured(&self) -> Result<CapturedAudio, AudioCaptureError> {
        let samples = self.stop()?;
        Ok(CapturedAudio {
            sample_rate_hz: self.sample_rate_hz,
            samples,
        })
    }

    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate_hz
    }

    pub fn resample_to_16k(
        samples: &[f32],
        input_rate_hz: u32,
    ) -> Result<Vec<f32>, AudioCaptureError> {
        Ok(resample_mono_f32(samples, input_rate_hz, 16_000)
            .map_err(AudioCaptureError::Resample)?)
    }
}

fn build_input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sample_tx: mpsc::Sender<Vec<f32>>,
) -> Result<Stream, cpal::BuildStreamError>
where
    T: Sample + SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
{
    let output_buffer: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let out_buf = output_buffer.clone();

    let cb = move |data: &[T], _: &cpal::InputCallbackInfo| {
        let mut buf = out_buf.lock().unwrap();
        buf.clear();

        if channels == 1 {
            buf.extend(data.iter().map(|&s| s.to_sample::<f32>()));
        } else {
            // Many multi-channel microphone devices expose channels where only one channel contains
            // the user's voice (or channels can be out of phase). A naive signed average can cancel
            // the signal and produce near-silence.
            //
            // Pick the channel with the highest energy for this chunk.
            let frames = data.len() / channels;
            let mut best_ch = 0usize;
            let mut best_energy = -1.0f32;

            for ch in 0..channels {
                let mut e = 0.0f32;
                for frame in data.chunks_exact(channels).take(frames) {
                    let s = frame[ch].to_sample::<f32>();
                    e += s * s;
                }
                if e > best_energy {
                    best_energy = e;
                    best_ch = ch;
                }
            }

            for frame in data.chunks_exact(channels).take(frames) {
                buf.push(frame[best_ch].to_sample::<f32>());
            }
        }

        let _ = sample_tx.send(buf.clone());
    };

    device.build_input_stream(
        config,
        cb,
        |err| {
            // These errors are crucial to debug “recording started but silent”.
            log::error!("Audio stream error: {err}");
        },
        None,
    )
}

fn run_consumer(
    sample_rx: mpsc::Receiver<Vec<f32>>,
    cmd_rx: mpsc::Receiver<Cmd>,
    level_cb: Arc<Mutex<Option<Arc<dyn Fn(&[f32]) + Send + Sync + 'static>>>>,
) {
    let mut recording = false;
    let mut captured: Vec<f32> = Vec::new();

    loop {
        // Always drain commands promptly, even if the stream is stalled.
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                Cmd::Start => {
                    recording = true;
                    captured.clear();
                }
                Cmd::Stop(resp) => {
                    recording = false;
                    let out = std::mem::take(&mut captured);
                    let _ = resp.send(out);
                }
                Cmd::Shutdown => return,
            }
        }

        match sample_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(samples) => {
                if let Some(cb) = level_cb.lock().unwrap().as_ref() {
                    cb(&samples);
                }
                if recording {
                    captured.extend_from_slice(&samples);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No audio chunk yet; loop around to check commands again.
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}
