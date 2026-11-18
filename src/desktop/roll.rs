use super::theme::Theme;
use gpui::{App, Bounds, PathBuilder, Pixels, TextRun, Window, font, point, px, quad, rgb, size};
use std::sync::Arc;
use voice::model::{AnalysisMode, Note, PitchFrame, TrackAnalysis};

pub fn note_name(midi: f32) -> String {
    let n = midi.round() as i32;
    let names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        names[n.rem_euclid(12) as usize],
        n.div_euclid(12) - 1
    )
}

pub fn clock(seconds: f64) -> String {
    let millis = (seconds.max(0.0) * 1000.0) as u64;
    format!(
        "{:02}:{:02}.{:01}",
        millis / 60_000,
        millis / 1000 % 60,
        millis / 100 % 10
    )
}

pub fn rect(window: &mut Window, x: f32, y: f32, w: f32, h: f32, color: u32, radius: f32) {
    if w > 0.0 && h > 0.0 {
        window.paint_quad(quad(
            Bounds::new(point(px(x), px(y)), size(px(w), px(h))),
            px(radius),
            rgb(color),
            px(0.),
            gpui::transparent_black(),
            Default::default(),
        ));
    }
}

fn text(
    window: &mut Window,
    cx: &mut App,
    label: String,
    x: f32,
    y: f32,
    color: u32,
    font_size: f32,
) {
    let run = TextRun {
        len: label.len(),
        font: font(".SystemUIFont"),
        color: rgb(color).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let shaped = window
        .text_system()
        .shape_line(label.into(), px(font_size), &[run], None);
    let _ = shaped.paint(point(px(x), px(y)), px(font_size + 3.), window, cx);
}

#[derive(Clone)]
pub struct RollData {
    pub target: Arc<TrackAnalysis>,
    pub selected: Arc<TrackAnalysis>,
    pub trace: Arc<Vec<PitchFrame>>,
    pub cursor: f64,
    pub start: f64,
    pub seconds: f64,
    pub loop_range: Option<(f64, f64)>,
    pub max_note_duration: f64,
}

pub const LABEL_WIDTH: f32 = 54.;
pub const RULER_HEIGHT: f32 = 28.;

fn visible_notes(
    notes: &[Note],
    start: f64,
    end: f64,
    max_duration: f64,
) -> impl Iterator<Item = &Note> {
    let first = notes.partition_point(|n| n.start < start - max_duration);
    notes[first..]
        .iter()
        .take_while(move |n| n.start <= end)
        .filter(move |n| n.end >= start)
}

pub fn paint_roll(bounds: Bounds<Pixels>, data: &RollData, window: &mut Window, cx: &mut App) {
    let theme = Theme::for_window(window);
    let bx: f32 = bounds.origin.x.into();
    let by: f32 = bounds.origin.y.into();
    let bw: f32 = bounds.size.width.into();
    let bh: f32 = bounds.size.height.into();
    let x0 = bx + LABEL_WIDTH;
    let width = (bw - LABEL_WIDTH).max(1.);
    let top = by + RULER_HEIGHT;
    let height = (bh - RULER_HEIGHT).max(1.);
    let end = data.start + data.seconds;
    let mut low = 60.;
    let mut high = 72.;
    let visible: Vec<_> = visible_notes(
        &data.selected.notes,
        data.start,
        end,
        data.max_note_duration,
    )
    .collect();
    let candidates: Vec<_> = if data.target.mode == AnalysisMode::Polyphonic {
        visible_notes(&data.target.notes, data.start, end, data.max_note_duration).collect()
    } else {
        Vec::new()
    };
    if !visible.is_empty() {
        low = visible.iter().map(|n| n.midi).fold(f32::INFINITY, f32::min);
        high = visible
            .iter()
            .map(|n| n.midi)
            .fold(f32::NEG_INFINITY, f32::max);
    } else {
        let first = data
            .selected
            .frames
            .partition_point(|f| f.time < data.start);
        for f in data.selected.frames[first..]
            .iter()
            .take_while(|f| f.time <= end)
            .filter(|f| f.voiced())
        {
            low = low.min(f.midi);
            high = high.max(f.midi);
        }
    }
    for note in &candidates {
        low = low.min(note.midi);
        high = high.max(note.midi);
    }
    let center = (low + high) / 2.;
    let span = (high - low + 6.).clamp(18., 128.);
    let min = (center - span / 2.).floor().max(0.);
    let max = (min + span).ceil();
    let row = height / (max - min + 1.);
    let tx = |t: f64| x0 + ((t - data.start) / data.seconds) as f32 * width;
    let ny = |m: f32| top + (max - m + 0.5) * row;

    rect(window, bx, by, bw, bh, theme.canvas, 0.);
    for n in min as i32..=max as i32 {
        let y = ny(n as f32) - row / 2.;
        let black = [1, 3, 6, 8, 10].contains(&n.rem_euclid(12));
        rect(
            window,
            x0,
            y,
            width,
            row,
            if black {
                theme.canvas
            } else {
                theme.canvas_alt
            },
            0.,
        );
        rect(
            window,
            bx,
            y,
            LABEL_WIDTH - 1.,
            row - 1.,
            if black { theme.black_key } else { theme.key },
            0.,
        );
        if row >= 12. || n.rem_euclid(12) == 0 {
            text(
                window,
                cx,
                note_name(n as f32),
                bx + 14.,
                y + (row - 13.) / 2.,
                if n.rem_euclid(12) == 0 {
                    theme.text
                } else {
                    theme.secondary
                },
                11.,
            );
        }
        rect(
            window,
            x0,
            y,
            width,
            1.,
            if n.rem_euclid(12) == 0 {
                theme.octave_grid
            } else {
                theme.grid
            },
            0.,
        );
    }
    let step = if data.seconds > 32. {
        10.
    } else if data.seconds > 16. {
        5.
    } else {
        2.
    };
    let mut t = (data.start / step).ceil() * step;
    while t <= end {
        let x = tx(t);
        rect(window, x, top, 1., height, theme.grid, 0.);
        text(window, cx, clock(t), x + 6., by + 6., theme.secondary, 11.);
        t += step;
    }
    if let Some((a, b)) = data.loop_range {
        for t in [a, b] {
            if t >= data.start && t <= end {
                rect(window, tx(t), by, 1., bh, theme.secondary, 0.);
            }
        }
    }
    if data.target.mode == AnalysisMode::Polyphonic {
        for n in candidates.iter().filter(|n| n.midi >= min && n.midi <= max) {
            let x = tx(n.start.max(data.start));
            let right = tx(n.end.min(end));
            rect(
                window,
                x,
                ny(n.midi) - row * 0.3,
                (right - x).max(2.),
                row * 0.6,
                theme.candidate,
                2.,
            );
        }
    }
    for n in visible.iter().filter(|n| n.midi >= min && n.midi <= max) {
        let x = tx(n.start.max(data.start));
        let right = tx(n.end.min(end));
        rect(
            window,
            x,
            ny(n.midi) - row * 0.29,
            (right - x).max(2.),
            (row * 0.58).max(4.),
            theme.note,
            3.,
        );
        if right - x > 40. && row > 17. {
            text(
                window,
                cx,
                note_name(n.midi),
                x + 7.,
                ny(n.midi) - 6.,
                theme.note_text,
                10.,
            );
        }
    }
    if data.selected.mode == AnalysisMode::Monophonic {
        pitch_path(
            &data.selected.frames,
            data,
            (min, max),
            &tx,
            &ny,
            (theme.accent, 1.5),
            window,
        );
    }
    pitch_path(
        &data.trace,
        data,
        (min, max),
        &tx,
        &ny,
        (theme.live, 2.5),
        window,
    );
    if data.cursor >= data.start && data.cursor <= end {
        let x = tx(data.cursor);
        rect(window, x, by, 1.5, bh, theme.cursor, 0.);
        rect(
            window,
            x - 3.,
            by + RULER_HEIGHT - 5.,
            7.,
            5.,
            theme.cursor,
            1.,
        );
    }
    rect(
        window,
        bx,
        by + RULER_HEIGHT - 1.,
        bw,
        1.,
        theme.separator,
        0.,
    );
}

fn pitch_path(
    frames: &[PitchFrame],
    data: &RollData,
    range: (f32, f32),
    tx: &impl Fn(f64) -> f32,
    ny: &impl Fn(f32) -> f32,
    stroke: (u32, f32),
    window: &mut Window,
) {
    let (min, max) = range;
    let (color, width) = stroke;
    let first = frames.partition_point(|f| f.time < data.start);
    let mut path = PathBuilder::stroke(px(width));
    let mut last: Option<PitchFrame> = None;
    let mut count = 0;
    for frame in frames[first..]
        .iter()
        .take_while(|f| f.time <= data.start + data.seconds)
    {
        if !frame.voiced() || frame.midi < min || frame.midi > max {
            last = None;
            continue;
        }
        let p = point(px(tx(frame.time)), px(ny(frame.midi)));
        if last.is_some_and(|f| frame.time - f.time < 0.1 && (frame.midi - f.midi).abs() < 3.) {
            path.line_to(p);
            count += 1;
        } else {
            path.move_to(p);
        }
        last = Some(*frame);
    }
    if count > 0
        && let Ok(path) = path.build()
    {
        window.paint_path(path, rgb(color));
    }
}

pub fn paint_overview(
    bounds: Bounds<Pixels>,
    target: &TrackAnalysis,
    cursor: f64,
    duration: f64,
    view: (f64, f64),
    window: &mut Window,
) {
    let x: f32 = bounds.origin.x.into();
    let y: f32 = bounds.origin.y.into();
    let w: f32 = bounds.size.width.into();
    let h: f32 = bounds.size.height.into();
    let theme = Theme::for_window(window);
    rect(window, x, y, w, h, theme.canvas_alt, 0.);
    let duration = duration.max(0.001);
    let bins = w.ceil() as usize;
    let mut levels = vec![0_f32; bins];
    for p in &target.waveform {
        let bin = ((p.time / duration) * w as f64) as usize;
        if bin < bins {
            levels[bin] = levels[bin].max(p.max.abs().max(p.min.abs()));
        }
    }
    for (i, level) in levels.into_iter().enumerate() {
        let a = level.clamp(0., 1.) * (h - 12.) / 2.;
        rect(
            window,
            x + i as f32,
            y + h / 2. - a,
            1.,
            (a * 2.).max(1.),
            theme.waveform,
            0.,
        );
    }
    let a = x + (view.0 / duration) as f32 * w;
    let b = x + (view.1.min(duration) / duration) as f32 * w;
    rect(window, a, y, (b - a).max(1.), 2., theme.accent, 0.);
    rect(
        window,
        x + (cursor / duration) as f32 * w,
        y,
        1.5,
        h,
        theme.cursor,
        0.,
    );
}
