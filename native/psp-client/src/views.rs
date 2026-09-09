use crate::{
    model::{Detail, Thread, Viewport},
    ui::{self, Frame},
};
use alloc::{format, string::String, vec::Vec};

pub const MESSAGE_ROWS: usize = 6;
pub const ACTIVITY_ROWS: usize = 6;
pub const DRAFT_ROWS: usize = 6;
pub const DRAFT_WIDTH: usize = 436;
pub const KEYS: &str = "abcdefghijklmnopqrstuvwxyz0123456789 .,?!-/";
const USER_WIDTH: usize = 350;
const CONTENT_TOP: i32 = 66;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub muted: bool,
    pub user: bool,
    pub first: bool,
    pub last: bool,
    pub bubble_width: usize,
}

impl Line {
    fn plain(text: String, muted: bool) -> Self {
        Self {
            text,
            muted,
            user: false,
            first: true,
            last: true,
            bubble_width: 0,
        }
    }
}

// Keep the visible snapshot fixed while reading a rolling server history.
pub fn refresh_content(
    current: &mut Vec<Line>,
    next: Vec<Line>,
    view: &mut Viewport,
    visible: usize,
) -> bool {
    if view.follow {
        *current = next;
        view.update(current.len(), visible);
        false
    } else {
        *current != next
    }
}

pub fn lines(detail: &Detail, activity: bool) -> Vec<Line> {
    let mut result = Vec::new();
    if activity {
        for item in &detail.activities {
            if !result.is_empty() {
                result.push(Line::plain(String::new(), true));
            }
            for text in ui::wrap_text(&format!("› {item}"), 452) {
                result.push(Line::plain(text, false));
            }
        }
    } else {
        for (role, body) in &detail.messages {
            if !result.is_empty() {
                result.push(Line::plain(String::new(), true));
            }
            let user = role == "user";
            if !user && role != "assistant" {
                result.push(Line::plain(
                    String::from(if role == "system" {
                        "System"
                    } else {
                        "Message"
                    }),
                    true,
                ));
            }
            let wrapped = ui::wrap_text(body, if user { USER_WIDTH } else { 452 });
            let bubble_width = wrapped
                .iter()
                .map(|text| ui::text_width(text))
                .max()
                .unwrap_or(0)
                + 20;
            let count = wrapped.len();
            for (index, text) in wrapped.into_iter().enumerate() {
                result.push(Line {
                    text,
                    muted: false,
                    user,
                    first: index == 0,
                    last: index + 1 == count,
                    bubble_width,
                });
            }
        }
    }
    result
}

pub fn state_label(state: &str) -> &str {
    match state {
        "running" => "Working",
        "waiting" => "Waiting on desktop",
        "error" => "Error",
        _ => "Idle",
    }
}

fn text_end(frame: &mut Frame, end: i32, y: i32, text: &str, color: ui::Color) {
    frame.small(end - ui::small_width(text) as i32, y, text, color);
}

fn sidebar_icon(frame: &mut Frame, x: i32, y: i32) {
    frame.outline(x, y, 15, 12, 2, ui::MUTED, ui::BG);
    frame.rect(x + 5, y + 1, 1, 10, ui::MUTED);
}

fn header(
    frame: &mut Frame,
    thread: &Thread,
    activity: bool,
    connection: &str,
    error: &str,
    tabs: bool,
) {
    sidebar_icon(frame, 10, 12);
    let label = if error.is_empty() {
        state_label(&thread.status)
    } else {
        "Cached view"
    };
    let color = if error.is_empty() {
        ui::state_color(label)
    } else {
        ui::ERROR
    };
    let state_x = 468 - ui::small_width(label) as i32;
    frame.text(
        33,
        7,
        &ui::ellipsis(&thread.title, (state_x - 48).max(0) as usize),
        ui::TEXT,
    );
    frame.rounded(state_x - 10, 14, 5, 5, 2, color);
    frame.small(state_x, 7, label, color);
    if tabs {
        frame.small(
            10,
            35,
            "L Messages",
            if activity { ui::MUTED } else { ui::TEXT },
        );
        frame.small(
            95,
            35,
            "R Activity",
            if activity { ui::TEXT } else { ui::MUTED },
        );
        frame.rect(0, 56, 480, 1, ui::LINE);
        frame.rect(
            if activity { 95 } else { 10 },
            55,
            ui::small_width(if activity { "R Activity" } else { "L Messages" }) as i32,
            1,
            ui::TEXT,
        );
    } else {
        frame.rect(0, 56, 480, 1, ui::LINE);
    }
    frame.connection(470, 35, connection);
}

