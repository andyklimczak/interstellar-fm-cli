mod app;
mod config;
mod input;
mod model;
mod player;
mod playlist;
mod radio_browser;
mod storage;
#[cfg(test)]
mod tests;
mod ui;

use anyhow::{Context, Result, bail};
use app::{App, Effect};
use clap::Parser;
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use model::{Library, Station, StationId};
use player::{Control, Player, PlayerEvent};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{IsTerminal, stdout},
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Parser)]
#[command(
    version,
    about = "Internet radio in your terminal — Saved, History, and Radio Browser search"
)]
struct Args {
    /// Override the library directory (also useful for testing an isolated instance)
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Override the TOML configuration file
    #[arg(long)]
    config: Option<PathBuf>,
    /// Disable Radio Browser requests; custom streams and playlist imports still work
    #[arg(long)]
    offline: bool,
    /// Check configuration, library, and mpv without opening the TUI
    #[arg(long)]
    check: bool,
}

enum WorkerEvent {
    Search(u64, Result<Vec<Station>, String>),
    Import(u64, Result<Vec<Station>, String>),
    Refreshed(u64, Option<Box<Station>>),
    StorageError(String),
}

struct Engine {
    client: reqwest::Client,
    radio: radio_browser::RadioBrowser,
    player: Player,
    workers: mpsc::Sender<WorkerEvent>,
    player_events: mpsc::Sender<(u64, PlayerEvent)>,
    search_task: Option<JoinHandle<()>>,
    import_task: Option<JoinHandle<()>>,
    refresh_task: Option<JoinHandle<()>>,
    generation: u64,
    recorded: bool,
    attempt_started: bool,
    retries: u8,
    retry_due: Option<Instant>,
    offline: bool,
}
impl Engine {
    fn effect(&mut self, app: &mut App, effect: Effect) -> bool {
        match effect {
            Effect::Quit => return true,
            Effect::Search => {
                if let Some(task) = self.search_task.take() {
                    task.abort();
                }
                let generation = app.search_generation;
                if self.offline {
                    app.search_result(generation, Err("Radio Browser is disabled (--offline). Press a to add a stream or local .pls.".into()));
                } else {
                    let radio = self.radio.clone();
                    let tx = self.workers.clone();
                    let query = app.query.clone();
                    let sort = app.sort;
                    let reverse = app.reversed;
                    self.search_task = Some(tokio::spawn(async move {
                        let result = radio
                            .search(&query, sort, reverse)
                            .await
                            .map_err(|e| format!("{e:#}"));
                        let _ = tx.send(WorkerEvent::Search(generation, result)).await;
                    }));
                }
            }
            Effect::Import {
                generation,
                source,
                name,
            } => {
                if let Some(task) = self.import_task.take() {
                    task.abort();
                }
                let client = self.client.clone();
                let tx = self.workers.clone();
                self.import_task = Some(tokio::spawn(async move {
                    let result = match tokio::time::timeout(
                        Duration::from_secs(15),
                        playlist::load(&client, &source, &name),
                    )
                    .await
                    {
                        Ok(result) => result.map_err(|e| format!("{e:#}")),
                        Err(_) => Err("Playlist import timed out".into()),
                    };
                    let _ = tx.send(WorkerEvent::Import(generation, result)).await;
                }));
            }
            Effect::Play(station) => {
                if app.current.as_ref().is_some_and(|s| s.id == station.id)
                    && app.paused
                    && self.recorded
                {
                    self.player.control(Control::Pause);
                    return false;
                }
                self.cancel_retry();
                self.recorded = false;
                self.retries = 0;
                app.current = Some(*station);
                self.start(app);
            }
            Effect::Stop => {
                self.cancel_retry();
                self.generation += 1;
                self.player.stop();
                app.playback = "STOPPED".into();
                app.paused = false;
                app.metadata.clear();
            }
            Effect::Pause => {
                if app.current.is_some() && matches!(app.playback.as_str(), "STOPPED" | "ERROR") {
                    self.cancel_retry();
                    self.recorded = false;
                    self.retries = 0;
                    self.start(app);
                } else {
                    self.player.control(Control::Pause);
                }
            }
            Effect::Volume(volume) => self.player.control(Control::Volume(volume)),
            Effect::Mute(muted) => self.player.control(Control::Mute(muted)),
        }
        false
    }
    fn cancel_retry(&mut self) {
        self.retry_due = None;
        if let Some(task) = self.refresh_task.take() {
            task.abort();
        }
    }
    fn start(&mut self, app: &mut App) {
        self.attempt_started = false;
        if let Some(station) = &app.current {
            self.generation += 1;
            app.playback = "CONNECTING".into();
            app.paused = false;
            app.metadata.clear();
            self.player.play(
                app.config.mpv_path.clone(),
                station.url.clone(),
                self.generation,
                app.library.volume,
                app.library.muted,
                self.player_events.clone(),
            );
        }
    }
    fn player_event(&mut self, app: &mut App, generation: u64, event: PlayerEvent) {
        if generation != self.generation {
            return;
        }
        match event {
            PlayerEvent::Started => {
                self.attempt_started = true;
                app.playback = "PLAYING".into();
                app.paused = false;
                if !self.recorded {
                    self.recorded = true;
                    if let Some(station) = &app.current {
                        app.library
                            .record(station.clone(), app.config.history_limit);
                        app.dirty = true;
                        if let StationId::RadioBrowser(id) = &station.id
                            && !self.offline
                        {
                            let radio = self.radio.clone();
                            let id = id.clone();
                            tokio::spawn(async move {
                                radio.click(&id).await;
                            });
                        }
                    }
                }
            }
            PlayerEvent::Paused(paused) => {
                app.paused = paused;
                if self.attempt_started {
                    app.playback = if paused { "PAUSED" } else { "PLAYING" }.into();
                }
            }
            PlayerEvent::Buffering(buffering) => {
                if self.attempt_started && !app.paused {
                    app.playback = if buffering { "BUFFERING" } else { "PLAYING" }.into();
                }
            }
            PlayerEvent::Metadata(text) => app.metadata = text,
            PlayerEvent::Failed(error) => {
                app.message = model::clean(&error);
                app.paused = false;
                if self.retries < 2 && !error.contains("Cannot start mpv") {
                    self.retries += 1;
                    app.playback = format!("RECONNECTING {}/2", self.retries);
                    if self.retries == 1
                        && !self.offline
                        && let Some(Station {
                            id: StationId::RadioBrowser(id),
                            ..
                        }) = &app.current
                    {
                        let id = id.clone();
                        let radio = self.radio.clone();
                        let tx = self.workers.clone();
                        self.refresh_task = Some(tokio::spawn(async move {
                            let station = radio.refresh(&id).await.ok().map(Box::new);
                            let _ = tx.send(WorkerEvent::Refreshed(generation, station)).await;
                        }));
                    } else {
                        self.retry_due =
                            Some(Instant::now() + Duration::from_secs(u64::from(self.retries)));
                    }
                } else {
                    app.playback = "ERROR".into();
                }
            }
        }
    }
    fn worker_event(&mut self, app: &mut App, event: WorkerEvent) {
        match event {
            WorkerEvent::Search(generation, result) => app.search_result(generation, result),
            WorkerEvent::Import(generation, result) => app.import_result(generation, result),
            WorkerEvent::Refreshed(generation, station) if generation == self.generation => {
                if let Some(station) = station {
                    for saved in &mut app.library.favorites {
                        if saved.id == station.id {
                            saved.url = station.url.clone();
                            saved.update_metadata_from(&station);
                            app.dirty = true;
                        }
                    }
                    for entry in &mut app.library.history {
                        if entry.station.id == station.id {
                            entry.station.url = station.url.clone();
                            entry.station.update_metadata_from(&station);
                            app.dirty = true;
                        }
                    }
                    app.current = Some(*station);
                }
                self.retry_due = Some(Instant::now() + Duration::from_secs(1));
            }
            WorkerEvent::StorageError(error) => {
                app.message = format!("Could not save library: {}", model::clean(&error))
            }
            _ => {}
        }
    }
    fn tick(&mut self, app: &mut App) {
        if app.search_due.is_some_and(|due| Instant::now() >= due) {
            app.search_due = None;
            self.effect(app, Effect::Search);
        }
        if self.retry_due.is_some_and(|due| Instant::now() >= due) {
            self.retry_due = None;
            self.start(app);
        }
        if !matches!(app.mode, app::Mode::Add(_))
            && let Some(task) = self.import_task.take()
        {
            task.abort();
        }
    }
    async fn shutdown(&mut self) {
        for task in [
            &mut self.search_task,
            &mut self.import_task,
            &mut self.refresh_task,
        ] {
            if let Some(task) = task.take() {
                task.abort();
            }
        }
        self.player.shutdown().await;
    }
}

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let guard = Self;
        execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste, Hide)?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen, Show);
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let config_path = match args.config {
        Some(path) => {
            if !path.is_file() {
                bail!("Configuration file not found: {}", path.display());
            }
            path
        }
        None => config::user_dir("XDG_CONFIG_HOME", ".config")?.join("config.toml"),
    };
    let config = config::Config::load(&config_path)?;
    let data_dir = match args.data_dir {
        Some(path) => path,
        None => config::user_dir("XDG_DATA_HOME", ".local/share")?,
    };
    let storage = storage::Storage::open(&data_dir)?;
    let library = storage.load(config.history_limit)?;
    if args.check {
        println!(
            "Config: {}{}",
            config_path.display(),
            if config_path.exists() {
                ""
            } else {
                " (defaults)"
            }
        );
        println!(
            "Library: {} ({} favorites, {} history entries)",
            data_dir.display(),
            library.favorites.len(),
            library.history.len()
        );
        let output = std::process::Command::new(&config.mpv_path)
            .arg("--version")
            .output()
            .context("mpv not found; install mpv or set mpv_path in config.toml")?;
        if !output.status.success() {
            bail!("mpv --version failed");
        }
        println!(
            "{}",
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("mpv found")
        );
        return Ok(());
    }
    if !stdout().is_terminal() || !std::io::stdin().is_terminal() {
        bail!("Run interstellarfm-cli in an interactive terminal (or use --check)");
    }
    let client = reqwest::Client::builder()
        .user_agent(concat!("interstellarfm-cli/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let radio = radio_browser::RadioBrowser::new(client.clone(), config.api_base.clone());
    let (worker_tx, mut worker_rx) = mpsc::channel(64);
    let (player_tx, mut player_rx) = mpsc::channel(64);
    let (save_tx, mut save_rx) = mpsc::unbounded_channel::<Library>();
    let storage_events = worker_tx.clone();
    let writer = tokio::task::spawn_blocking(move || -> Result<()> {
        let mut last_error = None;
        while let Some(mut library) = save_rx.blocking_recv() {
            while let Ok(newer) = save_rx.try_recv() {
                library = newer;
            }
            match storage.save(&library) {
                Ok(()) => last_error = None,
                Err(error) => {
                    let message = format!("{error:#}");
                    let _ = storage_events.try_send(WorkerEvent::StorageError(message.clone()));
                    last_error = Some(message);
                }
            }
        }
        if let Some(error) = last_error {
            bail!("Library was not saved: {error}");
        }
        Ok(())
    });
    let mut engine = Engine {
        client,
        radio,
        player: Player::default(),
        workers: worker_tx,
        player_events: player_tx,
        search_task: None,
        import_task: None,
        refresh_task: None,
        generation: 0,
        recorded: false,
        attempt_started: false,
        retries: 0,
        retry_due: None,
        offline: args.offline,
    };
    let mut app = App::new(config, library);
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous_hook(info);
    }));
    let result = run(
        &mut app,
        &mut engine,
        &save_tx,
        &mut worker_rx,
        &mut player_rx,
    )
    .await;
    engine.shutdown().await;
    let _ = save_tx.send(app.library.clone());
    drop(save_tx);
    let saved = writer.await.context("Storage worker failed")?;
    result?;
    saved
}

async fn run(
    app: &mut App,
    engine: &mut Engine,
    saves: &mpsc::UnboundedSender<Library>,
    workers: &mut mpsc::Receiver<WorkerEvent>,
    player: &mut mpsc::Receiver<(u64, PlayerEvent)>,
) -> Result<()> {
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;
    terminal.clear()?;
    let mut input = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;
        let mut quit = false;
        tokio::select! {
            event = input.next() => {
                let Some(event) = event else { break; };
                match event? {
                    Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => { for effect in app.key(key) { quit |= engine.effect(app, effect); } }
                    Event::Paste(text) => app.paste(&text),
                    _ => {}
                }
            }
            Some(event) = workers.recv() => engine.worker_event(app, event),
            Some((generation, event)) = player.recv() => engine.player_event(app, generation, event),
            _ = tick.tick() => engine.tick(app),
            _ = tokio::signal::ctrl_c() => quit = true,
            _ = terminate.recv() => quit = true,
        }
        if app.dirty {
            app.dirty = false;
            let _ = saves.send(app.library.clone());
        }
        if quit {
            break;
        }
    }
    Ok(())
}
