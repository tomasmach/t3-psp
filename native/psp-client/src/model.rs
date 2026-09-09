extern crate alloc;
use alloc::{string::String, vec::Vec};

pub const MAX_DRAFT_BYTES: usize = 60_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub id: String,
    pub status: String,
    pub title: String,
    pub project: String,
}

/// The picker overlays the current thread; dismissing it retains its read position.
#[derive(Default)]
pub struct Navigation {
    pub selected: usize,
    pub opened: Option<Thread>,
    pub sidebar_open: bool,
}

impl Navigation {
    pub fn showing_threads(&self) -> bool {
        self.sidebar_open || self.opened.is_none()
    }

    pub fn back(&mut self) {
        if self.opened.is_some() {
            self.sidebar_open = !self.sidebar_open;
        }
    }

    pub fn select(&mut self, delta: isize, total: usize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(total.saturating_sub(1));
    }

    /// Preserve the highlighted thread when the server reorders the recent list.
    pub fn replace_threads(&mut self, threads: &mut Vec<Thread>, next: Vec<Thread>) {
        let selected_id = threads.get(self.selected).map(|thread| &thread.id);
        self.selected = selected_id
            .and_then(|id| next.iter().position(|thread| &thread.id == id))
            .unwrap_or(0);
        *threads = next;
    }

    /// Returns true only when the caller must discard the previous thread's content.
    pub fn open_selected(&mut self, threads: &[Thread]) -> bool {
        let Some(thread) = threads.get(self.selected) else {
            return false;
        };
        let changed = self.opened.as_ref().map(|opened| &opened.id) != Some(&thread.id);
        self.opened = Some(thread.clone());
        self.sidebar_open = false;
        changed
    }
}

pub fn decode_field(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(c) => out.push(c),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn append_transcript(draft: &mut String, text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err(String::from(
            "Gateway returned an empty transcript. Draft retained.",
        ));
    }
    let separator = usize::from(!draft.is_empty());
    if draft.len() + separator + text.len() > MAX_DRAFT_BYTES {
        return Err(String::from(
            "The transcript exceeds the 60 kB draft limit. Nothing was truncated or sent.",
        ));
    }
    if separator != 0 {
        draft.push(' ');
    }
    draft.push_str(text);
    Ok(())
}

pub fn parse_threads(body: &str) -> Vec<Thread> {
    body.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\t');
            if fields.next()? != "THREAD" {
                return None;
            }
            let id = fields.next()?;
            if id.is_empty()
                || !id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
            {
                return None;
            }
            Some(Thread {
                id: String::from(id),
                status: decode_field(fields.next()?),
                title: decode_field(fields.next()?),
                project: decode_field(fields.next().unwrap_or("")),
            })
        })
        .take(32)
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detail {
    pub messages: Vec<(String, String)>,
    pub activities: Vec<String>,
}

pub fn parse_detail(body: &str) -> Detail {
    let mut detail = Detail::default();
    for line in body.lines() {
        let mut fields = line.splitn(3, '\t');
        match fields.next() {
            Some("MSG") => detail.messages.push((
                decode_field(fields.next().unwrap_or("")),
                decode_field(fields.next().unwrap_or("")),
            )),
            Some("ACT") => detail
                .activities
                .push(decode_field(fields.next().unwrap_or(""))),
            _ => {}
        }
    }
    detail
}

#[derive(Clone, Debug, PartialEq)]
pub struct Viewport {
    pub offset: usize,
    pub follow: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: 0,
            follow: true,
        }
    }
}

impl Viewport {
    pub fn update(&mut self, total: usize, visible: usize) {
        let end = total.saturating_sub(visible);
        self.offset = if self.follow {
            end
        } else {
            self.offset.min(end)
        };
    }

    pub fn move_by(&mut self, delta: isize, total: usize, visible: usize) {
        self.follow = false;
        self.offset = self
            .offset
            .saturating_add_signed(delta)
            .min(total.saturating_sub(visible));
    }
}

#[derive(Default)]
pub struct ThreadViews {
    pub activity: bool,
    pub messages: Viewport,
    pub activities: Viewport,
}

impl ThreadViews {
    pub fn current(&self) -> &Viewport {
        if self.activity {
            &self.activities
        } else {
            &self.messages
        }
    }

