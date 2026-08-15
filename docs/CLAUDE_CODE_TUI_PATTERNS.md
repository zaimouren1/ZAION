# Claude Code TUI Implementation Patterns

**Source**: Claude Code v2.1.88 (1,906 TypeScript files, decompiled from source map)

## Architecture Overview

### Tech Stack
- **Framework**: React + Ink (React renderer for CLIs)
- **Language**: TypeScript
- **Bundler**: Bun
- **Total Files**: 1,888 TypeScript files
- **Main Entry**: `main.tsx` (803,924 bytes, 4,684 lines)
- **Components**: 144+ React components

### Key Directories
```
source/src/
├── components/          # 144+ UI components
│   ├── design-system/  # Core design primitives
│   ├── LogoV2/         # Welcome screen & branding
│   ├── messages/       # Message rendering
│   └── ui/            # Reusable UI elements
├── screens/           # Top-level views
│   ├── REPL.tsx       # Main chat interface (895KB)
│   └── Doctor.tsx     # Health check screen
├── ink/              # Custom Ink extensions
└── utils/            # Theme system, helpers
```

---

## Theme System

### Color Architecture

**File**: `src/utils/theme.ts` (640 lines)

Claude Code uses a sophisticated theme system with **explicit RGB values** to avoid terminal ANSI inconsistencies:

```typescript
export type Theme = {
  // Brand colors
  claude: string                    // rgb(215,119,87) - Claude orange
  claudeShimmer: string            // Lighter for animations
  
  // Semantic colors
  text: string                      // Primary text
  inverseText: string              // Inverted text
  inactive: string                 // Dimmed/disabled
  subtle: string                   // Secondary text
  
  // UI elements
  promptBorder: string             // Input border
  promptBorderShimmer: string      // Animated border
  background: string               // Background highlights
  
  // Status colors
  success: string                  // rgb(44,122,57)
  error: string                    // rgb(171,43,63)
  warning: string                  // rgb(150,108,30)
  
  // Diff colors (6 variants)
  diffAdded: string
  diffRemoved: string
  diffAddedDimmed: string
  diffRemovedDimmed: string
  diffAddedWord: string
  diffRemovedWord: string
  
  // Agent colors (8 colors for sub-agents)
  red_FOR_SUBAGENTS_ONLY: string
  blue_FOR_SUBAGENTS_ONLY: string
  green_FOR_SUBAGENTS_ONLY: string
  yellow_FOR_SUBAGENTS_ONLY: string
  purple_FOR_SUBAGENTS_ONLY: string
  orange_FOR_SUBAGENTS_ONLY: string
  pink_FOR_SUBAGENTS_ONLY: string
  cyan_FOR_SUBAGENTS_ONLY: string
  
  // TUI V2 specific
  userMessageBackground: string
  userMessageBackgroundHover: string
  messageActionsBackground: string
  selectionBg: string
  bashMessageBackgroundColor: string
  
  // Rainbow colors for syntax highlighting (7 colors + shimmers)
  rainbow_red: string
  rainbow_orange: string
  rainbow_yellow: string
  rainbow_green: string
  rainbow_blue: string
  rainbow_indigo: string
  rainbow_violet: string
  // ... + shimmer variants
}
```

### Supported Themes

1. **`dark`** - Default, full RGB (440-515 lines of colors)
2. **`light`** - Full RGB light mode (115-191 lines)
3. **`dark-daltonized`** - Color-blind friendly dark (521-596 lines)
4. **`light-daltonized`** - Color-blind friendly light (359-434 lines)
5. **`dark-ansi`** - 16-color fallback dark (278-353 lines)
6. **`light-ansi`** - 16-color fallback light (197-272 lines)
7. **`auto`** - System theme detection via OSC 11

### Color Application Pattern