fn scrollbar(frame: &mut Frame, start: usize, total: usize, visible: usize, y: i32, height: usize) {
    if total <= visible || visible == 0 {
        return;
    }
    let thumb = (height * visible / total).max(8).min(height);
    let offset = (height - thumb) * start.min(total - visible) / (total - visible);
    frame.rect(474, y, 2, height as i32, ui::PANEL);
    frame.rect(474, y + offset as i32, 2, thumb as i32, ui::MUTED);
}

fn has_draft(thread: &Thread, drafts: &[(String, String)]) -> bool {
    drafts
        .iter()
        .any(|(id, text)| id == &thread.id && !text.is_empty())
}

fn row_height(thread: &Thread, drafts: &[(String, String)], drawer: bool) -> i32 {
    if has_draft(thread, drafts) || (drawer && thread.status == "waiting") {
        44
    } else {
        30
    }
}

fn group_start(threads: &[Thread], index: usize, first: usize) -> bool {
    index == first || threads[index].project != threads[index - 1].project
}

// Variable-height project groups must never push the highlighted thread offscreen.
fn thread_page(
    threads: &[Thread],
    selected: usize,
    drafts: &[(String, String)],
    drawer: bool,
) -> core::ops::Range<usize> {
    let selected = selected.min(threads.len().saturating_sub(1));
    let mut first = 0;
    loop {
        let mut y = 42;
        let mut end = first;
        while end < threads.len() {
            let height = row_height(&threads[end], drafts, drawer)
                + if group_start(threads, end, first) {
                    22
                } else {
                    0
                };
            if y + height > 222 {
                break;
            }
            y += height;
            end += 1;
        }
        if end > selected || end == threads.len() {
            return first..end;
        }
        first += 1;
    }
}

fn thread_rows(
    frame: &mut Frame,
    threads: &[Thread],
    selected: usize,
    drafts: &[(String, String)],
    drawer: bool,
) {
    let width = if drawer { 228 } else { 480 };
    let page = thread_page(threads, selected, drafts, drawer);
    let mut y = 42;
    for index in page.clone() {
        let thread = &threads[index];
        if group_start(threads, index, page.start) {
            frame.rect(12, y + 5, 6, 2, ui::MUTED);
            frame.outline(12, y + 7, 12, 8, 2, ui::MUTED, ui::CHROME);
            frame.small(
                31,
                y,
                &ui::ellipsis(
                    if thread.project.is_empty() {
                        "No project"
                    } else {
                        &thread.project
                    },
                    (width - 43) as usize,
                ),
                ui::MUTED,
            );
            y += 22;
        }
        let height = row_height(thread, drafts, drawer);
        if index == selected {
            frame.outline(
                6,
                y,
                width - 13,
                height - 2,
                5,
                ui::rgb(0x737373),
                ui::SELECTED,
            );
        }
        let label = state_label(&thread.status);
        let color = ui::state_color(label);
        if thread.status != "idle" {
            frame.rounded(14, y + 12, 5, 5, 2, color);
        }
        let status_width = if drawer {
            0
        } else {
            ui::small_width(label) as i32 + 14
        };
        let draft_indicator = drawer && thread.status == "waiting" && has_draft(thread, drafts);
        let indicator_width = if draft_indicator { 16 } else { 0 };
        frame.text(
            24,
            y + 3,
            &ui::ellipsis(
                &thread.title,
                (width - 38 - status_width - indicator_width) as usize,
            ),
            ui::TEXT,
        );
        if !drawer {
            text_end(frame, width - 16, y + 5, label, color);
        }
        if drawer && thread.status == "waiting" {
            frame.small(24, y + 22, "Waiting on desktop", ui::AMBER);
            if draft_indicator {
                frame.small(width - 24, y + 2, "•", ui::TEXT);
            }
        } else if has_draft(thread, drafts) {
            frame.small(24, y + 22, "Draft", ui::MUTED);
        }
        y += height;
    }
    if threads.is_empty() {
        for (row, text) in ui::wrap_text(
            "No threads yet. Open a thread on desktop.",
            (width - 24) as usize,
        )
        .iter()
        .enumerate()
        {
            frame.text(12, 70 + row as i32 * 18, text, ui::MUTED);
        }
    }
    if drawer {
        if page.len() < threads.len() {
            let height = (178 * page.len() / threads.len()).max(8);
            let offset = (178 - height) * page.start / (threads.len() - page.len());
            frame.rect(224, 42, 2, 178, ui::LINE);
            frame.rect(224, 42 + offset as i32, 2, height as i32, ui::MUTED);
        }
    } else {
        scrollbar(frame, page.start, threads.len(), page.len(), 42, 178);
    }
}

