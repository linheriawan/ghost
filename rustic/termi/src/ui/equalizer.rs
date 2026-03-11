use ratatui::{prelude::*, widgets::*};

pub fn draw(f: &mut Frame, area: Rect) {
    // 
    let bars = [
        ("60Hz", 50), ("150Hz", 80), ("400Hz", 20),
        ("1kHz", 60), ("2.4kHz", 90), ("15kHz", 30),
    ];

    let barchart = BarChart::default()
        .block(Block::default().title(" Equalizer Bars ").borders(Borders::ALL))
        .data(&bars)
        .bar_width(7)
        .bar_style(Style::default().fg(Color::Green))
        .value_style(Style::default().fg(Color::Black).bg(Color::Green));

    f.render_widget(barchart, area);
}