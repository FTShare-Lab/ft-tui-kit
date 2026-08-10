use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{App, Focus};

const BACKGROUND: Color = Color::Rgb(10, 14, 18);
const CARD_BACKGROUND: Color = Color::Rgb(16, 22, 27);
const ACTIVE_BACKGROUND: Color = Color::Rgb(25, 32, 38);
const SELECTED_BACKGROUND: Color = Color::Rgb(10, 38, 43);
const HIGHLIGHTED_BACKGROUND: Color = Color::Rgb(45, 38, 14);
const MUTED: Color = Color::Rgb(138, 148, 156);
const BORDER: Color = Color::Rgb(72, 82, 90);
const CYAN: Color = Color::Rgb(84, 205, 214);
const GOLD: Color = Color::Rgb(255, 205, 92);
const LINK: Color = Color::Rgb(99, 174, 255);
const CARD_HEIGHT: u16 = 8;
const CARD_GAP: u16 = 1;

#[derive(Debug, Clone, Default)]
pub struct ViewInfo {
    pub search_box: Rect,
    pub search_button: Rect,
    pub list_area: Rect,
    pub explain_button: Rect,
    pub clear_button: Rect,
    pub cards: Vec<CardHit>,
    pub visible_count: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct CardHit {
    pub index: usize,
    pub area: Rect,
    pub url_area: Option<Rect>,
}

pub fn draw(frame: &mut Frame, app: &App) -> ViewInfo {
    let [search_area, notice_area, list_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(3),
    ])
    .areas(frame.area());

    let (search_box, search_button) = render_search(frame, app, search_area);
    render_notice(frame, app, notice_area);
    let (cards, visible_count) = render_cards(frame, app, list_area);
    let (explain_button, clear_button) = render_footer(frame, app, footer_area);

    ViewInfo {
        search_box,
        search_button,
        list_area,
        explain_button,
        clear_button,
        cards,
        visible_count,
    }
}

fn render_search(frame: &mut Frame, app: &App, area: Rect) -> (Rect, Rect) {
    let button_width = area.width.min(12);
    let [input_area, button_area] =
        Layout::horizontal([Constraint::Min(1), Constraint::Length(button_width)]).areas(area);
    let focused = app.focus == Focus::Search;
    let input_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if focused { CYAN } else { BORDER }))
        .style(Style::new().bg(BACKGROUND))
        .title(" Search news ");
    let input_inner = input_block.inner(input_area);
    let available = usize::from(input_inner.width);
    let (display, cursor_column) = input_window(&app.query_input, app.input_cursor, available);
    frame.render_widget(
        Paragraph::new(display)
            .style(Style::new().fg(Color::White).bg(BACKGROUND))
            .block(input_block),
        input_area,
    );
    if focused && input_inner.width > 0 && input_inner.height > 0 {
        frame.set_cursor_position((
            input_inner
                .x
                .saturating_add(cursor_column.min(available.saturating_sub(1)) as u16),
            input_inner.y,
        ));
    }

    let button_style = if app.loading {
        Style::new().fg(MUTED).bg(BACKGROUND)
    } else if focused {
        Style::new().fg(Color::Black).bg(CYAN)
    } else {
        Style::new().fg(CYAN).bg(BACKGROUND)
    };
    frame.render_widget(
        Paragraph::new(if app.loading { "Loading" } else { "Search" })
            .alignment(Alignment::Center)
            .style(button_style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(if app.loading { BORDER } else { CYAN })),
            ),
        button_area,
    );
    (input_area, button_area)
}

fn render_notice(frame: &mut Frame, app: &App, area: Rect) {
    let count = app.items().len();
    let current = if count == 0 { 0 } else { app.cursor + 1 };
    let notice = format!(
        " FTShare semantic news | current year, latest 15 days only | {current}/{count} | {} selected",
        app.selected.len()
    );
    frame.render_widget(
        Paragraph::new(truncate_text(&notice, usize::from(area.width)))
            .style(Style::new().fg(MUTED).bg(BACKGROUND)),
        area,
    );
}