pub fn thread_list(
    frame: &mut Frame,
    threads: &[Thread],
    selected: usize,
    drafts: &[(String, String)],
    connection: &str,
    error: &str,
) {
    frame.clear();
    frame.small(12, 8, "T3 Code", ui::TEXT);
    frame.connection(468, 8, connection);
    frame.rect(0, 34, 480, 1, ui::LINE);
    thread_rows(frame, threads, selected, drafts, false);
    frame.compact_footer("↑↓ / Analog Select   ←→ Page   × Open   SELECT Wi-Fi");
    error_banner(frame, error, 225);
}

pub fn thread_drawer(
    frame: &mut Frame,
    threads: &[Thread],
    selected: usize,
    drafts: &[(String, String)],
    error: &str,
) {
    for y in 0..248 {
        for x in 228..ui::WIDTH {
            let pixel = &mut frame.pixels[y * ui::WIDTH + x];
            let mut dim = 0xff000000;
            for shift in [0, 8, 16] {
                dim |= (((*pixel >> shift) & 255) * 35 / 100) << shift;
            }
            *pixel = dim;
        }
    }
    frame.rect(0, 0, 228, 248, ui::CHROME);
    frame.rect(227, 0, 1, 248, ui::LINE);
    frame.small(12, 8, "T3 Code", ui::TEXT);
    sidebar_icon(frame, 201, 12);
    thread_rows(frame, threads, selected, drafts, true);
    frame.rect(10, 225, 208, 1, ui::LINE);
    frame.small(12, 229, "SELECT Connections", ui::MUTED);
    frame.compact_footer("↑↓ / Analog Select    ←→ Page    × Open    ○ Close");
    error_banner(frame, error, 225);
}

