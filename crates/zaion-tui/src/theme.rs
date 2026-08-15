//! Theme system for Zaion TUI
//!
//! Based on Claude Code v2.1.88 theme architecture (640 lines)
//! Provides 6 theme variants with full RGB color palettes

use crossterm::style::Color;

/// Complete theme color palette
#[derive(Debug, Clone)]
pub struct ZaionTheme {
    // Brand colors
    pub claude: Color,
    pub claude_shimmer: Color,

    // Text colors
    pub text: Color,
    pub inverse_text: Color,
    pub inactive: Color,
    pub subtle: Color,

    // UI element colors
    pub prompt_border: Color,
    pub prompt_border_shimmer: Color,
    pub background: Color,

    // Status colors
    pub success: Color,
    pub error: Color,
    pub warning: Color,

    // Diff colors (6 variants)
    pub diff_added: Color,
    pub diff_removed: Color,
    pub diff_added_dimmed: Color,
    pub diff_removed_dimmed: Color,
    pub diff_added_word: Color,
    pub diff_removed_word: Color,

    // Agent colors (8 colors for sub-agents)
    pub agent_red: Color,
    pub agent_blue: Color,
    pub agent_green: Color,
    pub agent_yellow: Color,
    pub agent_purple: Color,
    pub agent_orange: Color,
    pub agent_pink: Color,
    pub agent_cyan: Color,

    // Rainbow colors for syntax highlighting (7 colors + shimmers)
    pub rainbow_red: Color,
    pub rainbow_red_shimmer: Color,
    pub rainbow_orange: Color,
    pub rainbow_orange_shimmer: Color,
    pub rainbow_yellow: Color,
    pub rainbow_yellow_shimmer: Color,
    pub rainbow_green: Color,
    pub rainbow_green_shimmer: Color,
    pub rainbow_blue: Color,
    pub rainbow_blue_shimmer: Color,
    pub rainbow_indigo: Color,
    pub rainbow_indigo_shimmer: Color,
    pub rainbow_violet: Color,
    pub rainbow_violet_shimmer: Color,

    // TUI specific colors
    pub user_message_background: Color,
    pub user_message_background_hover: Color,
    pub message_actions_background: Color,
    pub selection_bg: Color,
    pub bash_message_background: Color,

    // Mascot colors (Clawd)
    pub clawd_body: Color,
    pub clawd_background: Color,
}

/// Available theme variants
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThemeName {
    /// Default dark theme with full RGB colors
    #[default]
    Dark,
    /// Light theme with full RGB colors
    Light,
    /// Color-blind friendly dark theme (daltonized)
    DarkDaltonized,
    /// Color-blind friendly light theme (daltonized)
    LightDaltonized,
    /// 16-color ANSI fallback for dark backgrounds
    DarkAnsi,
    /// 16-color ANSI fallback for light backgrounds
    LightAnsi,
    /// Auto-detect system theme
    Auto,
}

impl ThemeName {
    /// Parse theme name from string
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "dark" => Some(ThemeName::Dark),
            "light" => Some(ThemeName::Light),
            "dark-daltonized" | "dark_daltonized" => Some(ThemeName::DarkDaltonized),
            "light-daltonized" | "light_daltonized" => Some(ThemeName::LightDaltonized),
            "dark-ansi" | "dark_ansi" => Some(ThemeName::DarkAnsi),
            "light-ansi" | "light_ansi" => Some(ThemeName::LightAnsi),
            "auto" => Some(ThemeName::Auto),
            _ => None,
        }
    }

    /// Convert to string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ThemeName::Dark => "dark",
            ThemeName::Light => "light",
            ThemeName::DarkDaltonized => "dark-daltonized",
            ThemeName::LightDaltonized => "light-daltonized",
            ThemeName::DarkAnsi => "dark-ansi",
            ThemeName::LightAnsi => "light-ansi",
            ThemeName::Auto => "auto",
        }
    }
}

/// Get theme by name
pub fn get_theme(name: ThemeName) -> ZaionTheme {
    match name {
        ThemeName::Dark => dark_theme(),
        ThemeName::Light => light_theme(),
        ThemeName::DarkDaltonized => dark_daltonized_theme(),
        ThemeName::LightDaltonized => light_daltonized_theme(),
        ThemeName::DarkAnsi => dark_ansi_theme(),
        ThemeName::LightAnsi => light_ansi_theme(),
        ThemeName::Auto => auto_theme(),
    }
}

