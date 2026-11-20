use crate::{
    app::{App, Mode, Section},
    model::clean,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap,
    },
};

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
        Constraint::Length(2),
    ])
    .split(size);
    let color = accent(app);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " INTERSTELLAR FM ",
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" · radio from the universe via the internet"),
        ])),
        rows[0],
    );
    let main = if size.width >= 85 {
        let columns =
            Layout::horizontal([Constraint::Length(17), Constraint::Min(20)]).split(rows[1]);
        let items = [
            format!("1 Saved ({})", app.library.favorites.len()),
            format!("2 History ({})", app.library.history.len()),
            "3 Search".into(),
        ];
        let mut state = ListState::default().with_selected(Some(app.section.index()));
        frame.render_stateful_widget(
            List::new(items)
                .block(panel(
                    " Library ",
                    if app.sidebar_focus {
                        color
                    } else {
                        Color::DarkGray
                    },
                ))
                .highlight_style(Style::default().fg(color).add_modifier(Modifier::BOLD))
                .highlight_symbol("> "),
            columns[0],
            &mut state,
        );
        columns[1]
    } else {
        let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(rows[1]);
        frame.render_widget(
            Tabs::new(["1 Saved", "2 History", "3 Search"])
                .select(app.section.index())
                .highlight_style(Style::default().fg(color).add_modifier(Modifier::BOLD)),
            rows[0],
        );
        rows[1]
    };
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
            let mut cells = vec![mark, clean(&s.name)];
            if main.width >= 70 {
                cells.push(clean(&s.country));
                cells.push(clean(&s.codec));
            }
            if main.width >= 50 {
                cells.push(if s.bitrate == 0 {
                    "—".into()
                } else {
                    s.bitrate.to_string()
                });
            }
            Row::new(cells).style(if playing {
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            })
        })
        .collect();
    let mut headers = vec!["", "Station"];
    let mut widths = vec![Constraint::Length(2), Constraint::Min(10)];
    if main.width >= 70 {
        headers.extend(["Country", "Codec"]);
        widths.extend([Constraint::Length(17), Constraint::Length(7)]);
    }
    if main.width >= 50 {
        headers.push("kbps");
        widths.push(Constraint::Length(5));
    }
    let title = if app.section == Section::Search && app.query.trim().chars().count() < 2 {
        " Popular right now "
    } else {
        app.section.title()
    };
    let table = Table::new(rows_data, widths)
        .header(
            Row::new(headers)
                .style(Style::default().fg(Color::DarkGray))
                .bottom_margin(1),
        )
        .block(panel(
            title,
            if !app.sidebar_focus && matches!(app.mode, Mode::Normal) {
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
        Line::from(vec![
            Span::styled(
                format!("{}  ", app.playback),
                Style::default().fg(if app.playback == "ERROR" {
                    Color::Red
                } else {
                    Color::Green
                }),
            ),
            Span::raw(current),
        ]),
        Line::from(format!(
            "{} · Volume {}%{}",
            clean(metadata),
            app.library.volume,
            if app.library.muted { " (muted)" } else { "" }
        )),
    ];
    frame.render_widget(
        Paragraph::new(player).block(panel(" Player ", Color::DarkGray)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                clean(&app.message),
                Style::default().fg(color),
            )),
            Line::from(format!(
                "{} play · {} favorite · {} add · {} search · {} help · {} quit",
                app.keymap.hint("play"),
                app.keymap.hint("favorite"),
                app.keymap.hint("add"),
                app.keymap.hint("filter"),
                app.keymap.hint("help"),
                app.keymap.hint("quit")
            )),
        ]),
        rows[3],
    );
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
            frame.render_widget(Paragraph::new(clean(&format!("{}\n{}\n\nCountry: {}\nLanguage: {}\nCodec: {} · {} kbps\nTags: {}\nHomepage: {}\nStream: {}\nPlaylist: {}", s.name, source, s.country, s.language, s.codec, s.bitrate, s.tags, s.homepage, s.url, s.playlist_source.as_deref().unwrap_or("—")))).wrap(Wrap { trim: true }), area);
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
