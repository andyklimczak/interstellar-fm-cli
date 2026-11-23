use crate::{
    app::{App, Mode, Section},
    model::clean,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap,
    },
};

/// Compact table counts: one decimal below 10 units, otherwise whole units.
fn count(value: Option<u64>) -> String {
    let Some(value) = value else {
        return "—".into();
    };
    if value < 1_000 {
        return value.to_string();
    }
    let units = [
        (1_000_u64, "k"),
        (1_000_000, "m"),
        (1_000_000_000, "b"),
        (1_000_000_000_000, "t"),
    ];
    let mut index = units
        .iter()
        .rposition(|(scale, _)| value >= *scale)
        .unwrap();
    let rounded = |scale: u64| (u128::from(value) + u128::from(scale) / 2) / u128::from(scale);
    // Promote rounded values at boundaries: 999,999 is 1m, not 1000k.
    if rounded(units[index].0) >= 1_000 && index + 1 < units.len() {
        index += 1;
    }
    let (scale, suffix) = units[index];
    if u128::from(value) < u128::from(scale) * 10 {
        let tenths = (u128::from(value) * 10 + u128::from(scale) / 2) / u128::from(scale);
        if tenths.is_multiple_of(10) {
            format!("{}{suffix}", tenths / 10)
        } else {
            format!("{}.{:01}{suffix}", tenths / 10, tenths % 10)
        }
    } else {
        format!("{}{suffix}", rounded(scale))
    }
}

fn exact_count(value: Option<u64>) -> String {
    let Some(value) = value else {
        return "—".into();
    };
    let digits = value.to_string();
    let mut formatted = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}

fn right(text: String) -> Cell<'static> {
    Cell::from(Line::from(text).alignment(Alignment::Right))
}

fn accent(app: &App) -> Color {
    match app.config.accent.as_str() {
        "cyan" => Color::Cyan,
        "green" => Color::Green,
        "blue" => Color::Blue,
        "yellow" => Color::Yellow,
        "red" => Color::Red,
        _ => Color::Magenta,
    }
}
fn panel(title: &str, color: Color) -> Block<'_> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
}

