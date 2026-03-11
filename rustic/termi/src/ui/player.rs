use ratatui::{prelude::*, widgets::*};

pub fn draw(f: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Info
            Constraint::Length(3), // Waveform (Sparkline)
            Constraint::Length(3), // Progress
        ])
        .margin(1)
        .split(area);

    // 1. Info Paragraph
    f.render_widget(
        Paragraph::new("Now Playing: Rust_Beats_2026.wav").alignment(Alignment::Center),
        chunks[0]
    );

    // 2. Sparkline (The "Live" Waveform)
    // In a real app, this data would come from your audio buffer
    let data = [0, 4, 3, 7, 2, 1, 6, 5, 0, 8, 3, 2, 7, 4, 1, 9, 3, 5, 2, 8];
    let sparkline = Sparkline::default()
        .block(Block::default().title(" Waveform "))
        .data(&data)
        .style(Style::default().fg(Color::Yellow));
    f.render_widget(sparkline, chunks[1]);

    // 3. Progress Gauge
    let gauge = Gauge::default()
        .block(Block::default().title(" Progress "))
        .gauge_style(Style::default().fg(Color::Cyan))
        .percent(65);
    f.render_widget(gauge, chunks[2]);
}