fn render_cards(frame: &mut Frame, app: &App, area: Rect) -> (Vec<CardHit>, usize) {
    let items = app.items();
    if items.is_empty() {
        let message = if app.loading {
            "Searching FTShare news..."
        } else if app.dataset.is_some() {
            "No news matched this search."
        } else {
            "Enter a query to search recent news."
        };
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .style(Style::new().fg(MUTED).bg(BACKGROUND)),
            area,
        );
        return (Vec::new(), visible_card_count(area));
    }

    let mut cards = Vec::new();
    let mut y = area.y;
    let bottom = area.bottom();
    for (index, item) in items.iter().enumerate().skip(app.offset) {
        if y >= bottom {
            break;
        }
        let remaining = bottom.saturating_sub(y);
        if remaining < 4 {
            break;
        }
        let height = CARD_HEIGHT.min(remaining);
        let card_area = Rect::new(area.x, y, area.width.saturating_sub(1), height);
        let selected = app.selected.contains(&item.news_id);
        let active = index == app.cursor;
        let highlight = app.highlight(&item.news_id);
        let border_color = if selected {
            CYAN
        } else if highlight.is_some() {
            GOLD
        } else if active {
            Color::White
        } else {
            BORDER
        };
        let background = if selected {
            SELECTED_BACKGROUND
        } else if highlight.is_some() {
            HIGHLIGHTED_BACKGROUND
        } else if active {
            ACTIVE_BACKGROUND
        } else {
            CARD_BACKGROUND
        };
        let markers = match (selected, highlight.is_some()) {
            (true, true) => "[x] [AI]",
            (true, false) => "[x]",
            (false, true) => "[AI]",
            (false, false) => "[ ]",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(border_color).add_modifier(if active {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }))
            .style(Style::new().bg(background))
            .title(format!(" {} {markers} ", index + 1));
        let inner = block.inner(card_area);
        let width = usize::from(inner.width);
        let summary = item
            .summary
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                if item.content.is_empty() {
                    "No summary available."
                } else {
                    item.content.as_str()
                }
            });
        let source = match item.media_name.as_deref() {
            Some(media) if media != item.source_site => format!("{} / {media}", item.source_site),
            _ => item.source_site.clone(),
        };
        let published = item
            .publish_time
            .as_deref()
            .map(display_time)
            .unwrap_or_else(|| "Unknown time".into());
        let metadata = format!(
            "Source: {source} | Published: {published} | Match: {:.1}%",
            item.score * 100.0
        );
        let mut lines = vec![
            Line::from(Span::styled(
                truncate_text(&item.title, width),
                Style::new()
                    .fg(if highlight.is_some() {
                        GOLD
                    } else {
                        Color::White
                    })
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                truncate_text(&metadata, width),
                Style::new().fg(MUTED),
            )),
        ];
        let summary_width = width.saturating_sub(9).max(1);
        let summary_lines = wrap_text(summary, summary_width, 2);
        for (line_index, text) in summary_lines.into_iter().enumerate() {
            let prefix = if line_index == 0 {
                "Summary: "
            } else {
                "         "
            };
            lines.push(Line::from(vec![
                Span::styled(prefix, Style::new().fg(MUTED)),
                Span::styled(text, Style::new().fg(Color::Gray)),
            ]));
        }
        while lines.len() < 4 {
            lines.push(Line::default());
        }
        let url_line = lines.len();
        let url_text = item.article_url.as_deref().unwrap_or("Unavailable");
        lines.push(Line::from(vec![
            Span::styled("Original: ", Style::new().fg(MUTED)),
            Span::styled(
                truncate_text(url_text, width.saturating_sub(10)),
                if item.article_url.is_some() {
                    Style::new().fg(LINK).add_modifier(Modifier::UNDERLINED)
                } else {
                    Style::new().fg(MUTED)
                },
            ),
        ]));
        if let Some(reason) = highlight.and_then(|value| value.as_deref()) {
            lines.push(Line::from(vec![
                Span::styled("AI: ", Style::new().fg(GOLD).add_modifier(Modifier::BOLD)),
                Span::styled(
                    truncate_text(reason, width.saturating_sub(4)),
                    Style::new().fg(GOLD),
                ),
            ]));
        }

        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::new().bg(background))
                .block(block),
            card_area,
        );
        let url_area = item.article_url.as_ref().and_then(|_| {
            let row = inner.y.saturating_add(url_line as u16);
            (row < inner.bottom()).then_some(Rect::new(inner.x, row, inner.width, 1))
        });
        cards.push(CardHit {
            index,
            area: card_area,
            url_area,
        });
        y = y.saturating_add(CARD_HEIGHT + CARD_GAP);
    }

    let visible = cards.len().max(visible_card_count(area).min(items.len()));
    if items.len() > visible {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_style(Style::new().fg(BORDER))
            .thumb_style(Style::new().fg(CYAN));
        let mut state = ScrollbarState::new(items.len())
            .position(app.offset)
            .viewport_content_length(visible);
        frame.render_stateful_widget(scrollbar, area, &mut state);
    }
    (cards, visible.max(1))
}

