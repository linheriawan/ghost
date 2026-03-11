use ratatui::{prelude::*, widgets::*};
use crossterm::{event::{self, KeyCode, Event, KeyModifiers}, terminal::*, execute};
use std::io;

mod ui; // Load our UI modules
mod audio; // Audio processing modules
mod ai; // AI modules
mod widget; // Custom widgets

#[derive(PartialEq)]
pub enum Page {
    Player,
    Equalizer,
    Playlist,
    AIChat,
}

struct App {
    current_page: Page,
    input_text: String,
    chat_state: ui::ai_chat::ChatState,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    // Setup Terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // 2. The Critical Part:
    // EnterAlternateScreen: Switches to a clean buffer
    // Clear(ClearType::All): Wipes any ghost characters
    execute!(
        stdout, 
        EnterAlternateScreen, 
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All)
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    
    // Optional: Hide the cursor globally if you don't want it flickering
    terminal.hide_cursor()?;
    // This ESC sequence clears the scrollback buffer on most Xterm-compatible terminals
    execute!(io::stdout(), crossterm::style::Print("\x1b[3J"))?;
    // execute!(stdout, EnterAlternateScreen)?;
    // let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    // Initialize App State
    let mut app = App {
        current_page: Page::Player,
        input_text: String::new(),
        chat_state: ui::ai_chat::ChatState::new().await,
    };

    loop {
        terminal.draw(|f| draw_main_layout(f, &app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == event::KeyEventKind::Press {
                match key.code {
                    // Global Quit
                    KeyCode::Char('q') => break,

                    // Global Navigation with Alt key combinations
                    KeyCode::Char('1') if key.modifiers.contains(KeyModifiers::ALT) => app.current_page = Page::Player,
                    KeyCode::Char('2') if key.modifiers.contains(KeyModifiers::ALT) => app.current_page = Page::Equalizer,
                    KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::ALT) => app.current_page = Page::Playlist,
                    KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::ALT) => app.current_page = Page::AIChat,

                    // Chat Specific Logic
                    KeyCode::Tab if app.current_page == Page::AIChat => {
                        app.chat_state.focus = match app.chat_state.focus {
                            ui::ai_chat::ChatFocus::Input => ui::ai_chat::ChatFocus::ModelDropdown,
                            ui::ai_chat::ChatFocus::ModelDropdown => ui::ai_chat::ChatFocus::ConfigButton,
                            ui::ai_chat::ChatFocus::ConfigButton => ui::ai_chat::ChatFocus::Input,
                        };
                    }

                    KeyCode::Enter if app.current_page == Page::AIChat => {
                        match app.chat_state.focus {
                            ui::ai_chat::ChatFocus::Input => {
                                if !app.input_text.is_empty() {
                                    // Store the input temporarily to pass to the async function
                                    let user_input = app.input_text.clone();
                                    app.input_text.clear();
                                    
                                    // Call the async function to send the message
                                    app.chat_state.send_message(&user_input).await;
                                }
                            }
                            ui::ai_chat::ChatFocus::ModelDropdown => {
                                if !app.chat_state.available_models.is_empty() {
                                    app.chat_state.selected_model = (app.chat_state.selected_model + 1) % app.chat_state.available_models.len();
                                }
                            }
                            ui::ai_chat::ChatFocus::ConfigButton => {
                                app.chat_state.show_config = !app.chat_state.show_config;
                            }
                        }
                    }

                    // Text Input (Only when on Chat Page and Input is Focused)
                    KeyCode::Char(c) if app.current_page == Page::AIChat && app.chat_state.focus == ui::ai_chat::ChatFocus::Input => {
                        app.input_text.push(c);
                    }
                    KeyCode::Backspace if app.current_page == Page::AIChat && app.chat_state.focus == ui::ai_chat::ChatFocus::Input => {
                        app.input_text.pop();
                    }

                    _ => {}
                }
            }
        }
    }

    // Cleanup
    // disable_raw_mode()?;
    // execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    // Ok(())
    // --- At the very end of main() ---
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(), 
        LeaveAlternateScreen,   // Switch back to the main terminal buffer
        crossterm::cursor::Show  // Make sure the cursor comes back
    )?;
    Ok(())
}

fn draw_main_layout(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(0),    // Body
            Constraint::Length(1), // Footer
        ])
        .split(f.size());

    // 1. Header (Menu) - Button-like appearance using KeyButton widget
    let active_idx = match app.current_page {
        Page::Player => 0,
        Page::Equalizer => 1,
        Page::Playlist => 2,
        Page::AIChat => 3,
    };
    
    // Create button-like elements with enhanced styling
    let button_texts = [
        ("PLAYER", "Alt+1"),
        ("EQUALIZER", "Alt+2"), 
        ("PLAYLIST", "Alt+3"),
        ("CHAT", "Alt+4")
    ];
    
    // Create a horizontal layout for the buttons
    let button_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25), 
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(chunks[0]);
        
    for (i, (title, key_combo)) in button_texts.iter().enumerate() {
        let is_active = i == active_idx;
        
        // Define colors based on active state
        let _button_style = if is_active { 
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD) 
        } else { 
            Style::default().fg(Color::DarkGray) 
        };
        
        // Create the KeyButton widget
        let key_button = widget::button::KeyButton::new(title, key_combo)
            .focused(is_active)
            .style(Style::default().fg(if is_active { Color::Yellow } else { Color::DarkGray }))
            .focused_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        
        f.render_widget(key_button, button_chunks[i]);
    }

    // 2. Body (Router)
    match app.current_page {
        Page::Player => ui::player::draw(f, chunks[1]),
        Page::Equalizer => ui::equalizer::draw(f, chunks[1]),
        Page::Playlist => ui::playlist::draw(f, chunks[1]),
        Page::AIChat => ui::ai_chat::draw(f, chunks[1], &app.chat_state, &app.input_text),
    }

    // 3. Footer
    let footer = Paragraph::new("Alt+1-4: Switch Pages | Tab: Focus | Enter: Select | q: Quit")
        .style(Style::default().dim());
    f.render_widget(footer, chunks[2]);
}