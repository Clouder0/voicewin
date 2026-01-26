# VoiceWin UI/UX Specification Document
**Version:** 3.0 (Golden Master)
**Status:** FROZEN
**Target System:** Windows 11 (Fluent Design System)

---

## ⚠️ Implementation Directive
This document is the **single source of truth**. Developers must not deviate from these specifications without a formal design change request.
- **Do not** improvise spacing.
- **Do not** guess animations.
- **Do not** use "standard" HTML styles.
- **Strictly** adhere to the layouts, measurements, and state transitions defined below.

---

## 1. Design Tokens (The DNA)

All UI elements must utilize these specific tokens. Do not use hardcoded pixel values in the implementation unless explicitly stated here.

### 1.1 Spatial System (Grid & Spacing)
*Base Unit:* **4px**.
*   `space-4`: 4px (Tiny gap)
*   `space-8`: 8px (Small gap, component internal padding)
*   `space-12`: 12px (Standard gap between related items)
*   `space-16`: 16px (Container padding, button horizontal padding)
*   `space-24`: 24px (Section separation)
*   `space-32`: 32px (Page margins)
*   `space-48`: 48px (Major layout separation)

### 1.2 Typography (Segoe UI Variable)
*Font Stack:* `"Segoe UI Variable Display" (Headers), "Segoe UI Variable Text" (Body), "Consolas" (Code).*

| Token Name | Font Family | Size | Line Height | Weight | Character Spacing |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Type-Display** | Variable Display | 28px | 36px | 600 (Semibold) | -0.01em |
| **Type-Title** | Variable Display | 20px | 28px | 600 (Semibold) | Normal |
| **Type-Subtitle** | Variable Text | 16px | 24px | 600 (Semibold) | Normal |
| **Type-BodyStrong**| Variable Text | 14px | 20px | 600 (Semibold) | Normal |
| **Type-Body** | Variable Text | 14px | 20px | 400 (Regular) | Normal |
| **Type-Caption** | Variable Text | 12px | 16px | 400 (Regular) | Normal |
| **Type-Mono** | Consolas | 13px | 20px | 400 (Regular) | Normal |

### 1.3 Color Palette (Semantic)
*Note: Values refer to WinUI 3 Theme Resources. Implementation must map these to CSS Variables.*

**Surfaces:**
*   `Surface-Window`: **Mica Alt** (OS Material). Fallback: `#F3F3F3` (Light), `#202020` (Dark).
*   `Surface-Card`: `LayerOnMicaBaseAltFillColorDefault`. (Approx: White 50% opacity Light / Grey 10% opacity Dark).
*   `Stroke-Card`: `CardStrokeColorDefault` (Approx: Black 5% Light / White 10% Dark).

**Brand & Status:**
*   `Color-Accent`: **System Accent Color** (Dynamic).
*   `Color-Accent-Text`: **TextOnAccentFillColorPrimary** (Usually White).
*   `Color-Danger-Bg`: `#FDE7E9` (Light) / `#431316` (Dark).
*   `Color-Danger-Fg`: `#C50F1F` (Light) / `#FF99A4` (Dark).
*   `Color-Success-Fg`: `#107C10` (Light) / `#6CCB5F` (Dark).

### 1.4 Shapes & Elevation
*   `Radius-Window`: **8px** (Main Window).
*   `Radius-Overlay`: **24px** (HUD Pill).
*   `Radius-Card`: **4px** (Standard).
*   `Radius-Button`: **4px**.
*   `Shadow-Flyout`: `0px 8px 16px rgba(0,0,0,0.14)` (HUD).
*   `Shadow-Card`: None (Flat border).

---

## 2. Surface Specification: The Overlay HUD

**Context:** This is a Floating Window (Topmost, No Focus Stealing).
**Position:** `Bottom: 80px` (from screen bottom), `Left: 50%` (Centered). Transform: `translateX(-50%)`.