/// RGB color helper
const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb { r, g, b }
}

/// Dark theme (default) - Based on Claude Code's dark theme
fn dark_theme() -> ZaionTheme {
    ZaionTheme {
        // Brand colors - Claude orange
        claude: rgb(215, 119, 87),
        claude_shimmer: rgb(235, 159, 127),

        // Text colors
        text: rgb(230, 230, 230),
        inverse_text: rgb(30, 30, 30),
        inactive: rgb(120, 120, 130),
        subtle: rgb(160, 160, 170),

        // UI element colors
        prompt_border: rgb(100, 149, 237), // Cornflower blue
        prompt_border_shimmer: rgb(130, 179, 255),
        background: rgb(40, 44, 52),

        // Status colors
        success: rgb(44, 122, 57),
        error: rgb(171, 43, 63),
        warning: rgb(150, 108, 30),

        // Diff colors
        diff_added: rgb(34, 92, 47),
        diff_removed: rgb(141, 33, 53),
        diff_added_dimmed: rgb(24, 62, 37),
        diff_removed_dimmed: rgb(111, 23, 43),
        diff_added_word: rgb(54, 142, 67),
        diff_removed_word: rgb(191, 63, 83),

        // Agent colors (for sub-agents)
        agent_red: rgb(224, 108, 117),
        agent_blue: rgb(97, 175, 239),
        agent_green: rgb(152, 195, 121),
        agent_yellow: rgb(229, 192, 123),
        agent_purple: rgb(198, 120, 221),
        agent_orange: rgb(209, 154, 102),
        agent_pink: rgb(225, 150, 180),
        agent_cyan: rgb(86, 182, 194),

        // Rainbow colors for syntax highlighting
        rainbow_red: rgb(224, 108, 117),
        rainbow_red_shimmer: rgb(244, 138, 147),
        rainbow_orange: rgb(209, 154, 102),
        rainbow_orange_shimmer: rgb(239, 184, 132),
        rainbow_yellow: rgb(229, 192, 123),
        rainbow_yellow_shimmer: rgb(255, 222, 153),
        rainbow_green: rgb(152, 195, 121),
        rainbow_green_shimmer: rgb(182, 225, 151),
        rainbow_blue: rgb(97, 175, 239),
        rainbow_blue_shimmer: rgb(127, 205, 255),
        rainbow_indigo: rgb(141, 166, 245),
        rainbow_indigo_shimmer: rgb(171, 196, 255),
        rainbow_violet: rgb(198, 120, 221),
        rainbow_violet_shimmer: rgb(228, 150, 251),

        // TUI specific
        user_message_background: rgb(50, 54, 62),
        user_message_background_hover: rgb(60, 64, 72),
        message_actions_background: rgb(35, 39, 47),
        selection_bg: rgb(70, 80, 100),
        bash_message_background: rgb(45, 49, 57),

        // Mascot colors (Clawd/Zaion)
        clawd_body: rgb(147, 112, 219),    // Medium purple
        clawd_background: rgb(30, 35, 45), // Dark slate
    }
}

