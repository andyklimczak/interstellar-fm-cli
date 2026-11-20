use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
    process::{Child, Command},
    sync::mpsc,
    task::JoinHandle,
};

#[derive(Debug)]
pub enum PlayerEvent {
    Started,
    Paused(bool),
    Buffering(bool),
    Metadata(String),
    Failed(String),
}
#[derive(Debug)]
pub enum Control {
    Pause,
    Volume(u8),
    Mute(bool),
    Quit,
}

#[derive(Default)]
pub struct Player {
    task: Option<JoinHandle<()>>,
    commands: Option<mpsc::Sender<Control>>,
}
impl Player {
    pub fn play(
        &mut self,
        path: String,
        url: String,
        generation: u64,
        volume: u8,
        muted: bool,
        events: mpsc::Sender<(u64, PlayerEvent)>,
    ) {
        // A fresh child per station isolates late mpv events from a replacement stream.
        if let Some(task) = self.task.take() {
            task.abort();
        }
        let (tx, rx) = mpsc::channel(32);
        self.commands = Some(tx);
        self.task = Some(tokio::spawn(async move {
            if let Err(error) =
                session(&path, &url, volume, muted, rx, generation, events.clone()).await
            {
                let _ = events
                    .send((generation, PlayerEvent::Failed(format!("{error:#}"))))
                    .await;
            }
        }));
    }
    pub fn control(&self, command: Control) {
        if let Some(tx) = &self.commands {
            let _ = tx.try_send(command);
        }
    }
    pub fn stop(&mut self) {
        self.commands = None;
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
    pub async fn shutdown(&mut self) {
        self.control(Control::Quit);
        if let Some(mut task) = self.task.take()
            && tokio::time::timeout(Duration::from_secs(2), &mut task)
                .await
                .is_err()
        {
            task.abort();
            let _ = task.await;
        }
        self.commands = None;
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Session {
    child: Child,
    lines: tokio::io::Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
    next_id: u64,
    _dir: tempfile::TempDir,
}
impl Session {
    async fn open(path: &str) -> Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("interstellarfm-")
            .tempdir()?;
        let socket = dir.path().join("mpv.sock");
        let mut child = Command::new(path)
            .args([
                "--no-config",
                "--load-scripts=no",
                "--ytdl=no",
                "--idle=yes",
                "--no-video",
                "--audio-display=no",
                "--input-terminal=no",
                "--terminal=no",
                "--load-unsafe-playlists=no",
                // HLS must be able to fetch segments. Restrict protocols instead of
                // disabling references entirely (which also disables HLS).
                "--access-references=yes",
                "--demuxer-lavf-o=protocol_whitelist=[http,https,tcp,tls,crypto,httpproxy]",
                "--network-timeout=10",
            ])
            .arg(format!("--input-ipc-server={}", socket.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .context("Cannot start mpv. Install mpv or set mpv_path in config.toml")?;
        let stream = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(status) = child.try_wait()? {
                    bail!("mpv exited during startup: {status}");
                }
                if let Ok(stream) = UnixStream::connect(&socket).await {
                    return Ok(stream);
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .context("mpv IPC socket did not become ready")??;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            child,
            lines: BufReader::new(reader).lines(),
            writer,
            next_id: 0,
            _dir: dir,
        })
    }
    async fn command(&mut self, command: Value) -> Result<()> {
        self.next_id += 1;
        let mut bytes =
            serde_json::to_vec(&json!({"command": command, "request_id": self.next_id}))?;
        bytes.push(b'\n');
        tokio::time::timeout(Duration::from_secs(2), self.writer.write_all(&bytes))
            .await
            .context("mpv command timed out")??;
        Ok(())
    }
}

async fn session(
    path: &str,
    url: &str,
    volume: u8,
    muted: bool,
    mut commands: mpsc::Receiver<Control>,
    generation: u64,
    events: mpsc::Sender<(u64, PlayerEvent)>,
) -> Result<()> {
    crate::playlist::stream_url(url)?;
    let mut session = Session::open(path).await?;
    for (i, property) in ["pause", "paused-for-cache", "metadata", "media-title"]
        .iter()
        .enumerate()
    {
        session
            .command(json!(["observe_property", i + 1, property]))
            .await?;
    }
    session
        .command(json!(["set_property", "volume", volume]))
        .await?;
    session
        .command(json!(["set_property", "mute", muted]))
        .await?;
    session.command(json!(["loadfile", url, "replace"])).await?;
    let mut started = false;
    let mut watchdog = tokio::time::interval(Duration::from_secs(1));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        tokio::select! {
            command = commands.recv() => {
                match command {
                    Some(Control::Pause) => session.command(json!(["cycle", "pause"])).await?,
                    Some(Control::Volume(value)) => session.command(json!(["set_property", "volume", value])).await?,
                    Some(Control::Mute(value)) => session.command(json!(["set_property", "mute", value])).await?,
                    Some(Control::Quit) | None => {
                        let _ = session.command(json!(["quit"])).await;
                        let _ = tokio::time::timeout(Duration::from_secs(1), session.child.wait()).await;
                        return Ok(());
                    }
                }
            }
            line = session.lines.next_line() => {
                let Some(line) = line? else { bail!("mpv disconnected; press Enter to retry"); };
                let value: Value = serde_json::from_str(&line).context("Invalid mpv IPC response")?;
                if let Some(error) = value.get("error").and_then(Value::as_str)
                    && error != "success" { bail!("mpv: {error}"); }
                let event = match value["event"].as_str() {
                    Some("playback-restart") if !started => { started = true; Some(PlayerEvent::Started) }
                    Some("end-file") => {
                        let reason = value["reason"].as_str().unwrap_or("unknown");
                        bail!("Stream ended ({reason}): {}", value["file_error"].as_str().unwrap_or("press Enter to retry"));
                    }
                    Some("property-change") => property_event(&value),
                    _ => None,
                };
                if let Some(event) = event { let _ = events.send((generation, event)).await; }
            }
            _ = watchdog.tick() => {
                if !started && tokio::time::Instant::now() >= deadline { bail!("Stream did not start within 30 seconds"); }
                if let Some(status) = session.child.try_wait()? { bail!("mpv exited: {status}"); }
            }
        }
    }
}

fn property_event(value: &Value) -> Option<PlayerEvent> {
    match value["name"].as_str()? {
        "pause" => value["data"].as_bool().map(PlayerEvent::Paused),
        "paused-for-cache" => value["data"].as_bool().map(PlayerEvent::Buffering),
        "media-title" => value["data"]
            .as_str()
            .map(|s| PlayerEvent::Metadata(crate::model::clean(s))),
        "metadata" => {
            let data = value["data"].as_object()?;
            let text = data
                .get("icy-title")
                .or_else(|| data.get("title"))
                .or_else(|| data.get("TITLE"))
                .and_then(Value::as_str)?;
            Some(PlayerEvent::Metadata(crate::model::clean(text)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_metadata_and_pause_events() {
        assert!(matches!(
            property_event(&json!({"name":"pause", "data":true})),
            Some(PlayerEvent::Paused(true))
        ));
        assert!(
            matches!(property_event(&json!({"name":"metadata", "data":{"icy-title":"Artist — Track"}})), Some(PlayerEvent::Metadata(s)) if s == "Artist — Track")
        );
    }
    #[tokio::test]
    async fn missing_mpv_reports_actionable_error() {
        let (tx, mut rx) = mpsc::channel(8);
        let mut player = Player::default();
        player.play(
            "/no/such/mpv".into(),
            "https://example.com/live".into(),
            7,
            70,
            false,
            tx,
        );
        let (generation, event) = rx.recv().await.unwrap();
        assert_eq!(generation, 7);
        assert!(matches!(event, PlayerEvent::Failed(s) if s.contains("Install mpv")));
        player.shutdown().await;
    }
}
