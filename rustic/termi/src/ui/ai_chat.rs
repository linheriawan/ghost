use ratatui::{prelude::*, widgets::*};
use crate::ai::ollama::{OllamaClient, ChatMessage, ChatRequest, OllamaConfig};
use textwrap::fill;

#[derive(PartialEq)]
pub enum ChatFocus {
    Input,
    ModelDropdown,
    ConfigButton,
}

pub struct ChatState {
    pub history: Vec<(String, String)>,
    pub focus: ChatFocus,
    pub show_config: bool,
    pub selected_model: usize,
    pub scroll_offset: usize,
    pub ollama_client: OllamaClient,
    pub available_models: Vec<String>,
}

impl ChatState {
    pub async fn new() -> Self {
        let config = OllamaConfig::default(); // Uses localhost:11434 by default
        let ollama_client = OllamaClient::new(config);
        
        // Fetch available models from Ollama server
        let available_models = match ollama_client.list_models().await {
            Ok(models) => {
                models.into_iter().map(|model| model.name).collect()
            },
            Err(_) => {
                // Fallback to default models if connection fails
                vec!["llama3".to_string(), "mistral".to_string(), "phi".to_string()]
            }
        };
        
        Self {
            history: vec![("AI".to_string(), "Welcome to the AI terminal! Using Ollama for chat.".to_string())],
            focus: ChatFocus::Input,
            show_config: false,
            selected_model: 0,
            scroll_offset: 0,
            ollama_client,
            available_models,
        }
    }
    
    pub async fn refresh_models(&mut self) {
        match self.ollama_client.list_models().await {
            Ok(models) => {
                self.available_models = models.into_iter().map(|model| model.name).collect();
                
                // Adjust selected_model if the list changed
                if self.selected_model >= self.available_models.len() && !self.available_models.is_empty() {
                    self.selected_model = 0;
                }
            },
            Err(_) => {
                // Keep existing models if refresh fails
            }
        }
    }
    
    pub async fn send_message(&mut self, user_input: &str) {
        // Add user message to history
        self.history.push(("User".to_string(), user_input.to_string()));
        
        // Prepare messages for the API call
        let messages: Vec<ChatMessage> = self.history.iter()
            .map(|(role, content)| ChatMessage {
                role: role.clone(),
                content: content.clone(),
            })
            .collect();
        
        // Get the currently selected model
        let current_model = if !self.available_models.is_empty() {
            &self.available_models[self.selected_model % self.available_models.len()]
        } else {
            "llama3" // fallback
        };
        
        // Create the chat request
        let request = ChatRequest {
            model: current_model.to_string(),
            messages,
            stream: Some(false),
            options: None,
        };
        
        // Send the request to Ollama
        match self.ollama_client.chat(&request).await {
            Ok(response) => {
                // Add AI response to history
                self.history.push(("Assistant".to_string(), response.message.content));
                self.scroll_offset = self.history.len() - 1;
            }
            Err(e) => {
                // Add error message to history
                self.history.push(("Error".to_string(), format!("Failed to get response: {}", e)));
                self.scroll_offset = self.history.len() - 1;
            }
        }
    }
}

pub fn draw(f: &mut Frame, area: Rect, state: &ChatState, input_text: &str) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),      // Chat History
            Constraint::Length(3),   // Mid Row
            Constraint::Length(3),   // Input
        ])
        .split(area);

    // 1. Chat History
    let message_width = chunks[0].width.saturating_sub(4); // Calculate available width for messages
    let messages: Vec<ListItem> = state.history.iter().map(|(sender, msg)| {
        let color = if sender == "User" { Color::Blue } else if sender == "Assistant" { Color::Green } else { Color::Red };
        let content = Text::from(vec![
            Line::from(sender.as_str()).style(Style::default().bold().fg(color)),
            Line::from(fill(msg.as_str(), message_width as usize)), // Wrap the message content
        ]);
        ListItem::new(content)
    }).collect();

    let chat_list = List::new(messages)
        .block(Block::default().borders(Borders::ALL).title(" AI Conversation "));
    f.render_widget(chat_list, chunks[0]);

    // Scrollbar
    let mut scrollbar_state = ScrollbarState::new(state.history.len()).position(state.scroll_offset);
    f.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight),
        chunks[0].inner(&Margin { vertical: 1, horizontal: 0 }),
        &mut scrollbar_state,
    );

    // 2. Mid Row (Dropdown & Settings)
    let mid_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(chunks[1]);

    let _model_style = if state.focus == ChatFocus::ModelDropdown { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::Gray) };
    let current_model = if !state.available_models.is_empty() {
        &state.available_models[state.selected_model % state.available_models.len()]
    } else {
        "No models available"
    };
    
    // Create a button for the model selector
    let model_button = crate::widget::button::Button::new(format!(" Model: {} ▼", current_model))
        .focused(state.focus == ChatFocus::ModelDropdown)
        .style(Style::default().fg(if state.focus == ChatFocus::ModelDropdown { Color::Yellow } else { Color::Gray }))
        .focused_style(Style::default().fg(Color::Yellow));
    
    f.render_widget(model_button, mid_chunks[0]);

    let _config_style = if state.focus == ChatFocus::ConfigButton { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::Gray) };
    
    // Create a button for the settings
    let config_button = crate::widget::button::Button::new(" Settings ")
        .focused(state.focus == ChatFocus::ConfigButton)
        .style(Style::default().fg(if state.focus == ChatFocus::ConfigButton { Color::Yellow } else { Color::Gray }))
        .focused_style(Style::default().fg(Color::Yellow));
    
    f.render_widget(config_button, mid_chunks[1]);

    // 3. Input Field
    let input_style = if state.focus == ChatFocus::Input { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::Gray) };
    f.render_widget(
        Paragraph::new(input_text)
            .block(Block::default().borders(Borders::ALL).title(" Prompt ").border_style(input_style)),
        chunks[2],
    );

    if state.focus == ChatFocus::Input {
        f.set_cursor(chunks[2].x + input_text.len() as u16 + 1, chunks[2].y + 1);
    }

    // 4. Config Dialog
    if state.show_config {
        draw_config_popup(f);
    }
}

fn draw_config_popup(f: &mut Frame) {
    let area = centered_rect(60, 40, f.size());
    f.render_widget(Clear, area);
    let rows = vec![
        Row::new(vec!["Host", "localhost"]),
        Row::new(vec!["Port", "11434"]),
        Row::new(vec!["Status", "Connected"]),
    ];
    let table = Table::new(rows, [Constraint::Percentage(50), Constraint::Percentage(50)])
        .block(Block::default().title(" Config ").borders(Borders::ALL).border_type(BorderType::Double))
        .header(Row::new(vec!["Setting", "Value"]).style(Style::default().bold()));
    f.render_widget(table, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage((100-percent_y)/2), Constraint::Percentage(percent_y), Constraint::Percentage((100-percent_y)/2)])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage((100-percent_x)/2), Constraint::Percentage(percent_x), Constraint::Percentage((100-percent_x)/2)])
        .split(popup_layout[1])[1]
}