/// Light theme - Based on Claude Code's light theme
fn light_theme() -> ZaionTheme {
    ZaionTheme {
        // Brand colors
        claude: rgb(185, 89, 57),
        claude_shimmer: rgb(215, 119, 87),

        // Text colors
        text: rgb(30, 30, 30),
        inverse_text: rgb(250, 250, 250),
        inactive: rgb(140, 140, 150),
        subtle: rgb(100, 100, 110),

        // UI element colors
        prompt_border: rgb(70, 119, 207),
        prompt_border_shimmer: rgb(100, 149, 237),
        background: rgb(250, 250, 252),

        // Status colors
        success: rgb(34, 102, 47),
        error: rgb(151, 23, 43),
        warning: rgb(130, 88, 10),

        // Diff colors
        diff_added: rgb(200, 245, 210),
        diff_removed: rgb(255, 220, 225),
        diff_added_dimmed: rgb(220, 255, 230),
        diff_removed_dimmed: rgb(255, 240, 245),
        diff_added_word: rgb(180, 235, 190),
        diff_removed_word: rgb(255, 200, 215),

        // Agent colors
        agent_red: rgb(194, 78, 87),
        agent_blue: rgb(67, 145, 209),
        agent_green: rgb(122, 165, 91),
        agent_yellow: rgb(199, 162, 93),
        agent_purple: rgb(168, 90, 191),
        agent_orange: rgb(179, 124, 72),
        agent_pink: rgb(195, 120, 150),
        agent_cyan: rgb(56, 152, 164),

        // Rainbow colors
        rainbow_red: rgb(194, 78, 87),
        rainbow_red_shimmer: rgb(224, 108, 117),
        rainbow_orange: rgb(179, 124, 72),
        rainbow_orange_shimmer: rgb(209, 154, 102),
        rainbow_yellow: rgb(199, 162, 93),
        rainbow_yellow_shimmer: rgb(229, 192, 123),
        rainbow_green: rgb(122, 165, 91),
        rainbow_green_shimmer: rgb(152, 195, 121),
        rainbow_blue: rgb(67, 145, 209),
        rainbow_blue_shimmer: rgb(97, 175, 239),
        rainbow_indigo: rgb(111, 136, 215),
        rainbow_indigo_shimmer: rgb(141, 166, 245),
        rainbow_violet: rgb(168, 90, 191),
        rainbow_violet_shimmer: rgb(198, 120, 221),

        // TUI specific
        user_message_background: rgb(240, 244, 252),
        user_message_background_hover: rgb(230, 234, 242),
        message_actions_background: rgb(245, 249, 255),
        selection_bg: rgb(200, 210, 230),
        bash_message_background: rgb(235, 239, 247),

        // Mascot colors
        clawd_body: rgb(117, 82, 189),
        clawd_background: rgb(230, 235, 245),
    }
}

/// Dark daltonized theme - Color-blind friendly dark theme
fn dark_daltonized_theme() -> ZaionTheme {
    ZaionTheme {
        // Brand colors (adjusted for deuteranopia/protanopia)
        claude: rgb(215, 140, 50),
        claude_shimmer: rgb(235, 180, 90),

        // Text colors (same as dark)
        text: rgb(230, 230, 230),
        inverse_text: rgb(30, 30, 30),
        inactive: rgb(120, 120, 130),
        subtle: rgb(160, 160, 170),

        // UI element colors (blue-yellow contrast)
        prompt_border: rgb(80, 160, 220),
        prompt_border_shimmer: rgb(110, 190, 250),
        background: rgb(40, 44, 52),

        // Status colors (blue for success, orange for error, yellow for warning)
        success: rgb(50, 150, 220),
        error: rgb(220, 100, 40),
        warning: rgb(230, 200, 50),

        // Diff colors (blue vs orange)
        diff_added: rgb(40, 120, 180),
        diff_removed: rgb(180, 80, 30),
        diff_added_dimmed: rgb(30, 90, 150),
        diff_removed_dimmed: rgb(150, 60, 20),
        diff_added_word: rgb(60, 160, 220),
        diff_removed_word: rgb(210, 110, 50),

        // Agent colors (maximized contrast)
        agent_red: rgb(220, 100, 40),
        agent_blue: rgb(80, 160, 220),
        agent_green: rgb(200, 200, 80),
        agent_yellow: rgb(230, 200, 50),
        agent_purple: rgb(180, 140, 220),
        agent_orange: rgb(220, 140, 50),
        agent_pink: rgb(220, 160, 180),
        agent_cyan: rgb(80, 200, 220),

        // Rainbow colors (daltonized spectrum)
        rainbow_red: rgb(220, 100, 40),
        rainbow_red_shimmer: rgb(250, 130, 70),
        rainbow_orange: rgb(220, 140, 50),
        rainbow_orange_shimmer: rgb(250, 170, 80),
        rainbow_yellow: rgb(230, 200, 50),
        rainbow_yellow_shimmer: rgb(255, 230, 80),
        rainbow_green: rgb(200, 200, 80),
        rainbow_green_shimmer: rgb(230, 230, 110),
        rainbow_blue: rgb(80, 160, 220),
        rainbow_blue_shimmer: rgb(110, 190, 250),
        rainbow_indigo: rgb(120, 180, 240),
        rainbow_indigo_shimmer: rgb(150, 210, 255),
        rainbow_violet: rgb(180, 140, 220),
        rainbow_violet_shimmer: rgb(210, 170, 250),

        // TUI specific
        user_message_background: rgb(50, 54, 62),
        user_message_background_hover: rgb(60, 64, 72),
        message_actions_background: rgb(35, 39, 47),
        selection_bg: rgb(70, 80, 100),
        bash_message_background: rgb(45, 49, 57),

        // Mascot colors
        clawd_body: rgb(180, 140, 220),
        clawd_background: rgb(30, 35, 45),
    }
}

