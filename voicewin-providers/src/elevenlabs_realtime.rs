use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, anyhow};
use base64::Engine;
use futures_util::{SinkExt, StreamExt, future};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
use url::Url;

const WS_SEND_TIMEOUT: Duration = Duration::from_secs(3);
const FINALIZE_FAST_PATH_DURATION: Duration = Duration::from_millis(450);

fn join_committed_and_partial(committed: &str, partial: &str) -> String {
    let c = committed.trim();
    let p = partial.trim();

    if c.is_empty() {
        return p.to_string();
    }
    if p.is_empty() {
        return c.to_string();
    }
    format!("{c} {p}")
}

fn should_emit_backpressure_warning(dropped: u64) -> bool {
    // Emit on first drop, then periodically.
    dropped > 0 && (dropped == 1 || dropped % 50 == 0)
}

fn format_ms_as_secs_string(ms: u32) -> String {
    // Format seconds without using floats (stable across platforms).
    let secs = ms / 1000;
    let frac = ms % 1000;

    if frac == 0 {
        return secs.to_string();
    }

    // Trim trailing zeros from the fractional part.
    let mut frac_str = format!("{frac:03}");
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{secs}.{frac_str}")
}

fn format_milli_ratio_string(milli: u32) -> String {
    // Convert 0..1000 to a "0.xxx" string without floats.
    let milli = milli.min(1000);
    let int = milli / 1000;
    let frac = milli % 1000;

    if frac == 0 {
        return int.to_string();
    }

    let mut frac_str = format!("{frac:03}");
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{int}.{frac_str}")
}

