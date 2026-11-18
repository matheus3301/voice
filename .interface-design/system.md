# Voice interface system — macOS v0.5

This documents the current implementation in `src/desktop/`. It is a native document application for a singer studying a recorded part. The pitch canvas leads; file management and hardware configuration use familiar macOS conventions. These are implementation patterns, not a record of runtime visual validation.

## Direction

- **Domain:** vocal reference, accompaniment, live pitch, notes, cents, intervals, register, A–B repetition and practice takes.
- **Color world:** neutral macOS window chrome, white/charcoal score surfaces, quiet staff-like grid lines, blue reference notes, orange live pitch and gray transcription candidates.
- **Signature:** a note timeline with the selected vocal part and live singing on the same pitch/time coordinates, supported by a full-recording waveform and loop markers.
- **Hierarchy:** the musical canvas takes remaining document space. A transport of at least 52 px, 34 px legend, compact waveform and 42 px status row support the canvas. A 330 px mixer on the right keeps routing and the three live levels available while singing. Reference review and history remain secondary.
- **Avoid:** terminal typography/palettes, a web navigation shell, decorative metric cards, promotional copy and a dashboard of equally weighted panels.

## Native surfaces and navigation

Use the actual macOS menu bar for application, file, playback, view and window commands. File actions use attached AppKit Open/Save sheets. Dirty state and represented file belong to NSWindow document chrome. Save/discard/cancel protection belongs to native document alerts. Microphone authorization belongs to the operating system, requested only after the person enables input.

Audio Settings opens in its own window via Cmd+,. Reuse/focus that window rather than creating duplicates. The current default size is 520 × 650 px, minimum 450 × 450 px, with a scrollable 20 px inset. Escape or Cmd+W closes Settings without closing the document.

The mixer is a persistent, hideable right sidebar. It contains input/output/channel selection, microphone and monitoring controls, recording, three vertical faders and the latest take. Native View > Mixer and the toolbar toggle its visibility. Keep the musical canvas primary; the mixer is a compact working surface rather than a collection of metric cards.

Vocal Reference, Practice History and keyboard help are GPUI overlays within the document. Do not describe these as AppKit sheets. They use a 600 px maximum width, 12 px corner radius, 20 px content inset, scrollable body and a trailing Done button. The parent controls are inactive behind the overlay; dismissing returns document focus.

## Typography and spacing

Use `.SystemUIFont` throughout controls and the musical canvas. Base text is 13 px; supporting labels are 12 px; overlay headings are 17 px semibold; the empty-state instruction is 21 px medium. Weight, foreground tone and spacing establish hierarchy. The canvas has a 54 px note-label gutter and 28 px time ruler.

Use a 4 px spacing rhythm. Common gaps are 8/12/16/20 px. Related controls stay compact; sections receive 16 px separation. Document toolbars use 16 px horizontal inset. Mixer controls use 12 px insets, compact labeled routing fields and three equally spaced 94 px channel strips. Avoid enlarged marketing headings.

## Palette and depth

`Theme::for_window` selects light or dark tokens from window appearance. Keep all GPUI colors in `src/desktop/theme.rs`; it is the source of truth. Current semantic values:

| Token | Light | Dark | Role |
| --- | --- | --- | --- |
| window | `#F2F2F4` | `#242426` | Document/settings chrome |
| toolbar | `#F7F7F8` | `#2A2A2D` | Transport and status |
| content | `#FFFFFF` | `#202023` | Main content |
| text | `#1D1D1F` | `#F2F2F7` | Primary text |
| secondary | `#6E6E73` | `#A5A5AB` | Supporting labels |
| separator | `#D5D5DA` | `#414146` | Quiet structural edges |
| control | `#FFFFFF` | `#3A3A3F` | Control surface |
| control_hover | `#ECECF0` | `#48484D` | Hover feedback |
| accent | `#007AFF` | `#0A84FF` | Action/selected reference |
| live | `#C76C09` | `#FF9F0A` | Your voice |
| meter | `#238C48` | `#30D158` | Active signal below warning level |
| recording | `#D93025` | `#FF453A` | Recording action and near-clipping signal |
| candidate | `#DCE1E9` | `#474D58` | Unselected estimated notes |

