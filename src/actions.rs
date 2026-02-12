//! Business logic - what happens when user interacts

use ghost_ui::Callout;

/// Handle button click actions by button ID string
pub fn on_button_click(button_id: &str, callout: &mut Callout) {
    match button_id {
        "greet" => {
            callout.say("Hi, how are you today?");
            log::info!("Action: Greeting");
        }
        "think" => {
            callout.think("Hmm, let me think about that...");
            log::info!("Action: Thinking");
        }
        "scream" => {
            callout.scream("WATCH OUT!");
            log::info!("Action: Screaming");
        }
        _ => {
            // Generic action for unknown buttons
            callout.say(&format!("Button '{}' clicked!", button_id));
            log::info!("Action: Unknown button '{}'", button_id);
        }
    }
}