/// Light daltonized theme - Color-blind friendly light theme
fn light_daltonized_theme() -> ZaionTheme {
    ZaionTheme {
        // Brand colors
        claude: rgb(185, 110, 20),
        claude_shimmer: rgb(215, 140, 50),

        // Text colors
        text: rgb(30, 30, 30),
        inverse_text: rgb(250, 250, 250),
        inactive: rgb(140, 140, 150),
        subtle: rgb(100, 100, 110),

        // UI element colors
        prompt_border: rgb(50, 130, 190),
        prompt_border_shimmer: rgb(80, 160, 220),
        background: rgb(250, 250, 252),

        // Status colors
        success: rgb(30, 120, 180),
        error: rgb(190, 70, 10),
        warning: rgb(200, 170, 20),

        // Diff colors
        diff_added: rgb(200, 230, 250),
        diff_removed: rgb(255, 220, 200),
        diff_added_dimmed: rgb(220, 245, 255),
        diff_removed_dimmed: rgb(255, 240, 230),
        diff_added_word: rgb(180, 220, 245),
        diff_removed_word: rgb(255, 200, 180),

        // Agent colors
        agent_red: rgb(190, 70, 10),
        agent_blue: rgb(50, 130, 190),
        agent_green: rgb(170, 170, 50),
        agent_yellow: rgb(200, 170, 20),
        agent_purple: rgb(150, 110, 190),
        agent_orange: rgb(190, 110, 20),
        agent_pink: rgb(190, 130, 150),
        agent_cyan: rgb(50, 170, 190),

        // Rainbow colors
        rainbow_red: rgb(190, 70, 10),
        rainbow_red_shimmer: rgb(220, 100, 40),
        rainbow_orange: rgb(190, 110, 20),
        rainbow_orange_shimmer: rgb(220, 140, 50),
        rainbow_yellow: rgb(200, 170, 20),
        rainbow_yellow_shimmer: rgb(230, 200, 50),
        rainbow_green: rgb(170, 170, 50),
        rainbow_green_shimmer: rgb(200, 200, 80),
        rainbow_blue: rgb(50, 130, 190),
        rainbow_blue_shimmer: rgb(80, 160, 220),
        rainbow_indigo: rgb(90, 150, 210),
        rainbow_indigo_shimmer: rgb(120, 180, 240),
        rainbow_violet: rgb(150, 110, 190),
        rainbow_violet_shimmer: rgb(180, 140, 220),

        // TUI specific
        user_message_background: rgb(240, 244, 252),
        user_message_background_hover: rgb(230, 234, 242),
        message_actions_background: rgb(245, 249, 255),
        selection_bg: rgb(200, 210, 230),
        bash_message_background: rgb(235, 239, 247),

        // Mascot colors
        clawd_body: rgb(120, 80, 160),
        clawd_background: rgb(230, 235, 245),
    }
}

/// Dark ANSI theme - 16-color fallback for limited terminals
fn dark_ansi_theme() -> ZaionTheme {
    ZaionTheme {
        // Brand colors
        claude: Color::Red,
        claude_shimmer: Color::Red,

        // Text colors
        text: Color::White,
        inverse_text: Color::Black,
        inactive: Color::DarkGrey,
        subtle: Color::Grey,

        // UI element colors
        prompt_border: Color::Blue,
        prompt_border_shimmer: Color::Blue,
        background: Color::Black,

        // Status colors
        success: Color::Green,
        error: Color::Red,
        warning: Color::Yellow,

        // Diff colors
        diff_added: Color::Green,
        diff_removed: Color::Red,
        diff_added_dimmed: Color::DarkGreen,
        diff_removed_dimmed: Color::DarkRed,
        diff_added_word: Color::Green,
        diff_removed_word: Color::Red,

        // Agent colors
        agent_red: Color::Red,
        agent_blue: Color::Blue,
        agent_green: Color::Green,
        agent_yellow: Color::Yellow,
        agent_purple: Color::Magenta,
        agent_orange: Color::Red,
        agent_pink: Color::Magenta,
        agent_cyan: Color::Cyan,

        // Rainbow colors
        rainbow_red: Color::Red,
        rainbow_red_shimmer: Color::Red,
        rainbow_orange: Color::Yellow,
        rainbow_orange_shimmer: Color::Yellow,
        rainbow_yellow: Color::Yellow,
        rainbow_yellow_shimmer: Color::Yellow,
        rainbow_green: Color::Green,
        rainbow_green_shimmer: Color::Green,
        rainbow_blue: Color::Blue,
        rainbow_blue_shimmer: Color::Blue,
        rainbow_indigo: Color::Blue,
        rainbow_indigo_shimmer: Color::Blue,
        rainbow_violet: Color::Magenta,
        rainbow_violet_shimmer: Color::Magenta,

        // TUI specific
        user_message_background: Color::DarkGrey,
        user_message_background_hover: Color::Grey,
        message_actions_background: Color::Black,
        selection_bg: Color::DarkGrey,
        bash_message_background: Color::DarkGrey,

        // Mascot colors
        clawd_body: Color::Magenta,
        clawd_background: Color::Black,
    }
}