fn playback_indicator(app: &App) -> Span<'static> {
    const METER: [&str; 6] = ["·····", "▂····", "▂▄···", "▂▄▅··", "▂▄▅▆·", "▂▄▅▆█"];
    const THRESHOLDS_DB: [f32; 5] = [-42.0, -32.0, -24.0, -18.0, -12.0];
    let (label, color) = match app.playback.as_str() {
        "PLAYING" => {
            let label = app.audio_level.map_or("▶", |db| {
                METER[THRESHOLDS_DB.partition_point(|threshold| db >= *threshold)]
            });
            (label, Color::Green)
        }
        "PAUSED" => ("Ⅱ", Color::Yellow),
        "ERROR" => ("ERROR", Color::Red),
        "IDLE" => ("IDLE", Color::DarkGray),
        "STOPPED" => ("STOPPED", Color::DarkGray),
        state => (state, Color::Yellow),
    };
    // Keep the station name aligned when switching between bars and pause.
    let padding = 5usize.saturating_sub(Line::from(label).width()) + 2;
    Span::styled(
        format!("{label}{}", " ".repeat(padding)),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let size = frame.area();
    if size.width < 36 || size.height < 18 {
        frame.render_widget(
            Paragraph::new("Interstellar FM\nResize terminal to at least 36 × 18")
                .wrap(Wrap { trim: true }),
            size,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(4),
        Constraint::Length(u16::from(!app.message.is_empty())),
    ])
    .split(size);
    let color = accent(app);
    let help = Line::from(format!("{} help ", app.keymap.hint("help")));
    let header = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(help.width().min(size.width as usize) as u16),
    ])
    .split(rows[0]);
    frame.render_widget(
        Paragraph::new(help)
            .alignment(Alignment::Right)
            .style(Style::default().fg(color)),
        header[1],
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " INTERSTELLAR FM ",
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" · radio from the universe via the internet"),
        ])),
        header[0],
    );
    let content = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(rows[1]);
    let tab_highlight = Style::default().fg(color).add_modifier(Modifier::BOLD);
    frame.render_widget(
        Tabs::new(["1 Saved", "2 History", "3 Search"])
            .select(app.section.index())
            .highlight_style(if app.tabs_focus {
                tab_highlight.add_modifier(Modifier::UNDERLINED)
            } else {
                tab_highlight
            }),
        content[0],
    );
    let main = content[1];
    let panes = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(main);
    let editing = matches!(app.mode, Mode::Editing);
    let query = if app.section == Section::Search {
        &app.query
    } else {
        &app.filters[app.section.index()]
    };
    let title = if app.section == Section::Search {
        format!(
            " Search · {}{}{} ",
            app.sort.label(),
            if app.reversed { " (reversed)" } else { "" },
            if app.searching {
                " · searching…"
            } else {
                ""
            }
        )
    } else {
        format!(" Filter {} ", app.section.title())
    };
    input(frame, panes[0], &title, query, editing, color);
    let stations = app.visible();
    let rows_data: Vec<Row> = stations
        .iter()
        .map(|s| {
            let playing = app
                .current
                .as_ref()
                .is_some_and(|current| current.id == s.id);
            let mark = format!(
                "{}{}",
                if playing { ">" } else { " " },
                if app.library.is_saved(s) { "*" } else { " " }
            );
            let mut cells = vec![Cell::from(mark), Cell::from(clean(&s.name))];
            if main.width >= 70 {
                cells.push(Cell::from(clean(s.country())));
            }
            if main.width >= 85 {
                cells.push(right(if s.bitrate == 0 {
                    "—".into()
                } else {
                    format!("{} kbps", s.bitrate)
                }));
            }
            cells.push(right(count(s.votes)));
            if main.width >= 60 {
                cells.push(right(count(s.clicks)));
            }
            Row::new(cells).style(if playing {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            })
        })
        .collect();
    let mut headers = vec![Cell::from(""), Cell::from("Station")];
    let mut widths = vec![Constraint::Length(2), Constraint::Min(10)];
    if main.width >= 70 {
        headers.push(Cell::from("Country"));
        // Old snapshots can still contain full names until a directory refresh.
        let legacy_names = stations
            .iter()
            .any(|s| s.country_code.is_empty() && !s.legacy_country.is_empty());
        widths.push(Constraint::Length(if legacy_names { 17 } else { 7 }));
    }
    if main.width >= 85 {
        headers.push(right("Bitrate".into()));
        widths.push(Constraint::Length(9));
    }
    headers.push(right("Votes".into()));
    widths.push(Constraint::Length(7));
    if main.width >= 60 {
        headers.push(right("Clicks".into()));
        widths.push(Constraint::Length(7));
    }
    let title = if app.section == Section::Search && app.query.trim().chars().count() < 2 {
        "Discover stations"
    } else {
        app.section.title()
    };
    let title = format!(" {title} ({}) ", stations.len());
    let table = Table::new(rows_data, widths)
        .header(
            Row::new(headers)
                .style(Style::default().fg(Color::DarkGray))
                .bottom_margin(1),
        )
        .block(panel(
            &title,
            if !app.tabs_focus && matches!(app.mode, Mode::Normal) {
                color
            } else {
                Color::DarkGray
            },
        ))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    frame.render_stateful_widget(table, panes[1], &mut app.tables[app.section.index()]);
    if stations.is_empty() && panes[1].height >= 5 {
        let empty = match app.section {
            Section::Saved => "No favorites. Press a to add a stream / .pls, or 3 to search.",
            Section::History => "Play a station to start your listening history.",
            Section::Search if app.searching => "Contacting Radio Browser…",
            Section::Search => "No results. Type / to search, or r to retry.",
        };
        frame.render_widget(
            Paragraph::new(empty).wrap(Wrap { trim: true }),
            Rect {
                x: panes[1].x + 2,
                y: panes[1].y + 3,
                width: panes[1].width.saturating_sub(4),
                height: panes[1].height.saturating_sub(4),
            },
        );
    }
    let current = app
        .current
        .as_ref()
        .map(|s| clean(&s.name))
        .unwrap_or_else(|| "Select a station and press Enter".into());
    let metadata = if app.metadata.is_empty() {
        "LIVE"
    } else {
        &app.metadata
    };
    let player = vec![
        Line::from(vec![playback_indicator(app), Span::raw(current)]),
        Line::from(format!(
            "{} · Volume {}%{}",
            clean(metadata),
            app.library.volume,
            if app.library.muted { " (muted)" } else { "" }
        )),
    ];
    frame.render_widget(
        Paragraph::new(player).block(panel(" Now Playing ", Color::DarkGray)),
        rows[2],
    );
    if !app.message.is_empty() {
        frame.render_widget(
            Paragraph::new(clean(&app.message)).style(Style::default().fg(color)),
            rows[3],
        );
    }
    match &app.mode {
        Mode::Help => {
            let area = popup(frame, 78, 34, " Controls · j/k scroll · Esc close ", color);
            let mut help = app.keymap.help();
            help.push(
                "Search/filter: Tab to results · Enter submit · Esc leave · Ctrl-u clear".into(),
            );
            help.push("Import preview: Space select · a all · Enter save · p save/play".into());
            frame.render_widget(
                Paragraph::new(help.join("\n"))
                    .scroll((app.help_scroll, 0))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Mode::Details(s) => {
            let area = popup(frame, 80, 17, " Station details · Esc close ", color);
            let source = match s.id {
                crate::model::StationId::Custom(_) => "Custom stream",
                _ => "Radio Browser",
            };
            let details = format!(
                "{}\n{}\n\nCountry: {}\nLanguage: {}\nVotes: {} · Clicks: {}\nCodec: {} · {} kbps\nTags: {}\nHomepage: {}\nStream: {}\nPlaylist: {}",
                s.name,
                source,
                s.country(),
                s.language,
                exact_count(s.votes),
                exact_count(s.clicks),
                s.codec,
                s.bitrate,
                s.tags,
                s.homepage,
                s.url,
                s.playlist_source.as_deref().unwrap_or("—")
            );
            frame.render_widget(
                Paragraph::new(clean(&details)).wrap(Wrap { trim: true }),
                area,
            );
        }
        Mode::Confirm(s) => {
            let area = popup(frame, 65, 7, " Remove favorite ", color);
            frame.render_widget(Paragraph::new(format!("Remove {} from Saved?\nPlayback and History are unaffected.\n\ny / Enter remove · n / Esc cancel", clean(&s.name))).wrap(Wrap { trim: true }), area);
        }
        Mode::Add(form) => {
            let area = popup(frame, 85, 12, " Add favorite ", color);
            let chunks = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(1),
            ])
            .split(area);
            input(
                frame,
                chunks[0],
                " Stream URL / .pls URL / local .pls path ",
                &form.source,
                form.field == 0 && !form.busy,
                color,
            );
            input(
                frame,
                chunks[1],
                " Name (optional; single entry only) ",
                &form.name,
                form.field == 1 && !form.busy,
                color,
            );
            frame.render_widget(Paragraph::new(if form.busy { "Reading playlist… Esc cancels" } else { "Tab switch field · Enter preview · Esc cancel\nDirect stream URLs can be saved even while offline." }).wrap(Wrap { trim: true }), chunks[2]);
        }
        Mode::Import(preview) => {
            let area = popup(frame, 90, 22, " Choose favorites to add ", color);
            let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(3)]).split(area);
            let items: Vec<_> = preview
                .entries
                .iter()
                .zip(&preview.selected)
                .map(|(station, selected)| {
                    ListItem::new(format!(
                        "[{}] {} · {}",
                        if *selected { "x" } else { " " },
                        clean(&station.name),
                        clean(&station.url)
                    ))
                })
                .collect();
            let mut state = ListState::default().with_selected(Some(preview.cursor));
            frame.render_stateful_widget(
                List::new(items)
                    .highlight_style(Style::default().bg(Color::DarkGray))
                    .highlight_symbol("> "),
                chunks[0],
                &mut state,
            );
            frame.render_widget(Paragraph::new("Space select · a all/none · Enter save · Esc cancel\np save and play (one selected entry)\nDuplicates are skipped. Saving does not autoplay.").wrap(Wrap { trim: true }), chunks[1]);
        }
        _ => {}
    }
}