```typescript
// Theme-aware color function (curried)
export function color(
  c: keyof Theme | Color | undefined,
  theme: ThemeName,
  type: ColorType = 'foreground',
): (text: string) => string {
  return text => {
    if (!c) return text
    
    // Raw color passthrough
    if (c.startsWith('rgb(') || c.startsWith('#') || 
        c.startsWith('ansi256(') || c.startsWith('ansi:')) {
      return colorize(text, c, type)
    }
    
    // Theme key lookup
    return colorize(text, getTheme(theme)[c as keyof Theme], type)
  }
}
```

**Usage in Components**:
```tsx
<Text color="claude">Welcome to Claude Code</Text>
<Text color="clawd_body" backgroundColor="clawd_background">
  █████
</Text>
```

---

## Logo & Welcome Screen

### ASCII Art Mascot: "Clawd"

**File**: `src/components/LogoV2/Clawd.tsx` (240 lines)

The mascot is **9 columns wide, 3 rows tall** using block drawing characters:

```
Standard Terminal Poses:

default:
 ▐▛███▜▌   <- Row 1: side | eyes+forehead | side
▝▜█████▛▘  <- Row 2: arm | body | arm
  ▘▘ ▝▝    <- Row 3: feet

arms-up:
▗▟▛███▜▙▖  <- Raised arms (bottom-heavy chars)
 ▜█████▛   <- Body
  ▘▘ ▝▝    <- Feet

look-left / look-right:
Different eye characters (▙▟ for pupils)
```

**Apple Terminal Variant**:
Uses background-fill trick (no vertical spacing between bg colors):
```
▗ ▗   ▖ ▖  <- Forehead + inverted eyes
  (7 spaces with bg)
▘▘ ▝▝      <- Feet
```

### Welcome Screen Layout

**File**: `src/components/LogoV2/WelcomeV2.tsx` (57KB, heavily optimized)

**Width**: Fixed at 58 columns (`WELCOME_V2_WIDTH = 58`)

**Structure**:
```
Welcome to Claude Code v2.1.88
…………………………………………………………………………………………………
                                          
     *                         █████▓▓▒    <- ASCII art
                    *        ███▓▒     ▒▒  
            ▒▒▒▒▒▒           ███▓▒          
    ▒▒▒   ▒▒▒▒▒▒▒▒▒▒         ███▓▒          
   ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒  *     ██▓▒▒      ▓  
                              ▒▓▓███▓▓▒   
 *                  ▒▒▒▒                  
                  ▒▒▒▒▒▒▒▒                
                ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒          
      █████████                      *    
      ██▄█████▄██                *        
      █████████      *                   
……………… ▌ ▌   ▌ ▌…………………………………………………………
```

**Rendering Pattern**:
- Uses `<Box width={58}>` for consistent width
- Light theme: subtle shading with ▒ (░▒▓█)
- Dark theme: stars (*) and ASCII art with dimmed sections
- Heavy use of React compiler memoization (`Symbol.for("react.memo_cache_sentinel")`)

---

## Design System Components

### ThemedText

**File**: `src/components/design-system/ThemedText.tsx`

Wrapper around Ink's `<Text>` with theme-aware colors:

```tsx
<ThemedText color="claude">Text</ThemedText>
<ThemedText color="success" bold>Success</ThemedText>
<ThemedText dimColor>Secondary text</ThemedText>
```

### ThemedBox

**File**: `src/components/design-system/ThemedBox.tsx`

Layout primitive with theme-aware borders and backgrounds:

```tsx
<ThemedBox
  borderStyle="round"
  borderColor="promptBorder"
  padding={1}
>
  {children}
</ThemedBox>
```

### Dialog

**File**: `src/components/design-system/Dialog.tsx` (14KB)

Modal dialog pattern:
- Centered overlay with backdrop
- ▔ top border for separation
- Keyboard navigation support
- Auto-sizing based on content

### Divider

**File**: `src/components/design-system/Divider.tsx` (11KB)