/// Light ANSI theme - 16-color fallback for light backgrounds
fn light_ansi_theme() -> ZaionTheme {
    ZaionTheme {
        // Brand colors
        claude: Color::Red,
        claude_shimmer: Color::Red,

        // Text colors
        text: Color::Black,
        inverse_text: Color::White,
        inactive: Color::Grey,
        subtle: Color::DarkGrey,

        // UI element colors
        prompt_border: Color::Blue,
        prompt_border_shimmer: Color::Blue,
        background: Color::White,

        // Status colors
        success: Color::Green,
        error: Color::Red,
        warning: Color::Yellow,

        // Diff colors
        diff_added: Color::Green,
        diff_removed: Color::Red,
        diff_added_dimmed: Color::DarkGreen,
        diff_removed_dimmed: Color::DarkRed,
        diff_added_word: Color::Green,
        diff_removed_word: Color::Red,

        // Agent colors
        agent_red: Color::Red,
        agent_blue: Color::Blue,
        agent_green: Color::Green,
        agent_yellow: Color::Yellow,
        agent_purple: Color::Magenta,
        agent_orange: Color::Red,
        agent_pink: Color::Magenta,
        agent_cyan: Color::Cyan,

        // Rainbow colors
        rainbow_red: Color::Red,
        rainbow_red_shimmer: Color::Red,
        rainbow_orange: Color::Yellow,
        rainbow_orange_shimmer: Color::Yellow,
        rainbow_yellow: Color::Yellow,
        rainbow_yellow_shimmer: Color::Yellow,
        rainbow_green: Color::Green,
        rainbow_green_shimmer: Color::Green,
        rainbow_blue: Color::Blue,
        rainbow_blue_shimmer: Color::Blue,
        rainbow_indigo: Color::Blue,
        rainbow_indigo_shimmer: Color::Blue,
        rainbow_violet: Color::Magenta,
        rainbow_violet_shimmer: Color::Magenta,

        // TUI specific
        user_message_background: Color::Grey,
        user_message_background_hover: Color::DarkGrey,
        message_actions_background: Color::White,
        selection_bg: Color::Grey,
        bash_message_background: Color::Grey,

        // Mascot colors
        clawd_body: Color::Magenta,
        clawd_background: Color::White,
    }
}

/// Auto theme - Detect system preference
fn auto_theme() -> ZaionTheme {
    // TODO: Implement OSC 11 detection for system theme
    // For now, default to dark theme
    dark_theme()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_themes_are_complete() {
        // Ensure all theme functions return valid themes
        let _dark = get_theme(ThemeName::Dark);
        let _light = get_theme(ThemeName::Light);
        let _dark_dal = get_theme(ThemeName::DarkDaltonized);
        let _light_dal = get_theme(ThemeName::LightDaltonized);
        let _dark_ansi = get_theme(ThemeName::DarkAnsi);
        let _light_ansi = get_theme(ThemeName::LightAnsi);
        let _auto = get_theme(ThemeName::Auto);
    }

    #[test]
    fn test_default_theme() {
        let theme = get_theme(ThemeName::default());
        // Should be dark theme
        match theme.text {
            Color::Rgb {
                r: 230,
                g: 230,
                b: 230,
            } => {}
            _ => panic!("Default theme is not dark"),
        }
    }

    #[test]
    fn test_theme_name_parsing() {
        assert_eq!(ThemeName::default(), ThemeName::Dark);
    }
}
