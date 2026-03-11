use ratatui::{
    prelude::{Constraint, Direction, Layout, *},
    widgets::{Block, Borders, Widget, Paragraph},
};

/// A simple button widget with configurable text and styling
pub struct Button<'a> {
    text: Line<'a>,
    block: Option<Block<'a>>,
    style: Style,
    focused_style: Style,
    is_focused: bool,
}

impl<'a> Button<'a> {
    /// Creates a new button with the given text
    pub fn new<T>(text: T) -> Self
    where
        T: Into<Line<'a>>,
    {
        Self {
            text: text.into(),
            block: None,
            style: Style::default(),
            focused_style: Style::default().fg(Color::Yellow),
            is_focused: false,
        }
    }

    /// Sets the block of the widget
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the style of the widget
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style when the button is focused
    pub fn focused_style(mut self, style: Style) -> Self {
        self.focused_style = style;
        self
    }

    /// Sets whether the button is focused
    pub fn focused(mut self, focused: bool) -> Self {
        self.is_focused = focused;
        self
    }
}

impl<'a> Widget for Button<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Determine the style based on focus state
        let current_style = if self.is_focused {
            self.focused_style
        } else {
            self.style
        };

        // Create a default block with borders if none is provided, or apply borders to the existing block
        let block = self.block.unwrap_or_else(|| Block::default())
            .borders(Borders::ALL)
            .style(current_style);

        let inner_area = block.inner(area);
        block.render(area, buf);

        // Render the button content as a paragraph
        let mut spans = vec![Span::raw(" ")];
        spans.extend(self.text.spans);
        spans.push(Span::raw(" "));
        let padded_text = Line::from(spans).alignment(Alignment::Center);

        let paragraph = Paragraph::new(padded_text)
            .style(current_style);

        paragraph.render(inner_area, buf);
    }
}

/// A button widget with configurable text and key combination display
pub struct KeyButton<'a> {
    title: &'a str,
    key_combo: &'a str,
    block: Option<Block<'a>>,
    style: Style,
    focused_style: Style,
    is_focused: bool,
}

impl<'a> KeyButton<'a> {
    /// Creates a new key button with the given title and key combination
    pub fn new(title: &'a str, key_combo: &'a str) -> Self {
        Self {
            title,
            key_combo,
            block: None,
            style: Style::default(),
            focused_style: Style::default().fg(Color::Yellow),
            is_focused: false,
        }
    }

    /// Sets the block of the widget
    pub fn block(mut self, block: Block<'a>) -> Self {
        self.block = Some(block);
        self
    }

    /// Sets the style of the widget
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Sets the style when the button is focused
    pub fn focused_style(mut self, style: Style) -> Self {
        self.focused_style = style;
        self
    }

    /// Sets whether the button is focused
    pub fn focused(mut self, focused: bool) -> Self {
        self.is_focused = focused;
        self
    }
}

impl<'a> Widget for KeyButton<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Determine the style based on focus state
        let current_style = if self.is_focused {
            self.focused_style
        } else {
            self.style
        };

        // Create a default block with borders if none is provided, or apply borders to the existing block
        let block = self.block.unwrap_or_else(|| Block::default())
            .borders(Borders::ALL)
            .style(current_style);

        let inner_area = block.inner(area);
        block.render(area, buf);

        // Create a layout to divide the inner area for title and key_combo
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(inner_area);

        // Render the title as a paragraph
        let title_paragraph = Paragraph::new(format!(" {} ", self.title))
            .alignment(Alignment::Center)
            .style(current_style);
        title_paragraph.render(chunks[0], buf);

        // Render the key_combo as a paragraph with a specific style
        let key_combo_paragraph = Paragraph::new(format!(" {} ", self.key_combo))
            .alignment(Alignment::Center)
            .style(Style::default().fg(Color::Blue));
        key_combo_paragraph.render(chunks[1], buf);
    }
}