### 2.1 Layout (The Pill)
*   **Container Size:**
    *   Height: **48px** (Fixed).
    *   Width: **Variable** (Fit content). Min-width: **160px**. Max-width: **600px**.
*   **Background:** Acrylic Brush (Blur 30px, Tint Opacity 80%).
*   **Border:** 1px Solid `SurfaceStrokeColorDefault`.
*   **Corner Radius:** **24px** (Full pill shape).
*   **Padding:** `0px 6px 0px 16px` (Left padding 16px, Right padding 6px for close button).
*   **Content Alignment:** Flexbox. `Align-Items: Center`. `Justify-Content: Space-Between`. `Gap: 16px`.

### 2.2 Internal States (Mutually Exclusive)

#### State A: Recording (Active)
*   **Left Element:** **Mic Icon** (16px).
    *   Color: `Color-Danger-Fg`.
    *   *Animation:* "Breath" -> Opacity oscillates 0.4 to 1.0 over 1.5s (Ease-in-out infinite).
*   **Center Element:** **Visualizer**.
    *   Type: 5 Vertical Bars.
    *   Dimensions: Each bar 3px wide, 2px gap.
    *   Height: Dynamic (4px to 24px) based on audio amplitude.
    *   Color: `Color-Danger-Fg`.
    *   *Alignment:* Left-aligned next to text.
*   **Text:** "Listening..." (`Type-BodyStrong`).
*   **Right Element:** **Stop Button**.
    *   Size: 36x36px.
    *   Icon: Square (Filled) 12px.
    *   Style: `Button-Ghost`.

#### State B: Processing (Transcribing/Enhancing)
*   **Left Element:** **Spinner** (16px).
    *   Type: Indeterminate Ring (Fluent style).
    *   Color: `Color-Accent`.
*   **Center Element:** **Text**.
    *   Content: "Thinking..." (Cycle to "Enhancing..." if LLM active).
    *   Style: `Type-Body`.
*   **Right Element:** **Cancel Button**.
    *   Size: 36x36px.
    *   Icon: 'X' (14px).

#### State C: Success (Completion)
*   **Left Element:** **Checkmark Icon** (16px).
    *   Color: `Color-Success-Fg`.
    *   *Animation:* Scale Up (0 -> 1.2 -> 1.0) duration 300ms.
*   **Center Element:** **Text**.
    *   Content: "Copied" or "Inserted".
    *   Style: `Type-BodyStrong`.
*   **Right Element:** None (Hidden).
*   **Transition:** After 1500ms delay -> **HUD Exit Animation**.

### 2.3 HUD Motion Physics
*   **Entry (Appear):**
    *   Opacity: 0% -> 100%.
    *   TranslateY: +20px -> 0px.
    *   Duration: **200ms**.
    *   Curve: `cubic-bezier(0.0, 0.0, 0.2, 1.0)` (Decelerate).
*   **Exit (Disappear):**
    *   Opacity: 100% -> 0%.
    *   Scale: 1.0 -> 0.95.
    *   Duration: **150ms**.
    *   Curve: `Linear`.

---

## 3. Surface Specification: Main Window

**Window Size:** Default 960px x 680px.
**Material:** `Mica Alt` (Content extends into Titlebar).

### 3.1 Global Layout (Grid)
The Main Window uses a 2-column Grid.
*   **Column 1 (Nav Rail):** Width **68px** (Fixed).
*   **Column 2 (Content):** `1fr` (Flexible).

### 3.2 Component: Navigation Rail (Left)
*   **Background:** Transparent (Mica passes through).
*   **Padding:** `Top: 40px` (To clear Titlebar controls), `Bottom: 16px`.
*   **Alignment:** Flex Column, `Align-Items: Center`.
*   **Item Spec:**
    *   Size: 40px x 40px.
    *   Margin-bottom: 4px.
    *   Corner Radius: 4px.
    *   Icon Size: 20px.
    *   **Normal State:** Color `TextSecondary`, Bg Transparent.
    *   **Hover State:** Bg `SubtleFillColorSecondary`.
    *   **Active State:** Bg `SubtleFillColorTertiary`, Icon Color `Color-Accent`.
    *   *Active Indicator:* A vertical pill (Height 16px, Width 3px) positioned Absolute Left (-12px from icon center) colored `Color-Accent`.

