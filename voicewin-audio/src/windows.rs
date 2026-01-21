//! Windows (and general CPAL) microphone capture.
//!
//! This is a minimal MVP recorder:
//! - Records raw input from the default device
//! - Converts to mono `f32`
//! - Optionally resamples to 16kHz for Whisper
//!
//! It is intentionally simple and avoids VAD for now.

#![cfg(windows)]

use std::sync::{mpsc, Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Sample, SampleFormat, SizedSample, Stream};

use crate::resample::resample_mono_f32;

#[derive(Debug, thiserror::Error)]
pub enum AudioCaptureError {
    #[error("no input device found")]
    NoInputDevice,

    #[error("failed to query supported configs: {0}")]
    SupportedConfigs(#[from] cpal::SupportedStreamConfigsError),

    #[error("failed to build input stream: {0}")]
    BuildStream(#[from] cpal::BuildStreamError),

    #[error("failed to play stream: {0}")]
    PlayStream(#[from] cpal::PlayStreamError),

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

impl AudioRecorder {
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

        let supported = device.supported_input_configs()?;

        let mut best: Option<cpal::SupportedStreamConfigRange> = None;
        for cfg in supported {
            // Prefer configs that include 16kHz.
            if cfg.min_sample_rate().0 <= 16_000 && cfg.max_sample_rate().0 >= 16_000 {
                best = Some(cfg);
                break;
            }
            if best.is_none() {
                best = Some(cfg);
            }
        }

        let best = best.ok_or(AudioCaptureError::NoInputDevice)?;

        // We will explicitly request 16kHz if supported by the selected range, otherwise use max.
        let target_rate = if best.min_sample_rate().0 <= 16_000 && best.max_sample_rate().0 >= 16_000 {
            cpal::SampleRate(16_000)
        } else {
            best.max_sample_rate()
        };

        let config = best.with_sample_rate(target_rate);
        let sample_rate_hz = config.sample_rate().0;

        let (sample_tx, sample_rx) = mpsc::channel::<Vec<f32>>();
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();

        let level_cb: Arc<Mutex<Option<Arc<dyn Fn(&[f32]) + Send + Sync + 'static>>>> =
            Arc::new(Mutex::new(None));
        let level_cb_worker = level_cb.clone();

        let worker_handle = std::thread::spawn(move || {
            let sample_format = config.sample_format();
            let channels = config.channels() as usize;

            let stream = match sample_format {
                SampleFormat::F32 => {
                    build_input_stream::<f32>(&device, &config.into(), channels, sample_tx)
                }
                SampleFormat::I16 => build_input_stream::<i16>(&device, &config.into(), channels, sample_tx),
                SampleFormat::U16 => build_input_stream::<u16>(&device, &config.into(), channels, sample_tx),
                SampleFormat::I8 => build_input_stream::<i8>(&device, &config.into(), channels, sample_tx),
                SampleFormat::U8 => build_input_stream::<u8>(&device, &config.into(), channels, sample_tx),
                SampleFormat::I32 => build_input_stream::<i32>(&device, &config.into(), channels, sample_tx),
                SampleFormat::U32 => build_input_stream::<u32>(&device, &config.into(), channels, sample_tx),
                SampleFormat::F64 => build_input_stream::<f64>(&device, &config.into(), channels, sample_tx),
                // Fall back: attempt f32.
                _ => build_input_stream::<f32>(&device, &config.into(), channels, sample_tx),
            };

            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Audio stream build failed: {e}");
                    return;
                }
            };

            if let Err(e) = stream.play() {
                eprintln!("Audio stream play failed: {e}");
                return;
            }

            run_consumer(sample_rx, cmd_rx, level_cb_worker);
            drop(stream);
        });

        Ok(Self {
            cmd_tx,
            worker_handle: Some(worker_handle),
            sample_rate_hz,
            level_cb,
        })
    }

    pub fn start(&self) -> Result<(), AudioCaptureError> {
        self.cmd_tx.send(Cmd::Start).map_err(|_| AudioCaptureError::Channel)
    }

    pub fn stop(&self) -> Result<Vec<f32>, AudioCaptureError> {
        let (resp_tx, resp_rx) = mpsc::channel();
        self.cmd_tx
            .send(Cmd::Stop(resp_tx))
            .map_err(|_| AudioCaptureError::Channel)?;
        resp_rx.recv().map_err(|_| AudioCaptureError::Channel)
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

    pub fn resample_to_16k(samples: &[f32], input_rate_hz: u32) -> Result<Vec<f32>, AudioCaptureError> {
        Ok(resample_mono_f32(samples, input_rate_hz, 16_000).map_err(AudioCaptureError::Resample)?)
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
            for frame in data.chunks_exact(channels) {
                let mono = frame
                    .iter()
                    .map(|&s| s.to_sample::<f32>())
                    .sum::<f32>()
                    / channels as f32;
                buf.push(mono);
            }
        }

        let _ = sample_tx.send(buf.clone());
    };

    device.build_input_stream(config, cb, |_err| {}, None)
}

fn run_consumer(
    sample_rx: mpsc::Receiver<Vec<f32>>,
    cmd_rx: mpsc::Receiver<Cmd>,
    level_cb: Arc<Mutex<Option<Arc<dyn Fn(&[f32]) + Send + Sync + 'static>>>>,
) {
    let mut recording = false;
    let mut captured: Vec<f32> = Vec::new();

    loop {
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

        match sample_rx.recv() {
            Ok(samples) => {
                if let Some(cb) = level_cb.lock().unwrap().as_ref() {
                    cb(&samples);
                }
                if recording {
                    captured.extend_from_slice(&samples);
                }
            }
            Err(_) => return,
        }

        // Keep looping; commands are polled between chunks.
    }
}