Horizontal/vertical separators with customizable characters:
- Default: `─` horizontal, `│` vertical
- Supports labels and color theming
- Auto-width/height based on parent

### ProgressBar

**File**: `src/components/design-system/ProgressBar.tsx` (7KB)

Animated progress indicator:
```
████████░░░░░░░░ 45%
```
- Supports indeterminate mode (shimmer animation)
- Theme-aware colors
- Configurable width and characters

---

## Layout Patterns

### FullscreenLayout

**File**: `src/components/FullscreenLayout.tsx` (85KB)

**Core structure**:
```tsx
<Box flexDirection="column" height="100%">
  {/* Sticky header (when scrolled) */}
  {stickyPrompt && <StickyHeader />}
  
  {/* Scrollable content area */}
  <ScrollBox ref={scrollRef} flexGrow={1}>
    {scrollable}
    {overlay}
  </ScrollBox>
  
  {/* Fixed bottom area */}
  <Box flexDirection="column">
    {bottom}
  </Box>
  
  {/* Modal overlay (absolute positioned) */}
  {modal && <ModalPane>{modal}</ModalPane>}
</Box>
```

**Features**:
- **Unseen divider tracking**: Snapshots scroll position when user scrolls up
- **"N new messages" pill**: Shows count of unseen messages with jump button
- **Sticky prompt header**: Shows user's original prompt when scrolled away
- **Modal system**: Overlays dialogs without blocking scrollback
- **Viewport management**: Handles terminal resize gracefully

### VirtualMessageList

**File**: `src/components/VirtualMessageList.tsx` (148KB)

Virtualized rendering for performance:
- Only renders visible messages + buffer
- Infinite scroll-back (loads older messages on demand)
- Height estimation and correction
- Scroll position preservation during updates

---

## Typography Patterns

### Font Weights
Claude Code uses **bold** sparingly:
- Main headings: `<Text bold>`
- Important callouts: `<Text bold color="warning">`
- Default: regular weight

### Dimming Pattern
```tsx
<Text dimColor>Secondary info</Text>
<Text color="inactive">Disabled state</Text>
<Text color="subtle">Hints and metadata</Text>
```

### Icons with Figures

Uses `figures` package for cross-platform icons:
```typescript
import figures from 'figures'

figures.tick     // ✔
figures.cross    // ✖
figures.info     // ℹ
figures.warning  // ⚠
figures.pointer  // ❯
figures.dot      // ․
figures.ellipsis // …
```

---

## Box Drawing Characters

### Used Throughout

```
Box Drawing:
┌─┬─┐  ╭─┬─╮  ╔═╦═╗
│ │ │  │ │ │  ║ ║ ║
├─┼─┤  ├─┼─┤  ╠═╬═╣
└─┴─┘  ╰─┴─╯  ╚═╩═╝

Block Elements:
█ ▓ ▒ ░  (full to light shading)
▀ ▄ ▌ ▐  (half blocks)
▗▖▘▝▟▙▜▛ (quadrants)

Borders:
─ │ ┼ ┴ ┬ ├ ┤ (single line)
═ ║ ╬ ╩ ╦ ╠ ╣ (double line)
```

### ASCII Art Technique