These are appearance-specific application tokens; the accent is currently blue rather than the user's configurable macOS accent color. Depth comes from neutral surface changes and thin separators. Reserve a shadow and subtle backdrop for document overlays. File sheets and native panels use system rendering. Avoid decorative gradients or multiple competing accents.

## Reusable controls and states

The shared GPUI button pattern is at least 28 px high, with 12 px horizontal/4 px vertical padding, a 6 px radius, 1 px separator border and 13 px text. Active controls use blue with white text; hover changes the surface; disabled controls use 42% opacity. Focus uses the accent border. Enabled tab stops support Enter/Space activation. Do not imply these controls are AppKit widgets.

Labeled preference rows are at least 30 px high with a 16 px gap, secondary 12 px labels and trailing controls. Section labels use semibold text and a 16 px top margin. Reuse these patterns for device, gain, buffer, latency and note-range settings.

The empty document stages a required vocal reference and optional accompaniment before Continue. Both fields support choosing, replacing, removing and dropping a file. The reference mode is chosen in this form. Continue imports the staged pair; no analysis begins from merely choosing the first file. Open Project remains a secondary action. Busy states name the operation and show progress; safe analysis cancellation preserves the current reference. Keep errors visible without replacing the musical workspace.

Microphone states distinguish off, permission pending, connecting, on, denied and restricted. Do not show capture as active until the stream reports it. Settings offers an operating-system shortcut when permission is denied. Hear myself explicitly starts input and monitoring together; it does not require a second microphone action. Use headphones guidance beside this control.

Input/output/channel selectors act immediately and persist their choice on this Mac. Rendering reads cached inventory and cached persistence status only. GPUI popup choices support arrows, Home/End, Enter/Space, Escape, outside-click dismissal and Tab navigation; keep the active item visible when scrolling. Advanced buffer, capture gain and timing correction remain in Audio Settings.

Faders control Your voice monitoring, Vocal reference and Accompaniment independently. All use linear gain 0–2, displayed in dB: silence at the bottom, unity (0 dB) at 80% of travel and approximately +6 dB at the top. Pointer dragging updates continuously. Arrow keys move by 1 dB, Shift+arrows by 0.1 dB; Home silences, End sets maximum, Enter or double-click restores unity. The voice fader changes monitor gain, never capture gain. Its adjacent meter shows actual input even when monitoring is off; the other meters show actual track peaks. Paint only cached audio snapshots, never fabricated activity or device queries.

Record starts a saved take; Stop recording finishes it. A connecting recording can be canceled. Finalization shows a disabled Saving take… control, distinct from cancellation. Recording uses a single pass and temporarily suspends looping; Loop and A/B controls are disabled until completion. R invokes the same recording action when a text/control context is not focused.

Latest take and Practice History expose Listen/Stop for stored audio, its duration and stored practice result. Pitch-only historical takes say so. Disable comparison against a different vocal reference and explain the mismatch. Listening maps the saved voice trace onto the musical timeline; the displayed stored result is not recomputed from a later reference selection.

## Musical semantics

- Label the studied recording **Vocal reference**, optional music **Accompaniment**, and live input **Your voice**.
- Reference notes are blue; live pitch is orange. Repeat these colors in the legend and note/status labels, with text identifying each meaning.
- Polyphonic candidates are gray; the chosen line is blue. Higher/lower selection plus a note range describes the operation without assigning singer identities or choir sections.
- An unreviewed polyphonic line has a visible review action and a blocked score state. Changing the selection requires review again.
- Keep start/play/pause/loop controls close to the time display. Toolbar groups wrap at narrow window widths. A/B markers use the current cursor. The waveform is an overview and a seek surface, not a second competing editor.
- Show In tune, Out of tune, Missed and coverage only when there is evaluated reference time. In tune is correct duration divided by expected reference duration; Out of tune is coverage minus In tune; Missed is 100 minus coverage. An absent evaluated capture reads Not scored rather than a misleading zero score.

## Maintenance

Update this file when shared patterns change. Match claims to implemented code and keep runtime visual/accuracy evidence in the validation documents. Preserve `Research/web-v0.2/` as historical material; its graphite/monospace/mint direction is not the active design system.