    pub fn current_mut(&mut self) -> &mut Viewport {
        if self.activity {
            &mut self.activities
        } else {
            &mut self.messages
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drawer_dismissal_and_reselection_preserve_the_open_thread() {
        let threads =
            parse_threads("THREAD\ta\trunning\tFirst\tProject\nTHREAD\tb\tidle\tSecond\tProject\n");
        let mut navigation = Navigation::default();
        assert!(navigation.showing_threads());
        navigation.back();
        assert!(navigation.showing_threads());
        assert!(!navigation.open_selected(&[]));
        assert!(navigation.open_selected(&threads));
        assert!(!navigation.showing_threads());
        navigation.back();
        navigation.select(1, threads.len());
        navigation.back();
        assert_eq!(navigation.opened.as_ref().unwrap().id, "a");
        assert!(!navigation.showing_threads());
        navigation.back();
        navigation.select(-1, threads.len());
        assert!(!navigation.open_selected(&threads));
        assert!(!navigation.showing_threads());
        navigation.back();
        navigation.select(1, threads.len());
        assert!(navigation.open_selected(&threads));
        assert_eq!(navigation.opened.as_ref().unwrap().id, "b");
    }

    #[test]
    fn refreshing_picker_keeps_selection_by_id_and_handles_disappearing_threads() {
        let mut threads = parse_threads("THREAD\ta\tidle\tFirst\nTHREAD\tb\tidle\tSecond\n");
        let mut navigation = Navigation::default();
        navigation.select(1, threads.len());
        assert!(navigation.open_selected(&threads));
        navigation.back();
        navigation.replace_threads(
            &mut threads,
            parse_threads("THREAD\tb\trunning\tSecond\nTHREAD\ta\tidle\tFirst\n"),
        );
        assert_eq!(navigation.selected, 0);
        assert!(!navigation.open_selected(&threads));
        assert_eq!(navigation.opened.as_ref().unwrap().status, "running");
        navigation.back();
        navigation.replace_threads(&mut threads, Vec::new());
        navigation.select(1, threads.len());
        assert_eq!(navigation.selected, 0);
        assert!(!navigation.open_selected(&threads));
        navigation.back();
        assert_eq!(navigation.opened.as_ref().unwrap().id, "b");
        assert!(!navigation.showing_threads());
    }
    #[test]
    fn fields_and_utf8() {
        let threads = parse_threads("THREAD\tabc-123\trunning\tČeský\\tthread\\nhello\\\\end\n");
        assert_eq!(threads[0].title, "Český\tthread\nhello\\end");
        assert!(parse_threads("THREAD\t../bad\tidle\tbad").is_empty());
    }
    #[test]
    fn transcript_limit_preserves_draft_and_utf8() {
        let mut draft = String::from("a").repeat(MAX_DRAFT_BYTES - 3);
        append_transcript(&mut draft, "č").unwrap();
        assert_eq!(draft.len(), MAX_DRAFT_BYTES);
        let before = draft.clone();
        assert!(append_transcript(&mut draft, "x").is_err());
        assert_eq!(draft, before);
        assert!(append_transcript(&mut draft, " ").is_err());
        assert_eq!(draft, before);
        assert_eq!(decode_field("a\\rb"), "a\rb");
    }

    #[test]
    fn views_keep_independent_scroll_and_follow_state() {
        let mut views = ThreadViews::default();
        views.messages.update(50, 7);
        views.messages.move_by(-10, 50, 7);
        assert_eq!(views.messages.offset, 33);
        views.activity = !views.activity;
        views.current_mut().update(20, 8);
        assert_eq!(views.current().offset, 12);
        views.current_mut().move_by(-3, 20, 8);
        views.activity = !views.activity;
        views.current_mut().update(55, 7);
        assert_eq!(views.current().offset, 33);
        assert!(!views.current().follow);
        views.current_mut().follow = true;
        views.current_mut().update(55, 7);
        assert_eq!(views.current().offset, 48);
        views.activity = !views.activity;
        assert_eq!(views.current().offset, 9);
        assert!(!views.current().follow);
    }

    #[test]
    fn separates_activity_from_messages_and_accepts_project_names() {
        let detail =
            parse_detail("MSG\tuser\tPříliš\\nžluťoučký\nACT\tČtu\\tfile\nMSG\tassistant\tOK\n");
        assert_eq!(
            detail.messages,
            [
                (String::from("user"), String::from("Příliš\nžluťoučký")),
                (String::from("assistant"), String::from("OK"))
            ]
        );
        assert_eq!(detail.activities, ["Čtu\tfile"]);
        assert_eq!(
            parse_threads("THREAD\tx\tidle\tTitle\tČeský projekt\n")[0].project,
            "Český projekt"
        );
        assert_eq!(parse_threads("THREAD\tx\tidle\tTitle\n")[0].project, "");
    }
}