1. **Spacing characters**: Use `…` (…) for subtle backgrounds
2. **Shading gradient**: `░▒▓█` for depth
3. **Precise alignment**: Fixed-width fonts assumed
4. **Background colors**: Applied selectively (e.g., Clawd's eyes)

---

## Animation Patterns

### Spinner Component

**File**: `src/components/Spinner.tsx` (88KB)

**Types**:
1. **Dot spinner**: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` (Braille patterns)
2. **Progress**: `▁▂▃▄▅▆▇█` (vertical bars)
3. **Shimmer**: Color interpolation between base and shimmer variants

**Usage**:
```tsx
<Spinner
  type="dots"
  color="claude"
  message="Thinking..."
/>
```

### Shimmer Effect

Colors have paired shimmer variants:
```typescript
claude: 'rgb(215,119,87)'
claudeShimmer: 'rgb(235,159,127)'  // +20 lightness
```

Frame interpolation:
```typescript
const frame = Math.floor(Date.now() / 100) % 10
const isShimmer = frame < 5
const color = isShimmer ? 'claudeShimmer' : 'claude'
```

---

## Keyboard Shortcuts

### Standard Patterns

```typescript
// Example from keybindings system
Ctrl+C      → Cancel/interrupt
Ctrl+D      → Exit (bare prompt only)
Enter       → Submit
Shift+Enter → New line
↑/↓         → History navigation
Ctrl+R      → Reverse search
Tab         → Autocomplete
Esc         → Cancel dialog
```

### Custom Bindings

Stored in `~/.claude/keybindings.json`:
```json
{
  "submit": ["enter"],
  "newline": ["shift+enter"],
  "cancel": ["ctrl+c", "esc"],
  "scrollUp": ["pageup", "ctrl+u"],
  "scrollDown": ["pagedown", "ctrl+d"]
}
```

---

## State Management

### React Context Pattern

```typescript
// Example: ThemeProvider
const ThemeContext = createContext<ThemeContextValue>({
  themeSetting: 'dark',
  setThemeSetting: () => {},
  currentTheme: 'dark'
})

export function ThemeProvider({ children }) {
  const [themeSetting, setThemeSetting] = useState('dark')
  const [systemTheme, setSystemTheme] = useState('dark')
  
  // Auto theme detection via OSC 11
  useEffect(() => {
    if (themeSetting === 'auto') {
      watchSystemTheme(setSystemTheme)
    }
  }, [themeSetting])
  
  const currentTheme = themeSetting === 'auto' 
    ? systemTheme 
    : themeSetting
  
  return (
    <ThemeContext.Provider value={{ currentTheme, setThemeSetting }}>
      {children}
    </ThemeContext.Provider>
  )
}
```

### Performance: useSyncExternalStore

For scroll position tracking without re-renders:
```typescript
const isScrolledUp = useSyncExternalStore(
  // Subscribe
  callback => scrollBox.subscribe(callback),
  // Snapshot
  () => scrollBox.getScrollTop() < scrollBox.getMaxScroll()
)
```

---

## Ink Extensions

### Custom Components

Claude Code extends Ink with:
- **ScrollBox**: Custom scrollable container with precise control
- **VirtualList**: Windowed rendering for large lists
- **ModalPane**: Absolute positioning for dialogs
- **DragSelect**: Text selection with mouse in alt screen buffer

### Terminal Capabilities Detection

```typescript
import { env } from './utils/env.js'

// Apple Terminal has special handling
if (env.terminal === 'Apple_Terminal') {
  // Use background-fill trick for ASCII art
  // Disable certain animations
}

// Feature detection
const supportsHyperlinks = env.supportsHyperlinks
const supports256Colors = env.colorDepth >= 256
const supportsTrueColor = env.colorDepth >= 16777216
```

---

## Message Rendering

### Message Types

```typescript
type Message = {
  role: 'user' | 'assistant'
  content: ContentBlock[]
  timestamp: number
}

type ContentBlock =
  | { type: 'text', text: string }
  | { type: 'tool_use', name: string, input: any }
  | { type: 'tool_result', tool_use_id: string, content: any }
  | { type: 'thinking', thinking: string }
```

### Rendering Pattern

```tsx
// Simplified structure
<Message>
  <MessageHeader>
    <Avatar role={role} />
    <Timestamp />
  </MessageHeader>
  
  <MessageContent>
    {content.map(block => (
      <ContentBlock key={block.id} block={block} />
    ))}
  </MessageContent>
  
  <MessageActions>
    <Button>Copy</Button>
    <Button>Edit</Button>
    <Button>Retry</Button>
  </MessageActions>
</Message>
```

---

## File Structure Best Practices

### Component Organization

```
components/
├── ComponentName/
│   ├── ComponentName.tsx      # Main component
│   ├── ComponentNameHeader.tsx # Sub-component
│   ├── ComponentNameBody.tsx
│   └── hooks/
│       └── useComponentLogic.ts
```

### Naming Conventions

- **Components**: PascalCase (e.g., `FullscreenLayout`)
- **Hooks**: camelCase with `use` prefix (e.g., `useTheme`)
- **Utils**: camelCase (e.g., `getTheme`)
- **Constants**: SCREAMING_SNAKE_CASE (e.g., `WELCOME_V2_WIDTH`)
- **Types**: PascalCase (e.g., `ThemeName`)

---

## Performance Optimizations

### React Compiler

Claude Code uses the **React Compiler** extensively:
```typescript
import { c as _c } from "react/compiler-runtime"

export function Component() {
  const $ = _c(10)  // Memoization slots
  
  if ($[0] !== dep) {
    $[1] = <Text>Computed</Text>
    $[0] = dep
  }
  
  return $[1]
}
```

### Memoization Patterns

```typescript
// Symbol-based cache sentinel
if ($[0] === Symbol.for("react.memo_cache_sentinel")) {
  $[0] = <ExpensiveComponent />
}
```

### Lazy Loading

```typescript
// Dynamic imports for features
const importDialog = () => import('./Dialog.js')

// Feature-flagged code elimination
import { feature } from 'bun:bundle'
if (feature('ADVANCED_MODE')) {
  // Tree-shaken in production
}
```

---

## Testing Patterns

### Test Files Not Included

The decompiled source doesn't include test files, but based on structure:

**Likely structure**:
```
__tests__/
├── components/
│   └── Clawd.test.tsx
├── utils/
│   └── theme.test.ts
└── integration/
    └── fullscreen.test.tsx
```

---

## Key Takeaways for Zaion TUI

### 1. Color System
- Use explicit RGB values, not ANSI colors
- Provide 6 theme variants (dark/light × normal/daltonized/ansi)
- Include shimmer variants for animations
- Theme-aware components via React context

### 2. Layout
- Fixed-width welcome screen (58 cols)
- Fullscreen layout with scroll management
- Sticky elements for headers/prompts
- Modal overlay system

### 3. Typography
- Minimal bold usage
- Three dimming levels: normal / dimColor / inactive
- Cross-platform icons via `figures` package

### 4. Performance
- React Compiler for memoization
- Virtual scrolling for large lists
- useSyncExternalStore for scroll without re-renders
- Lazy loading for dialogs and features

### 5. Accessibility
- Daltonized themes for color blindness
- ANSI fallback for limited terminals
- Keyboard-first navigation
- Screen reader compatible (semantic roles)

---

## Additional Resources

### Files to Read for Deep Dives

1. **Theme System**: `src/utils/theme.ts` (640 lines)
2. **Main Layout**: `src/components/FullscreenLayout.tsx` (85KB)
3. **Scroll Logic**: `src/components/VirtualMessageList.tsx` (148KB)
4. **Message Rendering**: `src/components/Message.tsx` (79KB)
5. **Design System**: `src/components/design-system/` (17 files)

### Key Patterns Summary

| Pattern | File | Lines | Purpose |
|---------|------|-------|---------|
| Theme System | `theme.ts` | 640 | Color management |
| Clawd Mascot | `Clawd.tsx` | 240 | ASCII art character |
| Welcome Screen | `WelcomeV2.tsx` | 57KB | Branded intro |
| Fullscreen | `FullscreenLayout.tsx` | 85KB | Main layout |
| Spinner | `Spinner.tsx` | 88KB | Loading states |
| Messages | `Messages.tsx` | 147KB | Chat rendering |

---

**End of Document**