pub fn conversation(
    frame: &mut Frame,
    thread: &Thread,
    content: &[Line],
    view: &Viewport,
    activity: bool,
    latest: &str,
    connection: &str,
    error: &str,
    pending: bool,
    draft: &str,
) {
    frame.clear();
    header(frame, thread, activity, connection, error, true);
    let visible = if activity {
        ACTIVITY_ROWS
    } else {
        MESSAGE_ROWS
    };
    let shown = &content[view.offset.min(content.len())..];
    let shown = &shown[..shown.len().min(visible)];
    let mut index = 0;
    while index < shown.len() {
        let line = &shown[index];
        let y = CONTENT_TOP + index as i32 * 18;
        if line.user {
            let mut end = index + 1;
            while end < shown.len() && shown[end].user && !shown[end - 1].last {
                end += 1;
            }
            let x = 468 - line.bubble_width as i32;
            let height = ((end - index) as i32 * 18 + 8).min(178 - y);
            frame.rounded(x, y, line.bubble_width as i32, height, 8, ui::BUBBLE);
            if !line.first {
                frame.rect(x, y, line.bubble_width as i32, 8, ui::BUBBLE);
            }
            if !shown[end - 1].last {
                frame.rect(x, y + height - 8, line.bubble_width as i32, 8, ui::BUBBLE);
            }
            for (row, item) in shown[index..end].iter().enumerate() {
                frame.text(x + 10, y + row as i32 * 18 + 2, &item.text, ui::TEXT);
            }
            index = end;
        } else {
            if line.muted {
                frame.small(12, y, &line.text, ui::MUTED);
            } else {
                frame.text(
                    12,
                    y,
                    &line.text,
                    if activity { ui::MUTED } else { ui::TEXT },
                );
            }
            index += 1;
        }
    }
    if content.is_empty() {
        frame.text(
            12,
            77,
            if activity {
                "No activity yet."
            } else {
                "No messages yet."
            },
            ui::MUTED,
        );
    }
    let position = if !view.follow {
        if pending {
            String::from("△ New content")
        } else {
            format!(
                "History {} / {}",
                (view.offset + 1).min(content.len()),
                content.len()
            )
        }
    } else if !latest.is_empty() && !activity {
        format!("› {latest}")
    } else {
        String::from("▼ Latest")
    };
    let can_stop = matches!(thread.status.as_str(), "running" | "waiting");
    frame.small(
        12,
        179,
        &ui::ellipsis(&position, if can_stop { 318 } else { 450 }),
        ui::MUTED,
    );
    if can_stop {
        text_end(frame, 468, 179, "START Stop…", ui::MUTED);
    }
    scrollbar(frame, view.offset, content.len(), visible, CONTENT_TOP, 108);
    frame.outline(9, 200, 462, 43, 10, ui::LINE, ui::PANEL);
    frame.text(
        20,
        204,
        &ui::ellipsis(
            if draft.is_empty() {
                "Ask anything..."
            } else {
                draft
            },
            DRAFT_WIDTH,
        ),
        if draft.is_empty() {
            ui::MUTED
        } else {
            ui::TEXT
        },
    );
    frame.small(20, 223, "□ Voice", ui::MUTED);
    text_end(
        frame,
        458,
        223,
        if draft.is_empty() {
            "× Write"
        } else {
            "× Draft"
        },
        ui::TEXT,
    );
    frame.compact_footer("↑↓ / Analog Scroll    ←→ Page    △ Latest    ○ Threads");
    error_banner(frame, error, 178);
}

fn error_banner(frame: &mut Frame, error: &str, y: i32) {
    if !error.is_empty() {
        frame.rounded(8, y, 464, 20, 4, ui::rgb(0x2d1616));
        frame.small(12, y + 1, &ui::ellipsis(error, 448), ui::ERROR);
    }
}

