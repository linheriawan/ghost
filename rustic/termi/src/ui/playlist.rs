use ratatui::{prelude::*, widgets::*};

pub fn draw(f: &mut Frame, area: Rect) {
    let rows = vec![
        Row::new(vec!["01", "Holy Wars", "06:32"]),
        Row::new(vec!["02", "Hangar 18", "05:11"]),
        Row::new(vec!["03", "Take No Prisoners", "03:26"]),
    ];

    // Widget: Table (Structured data)
    let table = Table::new(rows, [
            Constraint::Length(4),
            Constraint::Min(20),
            Constraint::Length(10),
        ])
        .header(Row::new(vec!["#", "Title", "Duration"]).style(Style::default().bold()))
        .block(Block::default().title(" Track List ").borders(Borders::ALL))
        .column_spacing(1);

    f.render_widget(table, area);
}