### 3.3 Page: Overview (Dashboard)
**Layout:** Single Column, Centered, Max-Width 600px.
**Top Padding:** 64px.

1.  **Header Group:**
    *   `Type-Display`: "Ready to Dictate"
    *   `Type-Body`: "Press `Caps Lock` to start." (The key name is inside a `<kbd>` tag: Border 1px solid, Bg `LayerFill`, Radius 4px, Padding 2px 6px).
    *   *Spacing:* `space-24` below header.

2.  **Mic Check Hero:**
    *   Shape: Circle. Size: **120px**.
    *   Border: 4px Solid `Stroke-Card`.
    *   Content: Central Icon (48px) `Microphone`.
    *   *Interaction:* On Hover, show tooltip "Click to change device".
    *   *Animation:* When talking, the Border Color becomes `Color-Accent` and a `Box-Shadow` (0 0 20px Accent) pulses.

3.  **Status Cards (Row):**
    *   *Layout:* Grid, 3 Columns, Gap `space-12`.
    *   *Card Spec:* Height 80px, Padding `space-12`. Bg `Surface-Card`. Border 1px Solid `Stroke-Card`. Radius `Radius-Card`.
    *   **Card 1 (Model):**
        *   Top: Icon `HardDrive` (16px).
        *   Middle: "Whisper Base" (`Type-BodyStrong`).
        *   Bottom: "Loaded" (`Type-Caption`, Color `Success-Fg`).
    *   **Card 2 (Provider):**
        *   Top: Icon `Cloud` (16px).
        *   Middle: "Local Engine" (`Type-BodyStrong`).
    *   **Card 3 (Profile):**
        *   Top: Icon `AppWindow` (16px).
        *   Middle: "Default" (`Type-BodyStrong`).

### 3.4 Page: Scenarios (Profiles)
**Layout:** Grid.
*   **Col 1 (List):** 260px Fixed. Border-Right 1px Solid `Stroke-Card`.
*   **Col 2 (Detail):** 1fr. Padding `space-32`.

**List Panel:**
*   Padding: `Top: 40px`, `Left/Right: 12px`.
*   **Header:** Text "Profiles" (`Type-Subtitle`) + Icon Button "Add" (Right aligned).
*   **List Item:**
    *   Height: 64px.
    *   Padding: 12px.
    *   Radius: 4px.
    *   Layout: Grid (Auto 1fr). Gap 12px.
    *   Img: 32px Icon (Generic App Icon).
    *   Text: Title (`BodyStrong`) + Subtitle (`Caption`).
    *   **Selected State:** Bg `LayerOnMicaBaseAltFillColorSecondary`.
    *   **Hover State:** Bg `LayerOnMicaBaseAltFillColorTertiary`.

**Detail Panel (Form):**
*   **Header:** Input Field (Large, Display style) for Profile Name.
*   **App Matcher:**
    *   Label: "Target Application" (`BodyStrong`).
    *   Row: Input Field (Placeholder "code.exe") + Button "Pick Window" (Icon `Eyedropper`).
*   **Overrides (Toggles):**
    *   Layout: Vertical Stack, Gap `space-16`.
    *   **Toggle Row:**
        *   Left: Text "Override Model".
        *   Right: Toggle Switch.
        *   *If On:* Show Dropdown immediately below (Animate Height).

### 3.5 Page: Model Library
**Layout:** CSS Grid.
*   `grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));`
*   `gap: 16px;`
*   Padding: `space-32`.

