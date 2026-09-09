use crate::{
    model::{Detail, Thread, Viewport},
    ui::{self, Frame},
};
use alloc::{format, string::String, vec::Vec};

pub const MESSAGE_ROWS: usize = 7;
pub const ACTIVITY_ROWS: usize = 8;
pub const DRAFT_ROWS: usize = 8;
pub const KEYS: &str = "abcdefghijklmnopqrstuvwxyz0123456789 .,?!-/";

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub muted: bool,
}

// The gateway sends a rolling window. Freeze the visible snapshot while the user
// reads history, rather than applying offsets to a different set of messages.
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
            for text in ui::wrap_text(&format!("› {item}"), 452) {
                result.push(Line { text, muted: false });
            }
            result.push(Line {
                text: String::new(),
                muted: true,
            });
        }
        if !result.is_empty() {
            result.pop();
        }
    } else {
        for (role, body) in &detail.messages {
            let label = match role.as_str() {
                "user" => "Ty",
                "assistant" => "Agent",
                "system" => "Systém",
                _ => "Zpráva",
            };
            result.push(Line {
                text: String::from(label),
                muted: true,
            });
            for text in ui::wrap_text(body, 452) {
                result.push(Line { text, muted: false });
            }
        }
    }
    result
}

pub fn state_label(state: &str) -> &str {
    match state {
        "running" => "● Běží",
        "waiting" => "◆ Čeká",
        "error" => "! Chyba",
        _ => "○ Nečinný",
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
    frame.header(
        "Thready",
        &format!(
            "{} / {}",
            usize::from(!threads.is_empty()) + selected,
            threads.len()
        ),
        connection,
    );
    let first = selected / 4 * 4;
    for (row, thread) in threads.iter().skip(first).take(4).enumerate() {
        let y = 56 + row as i32 * 43;
        if first + row == selected {
            frame.rect(0, y, 480, 43, ui::PANEL);
            frame.rect(0, y, 3, 43, ui::ACCENT);
        }
        frame.text(11, y + 3, &ui::ellipsis(&thread.title, 350), ui::TEXT);
        let draft = drafts
            .iter()
            .any(|(id, text)| id == &thread.id && !text.is_empty());
        let project = if draft {
            format!("{}  • Koncept", thread.project)
        } else {
            thread.project.clone()
        };
        frame.small(11, y + 23, &ui::ellipsis(&project, 345), ui::MUTED);
        let color = match thread.status.as_str() {
            "waiting" => 0xff85cff4,
            "error" => ui::ERROR,
            "idle" => ui::MUTED,
            _ => ui::ACCENT,
        };
        frame.small(377, y + 13, state_label(&thread.status), color);
    }
    if threads.is_empty() {
        frame.text(12, 83, "Žádné otevřené thready.", ui::MUTED);
    }
    frame.scrollbar(first, threads.len(), 4);
    frame.footer("↑↓ Vybrat    × Otevřít", "SELECT Připojení    HOME Ukončit");
    error_banner(frame, error);
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
) {
    frame.clear();
    frame.header(
        &thread.title,
        if error.is_empty() {
            state_label(&thread.status)
        } else {
            "Neaktuální"
        },
        connection,
    );
    frame.small(
        11,
        59,
        if activity { "Aktivita" } else { "Zprávy" },
        ui::ACCENT,
    );
    let position = if !error.is_empty() {
        String::from("Uložený náhled")
    } else if view.follow {
        String::from("▼ Nejnovější")
    } else if pending {
        String::from("R: nový obsah")
    } else {
        format!(
            "Historie  {} / {}",
            (view.offset + 1).min(content.len()),
            content.len()
        )
    };
    frame.small(290, 59, &position, ui::MUTED);
    let visible = if activity {
        ACTIVITY_ROWS
    } else {
        MESSAGE_ROWS
    };
    for (row, line) in content.iter().skip(view.offset).take(visible).enumerate() {
        let y = 78 + row as i32 * 18;
        if line.muted {
            frame.small(11, y, &line.text, ui::MUTED);
        } else {
            frame.text(
                11,
                y,
                &line.text,
                if activity { ui::ACCENT } else { ui::TEXT },
            );
        }
    }
    if content.is_empty() {
        frame.text(
            12,
            87,
            if activity {
                "Zatím žádná aktivita."
            } else {
                "Zatím žádné zprávy."
            },
            ui::MUTED,
        );
    }
    if !activity && !latest.is_empty() {
        frame.rect(8, 211, 460, 19, ui::PANEL);
        frame.small(
            12,
            212,
            &ui::ellipsis(&format!("› {latest}"), 450),
            ui::ACCENT,
        );
    }
    frame.scrollbar(view.offset, content.len(), visible);
    frame.footer(
        if activity {
            "L Zprávy    □ Hlas    × Prompt    ○ Zpět"
        } else {
            "L Aktivita    □ Hlas    × Prompt    ○ Zpět"
        },
        "↑↓ Posun    R Nejnovější    △ + START Přerušit",
    );
    error_banner(frame, error);
}

fn error_banner(frame: &mut Frame, error: &str) {
    if !error.is_empty() {
        frame.rect(0, 211, 470, 21, ui::PANEL);
        frame.small(10, 212, &ui::ellipsis(error, 445), ui::ERROR);
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
    frame.header(&thread.title, "Koncept", connection);
    frame.small(
        11,
        59,
        if edit {
            "Úprava textu"
        } else {
            "Přepis ke kontrole"
        },
        ui::ACCENT,
    );
    frame.small(310, 59, "Zatím neodesláno", ui::MUTED);
    let wrapped = ui::wrap_text(draft, 452);
    let rows = if edit { 2 } else { DRAFT_ROWS };
    for (row, text) in wrapped.iter().skip(scroll).take(rows).enumerate() {
        frame.text(11, 78 + row as i32 * 18, text, ui::TEXT);
    }
    if draft.is_empty() {
        frame.text(11, 80, "□ Namluv prompt", ui::MUTED);
    }
    if edit {
        for (index, ch) in KEYS.chars().enumerate() {
            let x = 10 + (index % 7) as i32 * 65;
            let y = 114 + (index / 7) as i32 * 16;
            if index == key {
                frame.rect(x, y, 62, 16, ui::PANEL);
            }
            let label = if ch == ' ' {
                String::from("mezera")
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
                if index == key { ui::ACCENT } else { ui::MUTED },
            );
        }
        frame.footer(
            "↑↓←→ Vybrat    × Napsat    △ Smazat",
            "L/R Text    SELECT Aa    ○ Zpět na přepis",
        );
    } else {
        frame.scrollbar(scroll, wrapped.len(), DRAFT_ROWS);
        frame.footer(
            "START Odeslat    × Upravit    □ Přidat hlas",
            "↑↓ Posun    ○ Zpět (koncept zůstane)",
        );
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
    frame.header(&thread.title, "Mikrofon", connection);
    frame.text(
        12,
        77,
        if cancelled {
            "Ruším nahrávku…"
        } else if stopped {
            "Odesílám zbytek nahrávky…"
        } else {
            "Nahrávám. Mluv teď."
        },
        ui::TEXT,
    );
    frame.text(12, 108, &duration(captured / 11025), ui::ACCENT);
    frame.small(
        12,
        140,
        &format!("Uloženo na počítači: {}", duration(uploaded / 11025)),
        ui::MUTED,
    );
    let pending = captured.saturating_sub(uploaded) * 10 / 11025;
    frame.small(
        12,
        162,
        &format!("Čeká na odeslání: {}.{} s", pending / 10, pending % 10),
        ui::MUTED,
    );
    frame.small(
        12,
        194,
        "Přepis se neodešle bez tvého potvrzení.",
        ui::MUTED,
    );
    frame.footer(
        if stopped || cancelled {
            "○ Zrušit"
        } else {
            "□ Ukončit a přepsat    ○ Zrušit"
        },
        if cancelled {
            "Čekám na bezpečné dokončení přenosu."
        } else if stopped {
            "Potom následuje přepis na počítači."
        } else {
            "Nahrávání skončí až dalším stiskem □."
        },
    );
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
    for (index, line) in ui::wrap_text(message, 452).iter().take(9).enumerate() {
        frame.text(12, 62 + index as i32 * 18, line, ui::TEXT);
    }
    frame.footer(first, second);
}

#[cfg(test)]
mod tests {
    use super::*;
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
            ["Ty", "Ahoj", "Čeština"]
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