pub fn composer(
    frame: &mut Frame,
    thread: &Thread,
    draft: &str,
    scroll: usize,
    edit: bool,
    key: usize,
    uppercase: bool,
    connection: &str,
) {
    frame.clear();
    header(frame, thread, false, connection, "", false);
    if !edit {
        frame.small(12, 67, "Draft", ui::TEXT);
        text_end(frame, 468, 67, "Not sent", ui::MUTED);
    }
    let top = if edit { 62 } else { 91 };
    let text_top = if edit { 65 } else { 96 };
    frame.outline(
        9,
        top,
        462,
        if edit { 45 } else { 151 },
        10,
        ui::rgb(0x737373),
        ui::PANEL,
    );
    let wrapped = ui::wrap_text(draft, DRAFT_WIDTH);
    let rows = if edit { 2 } else { DRAFT_ROWS };
    for (row, text) in wrapped.iter().skip(scroll).take(rows).enumerate() {
        frame.text(21, text_top + row as i32 * 18, text, ui::TEXT);
    }
    if draft.is_empty() {
        frame.text(
            21,
            text_top,
            if edit {
                "Type a prompt"
            } else {
                "□ Record a prompt"
            },
            ui::MUTED,
        );
    }
    if edit {
        frame.rounded(9, 112, 462, 117, 8, ui::PANEL);
        // Keep every key above the two-line physical-button legend.
        for (index, ch) in KEYS.chars().enumerate() {
            let x = 13 + (index % 7) as i32 * 65;
            let y = 114 + (index / 7) as i32 * 16;
            if index == key {
                frame.outline(x, y, 61, 16, 3, ui::rgb(0x737373), ui::SELECTED);
            }
            let label = if ch == ' ' {
                String::from("space")
            } else {
                format!(
                    "{}",
                    if uppercase {
                        ch.to_ascii_uppercase()
                    } else {
                        ch
                    }
                )
            };
            frame.small(
                x + 4,
                y - 1,
                &label,
                if index == key { ui::TEXT } else { ui::MUTED },
            );
        }
        frame.footer(
            "↑↓←→ Select   × Type   □ Delete   △ Aa",
            "Analog / L/R Scroll text   ○ Review draft",
        );
    } else {
        frame.rect(20, 208, 440, 1, ui::LINE);
        frame.small(21, 219, "□ Voice   × Edit", ui::MUTED);
        let enabled = !draft.trim().is_empty();
        frame.rounded(
            360,
            213,
            99,
            24,
            7,
            if enabled { ui::TEXT } else { ui::SELECTED },
        );
        frame.small(
            367,
            216,
            "START Send",
            if enabled { ui::BG } else { ui::MUTED },
        );
        scrollbar(frame, scroll, wrapped.len(), DRAFT_ROWS, 96, 108);
        frame.compact_footer("↑↓ / Analog Scroll    ←→ Page    ○ Back (draft kept)");
    }
}

pub fn duration(seconds: u64) -> String {
    if seconds < 3600 {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    } else {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    }
}

pub fn stop_confirmation(frame: &mut Frame, thread: &Thread) {
    notice(
        frame,
        "Stop agent?",
        &format!("Stop the current response in \"{}\"?", thread.title),
        "× Stop agent    ○ Cancel",
        "",
        "T3 PSP",
    );
}

pub fn recording(
    frame: &mut Frame,
    thread: &Thread,
    captured: u64,
    uploaded: u64,
    stopped: bool,
    cancelled: bool,
    connection: &str,
) {
    frame.clear();
    header(frame, thread, false, connection, "", false);
    frame.outline(9, 67, 462, 176, 10, ui::LINE, ui::PANEL);
    frame.rounded(
        23,
        83,
        6,
        6,
        3,
        if stopped || cancelled {
            ui::MUTED
        } else {
            ui::ERROR
        },
    );
    frame.text(
        39,
        77,
        if cancelled {
            "Cancelling recording…"
        } else if stopped {
            "Finishing upload…"
        } else {
            "Recording"
        },
        ui::TEXT,
    );
    text_end(frame, 454, 79, &duration(captured / 11025), ui::TEXT);
    frame.small(
        23,
        115,
        &format!("Saved on desktop: {}", duration(uploaded / 11025)),
        ui::MUTED,
    );
    let pending = captured.saturating_sub(uploaded) * 10 / 11025;
    frame.small(
        23,
        138,
        &format!("Pending upload: {}.{} s", pending / 10, pending % 10),
        ui::MUTED,
    );
    frame.text(23, 181, "Review the transcript before sending.", ui::MUTED);
    frame.compact_footer(if stopped || cancelled {
        "○ Cancel"
    } else {
        "□ Stop and transcribe    ○ Cancel"
    });
}