**Model Card Spec:**
*   Height: 140px.
*   Bg: `Surface-Card`.
*   Border: 1px Solid `Stroke-Card`.
*   **Content:**
    *   **Top:** Title "Whisper Medium" (`Subtitle`) + Badge "Recommend" (Bg Accent, Text White, Radius 2px, Font 10px).
    *   **Middle:** Metadata Row. "1.5 GB" • "Slow Speed" • "High Accuracy". (`Caption`, Color Secondary).
    *   **Bottom:**
        *   *State Installed:* Button (Secondary) "Active".
        *   *State Not Installed:* Button (Primary) "Download".
        *   *State Downloading:* Progress Bar (Height 2px, Width 100%, Bottom Aligned).

### 3.6 Page: History
**Layout:** Table (Div-based).
*   Padding: `space-32`.
*   **Header Row:**
    *   Height: 32px.
    *   Border-Bottom: 1px Solid `Stroke-Card`.
    *   Text: `Type-Caption`, Color `TextSecondary`.
    *   Cols: Timestamp (100px) | App (150px) | Transcript (1fr) | Actions (80px).
*   **Data Row:**
    *   Height: 56px.
    *   Border-Bottom: 1px Solid `Stroke-Card` (Opacity 0.5).
    *   Hover: Bg `SubtleFillColorSecondary`.
    *   **Col 3 (Transcript):** `white-space: nowrap; overflow: hidden; text-overflow: ellipsis;`.
    *   **Col 4 (Actions):** Visible on Row Hover only. Icon Buttons (Copy, Delete).

---

## 4. Component Primitives (Implementation Details)

### 4.1 Inputs (Text Field)
*   **Height:** 32px.
*   **Bg:** `ControlFillColorDefault` (Solid relative to theme).
*   **Border:**
    *   Top/Left/Right: 1px Solid `ControlStrokeColorDefault`.
    *   Bottom: 1px Solid `ControlStrokeColorDefault` (Darker).
*   **Radius:** 4px.
*   **Focus State:** Bottom Border becomes 2px Solid `Color-Accent`.

### 4.2 Buttons
**Primary:**
*   Bg: `Color-Accent`.
*   Text: `Color-Accent-Text`.
*   Border: None.
*   Padding: `0 16px`.
*   Height: 32px.
*   *Active (Press):* Opacity 0.8. Scale 0.98.

**Secondary:**
*   Bg: `ControlFillColorDefault` (White/Grey).
*   Border: 1px Solid `ControlStrokeColorDefault`.
*   Text: `TextPrimary`.

---

## 5. Animations & Micro-Interactions

### 5.1 Page Transitions
When switching tabs in the Nav Rail:
1.  **Incoming Page:**
    *   `Opacity`: 0 -> 1.
    *   `TransformY`: 10px -> 0px.
    *   `Duration`: 250ms.
    *   `Easing`: `cubic-bezier(0, 0, 0, 1)`.
2.  **Outgoing Page:** Immediate removal (`display: none`).

### 5.2 Toggle Switch
*   **Track:**
    *   Off: Border 1px solid `TextSecondary`. Bg Transparent.
    *   On: Bg `Color-Accent`. Border None.
*   **Thumb:**
    *   Size: 12px.
    *   TranslateX: 4px (Off) -> 24px (On).
    *   Animation: Spring (stiffness 300, damping 20).

---

## 6. Tray Menu (Native Flyout)
*Use OS Native Menu if possible. If Custom:*
*   **Width:** 220px.
*   **Bg:** `Mica` or `Acrylic`.
*   **Padding:** 4px.
*   **Item:**
    *   Height: 36px.
    *   Padding: 0 12px.
    *   Radius: 4px.
    *   Hover: `SubtleFillColorSecondary`.
    *   Icon: Left aligned, 16px.
    *   Text: `Body`.

---

## 7. Responsive Behavior
*   **HUD:** Expands width based on text length. Max 600px. If text exceeds, truncate with ellipsis.
*   **Main Window:**
    *   Min-Width: 800px.
    *   If Width < 900px: "Overview" status cards stack vertically.
    *   If Height < 600px: Nav Rail becomes scrollable.

---

*(End of Specification)*