fn popup(frame: &mut Frame, width: u16, height: u16, title: &str, color: Color) -> Rect {
    let size = frame.area();
    let width = width.min(size.width.saturating_sub(2));
    let height = height.min(size.height.saturating_sub(2));
    let rect = Rect::new(
        size.x + (size.width - width) / 2,
        size.y + (size.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let block = panel(title, color).style(Style::default().bg(Color::Reset));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    inner
}
fn input(frame: &mut Frame, area: Rect, title: &str, value: &str, focused: bool, color: Color) {
    // Tail display keeps long pasted URLs visible without slicing inside UTF-8.
    let capacity = area.width.saturating_sub(3) as usize;
    let text = clean(value);
    let chars: Vec<_> = text.chars().collect();
    let tail: String = chars[chars.len().saturating_sub(capacity)..]
        .iter()
        .collect();
    let line = if focused {
        Line::from(vec![
            Span::raw(tail),
            Span::styled("▏", Style::default().fg(color)),
        ])
    } else {
        Line::raw(tail)
    };
    frame.render_widget(
        Paragraph::new(line).block(panel(title, if focused { color } else { Color::DarkGray })),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    fn screen(app: &mut App, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 35)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn measured_rms_drives_the_meter_only_while_playing() {
        let mut app = App::new(
            crate::config::Config::default(),
            crate::model::Library::default(),
        );
        app.playback = "PLAYING".into();
        for (level, symbol) in [
            (None, "▶"),
            (Some(f32::NEG_INFINITY), "·····"),
            (Some(-40.0), "▂····"),
            (Some(-20.0), "▂▄▅··"),
            (Some(-8.0), "▂▄▅▆█"),
        ] {
            app.audio_level = level;
            let indicator = playback_indicator(&app);
            assert_eq!(indicator.content.trim(), symbol);
            assert_eq!(indicator.style.fg, Some(Color::Green));
            assert_eq!(indicator.width(), 7);
        }
        for state in [
            "PAUSED",
            "CONNECTING",
            "BUFFERING",
            "RECONNECTING 1/2",
            "ERROR",
            "IDLE",
            "STOPPED",
        ] {
            app.playback = state.into();
            let indicator = playback_indicator(&app);
            if state == "PAUSED" {
                assert_eq!(indicator.content.trim(), "Ⅱ");
                assert_eq!(indicator.width(), 7);
                assert_eq!(indicator.style.fg, Some(Color::Yellow));
            } else {
                assert_eq!(indicator.content.trim(), state);
            }
        }
    }

    #[test]
    fn counts_are_compact_and_handle_rounding_boundaries() {
        assert_eq!(count(None), "—");
        for (value, expected) in [
            (0, "0"),
            (999, "999"),
            (1_000, "1k"),
            (1_049, "1k"),
            (1_050, "1.1k"),
            (2_300, "2.3k"),
            (9_999, "10k"),
            (23_059, "23k"),
            (67_890, "68k"),
            (999_499, "999k"),
            (999_500, "1m"),
            (1_234_567, "1.2m"),
            (23_059_000, "23m"),
            (1_234_567_890, "1.2b"),
            (1_000_000_000_000, "1t"),
        ] {
            assert_eq!(count(Some(value)), expected, "count for {value}");
        }
        assert!(!count(Some(u64::MAX)).is_empty());
    }

    #[test]
    fn details_keep_exact_counts() {
        assert_eq!(exact_count(None), "—");
        assert_eq!(exact_count(Some(0)), "0");
        assert_eq!(exact_count(Some(23_059)), "23,059");
        assert_eq!(exact_count(Some(1_234_567_890)), "1,234,567,890");
    }

    #[test]
    fn table_shows_votes_clicks_and_bitrate_when_space_allows() {
        let mut app = App::new(
            crate::config::Config::default(),
            crate::model::Library::default(),
        );
        let mut station = crate::model::Station::custom(
            url::Url::parse("https://example.com/live").unwrap(),
            "Jazz radio",
            None,
        );
        station.votes = Some(12345);
        station.clicks = Some(67890);
        station.codec = "AAC".into();
        station.bitrate = 128;
        app.library.add(station.clone());
        for width in [120, 85, 84, 70, 60, 36] {
            let text = screen(&mut app, width);
            assert!(text.contains("Votes"));
            assert!(text.contains("12k"));
            assert!(!text.contains("Codec"));
            assert!(!text.contains("AAC"));
            if width >= 85 {
                assert!(text.contains("Bitrate"));
                assert!(text.contains("128 kbps"));
            } else {
                assert!(!text.contains("Bitrate"));
                assert!(!text.contains("kbps"));
            }
            if width >= 60 {
                assert!(text.contains("Clicks"));
                assert!(text.contains("68k"));
            } else {
                assert!(!text.contains("Clicks"));
            }
        }
        app.mode = Mode::Details(station);
        let details = screen(&mut app, 120);
        assert!(details.contains("AAC"));
        assert!(details.contains("128 kbps"));
        assert!(details.contains("12,345"));
        assert!(details.contains("67,890"));
    }

    #[test]
    fn country_column_and_details_use_the_country_code() {
        let mut app = App::new(
            crate::config::Config::default(),
            crate::model::Library::default(),
        );
        let mut station = crate::model::Station::custom(
            url::Url::parse("https://example.com/live").unwrap(),
            "Radio",
            None,
        );
        station.country_code = "US".into();
        station.legacy_country = "Deprecated country name".into();
        app.library.add(station.clone());
        let table = screen(&mut app, 120);
        assert!(table.contains("Country"));
        assert!(table.contains("US"));
        assert!(!table.contains("Deprecated"));
        app.library.favorites[0].country_code.clear();
        app.library.favorites[0].legacy_country = "The United States".into();
        assert!(screen(&mut app, 120).contains("The United States"));
        app.mode = Mode::Details(station);
        assert!(screen(&mut app, 120).contains("Country: US"));
    }

    #[test]
    fn custom_streams_show_unknown_bitrate_and_counts_instead_of_zero() {
        let mut app = App::new(
            crate::config::Config::default(),
            crate::model::Library::default(),
        );
        app.library.add(crate::model::Station::custom(
            url::Url::parse("https://example.com/live").unwrap(),
            "My stream",
            None,
        ));
        let text = screen(&mut app, 120);
        assert!(text.contains("Votes"));
        assert!(text.contains("Clicks"));
        assert!(text.contains("Bitrate"));
        assert!(!text.contains("0 kbps"));
        assert_eq!(text.matches('—').count(), 4);
    }

    #[test]
    fn tabs_stay_at_the_top_and_content_uses_full_width_at_every_size() {
        for section in [Section::Saved, Section::History, Section::Search] {
            for width in [36, 60, 85, 120, 160] {
                let mut app = App::new(
                    crate::config::Config::default(),
                    crate::model::Library::default(),
                );
                app.section = section;
                let mut terminal = Terminal::new(TestBackend::new(width, 35)).unwrap();
                terminal.draw(|frame| draw(frame, &mut app)).unwrap();
                let buffer = terminal.backend().buffer();
                let tabs: String = (0..width).map(|x| buffer[(x, 1)].symbol()).collect();
                for label in ["1 Saved", "2 History", "3 Search"] {
                    assert!(tabs.contains(label), "missing {label} at width {width}");
                }
                assert_eq!(
                    buffer[(0, 3)].symbol(),
                    "┌",
                    "filter starts at the left edge"
                );
                assert_eq!(
                    buffer[(width - 1, 3)].symbol(),
                    "┐",
                    "filter uses the full width"
                );
                assert_eq!(
                    buffer[(0, 6)].symbol(),
                    "┌",
                    "table starts at the left edge"
                );
                assert_eq!(
                    buffer[(width - 1, 6)].symbol(),
                    "┐",
                    "table uses the full width"
                );
                let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
                assert!(!text.contains("Library"));
            }
        }
    }

    #[test]
    fn focused_tab_is_underlined_without_underlining_the_other_tabs() {
        let mut app = App::new(
            crate::config::Config::default(),
            crate::model::Library::default(),
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        for focused in [false, true] {
            app.tabs_focus = focused;
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let saved = (0..120).find(|&x| buffer[(x, 1)].symbol() == "1").unwrap();
            let history = (0..120).find(|&x| buffer[(x, 1)].symbol() == "2").unwrap();
            assert_eq!(
                buffer[(saved, 1)].modifier.contains(Modifier::UNDERLINED),
                focused
            );
            assert!(!buffer[(history, 1)].modifier.contains(Modifier::UNDERLINED));
        }
    }

    #[test]
    fn help_hint_stays_at_the_top_right_even_on_narrow_terminals_and_uses_the_keymap() {
        for (binding, hint) in [("?", "? help "), ("H", "H help ")] {
            let mut config = crate::config::Config::default();
            config
                .keybindings
                .insert("help".into(), vec![binding.into()]);
            let mut app = App::new(config, crate::model::Library::default());
            for width in [36, 60, 120] {
                let mut terminal = Terminal::new(TestBackend::new(width, 35)).unwrap();
                terminal.draw(|frame| draw(frame, &mut app)).unwrap();
                let buffer = terminal.backend().buffer();
                let header: String = (0..width).map(|x| buffer[(x, 0)].symbol()).collect();
                assert!(
                    header.ends_with(hint),
                    "wrong help hint at width {width}: {header}"
                );
                assert!(header.contains("INTERSTELLAR FM"));
            }
        }
    }

    #[test]
    fn routine_footer_is_gone_but_action_feedback_and_errors_stay_visible() {
        let mut app = App::new(
            crate::config::Config::default(),
            crate::model::Library::default(),
        );
        for message in ["", "Saved Jazz radio", "Could not save library"] {
            app.message = message.into();
            let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let buffer = terminal.backend().buffer();
            let bottom: String = (0..120).map(|x| buffer[(x, 34)].symbol()).collect();
            if message.is_empty() {
                assert_eq!(buffer[(0, 34)].symbol(), "└", "player uses the last row");
            } else {
                assert!(bottom.contains(message));
            }
            let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
            assert!(!text.contains("enter play"));
            assert!(!text.contains("f favorite"));
            assert!(!text.contains("Welcome!"));
            assert!(text.contains("Now Playing"));
        }
        app.section = Section::Search;
        app.query = "jazz".into();
        app.message.clear();
        assert!(screen(&mut app, 120).contains("Search (0)"));
    }

    #[test]
    fn header_displays_the_universe_tagline() {
        let mut app = App::new(
            crate::config::Config::default(),
            crate::model::Library::default(),
        );
        let mut terminal = Terminal::new(TestBackend::new(120, 35)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let header: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .take(120)
            .map(|cell| cell.symbol())
            .collect();
        assert!(header.contains("radio from the universe via the internet"));
        assert!(!header.contains("terminal radio"));
    }

    #[test]
    fn renders_normal_narrow_tiny_and_overlays() {
        for (width, height) in [(120, 35), (60, 22), (36, 12), (15, 5)] {
            let mut app = App::new(
                crate::config::Config::default(),
                crate::model::Library::default(),
            );
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            app.mode = Mode::Help;
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            app.mode = Mode::Add(Default::default());
            terminal.draw(|f| draw(f, &mut app)).unwrap();
        }
    }
}
