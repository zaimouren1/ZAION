//! Zaion brand: pixel "ZAION" wordmark + octopus mascot.
//!
//! Two surfaces live in this module:
//!   1. `render_word_mark` / `zaion_wordmark` — pixel "ZAION" wordmark built
//!      from a 5x7 ASCII bitmap. Each cell is `█` (U+2588 full block), which
//!      is a real square in any monospace font, so the output is a true pixel
//!      grid (not a hash-tangle of `#` chars). 3D depth is added with a
//!      right-bottom shadow row + a dim-brown underline.
//!   2. `octopus_banner` — 9-row ASCII octopus (8 tentacles + hex core),
//!      used as a full header side-by-side with the wordmark.
//!
//! Color: yellow 256-color gradient (top→mid→bottom = 220/214/130). The
//! gradient is only applied when `tty` is true; non-tty callers (tests,
//! pipes) get the plain pixel shape so output stays parseable.

use std::io::{self, IsTerminal};

/// One row of a 5-wide pixel glyph. `'█'` is a lit pixel; `' '` is dark.
type GlyphRow = [char; 5];

/// Glyph table — 26 letters, indexed by `(b - b'A')`. Rows are top-to-bottom.
const GLYPH_TABLE: [[GlyphRow; 7]; 26] = [
    // A — closed top
    [
        ['█', '█', '█', '█', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', '█', '█', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
    ],
    // B — rounded back
    [
        ['█', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', '█', '█', ' '],
    ],
    // C — open right
    [
        [' ', '█', '█', '█', '█'],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        [' ', '█', '█', '█', '█'],
    ],
    // D
    [
        ['█', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', '█', '█', ' '],
    ],
    // E
    [
        ['█', '█', '█', '█', '█'],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', '█', '█', '█', '█'],
    ],
    // F
    [
        ['█', '█', '█', '█', '█'],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
    ],
    // G — closed right with bar
    [
        [' ', '█', '█', '█', '█'],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', '█', '█', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        [' ', '█', '█', '█', '█'],
    ],
    // H
    [
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', '█', '█', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
    ],
    // I
    [
        ['█', '█', '█', '█', '█'],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        ['█', '█', '█', '█', '█'],
    ],
    // J
    [
        ['█', '█', '█', '█', '█'],
        [' ', ' ', ' ', '█', ' '],
        [' ', ' ', ' ', '█', ' '],
        [' ', ' ', ' ', '█', ' '],
        ['█', ' ', ' ', '█', ' '],
        ['█', ' ', ' ', '█', ' '],
        [' ', '█', '█', ' ', ' '],
    ],
    // K
    [
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', '█', ' '],
        ['█', ' ', '█', ' ', ' '],
        ['█', '█', ' ', ' ', ' '],
        ['█', ' ', '█', ' ', ' '],
        ['█', ' ', ' ', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
    ],
    // L
    [
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', '█', '█', '█', '█'],
    ],
    // M
    [
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', ' ', '█', '█'],
        ['█', ' ', '█', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
    ],
    // N — wide chevron: 2-px diagonal stair, no thin T-shape possibility.
    [
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', ' ', ' ', '█'],
        ['█', '█', '█', ' ', '█'],
        ['█', '█', '█', '█', '█'],
        ['█', ' ', '█', '█', '█'],
        ['█', ' ', ' ', '█', '█'],
        ['█', ' ', ' ', ' ', '█'],
    ],
    // O
    [
        [' ', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        [' ', '█', '█', '█', ' '],
    ],
    // P
    [
        ['█', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
    ],
    // Q
    [
        [' ', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', '█', ' ', '█'],
        ['█', ' ', ' ', '█', ' '],
        [' ', '█', '█', '█', '█'],
    ],
    // R
    [
        ['█', '█', '█', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', '█', '█', '█', ' '],
        ['█', ' ', '█', ' ', ' '],
        ['█', ' ', ' ', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
    ],
    // S
    [
        [' ', '█', '█', '█', '█'],
        ['█', ' ', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        [' ', '█', '█', '█', ' '],
        [' ', ' ', ' ', ' ', '█'],
        [' ', ' ', ' ', ' ', '█'],
        ['█', '█', '█', '█', ' '],
    ],
    // T
    [
        ['█', '█', '█', '█', '█'],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
    ],
    // U
    [
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        [' ', '█', '█', '█', ' '],
    ],
    // V
    [
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        [' ', '█', ' ', '█', ' '],
        [' ', ' ', '█', ' ', ' '],
    ],
    // W
    [
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', '█', ' ', '█'],
        ['█', ' ', '█', ' ', '█'],
        ['█', '█', ' ', '█', '█'],
        ['█', ' ', ' ', ' ', '█'],
    ],
    // X
    [
        ['█', ' ', ' ', ' ', '█'],
        [' ', '█', ' ', '█', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', '█', ' ', '█', ' '],
        ['█', ' ', ' ', ' ', '█'],
    ],
    // Y
    [
        ['█', ' ', ' ', ' ', '█'],
        ['█', ' ', ' ', ' ', '█'],
        [' ', '█', ' ', '█', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', ' ', '█', ' ', ' '],
    ],
    // Z
    [
        ['█', '█', '█', '█', '█'],
        [' ', ' ', ' ', ' ', '█'],
        [' ', ' ', ' ', '█', ' '],
        [' ', ' ', '█', ' ', ' '],
        [' ', '█', ' ', ' ', ' '],
        ['█', ' ', ' ', ' ', ' '],
        ['█', '█', '█', '█', '█'],
    ],
];

/// Letter spacing between glyphs in a rendered word, in columns.
const LETTER_SPACING: usize = 1;

/// Render a word into 7 rows of `String`s, one per row from top to bottom.
/// Non-`A-Z` characters are rendered as a single blank column (5 spaces).
pub fn render_word_mark(word: &str) -> [String; 7] {
    let mut rows: [String; 7] = Default::default();
    let mut first = true;
    for ch in word.chars() {
        if !first {
            for row in rows.iter_mut() {
                for _ in 0..LETTER_SPACING {
                    row.push(' ');
                }
            }
        }
        first = false;
        let glyph: [[char; 5]; 7] = if ch.is_ascii_uppercase() {
            GLYPH_TABLE[(ch as u8 - b'A') as usize]
        } else if ch.is_ascii_digit() {
            digit_glyph(ch)
        } else if ch == ' ' {
            [[' '; 5]; 7]
        } else {
            blank_glyph()
        };
        for (row, glyph_row) in rows.iter_mut().zip(glyph.iter()) {
            for cell in glyph_row.iter() {
                row.push(*cell);
            }
        }
    }
    rows
}

fn blank_glyph() -> [[char; 5]; 7] {
    [[' '; 5]; 7]
}

fn digit_glyph(d: char) -> [[char; 5]; 7] {
    match d {
        '0' => [
            ['█', '█', '█', '█', '█'],
            ['█', ' ', ' ', ' ', '█'],
            ['█', ' ', ' ', '█', '█'],
            ['█', ' ', '█', ' ', '█'],
            ['█', '█', ' ', ' ', '█'],
            ['█', ' ', ' ', ' ', '█'],
            ['█', '█', '█', '█', '█'],
        ],
        '1' => [
            [' ', ' ', '█', ' ', ' '],
            [' ', '█', '█', ' ', ' '],
            [' ', ' ', '█', ' ', ' '],
            [' ', ' ', '█', ' ', ' '],
            [' ', ' ', '█', ' ', ' '],
            [' ', ' ', '█', ' ', ' '],
            ['█', '█', '█', '█', '█'],
        ],
        '2' => [
            ['█', '█', '█', '█', '█'],
            [' ', ' ', ' ', ' ', '█'],
            [' ', ' ', ' ', ' ', '█'],
            ['█', '█', '█', '█', ' '],
            ['█', ' ', ' ', ' ', ' '],
            ['█', ' ', ' ', ' ', ' '],
            ['█', '█', '█', '█', '█'],
        ],
        '3' => [
            ['█', '█', '█', '█', '█'],
            [' ', ' ', ' ', ' ', '█'],
            [' ', ' ', ' ', ' ', '█'],
            ['█', '█', '█', '█', ' '],
            [' ', ' ', ' ', ' ', '█'],
            [' ', ' ', ' ', ' ', '█'],
            ['█', '█', '█', '█', ' '],
        ],
        '4' => [
            ['█', ' ', ' ', ' ', '█'],
            ['█', ' ', ' ', ' ', '█'],
            ['█', ' ', ' ', ' ', '█'],
            ['█', '█', '█', '█', '█'],
            [' ', ' ', ' ', ' ', '█'],
            [' ', ' ', ' ', ' ', '█'],
            [' ', ' ', ' ', ' ', '█'],
        ],
        '5' => [
            ['█', '█', '█', '█', '█'],
            ['█', ' ', ' ', ' ', ' '],
            ['█', '█', '█', '█', ' '],
            [' ', ' ', ' ', ' ', '█'],
            [' ', ' ', ' ', ' ', '█'],
            ['█', ' ', ' ', ' ', '█'],
            ['█', '█', '█', '█', ' '],
        ],
        '6' => [
            [' ', '█', '█', '█', ' '],
            ['█', ' ', ' ', ' ', ' '],
            ['█', ' ', ' ', ' ', ' '],
            ['█', '█', '█', '█', ' '],
            ['█', ' ', ' ', ' ', '█'],
            ['█', ' ', ' ', ' ', '█'],
            [' ', '█', '█', '█', ' '],
        ],
        '7' => [
            ['█', '█', '█', '█', '█'],
            [' ', ' ', ' ', ' ', '█'],
            [' ', ' ', ' ', '█', ' '],
            [' ', ' ', '█', ' ', ' '],
            [' ', '█', ' ', ' ', ' '],
            [' ', '█', ' ', ' ', ' '],
            [' ', '█', ' ', ' ', ' '],
        ],
        '8' => [
            [' ', '█', '█', '█', ' '],
            ['█', ' ', ' ', ' ', '█'],
            ['█', ' ', ' ', ' ', '█'],
            [' ', '█', '█', '█', ' '],
            ['█', ' ', ' ', ' ', '█'],
            ['█', ' ', ' ', ' ', '█'],
            [' ', '█', '█', '█', ' '],
        ],
        '9' => [
            [' ', '█', '█', '█', ' '],
            ['█', ' ', ' ', ' ', '█'],
            ['█', ' ', ' ', ' ', '█'],
            ['█', '█', '█', '█', '█'],
            [' ', ' ', ' ', ' ', '█'],
            [' ', ' ', ' ', ' ', '█'],
            ['█', '█', '█', '█', ' '],
        ],
        _ => blank_glyph(),
    }
}

/// Render "ZAION" with the 3D-shadow treatment. Returns 9 rows: 7 of glyph,
/// then row 7 is a right-edge shelf (duplicate of glyph bottom row), row 8 is
/// a thin `-` drop-shadow underline. `tty` controls ANSI yellow gradient.
pub fn zaion_wordmark(tty: bool) -> [String; 9] {
    let glyph = render_word_mark("ZAION");
    let mut out: [String; 9] = Default::default();
    for (i, row) in glyph.iter().enumerate() {
        out[i] = format!(" {}", row);
    }
    let width = out[0].chars().count();
    // Row 7: bottom-right shelf — same shape as letter bottom row, gives
    // the 3D depth illusion (light comes from upper-left).
    out[7] = format!(" {}", glyph[6]);
    // Row 8: thin drop-shadow underline using `-` chars, dimmer feel.
    out[8] = " ".repeat(width) + &"-".repeat(glyph[6].chars().filter(|c| *c == '█').count().max(1));
    if tty {
        apply_yellow_gradient(&mut out);
    }
    out
}

/// Yellow gradient: top 3 rows bright yellow, mid 3 rows amber, bottom 3 deep gold.
fn apply_yellow_gradient(rows: &mut [String; 9]) {
    const TOP: &str = "\x1b[38;5;220m";
    const MID: &str = "\x1b[38;5;214m";
    const LOW: &str = "\x1b[38;5;130m";
    const RESET: &str = "\x1b[0m";
    for (idx, row) in rows.iter_mut().enumerate() {
        let prefix = if idx < 3 {
            TOP
        } else if idx < 6 {
            MID
        } else {
            LOW
        };
        *row = format!("{}{}{}", prefix, row, RESET);
    }
}

/// Print the full Zaion header: octopus banner (left) + pixel ZAION wordmark
/// (right), stacked. Honors TTY for color.
pub fn print_header() {
    let tty = io::stdout().is_terminal();
    let octopus = octopus_banner(tty);
    let wordmark = zaion_wordmark(tty);
    let pad = "  ";
    for i in 0..9 {
        let octopus_line = &octopus[i];
        let wm_line: &str = if i < wordmark.len() { &wordmark[i] } else { "" };
        println!("{}{}{}", octopus_line, pad, wm_line);
    }
}

/// Pixel-art octopus (9 rows × 22 cols, 5x7 cell grid).
/// All cells are `█` (U+2588) so it pairs with the pixel "ZAION" wordmark
/// in the side-by-side header — no more hash-mark ASCII mix.
/// Color is a purple→magenta→cyan gradient (top→mid→bottom = 141/177/81),
/// gated on TTY. The shape is built to read as a friendly 8-tentacled
/// creature: row 0 hood/crown, rows 1-2 eyes, rows 3-4 mouth + collar,
/// rows 5-7 spreading tentacles, row 8 wave/sea.
pub fn octopus_banner(tty: bool) -> [String; 9] {
    // 22 columns of pixel cells. `'█'` (U+2588) = lit, `' '` = dark.
    // We use ONLY █, space, and `~` for waterline — so the grid is a true
    // 2-color bitmap in any monospace font, matching the ZAION wordmark
    // glyph table. 9 rows tall to align row-for-row with the wordmark.
    //
    // Shape anatomy (left to right):
    //   row 0   hood crown, 10 cells wide, centered
    //   rows 1-2  head with two carved eye sockets
    //   row 3   mouth, 2 cells, head narrows
    //   row 4   collar closes, 6 cells
    //   row 5   mantle: 4 tentacle roots fan out
    //   row 6   tentacle bodies: 4 arms × 2 cells, with gaps between arms
    //   row 7   tentacle tips curl inward (3 distinct curls at row-bottom)
    //   row 8   waterline wave under the creature
    //
    // The 8 tentacles are: 2 short front arms (row 6+7 inside curls),
    // 2 medium side arms (row 6+7 outside curls), 2 long back arms
    // (extending to row 7+8 in the back), 2 rear drapes (row 6+7
    // tail-like extensions). Each arm has 2 cells of width to read as
    // a "tube" not a "drip".
    const GRID: [&str; 9] = [
        // row 0 — hood crown (10 cells, centered in 26)
        "      ██████████      ",
        // row 1 — head outline, eye sockets carved (left + right vertical bars)
        "    ██          ██    ",
        // row 2 — head with eyes (2 single pixels 8 cols apart)
        "    ██  ██  ██  ██    ",
        // row 3 — mouth, head narrows to collar
        "     ██      ██       ",
        // row 4 — collar closes (6 cells)
        "      ██████          ",
        // row 5 — mantle + tentacle roots: 4 roots × 2 cells each
        "  ██  ██  ██  ██  ██  ",
        // row 6 — tentacle bodies: 4 tubes, 2 cells wide each, with gaps
        "██  ██  ██  ██  ██  ██",
        // row 7 — tentacle tips curl inward: 4 distinct arm-ends
        "  ██  ██  ██  ██  ██  ",
        // row 8 — waterline under creature, plus rear 2 drapes visible
        "~~      ██  ██      ~~",
    ];
    let mut out: [String; 9] = Default::default();
    for (i, row) in GRID.iter().enumerate() {
        out[i] = format!(" {}", row);
    }
    if tty {
        apply_octopus_gradient(&mut out);
    }
    out
}

/// Purple→magenta→cyan gradient for the pixel octopus. Top 3 rows deep
/// purple, mid 3 rows magenta/pink, bottom 3 rows teal/cyan. Reads as a
/// cool complement to the warm yellow ZAION wordmark.
fn apply_octopus_gradient(rows: &mut [String; 9]) {
    const TOP: &str = "\x1b[38;5;141m";
    const MID: &str = "\x1b[38;5;177m";
    const LOW: &str = "\x1b[38;5;81m";
    const RESET: &str = "\x1b[0m";
    for (idx, row) in rows.iter_mut().enumerate() {
        let prefix = if idx < 3 {
            TOP
        } else if idx < 6 {
            MID
        } else {
            LOW
        };
        *row = format!("{}{}{}", prefix, row, RESET);
    }
}

/// Compact single-line octopus glyph — used for status-bar badges, prompt
/// prefixes, and inline callouts. Picks emoji on TTY (1 visual cell), ASCII
/// fallback otherwise.
pub fn octopus_glyph(tty: bool) -> &'static str {
    if tty {
        "🐙\u{2009}"
    } else {
        "<*>"
    }
}

/// Pixel "ZAION" wordmark only (no octopus), for compact inline callouts.
pub fn zaion_wordmark_lines() -> [String; 9] {
    let tty = io::stdout().is_terminal();
    compact_wordmark_lines(tty)
}

/// Render the compact wordmark for a known output mode.
///
/// Interactive terminals keep the full block-pixel identity. Redirected and
/// captured output uses `#` pixels so stable CLI surfaces remain ASCII-safe.
pub fn compact_wordmark_lines(tty: bool) -> [String; 9] {
    let mut lines = zaion_wordmark(tty);
    if !tty {
        for line in &mut lines {
            *line = line.replace('█', "#");
        }
    }
    lines
}

/// Print compact banner (9-row pixel wordmark + subtitle line).
/// Used by CLI surfaces: --help, doctor, onboard, launch-check.
pub fn print_compact_banner(subtitle: &str) {
    for line in zaion_wordmark_lines() {
        println!("{line}");
    }
    println!();
    println!("{subtitle}");
    println!();
}

/// The single-glyph octopus for badges.
pub fn badge() -> String {
    let tty = io::stdout().is_terminal();
    octopus_glyph(tty).to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zaion_wordmark_has_5_letters_wide() {
        let rows = render_word_mark("ZAION");
        // 5 letters * 5 cols + 4 * 1 spacing = 29 columns per row
        for row in rows.iter() {
            assert_eq!(row.chars().count(), 29, "row = {:?}", row);
        }
    }

    #[test]
    fn zaion_wordmark_height_is_7() {
        let rows = render_word_mark("ZAION");
        assert_eq!(rows.len(), 7);
    }

    #[test]
    fn zaion_full_wordmark_is_9_rows() {
        let rows = zaion_wordmark(false);
        assert_eq!(rows.len(), 9);
    }

    #[test]
    fn compact_wordmark_is_ascii_when_output_is_not_a_tty() {
        let rows = compact_wordmark_lines(false);
        assert!(rows.iter().all(|row| row.is_ascii()));
        assert!(rows[0].contains("#####"));
    }

    #[test]
    fn zaion_full_wordmark_gradient_is_optional() {
        let plain = zaion_wordmark(false);
        assert!(plain.iter().all(|r| !r.contains('\u{1b}')));
        let colored = zaion_wordmark(true);
        assert!(colored.iter().any(|r| r.contains('\u{1b}')));
    }

    #[test]
    fn octopus_banner_is_22_cols_9_rows() {
        let b = octopus_banner(false);
        assert_eq!(b.len(), 9, "octopus must be 9 rows to pair with wordmark");
        for (idx, line) in b.iter().enumerate() {
            assert_eq!(
                line.chars().count(),
                23, // leading space + 22 pixel cells
                "row {idx} should be 23 cols, got {}: {:?}",
                line.chars().count(),
                line
            );
        }
    }

    #[test]
    fn octopus_uses_block_chars_not_ascii() {
        // Pixel octopus must use █ U+2588 (full block), not the old ASCII
        // hash mix (. / | \ o etc). The wave row 8 is the only exception —
        // it uses `~` for the sea surface, but still has █ for the rear
        // tentacle drapes.
        let b = octopus_banner(false);
        for (idx, line) in b.iter().enumerate() {
            assert!(
                line.contains('█'),
                "octopus row {idx} must contain █, got {line:?}"
            );
            for forbidden in ['.', '/', '\\', '|', 'o', '#'] {
                assert!(
                    !line.contains(forbidden),
                    "octopus row {idx} must not contain '{forbidden}', got {line:?}"
                );
            }
        }
    }

    #[test]
    fn badge_returns_nonempty_string() {
        assert!(!badge().is_empty());
    }

    #[test]
    fn wordmark_uses_block_chars_not_hash() {
        let rows = render_word_mark("ZAION");
        for row in rows.iter() {
            assert!(
                row.contains('█'),
                "wordmark must contain █ block chars, row = {row:?}"
            );
            assert!(
                !row.contains('#'),
                "wordmark must NOT contain # hash chars, row = {row:?}"
            );
        }
    }
}
