use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub async fn request_headers(socket: &mut tokio::net::TcpStream) -> String {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(socket.read_u8().await.expect("HTTP request ended early"));
        assert!(bytes.len() < 16384, "HTTP headers too large");
    }
    String::from_utf8(bytes).unwrap()
}

fn fixture() -> (Engine, App, Station) {
    let client = reqwest::Client::new();
    let (workers, _worker_rx) = mpsc::channel(8);
    let (player_events, _player_rx) = mpsc::channel(8);
    let engine = Engine {
        radio: radio_browser::RadioBrowser::new(client.clone(), None),
        client,
        player: Player::default(),
        workers,
        player_events,
        search_task: None,
        import_task: None,
        refresh_task: None,
        generation: 4,
        recorded: false,
        attempt_started: false,
        retries: 0,
        retry_due: None,
        offline: true,
    };
    let station = Station::custom(
        playlist::stream_url("https://example.com/live").unwrap(),
        "Test radio",
        None,
    );
    let mut app = App::new(config::Config::default(), Library::default());
    app.current = Some(station.clone());
    app.playback = "CONNECTING".into();
    (engine, app, station)
}

#[test]
fn playback_animation_ticks_and_pause_resume_preserve_history() {
    let (mut engine, mut app, _) = fixture();
    engine.player_event(&mut app, 4, PlayerEvent::Started);
    for _ in 0..3 {
        engine.tick(&mut app);
    }
    assert_eq!(app.animation_tick, 3);
    assert_eq!(app.playback, "PLAYING");
    engine.player_event(&mut app, 4, PlayerEvent::Paused(true));
    engine.tick(&mut app);
    assert!(app.paused);
    assert_eq!(app.playback, "PAUSED");
    engine.player_event(&mut app, 4, PlayerEvent::Paused(false));
    assert!(!app.paused);
    assert_eq!(app.playback, "PLAYING");
    assert_eq!(app.library.history.len(), 1);
}

#[test]
fn only_successful_current_attempt_records_history() {
    let (mut engine, mut app, station) = fixture();
    engine.player_event(&mut app, 3, PlayerEvent::Started);
    assert!(app.library.history.is_empty());
    engine.player_event(&mut app, 4, PlayerEvent::Paused(false));
    assert_eq!(app.playback, "CONNECTING");
    engine.player_event(&mut app, 4, PlayerEvent::Started);
    assert_eq!(app.library.history[0].station.id, station.id);
    app.library.history[0].last_played_at = 1;
    engine.player_event(&mut app, 4, PlayerEvent::Started);
    assert_eq!(app.library.history.len(), 1);
    assert_eq!(app.library.history[0].last_played_at, 1);
}

#[test]
fn stop_invalidates_late_events_and_pending_reconnects() {
    let (mut engine, mut app, _) = fixture();
    engine.player_event(&mut app, 4, PlayerEvent::Failed("connection lost".into()));
    assert!(engine.retry_due.is_some());
    engine.effect(&mut app, Effect::Stop);
    assert!(engine.retry_due.is_none());
    engine.player_event(&mut app, 4, PlayerEvent::Started);
    engine.player_event(&mut app, 4, PlayerEvent::Failed("old failure".into()));
    assert_eq!(app.playback, "STOPPED");
    assert!(app.library.history.is_empty());
}

#[test]
fn removing_current_favorite_preserves_playback_and_history() {
    let (_engine, mut app, station) = fixture();
    app.library.add(station.clone());
    app.library.record(station.clone(), 30);
    app.playback = "PLAYING".into();
    let key = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
    assert!(app.key(key('d')).is_empty());
    assert!(matches!(app.mode, app::Mode::Confirm(_)));
    assert!(app.key(key('y')).is_empty());
    assert!(app.library.favorites.is_empty());
    assert_eq!(app.library.history.len(), 1);
    assert_eq!(app.current.as_ref().unwrap().id, station.id);
    assert_eq!(app.playback, "PLAYING");
    app.key(key('2'));
    app.key(key('f'));
    assert_eq!(app.library.favorites[0].id, station.id);
}

#[test]
fn custom_keybindings_work_without_remapping_text_input() {
    let mut config = config::Config::default();
    config.keybindings.insert("add".into(), vec!["x".into()]);
    let mut app = App::new(config, Library::default());
    app.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
    assert!(matches!(app.mode, app::Mode::Normal));
    app.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(matches!(app.mode, app::Mode::Add(_)));
    app.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert!(matches!(&app.mode, app::Mode::Add(form) if form.source == "x"));
}
