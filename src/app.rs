use crate::{
    config::Config,
    model::{Library, Station, clean},
    radio_browser::Sort,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Saved,
    History,
    Search,
}
impl Section {
    pub fn index(self) -> usize {
        match self {
            Self::Saved => 0,
            Self::History => 1,
            Self::Search => 2,
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Saved => "Saved",
            Self::History => "History",
            Self::Search => "Search",
        }
    }
}
#[derive(Default)]
pub struct AddForm {
    pub source: String,
    pub name: String,
    pub field: usize,
    pub busy: bool,
}
pub struct ImportPreview {
    pub entries: Vec<Station>,
    pub selected: Vec<bool>,
    pub cursor: usize,
}
pub enum Mode {
    Normal,
    Editing,
    Add(AddForm),
    Import(ImportPreview),
    Confirm(Station),
    Help,
    Details(Station),
}

pub enum Effect {
    Search,
    Import {
        generation: u64,
        source: String,
        name: String,
    },
    Play(Station),
    Stop,
    Pause,
    Volume(u8),
    Mute(bool),
    Quit,
}

pub struct App {
    pub config: Config,
    pub keymap: crate::input::Keymap,
    pub help_scroll: u16,
    pub library: Library,
    pub section: Section,
    pub tables: [TableState; 3],
    pub mode: Mode,
    pub sidebar_focus: bool,
    pub query: String,
    pub filters: [String; 2],
    pub results: Vec<Station>,
    pub sort: Sort,
    pub reversed: bool,
    pub searching: bool,
    pub loaded: bool,
    pub search_generation: u64,
    pub import_generation: u64,
    pub search_due: Option<Instant>,
    pub message: String,
    pub current: Option<Station>,
    pub playback: String,
    pub metadata: String,
    pub paused: bool,
    pub dirty: bool,
    last_g: Option<Instant>,
}
impl App {
    pub fn new(config: Config, library: Library) -> Self {
        let keymap =
            crate::input::Keymap::new(&config.keybindings).expect("validated configuration");
        Self {
            keymap,
            help_scroll: 0,
            config,
            library,
            section: Section::Saved,
            tables: std::array::from_fn(|_| TableState::default().with_selected(Some(0))),
            mode: Mode::Normal,
            sidebar_focus: false,
            query: String::new(),
            filters: Default::default(),
            results: vec![],
            sort: Sort::default(),
            reversed: false,
            searching: false,
            loaded: false,
            search_generation: 0,
            import_generation: 0,
            search_due: None,
            message: "Welcome! 3 search · a add stream / .pls · ? help".into(),
            current: None,
            playback: "IDLE".into(),
            metadata: String::new(),
            paused: false,
            dirty: false,
            last_g: None,
        }
    }
    pub fn visible(&self) -> Vec<Station> {
        let entries: Vec<Station> = match self.section {
            Section::Saved => self.library.favorites.clone(),
            Section::History => self
                .library
                .history
                .iter()
                .map(|h| h.station.clone())
                .collect(),
            Section::Search => return self.results.clone(),
        };
        let filter = self.filters[self.section.index()].to_lowercase();
        entries
            .into_iter()
            .filter(|s| {
                format!("{} {} {}", s.name, s.country, s.tags)
                    .to_lowercase()
                    .contains(&filter)
            })
            .collect()
    }
    pub fn selected(&self) -> Option<Station> {
        self.visible()
            .get(self.tables[self.section.index()].selected().unwrap_or(0))
            .cloned()
    }
    pub fn move_row(&mut self, delta: isize) {
        let len = self.visible().len();
        let table = &mut self.tables[self.section.index()];
        let next = table
            .selected()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
        table.select(Some(next));
    }
    pub fn clamp(&mut self) {
        self.move_row(0);
    }
    pub fn set_section(&mut self, section: Section) -> Vec<Effect> {
        self.section = section;
        self.last_g = None;
        if section == Section::Search && !self.loaded && !self.searching {
            self.schedule_search(true);
            vec![Effect::Search]
        } else {
            vec![]
        }
    }
    pub fn schedule_search(&mut self, immediate: bool) {
        self.search_generation += 1;
        self.searching = true;
        self.search_due = if immediate {
            None
        } else {
            Some(Instant::now() + Duration::from_millis(self.config.debounce_ms))
        };
    }
    pub fn search_result(&mut self, generation: u64, result: Result<Vec<Station>, String>) {
        if generation != self.search_generation {
            return;
        }
        self.searching = false;
        self.loaded = true;
        match result {
            Ok(stations) => {
                for station in &stations {
                    for saved in &mut self.library.favorites {
                        self.dirty |= saved.update_counts_from(station);
                    }
                    for entry in &mut self.library.history {
                        self.dirty |= entry.station.update_counts_from(station);
                    }
                    if let Some(current) = &mut self.current {
                        current.update_counts_from(station);
                    }
                }
                self.results = stations;
                self.tables[2].select(Some(0));
                self.message = format!("{} stations · Enter play · f favorite", self.results.len());
            }
            Err(error) => {
                self.message = clean(&error);
            }
        }
    }
    pub fn import_result(&mut self, generation: u64, result: Result<Vec<Station>, String>) {
        if generation != self.import_generation {
            return;
        }
        if let Mode::Add(form) = &mut self.mode {
            form.busy = false;
            match result {
                Ok(entries) => {
                    let mut selected = vec![false; entries.len()];
                    if selected.len() == 1 {
                        selected[0] = true;
                    }
                    self.mode = Mode::Import(ImportPreview {
                        entries,
                        selected,
                        cursor: 0,
                    });
                }
                Err(error) => self.message = clean(&error),
            }
        }
    }
    fn toggle_favorite(&mut self) {
        if let Some(station) = self.selected() {
            if self.library.is_saved(&station) {
                self.library.remove(&station.id);
                self.message = format!("Removed {} from favorites", station.name);
            } else {
                self.library.add(station.clone());
                self.message = format!("Saved {}", station.name);
            }
            self.dirty = true;
            self.clamp();
        }
    }
    pub fn key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return vec![Effect::Quit];
        }
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        match mode {
            Mode::Editing => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Tab => self.sidebar_focus = false,
                    KeyCode::Enter => {
                        if self.section == Section::Search {
                            self.schedule_search(true);
                            return vec![Effect::Search];
                        }
                    }
                    _ => {
                        let value = if self.section == Section::Search {
                            &mut self.query
                        } else {
                            &mut self.filters[self.section.index()]
                        };
                        let changed = edit(value, key);
                        self.mode = Mode::Editing;
                        if changed {
                            self.tables[self.section.index()].select(Some(0));
                            if self.section == Section::Search {
                                self.schedule_search(false);
                            }
                        }
                    }
                }
                return vec![];
            }
            Mode::Add(mut form) => {
                if key.code == KeyCode::Esc {
                    self.import_generation += 1;
                    return vec![];
                }
                let mut effects = vec![];
                if !form.busy {
                    match key.code {
                        KeyCode::Tab | KeyCode::BackTab => form.field = 1 - form.field,
                        KeyCode::Enter => {
                            if form.source.trim().is_empty() {
                                self.message = "Enter a stream URL or .pls path".into();
                            } else {
                                form.busy = true;
                                self.import_generation += 1;
                                effects.push(Effect::Import {
                                    generation: self.import_generation,
                                    source: form.source.clone(),
                                    name: form.name.clone(),
                                });
                                self.message = "Reading source…".into();
                            }
                        }
                        _ => {
                            edit(
                                if form.field == 0 {
                                    &mut form.source
                                } else {
                                    &mut form.name
                                },
                                key,
                            );
                        }
                    }
                }
                self.mode = Mode::Add(form);
                return effects;
            }
            Mode::Import(mut preview) => {
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Up | KeyCode::Char('k') => {
                        preview.cursor = preview.cursor.saturating_sub(1);
                        self.mode = Mode::Import(preview);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        preview.cursor =
                            (preview.cursor + 1).min(preview.entries.len().saturating_sub(1));
                        self.mode = Mode::Import(preview);
                    }
                    KeyCode::Char(' ') => {
                        preview.selected[preview.cursor] = !preview.selected[preview.cursor];
                        self.mode = Mode::Import(preview);
                    }
                    KeyCode::Char('a') => {
                        let select = !preview.selected.iter().all(|v| *v);
                        preview.selected.fill(select);
                        self.mode = Mode::Import(preview);
                    }
                    KeyCode::Enter | KeyCode::Char('p') => {
                        let selected: Vec<_> = preview
                            .entries
                            .iter()
                            .zip(&preview.selected)
                            .filter(|(_, yes)| **yes)
                            .map(|(s, _)| s.clone())
                            .collect();
                        if selected.is_empty()
                            || (key.code == KeyCode::Char('p') && selected.len() != 1)
                        {
                            self.message =
                                "Select streams with Space; save-and-play requires exactly one"
                                    .into();
                            self.mode = Mode::Import(preview);
                        } else {
                            let mut added = 0;
                            for station in &selected {
                                if self.library.add(station.clone()) {
                                    added += 1;
                                }
                            }
                            self.dirty = added > 0;
                            self.message = format!(
                                "Added {added} favorite(s); {} already saved",
                                selected.len() - added
                            );
                            self.section = Section::Saved;
                            if key.code == KeyCode::Char('p') {
                                let chosen = &selected[0];
                                let station = self
                                    .library
                                    .favorites
                                    .iter()
                                    .find(|s| crate::model::same_url(&s.url, &chosen.url))
                                    .cloned()
                                    .unwrap_or_else(|| chosen.clone());
                                return vec![Effect::Play(station)];
                            }
                        }
                    }
                    _ => self.mode = Mode::Import(preview),
                }
                return vec![];
            }
            Mode::Confirm(station) => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => {
                        self.library.remove(&station.id);
                        self.dirty = true;
                        self.clamp();
                        self.message = format!("Removed {} from favorites", station.name);
                    }
                    KeyCode::Esc | KeyCode::Char('n') => {}
                    _ => self.mode = Mode::Confirm(station),
                }
                return vec![];
            }
            Mode::Help => {
                match key.code {
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {}
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.help_scroll = self.help_scroll.saturating_add(1).min(27);
                        self.mode = Mode::Help;
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.help_scroll = self.help_scroll.saturating_sub(1);
                        self.mode = Mode::Help;
                    }
                    _ => self.mode = Mode::Help,
                }
                return vec![];
            }
            Mode::Details(station) => {
                if !matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('i')) {
                    self.mode = Mode::Details(station);
                }
                return vec![];
            }
            Mode::Normal => {}
        }
        let key = if key.code == KeyCode::Char('g')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            key
        } else if let Some(mapped) = self.keymap.translate(key) {
            mapped
        } else {
            self.last_g = None;
            return vec![];
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('u') => self.move_row(-10),
                KeyCode::Char('d') => self.move_row(10),
                _ => {}
            }
            return vec![];
        }
        if key.code != KeyCode::Char('g') {
            self.last_g = None;
        }
        match key.code {
            KeyCode::Char('q') => return vec![Effect::Quit],
            KeyCode::Char('1') => return self.set_section(Section::Saved),
            KeyCode::Char('2') => return self.set_section(Section::History),
            KeyCode::Char('3') => return self.set_section(Section::Search),
            KeyCode::Tab | KeyCode::BackTab => self.sidebar_focus = !self.sidebar_focus,
            KeyCode::Down | KeyCode::Char('j') if self.sidebar_focus => {
                return self.set_section(match self.section {
                    Section::Saved => Section::History,
                    Section::History => Section::Search,
                    Section::Search => Section::Saved,
                });
            }
            KeyCode::Up | KeyCode::Char('k') if self.sidebar_focus => {
                return self.set_section(match self.section {
                    Section::Saved => Section::Search,
                    Section::History => Section::Saved,
                    Section::Search => Section::History,
                });
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_row(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_row(-1),
            KeyCode::PageDown => self.move_row(20),
            KeyCode::PageUp => self.move_row(-20),
            KeyCode::Home => self.tables[self.section.index()].select(Some(0)),
            KeyCode::End | KeyCode::Char('G') => self.move_row(isize::MAX),
            KeyCode::Char('g') => {
                if self
                    .last_g
                    .is_some_and(|time| time.elapsed() < Duration::from_millis(700))
                {
                    self.tables[self.section.index()].select(Some(0));
                    self.last_g = None;
                } else {
                    self.last_g = Some(Instant::now());
                }
            }
            KeyCode::Enter if self.sidebar_focus => self.sidebar_focus = false,
            KeyCode::Enter => {
                if let Some(s) = self.selected() {
                    return vec![Effect::Play(s)];
                }
            }
            KeyCode::Char('f') => self.toggle_favorite(),
            KeyCode::Char('d') if self.section == Section::Saved => {
                if let Some(s) = self.selected() {
                    self.mode = Mode::Confirm(s);
                }
            }
            KeyCode::Char('a') => {
                self.import_generation += 1;
                self.mode = Mode::Add(AddForm::default());
                self.message = "Paste a stream URL, .pls URL, or local .pls path".into();
            }
            KeyCode::Char('p' | ' ') => return vec![Effect::Pause],
            KeyCode::Char('s') => return vec![Effect::Stop],
            KeyCode::Char('v' | '+' | '=') => {
                self.library.volume = self.library.volume.saturating_add(5).min(100);
                self.dirty = true;
                return vec![Effect::Volume(self.library.volume)];
            }
            KeyCode::Char('V' | '-') => {
                self.library.volume = self.library.volume.saturating_sub(5);
                self.dirty = true;
                return vec![Effect::Volume(self.library.volume)];
            }
            KeyCode::Char('m') => {
                self.library.muted = !self.library.muted;
                self.dirty = true;
                return vec![Effect::Mute(self.library.muted)];
            }
            KeyCode::Char('/') => {
                self.sidebar_focus = false;
                self.mode = Mode::Editing;
            }
            KeyCode::Char('o') if self.section == Section::Search => {
                self.sort = self.sort.next();
                self.reversed = false;
                self.schedule_search(true);
                return vec![Effect::Search];
            }
            KeyCode::Char('O') if self.section == Section::Search => {
                self.reversed = !self.reversed;
                self.schedule_search(true);
                return vec![Effect::Search];
            }
            KeyCode::Char('r') if self.section == Section::Search => {
                self.schedule_search(true);
                return vec![Effect::Search];
            }
            KeyCode::Char('i') => {
                if let Some(s) = self.selected() {
                    self.mode = Mode::Details(s);
                }
            }
            KeyCode::Char('?') => {
                self.help_scroll = 0;
                self.mode = Mode::Help;
            }
            _ => {}
        }
        vec![]
    }
    pub fn paste(&mut self, text: &str) {
        let text: String = clean(text).chars().take(8192).collect();
        match &mut self.mode {
            Mode::Editing => {
                let value = if self.section == Section::Search {
                    &mut self.query
                } else {
                    &mut self.filters[self.section.index()]
                };
                value.push_str(&text);
                if self.section == Section::Search {
                    self.schedule_search(false);
                }
            }
            Mode::Add(form) if !form.busy => {
                if form.field == 0 {
                    form.source.push_str(&text);
                } else {
                    form.name.push_str(&text);
                }
            }
            _ => {}
        }
    }
}
fn edit(value: &mut String, key: KeyEvent) -> bool {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if key.code == KeyCode::Char('u') {
            value.clear();
            return true;
        }
        return false;
    }
    match key.code {
        KeyCode::Char(c) if !c.is_control() && value.len() < 8192 => {
            value.push(c);
            true
        }
        KeyCode::Backspace => value.pop().is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    #[test]
    fn search_defaults_to_votes_and_cycles_in_the_requested_order() {
        let mut app = App::new(Config::default(), Library::default());
        app.section = Section::Search;
        assert_eq!(app.sort, Sort::Votes);
        assert!(!app.reversed);
        for expected in [Sort::Clicks, Sort::Name, Sort::ClickTrend, Sort::Votes] {
            assert!(matches!(app.key(key('o')).as_slice(), [Effect::Search]));
            assert_eq!(app.sort, expected);
            assert!(!app.reversed);
        }
        assert!(matches!(app.key(key('O')).as_slice(), [Effect::Search]));
        assert!(app.reversed);
    }

    #[test]
    fn search_refreshes_saved_and_history_counts_without_changing_names() {
        let mut app = App::new(Config::default(), Library::default());
        let mut station = Station::custom(
            url::Url::parse("https://example.com/live").unwrap(),
            "My favorite",
            None,
        );
        station.id = crate::model::StationId::RadioBrowser("abc".into());
        app.library.add(station.clone());
        app.library.record(station.clone(), 30);
        app.current = Some(station.clone());
        station.name = "Directory name".into();
        station.votes = Some(123);
        station.clicks = Some(456);
        app.search_result(0, Ok(vec![station]));
        assert!(app.dirty);
        for saved in [
            &app.library.favorites[0],
            &app.library.history[0].station,
            app.current.as_ref().unwrap(),
        ] {
            assert_eq!((saved.votes, saved.clicks), (Some(123), Some(456)));
            assert_eq!(saved.name, "My favorite");
        }
        assert_eq!(app.library.history.len(), 1);
    }

    #[test]
    fn input_never_triggers_shortcuts() {
        let mut app = App::new(Config::default(), Library::default());
        app.section = Section::Search;
        app.key(key('/'));
        for c in "qfp 123".chars() {
            assert!(app.key(key(c)).is_empty());
        }
        assert_eq!(app.query, "qfp 123");
        assert_eq!(app.section, Section::Search);
        assert!(app.library.favorites.is_empty());
    }
    #[test]
    fn tab_leaves_search_input_and_focuses_results_without_resubmitting() {
        let mut app = App::new(Config::default(), Library::default());
        app.section = Section::Search;
        let station = Station::custom(
            url::Url::parse("https://example.com/live").unwrap(),
            "Jazz",
            None,
        );
        app.results = vec![station.clone(), station];
        app.key(key('/'));
        for c in "jazz".chars() {
            app.key(key(c));
        }
        let generation = app.search_generation;
        let due = app.search_due;

        assert!(
            app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
                .is_empty()
        );
        assert!(matches!(app.mode, Mode::Normal));
        assert!(!app.sidebar_focus);
        assert_eq!(app.query, "jazz");
        assert_eq!(app.search_generation, generation);
        assert_eq!(app.search_due, due);
        assert!(app.searching);

        // Navigation now operates on rows, not the query or sidebar.
        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.tables[2].selected(), Some(1));
        assert_eq!(app.query, "jazz");
        assert!(matches!(
            app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .as_slice(),
            [Effect::Play(_)]
        ));
    }

    #[test]
    fn tab_leaves_an_empty_search_field_without_playing() {
        let mut app = App::new(Config::default(), Library::default());
        app.section = Section::Search;
        app.key(key('/'));
        assert!(
            app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
                .is_empty()
        );
        assert!(matches!(app.mode, Mode::Normal));
        assert!(!app.sidebar_focus);
        assert!(app.query.is_empty());
        assert!(
            app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
                .is_empty()
        );
    }

    #[test]
    fn stale_search_and_import_results_are_ignored() {
        let mut app = App::new(Config::default(), Library::default());
        app.search_generation = 2;
        app.search_result(1, Err("old error".into()));
        assert!(!app.message.contains("old error"));
        app.key(key('a'));
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.import_result(1, Ok(vec![]));
        assert!(matches!(app.mode, Mode::Normal));
    }
    #[test]
    fn add_form_is_isolated_and_cancelable() {
        let mut app = App::new(Config::default(), Library::default());
        app.key(key('a'));
        for c in "https://example.com/qfp".chars() {
            assert!(app.key(key(c)).is_empty());
        }
        assert!(matches!(&app.mode, Mode::Add(form) if form.source == "https://example.com/qfp"));
        app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.library.favorites.is_empty());
    }
    #[test]
    fn extreme_navigation_is_safe_for_empty_lists() {
        let mut app = App::new(Config::default(), Library::default());
        app.move_row(isize::MAX);
        app.move_row(isize::MIN);
        assert!(app.selected().is_none());
    }
}