fn finalize_settle_duration_from_cfg(cfg: &ElevenLabsRealtimeConfig) -> Duration {
    // Low-latency settle window: keep it short, but long enough to capture
    // "one more" committed segment arriving shortly after the first.
    if cfg.commit_strategy == "vad" {
        if let Some(v) = cfg.vad.as_ref() {
            let ms = v.min_silence_duration_ms.saturating_add(100);
            let ms = ms.clamp(150, 350);
            return Duration::from_millis(ms as u64);
        }
    }
    Duration::from_millis(250)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevenLabsRealtimeConfig {
    pub ws_url: Url,
    pub api_key: String,

    // ElevenLabs query params
    pub model_id: String,
    pub language_code: Option<String>,
    pub sample_rate_hz: u32,
    pub commit_strategy: String, // "vad" or "manual"

    // Optional server-side VAD tuning (only relevant when commit_strategy=vad).
    pub vad: Option<ElevenLabsRealtimeVadParams>,

    // Safety/timeouts
    pub connect_timeout: Duration,
    pub finalize_timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElevenLabsRealtimeVadParams {
    // Spec expects seconds (double). Store ms to avoid floats.
    pub vad_silence_threshold_ms: u32,
    // Spec expects a 0..1 double. Store milli-units (0..1000) to avoid floats.
    pub vad_threshold_milli: u32,
    pub min_speech_duration_ms: u32,
    pub min_silence_duration_ms: u32,
}

impl ElevenLabsRealtimeConfig {
    pub fn production(api_key: impl Into<String>, sample_rate_hz: u32) -> anyhow::Result<Self> {
        Ok(Self {
            ws_url: Url::parse("wss://api.elevenlabs.io/v1/speech-to-text/realtime")
                .context("parse elevenlabs realtime url")?,
            api_key: api_key.into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz,
            commit_strategy: "vad".into(),
            // Latency-oriented defaults for dictation.
            vad: Some(ElevenLabsRealtimeVadParams {
                vad_silence_threshold_ms: 600,
                min_silence_duration_ms: 150,
                vad_threshold_milli: 400,
                min_speech_duration_ms: 100,
            }),
            connect_timeout: Duration::from_secs(10),
            finalize_timeout: Duration::from_secs(5),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtimeEvent {
    SessionStarted { session_id: String },
    LiveText { committed: String, partial: String },
    Warning { kind: String, message: String },
    Error { message_type: String, error: String },
}

#[derive(Debug)]
enum RealtimeCmd {
    AudioChunk { pcm_s16le: Vec<u8>, commit: bool },
    Finalize { respond_to: oneshot::Sender<anyhow::Result<String>> },
    Shutdown,
}

#[derive(Clone)]
pub struct ElevenLabsRealtimeHandle {
    tx: mpsc::Sender<RealtimeCmd>,
}

impl ElevenLabsRealtimeHandle {
    pub fn try_send_audio_chunk(&self, pcm_s16le: Vec<u8>) -> bool {
        self.tx
            .try_send(RealtimeCmd::AudioChunk {
                pcm_s16le,
                commit: false,
            })
            .is_ok()
    }

    pub async fn send_audio_chunk(&self, pcm_s16le: Vec<u8>) -> bool {
        self.tx
            .send(RealtimeCmd::AudioChunk {
                pcm_s16le,
                commit: false,
            })
            .await
            .is_ok()
    }

    pub async fn finalize(&self) -> anyhow::Result<String> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(RealtimeCmd::Finalize { respond_to: tx })
            .await
            .map_err(|_| anyhow!("realtime session closed"))?;
        rx.await.map_err(|_| anyhow!("realtime session closed"))?
    }

    pub async fn shutdown(&self) {
        let _ = self.tx.send(RealtimeCmd::Shutdown).await;
    }
}

pub async fn spawn_realtime_session(
    cfg: ElevenLabsRealtimeConfig,
) -> anyhow::Result<(ElevenLabsRealtimeHandle, mpsc::Receiver<RealtimeEvent>)> {
    if cfg.api_key.trim().is_empty() {
        return Err(anyhow!("missing ElevenLabs API key"));
    }

    let url = build_realtime_ws_url(&cfg)?;

    // `IntoClientRequest` isn't implemented for `url::Url` in tungstenite 0.26 without extra
    // features; convert to string-ish form first.
    let mut req = url
        .as_str()
        .into_client_request()
        .context("build websocket request")?;
    req.headers_mut().insert(
        "xi-api-key",
        cfg.api_key
            .parse()
            .map_err(|_| anyhow!("invalid ElevenLabs API key header"))?,
    );

    let (cmd_tx, mut cmd_rx) = mpsc::channel::<RealtimeCmd>(64);
    let (evt_tx, evt_rx) = mpsc::channel::<RealtimeEvent>(64);

    // Connect with a hard timeout so we can't hang on a bad network.
    let (ws, _resp) = tokio::time::timeout(cfg.connect_timeout, tokio_tungstenite::connect_async(req))
        .await
        .map_err(|_| anyhow!("ElevenLabs realtime connect timed out"))?
        .context("connect elevenlabs realtime websocket")?;

    let (ws_write, mut ws_read) = ws.split();

    // Writer task: keeps reads responsive by ensuring we never await socket writes in the main loop.
    // We keep control messages separate so pongs/finalize flush can't be starved by audio backlog.
    let (out_ctrl_tx, mut out_ctrl_rx) = mpsc::channel::<Message>(32);
    let (out_audio_tx, mut out_audio_rx) = mpsc::channel::<Message>(256);
    tokio::spawn(async move {
        let mut ws_write = ws_write;
        let mut ctrl_closed = false;
        let mut audio_closed = false;

        loop {
            let next_msg: Option<Message> = tokio::select! {
                biased;
                msg = out_ctrl_rx.recv(), if !ctrl_closed => {
                    match msg {
                        Some(m) => Some(m),
                        None => { ctrl_closed = true; None }
                    }
                }
                msg = out_audio_rx.recv(), if !audio_closed => {
                    match msg {
                        Some(m) => Some(m),
                        None => { audio_closed = true; None }
                    }
                }
            };

            let Some(msg) = next_msg else {
                if ctrl_closed && audio_closed {
                    break;
                }
                continue;
            };

            let res = tokio::time::timeout(WS_SEND_TIMEOUT, ws_write.send(msg)).await;
            if !matches!(res, Ok(Ok(()))) {
                break;
            }
        }

        let _ = ws_write.send(Message::Close(None)).await;
    });

    let finalize_timeout = cfg.finalize_timeout;
    let sample_rate_hz = cfg.sample_rate_hz;
    let finalize_settle_duration = finalize_settle_duration_from_cfg(&cfg);
    let finalize_fast_path_duration = FINALIZE_FAST_PATH_DURATION.min(finalize_timeout);

    tokio::spawn(async move {
        let mut committed = String::new();
        let mut partial = String::new();

        let mut dropped_outbound_audio_chunks: u64 = 0;

        // If ElevenLabs reports a session-level error (auth/quota/etc), we treat it as fatal.
        // We keep the details so `finalize()` can return a meaningful error even if the error
        // arrived earlier during recording.
        let mut fatal_error: Option<(String, String)> = None;

        let mut finalize_pending: Option<oneshot::Sender<anyhow::Result<String>>> = None;
        let mut finalize_deadline_sleep: Option<Pin<Box<tokio::time::Sleep>>> = None;
        let mut finalize_settle_sleep: Option<Pin<Box<tokio::time::Sleep>>> = None;
        let mut finalize_fast_path_sleep: Option<Pin<Box<tokio::time::Sleep>>> = None;
        let mut finalize_seen_committed = false;
        let mut finalize_had_partial_at_start = false;
        let mut finalize_updates_since_start: u32 = 0;

        let finalize_ok = |committed: &str, partial: &str| -> anyhow::Result<String> {
            Ok(join_committed_and_partial(committed, partial))
        };

        loop {
            tokio::select! {
                cmd = cmd_rx.recv() => {
                    let Some(cmd) = cmd else { break; };
                    match cmd {
                        RealtimeCmd::AudioChunk { pcm_s16le, commit } => {
                            if fatal_error.is_some() {
                                continue;
                            }

                            let msg = build_input_audio_chunk_message(&pcm_s16le, sample_rate_hz, commit, None);
                            match out_audio_tx.try_send(Message::Text(msg.into())) {
                                Ok(()) => {}
                                Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                                    // Best-effort: drop the chunk rather than stalling reads.
                                    // Surface it to the UI so this isn't silent.
                                    dropped_outbound_audio_chunks = dropped_outbound_audio_chunks.saturating_add(1);
                                    if should_emit_backpressure_warning(dropped_outbound_audio_chunks) {
                                        let _ = evt_tx.try_send(RealtimeEvent::Warning {
                                            kind: "client_backpressure".into(),
                                            message: format!(
                                                "ElevenLabs realtime backpressure: dropped {dropped_outbound_audio_chunks} audio chunks; transcript may be incomplete."
                                            ),
                                        });
                                    }
                                }
                                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                                    let _ = evt_tx.try_send(RealtimeEvent::Error { message_type: "disconnect".into(), error: "websocket closed".into() });
                                    break;
                                }
                            }
                        }
                        RealtimeCmd::Finalize { respond_to } => {
                            if finalize_pending.is_some() {
                                let _ = respond_to.send(Err(anyhow!("finalize already in progress")));
                                continue;
                            }

                            if let Some((t, e)) = fatal_error.take() {
                                let _ = respond_to.send(Err(anyhow!("ElevenLabs realtime error ({t}): {e}")));
                                break;
                            }

                            // Combine VAD during recording with a final manual flush at stop.
                            // We send a short silence chunk with commit=true to force a final commit.
                            let silence = silence_pcm_s16le(sample_rate_hz, 120);
                            let msg = build_input_audio_chunk_message(&silence, sample_rate_hz, true, None);

                            let sent = tokio::time::timeout(
                                Duration::from_secs(1),
                                out_ctrl_tx.send(Message::Text(msg.into())),
                            )
                            .await;
                            if !matches!(sent, Ok(Ok(()))) {
                                let _ = respond_to.send(Err(anyhow!("websocket closed")));
                                break;
                            }

                            finalize_pending = Some(respond_to);
                            finalize_deadline_sleep = Some(Box::pin(tokio::time::sleep(finalize_timeout)));
                            finalize_settle_sleep = None;
                            finalize_fast_path_sleep = Some(Box::pin(tokio::time::sleep(finalize_fast_path_duration)));
                            finalize_seen_committed = false;
                            finalize_had_partial_at_start = !partial.trim().is_empty();
                            finalize_updates_since_start = 0;
                        }
                        RealtimeCmd::Shutdown => {
                            break;
                        }
                    }
                }

                msg = ws_read.next() => {
                    let Some(msg) = msg else { break; };
                    let msg = match msg {
                        Ok(m) => m,
                        Err(_) => {
                            let _ = evt_tx.send(RealtimeEvent::Error { message_type: "disconnect".into(), error: "websocket read failed".into() }).await;
                            break;
                        }
                    };

                    let text = match msg {
                        Message::Text(t) => t.to_string(),
                        Message::Binary(b) => String::from_utf8_lossy(&b).to_string(),
                        Message::Close(_) => break,
                        Message::Ping(p) => {
                            // Best-effort: if we can't respond with Pong, treat as disconnect.
                            match out_ctrl_tx.try_send(Message::Pong(p)) {
                                Ok(()) => {}
                                Err(_) => {
                                    let _ = evt_tx.try_send(RealtimeEvent::Error { message_type: "disconnect".into(), error: "failed to send pong".into() });
                                    break;
                                }
                            }
                            continue;
                        }
                        Message::Pong(_) => continue,
                        _ => continue,
                    };

                    match parse_realtime_message(&text) {
                        Ok(ParsedRealtime::SessionStarted { session_id }) => {
                            let _ = evt_tx.send(RealtimeEvent::SessionStarted { session_id }).await;
                        }
                        Ok(ParsedRealtime::PartialTranscript { text }) => {
                            partial = text;
                            let _ = evt_tx.send(RealtimeEvent::LiveText { committed: committed.clone(), partial: partial.clone() }).await;

                            if finalize_pending.is_some() {
                                finalize_updates_since_start = finalize_updates_since_start.saturating_add(1);
                            }

                            // If we've seen at least one committed segment after finalize, keep waiting
                            // until the transcript has been quiet for a short settle period.
                            if finalize_pending.is_some() && finalize_seen_committed {
                                finalize_settle_sleep = Some(Box::pin(tokio::time::sleep(finalize_settle_duration)));
                            }
                        }
                        Ok(ParsedRealtime::CommittedTranscript { text }) => {
                            if !committed.is_empty() && !committed.ends_with(' ') {
                                committed.push(' ');
                            }
                            committed.push_str(text.trim());
                            partial.clear();
                            let _ = evt_tx.send(RealtimeEvent::LiveText { committed: committed.clone(), partial: partial.clone() }).await;

                            if finalize_pending.is_some() {
                                finalize_updates_since_start = finalize_updates_since_start.saturating_add(1);
                                finalize_seen_committed = true;
                                finalize_settle_sleep = Some(Box::pin(tokio::time::sleep(finalize_settle_duration)));
                            }
                        }
                        Ok(ParsedRealtime::Error { message_type, error }) => {
                            let _ = evt_tx.send(RealtimeEvent::Error { message_type: message_type.clone(), error: error.clone() }).await;

                            if fatal_error.is_none() {
                                fatal_error = Some((message_type.clone(), error.clone()));
                            }

                            if let Some(done) = finalize_pending.take() {
                                let _ = done.send(Err(anyhow!("ElevenLabs realtime error ({message_type}): {error}")));
                                finalize_deadline_sleep = None;
                                finalize_settle_sleep = None;
                                finalize_fast_path_sleep = None;
                                finalize_seen_committed = false;
                                finalize_had_partial_at_start = false;
                                finalize_updates_since_start = 0;
                            }
                        }
                        Err(_) => {
                            // Ignore unknown/bad frames.
                        }
                    }
                }

                _ = async {
                    if let Some(s) = finalize_deadline_sleep.as_mut() {
                        s.as_mut().await;
                    } else {
                        future::pending::<()>().await;
                    }
                } => {
                    if let Some(done) = finalize_pending.take() {
                        // Timeout waiting for a committed transcript; return best-effort.
                        let _ = done.send(finalize_ok(&committed, &partial));
                    }
                    finalize_deadline_sleep = None;
                    finalize_settle_sleep = None;
                    finalize_fast_path_sleep = None;
                    finalize_seen_committed = false;
                    finalize_had_partial_at_start = false;
                    finalize_updates_since_start = 0;
                }

                _ = async {
                    if let Some(s) = finalize_fast_path_sleep.as_mut() {
                        s.as_mut().await;
                    } else {
                        future::pending::<()>().await;
                    }
                } => {
                    // One-shot: disable fast-path after it fires.
                    finalize_fast_path_sleep = None;

                    // Fast-path: if stop didn't produce any new transcript updates and we didn't
                    // have a partial at stop-time, return immediately with best-effort text.
                    let eligible = finalize_pending.is_some()
                        && !finalize_seen_committed
                        && !finalize_had_partial_at_start
                        && finalize_updates_since_start == 0;
                    if eligible {
                        let out = join_committed_and_partial(&committed, &partial);
                        // Only return early if we already have non-empty text.
                        // If the transcript is still empty, keep waiting for the stop-flush commit.
                        if !out.trim().is_empty() {
                            if let Some(done) = finalize_pending.take() {
                            let _ = done.send(Ok(out));
                            finalize_deadline_sleep = None;
                            finalize_settle_sleep = None;
                            finalize_seen_committed = false;
                            finalize_had_partial_at_start = false;
                            finalize_updates_since_start = 0;
                        }
                        }
                    }
                }

                _ = async {
                    if let Some(s) = finalize_settle_sleep.as_mut() {
                        s.as_mut().await;
                    } else {
                        future::pending::<()>().await;
                    }
                } => {
                    if let Some(done) = finalize_pending.take() {
                        let _ = done.send(finalize_ok(&committed, &partial));
                    }
                    finalize_deadline_sleep = None;
                    finalize_settle_sleep = None;
                    finalize_fast_path_sleep = None;
                    finalize_seen_committed = false;
                    finalize_had_partial_at_start = false;
                    finalize_updates_since_start = 0;
                }
            }
        }

        // Best-effort: if finalize is still pending, resolve it with any text we have.
        if let Some(done) = finalize_pending.take() {
            let out = join_committed_and_partial(&committed, &partial);
            if out.trim().is_empty() {
                let _ = done.send(Err(anyhow!("realtime session closed")));
            } else {
                let _ = done.send(Ok(out));
            }
        }

        // Dropping `out_tx` ends the writer task, which will send Close.
    });

    Ok((ElevenLabsRealtimeHandle { tx: cmd_tx }, evt_rx))
}

fn build_realtime_ws_url(cfg: &ElevenLabsRealtimeConfig) -> anyhow::Result<Url> {
    let audio_format = audio_format_query(cfg.sample_rate_hz)?;

    let mut url = cfg.ws_url.clone();
    {
        let mut qp = url.query_pairs_mut();
        qp.append_pair("model_id", &cfg.model_id);
        qp.append_pair("commit_strategy", &cfg.commit_strategy);
        qp.append_pair("audio_format", audio_format);
        // Dictation defaults: we only need text.
        qp.append_pair("include_timestamps", "false");

        // Dictation defaults: we don't consume language detection metadata.
        qp.append_pair("include_language_detection", "false");

        // Server-side VAD tuning (only meaningful when commit_strategy=vad).
        if cfg.commit_strategy == "vad" {
            if let Some(vad) = cfg.vad.as_ref() {
                let silence_secs = format_ms_as_secs_string(vad.vad_silence_threshold_ms);
                let vad_threshold = format_milli_ratio_string(vad.vad_threshold_milli);

                qp.append_pair("vad_silence_threshold_secs", &silence_secs);
                qp.append_pair("vad_threshold", &vad_threshold);
                qp.append_pair(
                    "min_speech_duration_ms",
                    &vad.min_speech_duration_ms.to_string(),
                );
                qp.append_pair(
                    "min_silence_duration_ms",
                    &vad.min_silence_duration_ms.to_string(),
                );
            }
        }

        let lang = cfg
            .language_code
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        if let Some(lang) = lang {
            qp.append_pair("language_code", lang);
        }
    }
    Ok(url)
}

fn audio_format_query(sample_rate_hz: u32) -> anyhow::Result<&'static str> {
    match sample_rate_hz {
        8_000 => Ok("pcm_8000"),
        16_000 => Ok("pcm_16000"),
        22_050 => Ok("pcm_22050"),
        24_000 => Ok("pcm_24000"),
        44_100 => Ok("pcm_44100"),
        48_000 => Ok("pcm_48000"),
        other => Err(anyhow!("unsupported realtime sample rate: {other}")),
    }
}

fn silence_pcm_s16le(sample_rate_hz: u32, duration_ms: u32) -> Vec<u8> {
    let frames = (sample_rate_hz as u64 * duration_ms as u64 / 1000) as usize;
    vec![0u8; frames * 2]
}

fn build_input_audio_chunk_message(
    pcm_s16le: &[u8],
    sample_rate_hz: u32,
    commit: bool,
    previous_text: Option<&str>,
) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(pcm_s16le);
    let mut obj = serde_json::json!({
        "message_type": "input_audio_chunk",
        "audio_base_64": b64,
        "commit": commit,
        "sample_rate": sample_rate_hz,
    });

    if let Some(prev) = previous_text {
        if let Some(map) = obj.as_object_mut() {
            map.insert("previous_text".into(), serde_json::Value::String(prev.to_string()));
        }
    }

    obj.to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedRealtime {
    SessionStarted { session_id: String },
    PartialTranscript { text: String },
    CommittedTranscript { text: String },
    Error { message_type: String, error: String },
}

fn parse_realtime_message(s: &str) -> anyhow::Result<ParsedRealtime> {
    let v: serde_json::Value = serde_json::from_str(s).context("decode realtime json")?;
    let t = v
        .get("message_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("missing message_type"))?;

    match t {
        "session_started" => {
            let session_id = v
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Ok(ParsedRealtime::SessionStarted { session_id })
        }
        "partial_transcript" => {
            let text = v.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ParsedRealtime::PartialTranscript { text })
        }
        "committed_transcript" | "committed_transcript_with_timestamps" => {
            let text = v.get("text").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ParsedRealtime::CommittedTranscript { text })
        }
        // Error family: treat as fatal for realtime session.
        "error"
        | "auth_error"
        | "quota_exceeded"
        | "commit_throttled"
        | "unaccepted_terms"
        | "rate_limited"
        | "queue_overflow"
        | "resource_exhausted"
        | "session_time_limit_exceeded"
        | "input_error"
        | "chunk_size_exceeded"
        | "insufficient_audio_activity"
        | "transcriber_error" => {
            let err = v.get("error").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ParsedRealtime::Error {
                message_type: t.to_string(),
                error: err,
            })
        }
        other => Err(anyhow!("unknown message_type: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn joins_committed_and_partial_text() {
        assert_eq!(join_committed_and_partial("", ""), "");
        assert_eq!(join_committed_and_partial("hello", ""), "hello");
        assert_eq!(join_committed_and_partial("", "par"), "par");
        assert_eq!(join_committed_and_partial("hello", "par"), "hello par");
        assert_eq!(join_committed_and_partial(" hello ", " par "), "hello par");
    }

    #[test]
    fn backpressure_warning_throttles() {
        assert!(!should_emit_backpressure_warning(0));
        assert!(should_emit_backpressure_warning(1));
        assert!(!should_emit_backpressure_warning(2));
        assert!(!should_emit_backpressure_warning(49));
        assert!(should_emit_backpressure_warning(50));
        assert!(should_emit_backpressure_warning(100));
    }

    #[test]
    fn builds_ws_url_language_auto_disables_detection() {
        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse("wss://example.com/v1/speech-to-text/realtime").unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(1),
            finalize_timeout: Duration::from_secs(1),
        };

        let url = build_realtime_ws_url(&cfg).unwrap();
        let qp: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(qp.get("include_timestamps").map(|s| s.as_str()), Some("false"));
        assert_eq!(qp.get("include_language_detection").map(|s| s.as_str()), Some("false"));
        assert!(qp.get("language_code").is_none());
    }

    #[test]
    fn builds_ws_url_explicit_language_disables_detection() {
        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse("wss://example.com/v1/speech-to-text/realtime").unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: Some("en".into()),
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(1),
            finalize_timeout: Duration::from_secs(1),
        };

        let url = build_realtime_ws_url(&cfg).unwrap();
        let qp: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(
            qp.get("include_language_detection").map(|s| s.as_str()),
            Some("false")
        );
        assert_eq!(qp.get("language_code").map(|s| s.as_str()), Some("en"));
    }

    #[test]
    fn builds_ws_url_includes_vad_params_when_configured() {
        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse("wss://example.com/v1/speech-to-text/realtime").unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: Some(ElevenLabsRealtimeVadParams {
                vad_silence_threshold_ms: 600,
                min_silence_duration_ms: 150,
                vad_threshold_milli: 400,
                min_speech_duration_ms: 100,
            }),
            connect_timeout: Duration::from_secs(1),
            finalize_timeout: Duration::from_secs(1),
        };

        let url = build_realtime_ws_url(&cfg).unwrap();
        let qp: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(qp.get("vad_silence_threshold_secs").map(|s| s.as_str()), Some("0.6"));
        assert_eq!(qp.get("vad_threshold").map(|s| s.as_str()), Some("0.4"));
        assert_eq!(qp.get("min_speech_duration_ms").map(|s| s.as_str()), Some("100"));
        assert_eq!(qp.get("min_silence_duration_ms").map(|s| s.as_str()), Some("150"));
    }

    #[test]
    fn parses_partial_and_committed() {
        let p = parse_realtime_message(r#"{"message_type":"partial_transcript","text":"hi"}"#).unwrap();
        assert_eq!(p, ParsedRealtime::PartialTranscript { text: "hi".into() });

        let c = parse_realtime_message(r#"{"message_type":"committed_transcript","text":"hello"}"#).unwrap();
        assert_eq!(c, ParsedRealtime::CommittedTranscript { text: "hello".into() });
    }

    #[test]
    fn parses_error_message_types() {
        // Treat all known error message types as a typed fatal error.
        let types = [
            "error",
            "auth_error",
            "quota_exceeded",
            "commit_throttled",
            "unaccepted_terms",
            "rate_limited",
            "queue_overflow",
            "resource_exhausted",
            "session_time_limit_exceeded",
            "input_error",
            "chunk_size_exceeded",
            "insufficient_audio_activity",
            "transcriber_error",
        ];

        for t in types {
            let s = format!(r#"{{"message_type":"{t}","error":"boom"}}"#);
            let p = parse_realtime_message(&s).unwrap();
            assert_eq!(
                p,
                ParsedRealtime::Error {
                    message_type: t.to_string(),
                    error: "boom".into(),
                }
            );
        }
    }

    #[test]
    fn unknown_message_type_is_rejected() {
        let err = parse_realtime_message(r#"{"message_type":"new_type","text":"hi"}"#)
            .err()
            .unwrap();
        assert!(err.to_string().contains("unknown message_type"));
    }

    #[test]
    fn missing_message_type_is_rejected() {
        let err = parse_realtime_message(r#"{"text":"hi"}"#).err().unwrap();
        assert!(err.to_string().contains("missing message_type"));
    }

    #[tokio::test]
    async fn integration_ws_flow_finalize_returns_text() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            // Announce session
            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"session_started","session_id":"s"}"#.into(),
                ))
                .await;

            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(txt) = msg {
                    if txt.contains("\"commit\":true") {
                        let _ = ws
                            .send(Message::Text(
                                r#"{"message_type":"committed_transcript","text":"final"}"#.into(),
                            ))
                            .await;
                        break;
                    }
                }
            }
        });

        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse(&format!("ws://{addr}/v1/speech-to-text/realtime")).unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(2),
            finalize_timeout: Duration::from_secs(2),
        };

        let (handle, mut events) = spawn_realtime_session(cfg).await.unwrap();
        // Drain initial session_started.
        let _ = events.recv().await;

        let out = handle.finalize().await.unwrap();
        assert!(out.contains("final"));
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn integration_ws_flow_emits_live_text_events() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"session_started","session_id":"s"}"#.into(),
                ))
                .await;

            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(txt) = msg {
                    if txt.contains("\"commit\":true") {
                        let _ = ws
                            .send(Message::Text(
                                r#"{"message_type":"committed_transcript","text":"final"}"#.into(),
                            ))
                            .await;
                        break;
                    } else {
                        let _ = ws
                            .send(Message::Text(
                                r#"{"message_type":"partial_transcript","text":"par"}"#.into(),
                            ))
                            .await;
                    }
                }
            }
        });

        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse(&format!("ws://{addr}/v1/speech-to-text/realtime")).unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(2),
            finalize_timeout: Duration::from_secs(2),
        };

        let (handle, mut events) = spawn_realtime_session(cfg).await.unwrap();
        let _ = events.recv().await; // session_started

        // Send a non-commit chunk and expect a partial transcript event.
        assert!(handle.send_audio_chunk(vec![0u8; 8]).await);

        let got_partial = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match events.recv().await {
                    Some(RealtimeEvent::LiveText { committed: _, partial }) if partial.contains("par") => {
                        return true;
                    }
                    Some(_) => continue,
                    None => return false,
                }
            }
        })
        .await
        .unwrap();

        assert!(got_partial);

        let out = handle.finalize().await.unwrap();
        assert!(out.contains("final"));
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn integration_ws_finalize_fast_path_returns_existing_text_quickly() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"session_started","session_id":"s"}"#.into(),
                ))
                .await;

            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(txt) = msg {
                    if txt.contains("\"commit\":true") {
                        // Intentionally send nothing after stop flush.
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        break;
                    }

                    // Simulate VAD committing during recording.
                    let _ = ws
                        .send(Message::Text(
                            r#"{"message_type":"committed_transcript","text":"hello"}"#.into(),
                        ))
                        .await;
                }
            }
        });

        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse(&format!("ws://{addr}/v1/speech-to-text/realtime")).unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(2),
            finalize_timeout: Duration::from_secs(5),
        };

        let (handle, mut events) = spawn_realtime_session(cfg).await.unwrap();
        let _ = events.recv().await; // session_started

        // Trigger a committed transcript before finalize.
        assert!(handle.send_audio_chunk(vec![0u8; 8]).await);
        loop {
            if let Some(RealtimeEvent::LiveText { committed, .. }) = events.recv().await {
                if committed.contains("hello") {
                    break;
                }
            }
        }

        let out = tokio::time::timeout(Duration::from_millis(900), handle.finalize())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out, "hello");
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn integration_ws_finalize_partial_only_returns_partial_on_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"session_started","session_id":"s"}"#.into(),
                ))
                .await;

            // Respond with a partial transcript to any non-commit chunk; ignore commit flush.
            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(txt) = msg {
                    if txt.contains("\"commit\":true") {
                        continue;
                    }

                    let _ = ws
                        .send(Message::Text(
                            r#"{"message_type":"partial_transcript","text":"par"}"#.into(),
                        ))
                        .await;
                }
            }
        });

        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse(&format!("ws://{addr}/v1/speech-to-text/realtime")).unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(2),
            finalize_timeout: Duration::from_millis(250),
        };

        let (handle, mut events) = spawn_realtime_session(cfg).await.unwrap();
        let _ = events.recv().await; // session_started

        // Trigger at least one partial transcript.
        assert!(handle.send_audio_chunk(vec![0u8; 8]).await);
        loop {
            if let Some(RealtimeEvent::LiveText { partial, .. }) = events.recv().await {
                if partial.contains("par") {
                    break;
                }
            }
        }

        let out = tokio::time::timeout(Duration::from_secs(2), handle.finalize())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out, "par");
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn integration_ws_finalize_captures_multiple_committed_segments() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"session_started","session_id":"s"}"#.into(),
                ))
                .await;

            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(txt) = msg {
                    if txt.contains("\"commit\":true") {
                        let _ = ws
                            .send(Message::Text(
                                r#"{"message_type":"committed_transcript","text":"a"}"#.into(),
                            ))
                            .await;
                        // Keep the gap within the low-latency settle window.
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        let _ = ws
                            .send(Message::Text(
                                r#"{"message_type":"committed_transcript","text":"b"}"#.into(),
                            ))
                            .await;
                        break;
                    }
                }
            }
        });

        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse(&format!("ws://{addr}/v1/speech-to-text/realtime")).unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(2),
            finalize_timeout: Duration::from_secs(2),
        };

        let (handle, mut events) = spawn_realtime_session(cfg).await.unwrap();
        let _ = events.recv().await; // session_started

        let out = tokio::time::timeout(Duration::from_secs(3), handle.finalize())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out, "a b");
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn integration_double_finalize_is_rejected() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"session_started","session_id":"s"}"#.into(),
                ))
                .await;

            while let Some(Ok(msg)) = ws.next().await {
                if let Message::Text(txt) = msg {
                    if txt.contains("\"commit\":true") {
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        let _ = ws
                            .send(Message::Text(
                                r#"{"message_type":"committed_transcript","text":"final"}"#.into(),
                            ))
                            .await;
                        break;
                    }
                }
            }
        });

        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse(&format!("ws://{addr}/v1/speech-to-text/realtime")).unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(2),
            finalize_timeout: Duration::from_secs(2),
        };

        let (handle, mut events) = spawn_realtime_session(cfg).await.unwrap();
        let _ = events.recv().await; // session_started

        let h1 = handle.clone();
        let t1 = tokio::spawn(async move { h1.finalize().await });

        // Give the session loop a moment to set `finalize_pending`.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let err = handle.finalize().await.err().unwrap();
        assert!(err.to_string().contains("finalize already in progress"));

        let ok = tokio::time::timeout(Duration::from_secs(3), t1)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(ok.contains("final"));
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn integration_ws_auth_error_propagates_to_finalize() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"session_started","session_id":"s"}"#.into(),
                ))
                .await;
            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"auth_error","error":"bad key"}"#.into(),
                ))
                .await;

            // Keep the socket open long enough for the client to receive the error.
            let _ = ws.next().await;
        });

        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse(&format!("ws://{addr}/v1/speech-to-text/realtime")).unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(2),
            finalize_timeout: Duration::from_secs(2),
        };

        let (handle, mut events) = spawn_realtime_session(cfg).await.unwrap();
        let _ = events.recv().await; // session_started

        // Wait until the realtime task processes the auth error.
        loop {
            if let Some(RealtimeEvent::Error { message_type, error }) = events.recv().await {
                assert_eq!(message_type, "auth_error");
                assert!(error.contains("bad key"));
                break;
            }
        }

        let err = handle.finalize().await.err().unwrap();
        let s = err.to_string();
        assert!(s.contains("auth_error"));
        assert!(s.contains("bad key"));
        handle.shutdown().await;
    }

    #[tokio::test]
    async fn integration_ws_quota_exceeded_propagates_to_finalize() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = accept_async(stream).await.unwrap();

            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"session_started","session_id":"s"}"#.into(),
                ))
                .await;
            let _ = ws
                .send(Message::Text(
                    r#"{"message_type":"quota_exceeded","error":"no quota"}"#.into(),
                ))
                .await;

            let _ = ws.next().await;
        });

        let cfg = ElevenLabsRealtimeConfig {
            ws_url: Url::parse(&format!("ws://{addr}/v1/speech-to-text/realtime")).unwrap(),
            api_key: "k".into(),
            model_id: "scribe_v2".into(),
            language_code: None,
            sample_rate_hz: 16_000,
            commit_strategy: "vad".into(),
            vad: None,
            connect_timeout: Duration::from_secs(2),
            finalize_timeout: Duration::from_secs(2),
        };

        let (handle, mut events) = spawn_realtime_session(cfg).await.unwrap();
        let _ = events.recv().await; // session_started

        loop {
            if let Some(RealtimeEvent::Error { message_type, error }) = events.recv().await {
                assert_eq!(message_type, "quota_exceeded");
                assert!(error.contains("no quota"));
                break;
            }
        }

        let err = handle.finalize().await.err().unwrap();
        let s = err.to_string();
        assert!(s.contains("quota_exceeded"));
        assert!(s.contains("no quota"));
        handle.shutdown().await;
    }
}