pub fn notice(
    frame: &mut Frame,
    title: &str,
    message: &str,
    first: &str,
    second: &str,
    connection: &str,
) {
    frame.clear();
    frame.header(title, "", connection);
    frame.outline(8, 62, 464, 165, 10, ui::LINE, ui::PANEL);
    for (index, line) in ui::wrap_text(message, 436).iter().take(8).enumerate() {
        frame.text(20, 70 + index as i32 * 18, line, ui::TEXT);
    }
    frame.footer(first, second);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn user_bubbles_preserve_long_unicode_prompts_and_message_boundaries() {
        let prompt = "Příliš žluťoučký kůň ".repeat(40);
        let detail = Detail {
            messages: alloc::vec![
                (String::from("user"), prompt.clone()),
                (String::from("assistant"), String::from("Odpověď")),
            ],
            activities: Vec::new(),
        };
        let content = lines(&detail, false);
        let user: Vec<_> = content.iter().filter(|line| line.user).collect();
        assert!(user.len() > MESSAGE_ROWS);
        assert!(user.first().unwrap().first);
        assert!(user.last().unwrap().last);
        assert_eq!(user.iter().filter(|line| line.first).count(), 1);
        assert_eq!(user.iter().filter(|line| line.last).count(), 1);
        assert!(
            user.iter()
                .all(|line| ui::text_width(&line.text) <= USER_WIDTH)
        );
        assert_eq!(
            user.iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .concat()
                .replace(' ', ""),
            prompt.replace(' ', ""),
        );
        assert!(!content.last().unwrap().user);
        assert_eq!(content.last().unwrap().text, "Odpověď");
    }

    #[test]
    fn every_selected_thread_fits_with_project_headers_and_draft_labels() {
        let threads: Vec<_> = (0..32)
            .map(|index| Thread {
                id: format!("thread-{index}"),
                title: format!("Vlákno {index}"),
                project: format!("Projekt {}", index / 2),
                status: String::from(if index % 3 == 0 { "waiting" } else { "running" }),
            })
            .collect();
        let drafts = alloc::vec![(threads[5].id.clone(), String::from("Draft"))];
        for drawer in [false, true] {
            for selected in 0..threads.len() {
                let page = thread_page(&threads, selected, &drafts, drawer);
                assert!(page.contains(&selected));
                let height: i32 = page
                    .clone()
                    .map(|index| {
                        row_height(&threads[index], &drafts, drawer)
                            + if group_start(&threads, index, page.start) {
                                22
                            } else {
                                0
                            }
                    })
                    .sum();
                assert!(42 + height <= 222);
            }
        }
        assert!(thread_page(&[], 0, &[], true).is_empty());
    }

    #[test]
    fn recording_clock_keeps_counting_past_minutes_and_hours() {
        assert_eq!(duration(5), "00:05");
        assert_eq!(duration(65), "01:05");
        assert_eq!(duration(3605), "1:00:05");
        assert_eq!(duration(90_000), "25:00:00");
    }
    #[test]
    fn message_and_activity_views_have_separate_content() {
        let detail = crate::model::parse_detail("MSG\tuser\tAhoj\\nČeština\nACT\tČtu soubor\n");
        assert_eq!(
            lines(&detail, false)
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>(),
            ["Ahoj", "Čeština"]
        );
        assert_eq!(lines(&detail, true)[0].text, "› Čtu soubor");
    }
    #[test]
    fn rolling_window_does_not_move_history_until_follow_is_restored() {
        let old = crate::model::parse_detail("MSG\tuser\tOld first\nMSG\tassistant\tOld last\n");
        let new = crate::model::parse_detail("MSG\tassistant\tOld last\nMSG\tuser\tNew tail\n");
        let mut current = lines(&old, false);
        let snapshot = current.clone();
        let mut view = Viewport {
            offset: 1,
            follow: false,
        };
        assert!(refresh_content(
            &mut current,
            lines(&new, false),
            &mut view,
            2
        ));
        assert_eq!(current, snapshot);
        assert_eq!(view.offset, 1);
        view.follow = true;
        assert!(!refresh_content(
            &mut current,
            lines(&new, false),
            &mut view,
            2
        ));
        assert_eq!(current.last().unwrap().text, "New tail");
        assert_eq!(view.offset, current.len() - 2);
    }
}