fn render_footer(frame: &mut Frame, app: &App, area: Rect) -> (Rect, Rect) {
    let explain_width = area.width.min(22);
    let remaining = area.width.saturating_sub(explain_width);
    let clear_width = remaining.min(11);
    let [explain_area, clear_area, status_area] = Layout::horizontal([
        Constraint::Length(explain_width),
        Constraint::Length(clear_width),
        Constraint::Min(0),
    ])
    .areas(area);
    let explain_enabled = !app.items().is_empty() && !app.loading;
    let explain_color = if explain_enabled { CYAN } else { BORDER };
    let explain_label = if app.selected.is_empty() {
        "Explain current".to_string()
    } else {
        format!("Explain selected ({})", app.selected.len())
    };
    frame.render_widget(
        Paragraph::new(truncate_text(
            &explain_label,
            usize::from(explain_area.width.saturating_sub(2)),
        ))
        .alignment(Alignment::Center)
        .style(Style::new().fg(explain_color).bg(BACKGROUND))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(explain_color)),
        ),
        explain_area,
    );
    frame.render_widget(
        Paragraph::new("Clear")
            .alignment(Alignment::Center)
            .style(Style::new().fg(if app.selected.is_empty() {
                BORDER
            } else {
                MUTED
            }))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(BORDER)),
            ),
        clear_area,
    );
    frame.render_widget(
        Paragraph::new(truncate_text(
            &format!(" {}", app.status),
            usize::from(status_area.width),
        ))
        .alignment(Alignment::Left)
        .style(Style::new().fg(MUTED).bg(BACKGROUND)),
        status_area,
    );
    (explain_area, clear_area)
}

fn visible_card_count(area: Rect) -> usize {
    usize::from(area.height / (CARD_HEIGHT + CARD_GAP)).max(1)
}

fn display_time(value: &str) -> String {
    value.replacen('T', " ", 1)
}

pub fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x && column < rect.right() && row >= rect.y && row < rect.bottom()
}

fn input_window(value: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let characters: Vec<char> = value.chars().collect();
    let cursor = cursor.min(characters.len());
    let mut start = cursor;
    let mut used = 0;
    while start > 0 {
        let character_width = characters[start - 1].width().unwrap_or(0);
        if used + character_width >= width {
            break;
        }
        used += character_width;
        start -= 1;
    }
    let cursor_column = used;
    let mut end = cursor;
    let mut total = used;
    while end < characters.len() {
        let character_width = characters[end].width().unwrap_or(0);
        if total + character_width > width {
            break;
        }
        total += character_width;
        end += 1;
    }
    (characters[start..end].iter().collect(), cursor_column)
}

fn truncate_text(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if value.width() <= width {
        return value.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let target = width - 3;
    let mut output = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > target {
            break;
        }
        output.push(character);
        used += character_width;
    }
    output.push_str("...");
    output
}

fn wrap_text(value: &str, width: usize, max_lines: usize) -> Vec<String> {
    if width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut all_lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if !line.is_empty() && line_width + character_width > width {
            all_lines.push(line);
            line = String::new();
            line_width = 0;
        }
        if character_width <= width {
            line.push(character);
            line_width += character_width;
        }
    }
    if !line.is_empty() {
        all_lines.push(line);
    }
    if all_lines.len() <= max_lines {
        return all_lines;
    }
    all_lines.truncate(max_lines);
    let last = all_lines.pop().unwrap_or_default();
    all_lines.push(truncate_text(&last, width));
    all_lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_without_exceeding_terminal_width() {
        let text = truncate_text("人工智能行业新闻", 10);
        assert!(text.width() <= 10);
        assert!(text.ends_with("..."));
    }

    #[test]
    fn input_window_keeps_unicode_cursor_visible() {
        let (display, column) = input_window("abc人工智能", 7, 7);
        assert!(display.width() <= 7);
        assert!(column < 7);
    }

    #[test]
    fn wraps_chinese_without_spaces() {
        let lines = wrap_text("人工智能推动金融市场发展", 8, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|line| line.width() <= 8));
    }
}
