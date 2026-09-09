#![no_std]
#![feature(asm_experimental_arch)]
#![no_main]
extern crate alloc;
mod audio_stream;
mod diagnostics;
mod input;
mod model;
mod network;
mod stream_queue;
mod ui;
mod views;
use alloc::{format, string::String, vec::Vec};
use model::{Navigation, Thread, ThreadViews, decode_field, parse_detail, parse_threads};
use psp::sys::*;
use ui::Frame;
psp::module!("T3 PSP", 0, 2);
fn now() -> i64 {
    unsafe { sceKernelGetSystemTimeWide() }
}
fn pause() {
    unsafe {
        sceKernelDelayThread(16_000);
    }
}
struct Press {
    buttons: CtrlButtons,
    scroll: isize,
}

impl Press {
    fn contains(&self, button: CtrlButtons) -> bool {
        self.buttons.contains(button)
    }

    fn movement(&self, page: usize) -> isize {
        if self.contains(CtrlButtons::UP) {
            -1
        } else if self.contains(CtrlButtons::DOWN) {
            1
        } else if self.contains(CtrlButtons::LEFT) {
            -(page as isize)
        } else if self.contains(CtrlButtons::RIGHT) {
            page as isize
        } else {
            self.scroll
        }
    }
}

fn pressed(previous: &mut input::Input) -> Press {
    pressed_with_repeat(previous, CtrlButtons::empty())
}

fn pressed_with_repeat(previous: &mut input::Input, extra_repeat: CtrlButtons) -> Press {
    let mut pad = SceCtrlData::default();
    if unsafe { sceCtrlPeekBufferPositive(&mut pad, 1) } <= 0 {
        return Press {
            buttons: CtrlButtons::empty(),
            scroll: 0,
        };
    }
    let directions = CtrlButtons::UP | CtrlButtons::DOWN | CtrlButtons::LEFT | CtrlButtons::RIGHT;
    let (buttons, scroll) = previous.sample(
        pad.buttons.bits(),
        (directions | extra_repeat).bits(),
        pad.ly,
        now(),
    );
    Press {
        buttons: CtrlButtons::from_bits_truncate(buttons),
        scroll,
    }
}

fn suppress_input(previous: &mut input::Input) {
    let mut pad = SceCtrlData::default();
    unsafe {
        sceCtrlPeekBufferPositive(&mut pad, 1);
    }
    previous.suppress(pad.buttons.bits());
}
fn connection(online: bool) -> String {
    let battery = unsafe { scePowerGetBatteryLifePercent() };
    let status = if online {
        "Connected"
    } else {
        "Gateway offline"
    };
    if (0..=100).contains(&battery) {
        format!("{status}  •  {battery} %")
    } else {
        String::from(status)
    }
}
fn notice(frame: &mut Frame, title: &str, message: &str, online: bool) {
    views::notice(
        frame,
        title,
        message,
        "Please wait…",
        "HOME Exit",
        &connection(online),
    );
    frame.present();
}
fn dialog(frame: &mut Frame, title: &str, message: &str, previous: &mut input::Input) {
    let lines = ui::wrap_text(message, 452);
    let mut offset = 0usize;
    suppress_input(previous);
    loop {
        frame.clear();
        frame.header(title, "", "T3 PSP");
        for (row, line) in lines.iter().skip(offset).take(9).enumerate() {
            frame.text(12, 62 + row as i32 * 18, line, ui::TEXT);
        }
        frame.scrollbar(offset, lines.len(), 9);
        frame.footer("○ Back    ↑↓ / Analog Scroll", "←→ Page    × OK");
        frame.present();
        loop {
            let buttons = pressed(previous);
            if buttons.contains(CtrlButtons::CIRCLE) || buttons.contains(CtrlButtons::CROSS) {
                suppress_input(previous);
                return;
            }
            let movement = buttons.movement(9);
            if movement != 0 {
                offset = offset
                    .saturating_add_signed(movement)
                    .min(lines.len().saturating_sub(9));
                break;
            }
            pause();
        }
    }
}
fn confirm_stop(frame: &mut Frame, thread: &Thread, previous: &mut input::Input) -> bool {
    views::stop_confirmation(frame, thread);
    frame.present();
    suppress_input(previous);
    loop {
        let buttons = pressed(previous);
        if buttons.contains(CtrlButtons::CIRCLE) {
            suppress_input(previous);
            return false;
        }
        if buttons.contains(CtrlButtons::CROSS) {
            suppress_input(previous);
            return true;
        }
        pause();
    }
}

fn cancel_recording(config: &network::Config, id: &str) {
    // Best effort after the uploader has exited. Abandoned sessions expire on the desktop.
    let _ = network::request(config, &format!("/v1/recordings/{id}/cancel"), &[], true);
}

fn recording(
    frame: &mut Frame,
    config: &network::Config,
    thread: &Thread,
    previous: &mut input::Input,
    online: &mut bool,
) -> Result<Option<String>, String> {
    if network::disconnected()? {
        *online = false;
        notice(frame, "Wi-Fi", "Reconnecting…", false);
        network::connect(config)?;
    }
    notice(frame, &thread.title, "Preparing to record…", *online);
    let started = network::request(config, "/v1/recordings", &[], true);
    *online = started.is_ok();
    let response = started?;
    let id = response
        .trim()
        .strip_prefix("RECORDING\t")
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 100
                && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or_else(|| String::from("The gateway returned an invalid recording ID."))?;
    let mut session = match audio_stream::Session::start(config, id) {
        Ok(session) => session,
        Err(error) => {
            cancel_recording(config, id);
            return Err(error);
        }
    };
    let mut cancelled = false;
    let mut failure = None;
    let mut last_draw = 0;
    suppress_input(previous);
    loop {
        if let Err(error) = session.tick() {
            if failure.is_none() {
                failure = Some(error);
            }
            session.cancel();
        }
        let buttons = pressed(previous);
        if buttons.contains(CtrlButtons::CIRCLE) {
            cancelled = true;
            session.cancel();
        } else if buttons.contains(CtrlButtons::SQUARE) {
            session.stop();
        }
        if session.finished() {
            break;
        }
        if now() - last_draw >= 200_000 {
            let progress = session.progress();
            if let Some(error) = &failure {
                views::notice(
                    frame,
                    "Recording failed",
                    error,
                    "HOME Exit",
                    "Waiting for the microphone and connection to close.",
                    &connection(*online),
                );
            } else {
                views::recording(
                    frame,
                    thread,
                    progress.captured_samples,
                    progress.uploaded_samples,
                    progress.stopping,
                    cancelled || failure.is_some(),
                    &connection(*online),
                );
            }
            frame.present();
            last_draw = now();
        }
        // Never wait for HTTP here. Input polling and capture stay responsive while uploading.
        unsafe {
            sceKernelDelayThread(1_000);
        }
    }
    drop(session);
    if cancelled || failure.is_some() {
        notice(
            frame,
            &thread.title,
            "Cancelling the recording on desktop…",
            *online,
        );
        cancel_recording(config, id);
        return match failure {
            Some(error) => Err(error),
            None => Ok(None),
        };
    }
    notice(frame, &thread.title, "Transcribing on desktop…", *online);
    let finish = network::request(config, &format!("/v1/recordings/{id}/finish"), &[], true);
    *online = finish.is_ok();
    if let Err(error) = finish {
        cancel_recording(config, id);
        return Err(error);
    }
    let started = now();
    let mut poll_at = 0;
    let mut draw_at = 0;
    suppress_input(previous);
    loop {
        if pressed(previous).contains(CtrlButtons::CIRCLE) {
            notice(frame, &thread.title, "Cancelling transcription…", *online);
            cancel_recording(config, id);
            return Ok(None);
        }
        if now() >= poll_at {
            let response = network::request(config, &format!("/v1/recordings/{id}"), &[], false);
            *online = response.is_ok();
            match response {
                Ok(text) if text.trim() == "PENDING" => {}
                Ok(text) => {
                    let result = text
                        .lines()
                        .find_map(|line| line.strip_prefix("TEXT\t"))
                        .map(decode_field)
                        .ok_or_else(|| String::from("The gateway returned no transcript."));
                    cancel_recording(config, id);
                    return result.map(Some);
                }
                Err(error) => {
                    cancel_recording(config, id);
                    return Err(error);
                }
            }
            poll_at = now() + 500_000;
        }
        if now() >= draw_at {
            views::notice(
                frame,
                &thread.title,
                &format!(
                    "Transcribing on desktop…\n{}",
                    views::duration(((now() - started) / 1_000_000) as u64)
                ),
                "○ Cancel transcription",
                "The prompt has not been sent.",
                &connection(*online),
            );
            frame.present();
            draw_at = now() + 1_000_000;
        }
        pause();
    }
}

fn compose(
    frame: &mut Frame,
    config: &network::Config,
    thread: &Thread,
    draft: &mut String,
    previous: &mut input::Input,
    mut voice: bool,
    mut online: bool,
) {
    let mut key = 0usize;
    let mut scroll = 0usize;
    let mut edit = !voice && draft.is_empty();
    let mut uppercase = false;
    let mut dirty = true;
    suppress_input(previous);
    loop {
        if voice {
            voice = false;
            let result =
                recording(frame, config, thread, previous, &mut online).and_then(|transcript| {
                    match transcript {
                        Some(text) => model::append_transcript(draft, &text),
                        None => Ok(()),
                    }
                });
            if let Err(error) = result {
                dialog(
                    frame,
                    "Recording failed",
                    &format!("{error}\nYour draft was kept."),
                    previous,
                );
            }
            scroll = ui::wrap_text(draft, views::DRAFT_WIDTH)
                .len()
                .saturating_sub(views::DRAFT_ROWS);
            dirty = true;
            // Sending after recording/network waits needs a fresh START.
            suppress_input(previous);
        }
        if dirty {
            views::composer(
                frame,
                thread,
                draft,
                scroll,
                edit,
                key,
                uppercase,
                &connection(online),
            );
            frame.present();
            dirty = false;
        }
        let buttons = pressed_with_repeat(
            previous,
            if edit {
                CtrlButtons::SQUARE
            } else {
                CtrlButtons::empty()
            },
        );
        if buttons.contains(CtrlButtons::CIRCLE) {
            if !edit {
                suppress_input(previous);
                return;
            }
            edit = false;
            scroll = scroll.min(
                ui::wrap_text(draft, views::DRAFT_WIDTH)
                    .len()
                    .saturating_sub(views::DRAFT_ROWS),
            );
            dirty = true;
            suppress_input(previous);
        } else if edit {
            let count = views::KEYS.len();
            if buttons.contains(CtrlButtons::LEFT) {
                key = input::move_key(key, count, -1, 0);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::RIGHT) {
                key = input::move_key(key, count, 1, 0);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::UP) {
                key = input::move_key(key, count, 0, -1);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::DOWN) {
                key = input::move_key(key, count, 0, 1);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::TRIANGLE) {
                uppercase = !uppercase;
                dirty = true;
            }
            if buttons.contains(CtrlButtons::CROSS) && draft.len() < model::MAX_DRAFT_BYTES {
                let ch = views::KEYS.as_bytes()[key] as char;
                draft.push(if uppercase {
                    ch.to_ascii_uppercase()
                } else {
                    ch
                });
                scroll = ui::wrap_text(draft, views::DRAFT_WIDTH)
                    .len()
                    .saturating_sub(2);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::SQUARE) {
                draft.pop();
                scroll = ui::wrap_text(draft, views::DRAFT_WIDTH)
                    .len()
                    .saturating_sub(2);
                dirty = true;
            }
            let movement = if buttons.contains(CtrlButtons::LTRIGGER) {
                -2
            } else if buttons.contains(CtrlButtons::RTRIGGER) {
                2
            } else {
                buttons.scroll
            };
            if movement != 0 {
                scroll = scroll.saturating_add_signed(movement).min(
                    ui::wrap_text(draft, views::DRAFT_WIDTH)
                        .len()
                        .saturating_sub(2),
                );
                dirty = true;
            }
        } else if buttons.contains(CtrlButtons::SQUARE) {
            voice = true;
        } else if buttons.contains(CtrlButtons::CROSS) {
            edit = true;
            scroll = ui::wrap_text(draft, views::DRAFT_WIDTH)
                .len()
                .saturating_sub(2);
            dirty = true;
            suppress_input(previous);
        } else if buttons.contains(CtrlButtons::START) && !draft.trim().is_empty() {
            notice(frame, &thread.title, "Sending prompt…", online);
            match network::request(
                config,
                &format!("/v1/threads/{}/prompt", thread.id),
                draft.as_bytes(),
                true,
            ) {
                Ok(_) => {
                    draft.clear();
                    suppress_input(previous);
                    return;
                }
                Err(error) => {
                    online = false;
                    dialog(
                        frame,
                        "Delivery not confirmed",
                        &format!(
                            "{error}\nCheck the thread before trying again. The prompt may have arrived. Your draft was kept."
                        ),
                        previous,
                    );
                    dirty = true;
                }
            }
        } else {
            let movement = buttons.movement(views::DRAFT_ROWS);
            if movement != 0 {
                scroll = scroll.saturating_add_signed(movement).min(
                    ui::wrap_text(draft, views::DRAFT_WIDTH)
                        .len()
                        .saturating_sub(views::DRAFT_ROWS),
                );
                dirty = true;
            }
        }
        pause();
    }
}
fn select_profile(
    frame: &mut Frame,
    config: &mut network::Config,
    can_cancel: bool,
) -> Result<bool, String> {
    let profiles = network::profiles();
    if profiles.is_empty() {
        return Err(String::from(
            "No saved Wi-Fi profiles. Create a connection in PSP Network Settings first.",
        ));
    }
    let mut selected = profiles
        .iter()
        .position(|p| p.id == config.profile)
        .unwrap_or(0);
    let mut previous = input::Input::default();
    suppress_input(&mut previous);
    let mut dirty = true;
    loop {
        if dirty {
            frame.clear();
            frame.header(
                "Select Wi-Fi",
                &format!("{} / {}", selected + 1, profiles.len()),
                "T3 PSP",
            );
            for (row, profile) in profiles.iter().skip(selected / 4 * 4).take(4).enumerate() {
                let y = 56 + row as i32 * 43;
                if selected % 4 == row {
                    frame.outline(8, y + 1, 464, 41, 6, ui::rgb(0x737373), ui::SELECTED);
                }
                frame.text(18, y + 3, &ui::ellipsis(&profile.name, 438), ui::TEXT);
                frame.small(18, y + 23, &ui::ellipsis(&profile.ssid, 438), ui::MUTED);
            }
            frame.scrollbar(selected / 4 * 4, profiles.len(), 4);
            frame.footer(
                "↑↓ / Analog Select    × Connect",
                if can_cancel {
                    "←→ Page    ○ Back"
                } else {
                    "←→ Page    HOME Exit"
                },
            );
            frame.present();
            dirty = false;
        }
        let buttons = pressed(&mut previous);
        if can_cancel && buttons.contains(CtrlButtons::CIRCLE) {
            return Ok(false);
        }
        let movement = buttons.movement(4);
        if movement != 0 {
            selected = selected
                .saturating_add_signed(movement)
                .min(profiles.len() - 1);
            dirty = true;
        }
        if buttons.contains(CtrlButtons::CROSS) {
            config.profile = profiles[selected].id;
            notice(
                frame,
                "Wi-Fi",
                &format!("Connecting to {}…", profiles[selected].ssid),
                false,
            );
            return Ok(true);
        }
        pause();
    }
}
fn run(
    frame: &mut Frame,
    config: &mut network::Config,
    drafts: &mut Vec<(String, String)>,
) -> Result<(), String> {
    notice(frame, "Wi-Fi", "Initializing network…", false);
    network::init(config)?;
    select_profile(frame, config, false)?;
    network::connect(config)?;
    let mut threads: Vec<Thread> = Vec::new();
    let mut navigation = Navigation::default();
    let mut detail = model::Detail::default();
    let mut messages = Vec::new();
    let mut activities = Vec::new();
    let mut viewport = ThreadViews::default();
    let mut pending_messages = false;
    let mut pending_activities = false;
    let mut error = String::new();
    let mut previous = input::Input::default();
    suppress_input(&mut previous);
    let mut refresh_at = 0;
    let mut reconnect_at = 0;
    let mut dirty = true;
    loop {
        if now() >= refresh_at {
            let path = if navigation.showing_threads() {
                String::from("/v1/threads?projectNames=1")
            } else {
                format!(
                    "/v1/threads/{}?projectNames=1",
                    navigation.opened.as_ref().unwrap().id
                )
            };
            // Only reconnect outside recording/upload. Never replay a prompt POST.
            let response = match network::disconnected() {
                Ok(true) if now() >= reconnect_at => {
                    notice(frame, "Wi-Fi", "Connection lost. Reconnecting…", false);
                    let result = network::connect(config);
                    reconnect_at = now() + 30_000_000;
                    dirty = true;
                    result.and_then(|()| network::request(config, &path, &[], false))
                }
                Ok(true) => Err(String::from("Wi-Fi disconnected. SELECT: reconnect.")),
                Ok(false) => network::request(config, &path, &[], false),
                Err(error) => Err(error),
            };
            match response {
                Ok(response) => {
                    dirty |= !error.is_empty();
                    error.clear();
                    if navigation.showing_threads() {
                        let next = parse_threads(&response);
                        dirty |= next != threads;
                        navigation.replace_threads(&mut threads, next);
                    } else if let Some(thread) = &mut navigation.opened {
                        if let Some(updated) = parse_threads(&response).into_iter().next() {
                            dirty |= *thread != updated;
                            *thread = updated;
                        }
                        let next = parse_detail(&response);
                        if next != detail {
                            detail = next;
                            pending_messages = views::refresh_content(
                                &mut messages,
                                views::lines(&detail, false),
                                &mut viewport.messages,
                                views::MESSAGE_ROWS,
                            );
                            pending_activities = views::refresh_content(
                                &mut activities,
                                views::lines(&detail, true),
                                &mut viewport.activities,
                                views::ACTIVITY_ROWS,
                            );
                            dirty = true;
                        }
                    }
                }
                Err(message) => {
                    dirty |= error != message;
                    error = message;
                }
            }
            refresh_at = now() + 2_000_000;
        }
        if dirty {
            if let Some(thread) = &navigation.opened {
                views::conversation(
                    frame,
                    thread,
                    if viewport.activity {
                        &activities
                    } else {
                        &messages
                    },
                    viewport.current(),
                    viewport.activity,
                    detail.activities.last().map(String::as_str).unwrap_or(""),
                    &connection(error.is_empty()),
                    &error,
                    if viewport.activity {
                        pending_activities
                    } else {
                        pending_messages
                    },
                    drafts
                        .iter()
                        .find(|(id, _)| id == &thread.id)
                        .map(|(_, draft)| draft.as_str())
                        .unwrap_or(""),
                );
                if navigation.sidebar_open {
                    views::thread_drawer(frame, &threads, navigation.selected, drafts, &error);
                }
            } else {
                views::thread_list(
                    frame,
                    &threads,
                    navigation.selected,
                    drafts,
                    &connection(error.is_empty()),
                    &error,
                );
            }
            frame.present();
            dirty = false;
        }
        let buttons = pressed(&mut previous);
        if buttons.contains(CtrlButtons::SELECT) {
            match select_profile(frame, config, true).and_then(|connect| {
                if connect {
                    network::connect(config)?;
                }
                Ok(connect)
            }) {
                Ok(true) => {
                    error.clear();
                    refresh_at = 0;
                }
                Ok(false) => {}
                Err(message) => {
                    dialog(frame, "Connection failed", &message, &mut previous);
                    error = message;
                }
            }
            suppress_input(&mut previous);
            dirty = true;
            continue;
        }
        if navigation.showing_threads() {
            if buttons.contains(CtrlButtons::CIRCLE) {
                navigation.back();
                suppress_input(&mut previous);
                refresh_at = 0;
                dirty = true;
            } else {
                let movement = buttons.movement(4);
                if movement != 0 {
                    navigation.select(movement, threads.len());
                    dirty = true;
                }
                if buttons.contains(CtrlButtons::CROSS) {
                    if navigation.open_selected(&threads) {
                        detail = model::Detail::default();
                        messages.clear();
                        activities.clear();
                        viewport = ThreadViews::default();
                        pending_messages = false;
                        pending_activities = false;
                    }
                    refresh_at = 0;
                    dirty = true;
                    suppress_input(&mut previous);
                }
            }
        } else if let Some(thread) = navigation.opened.clone() {
            if buttons.contains(CtrlButtons::CIRCLE) {
                navigation.back();
                suppress_input(&mut previous);
                refresh_at = 0;
                dirty = true;
            } else if buttons.contains(CtrlButtons::CROSS) || buttons.contains(CtrlButtons::SQUARE)
            {
                let index = match drafts.iter().position(|(id, _)| id == &thread.id) {
                    Some(index) => index,
                    None => {
                        drafts.push((thread.id.clone(), String::new()));
                        drafts.len() - 1
                    }
                };
                compose(
                    frame,
                    config,
                    &thread,
                    &mut drafts[index].1,
                    &mut previous,
                    buttons.contains(CtrlButtons::SQUARE),
                    error.is_empty(),
                );
                refresh_at = 0;
                dirty = true;
                continue;
            } else if buttons.contains(CtrlButtons::START)
                && matches!(thread.status.as_str(), "running" | "waiting")
            {
                if !confirm_stop(frame, &thread, &mut previous) {
                    dirty = true;
                    continue;
                }
                notice(frame, &thread.title, "Requesting stop…", error.is_empty());
                match network::request(
                    config,
                    &format!("/v1/threads/{}/interrupt", thread.id),
                    &[],
                    true,
                ) {
                    Ok(_) => dialog(
                        frame,
                        "Stop requested",
                        "Stop requested. The status will update when the agent confirms it.",
                        &mut previous,
                    ),
                    Err(message) => dialog(frame, "Stop request failed", &message, &mut previous),
                }
                refresh_at = 0;
                dirty = true;
            } else if buttons.contains(CtrlButtons::LTRIGGER) {
                viewport.activity = false;
                dirty = true;
            } else if buttons.contains(CtrlButtons::RTRIGGER) {
                viewport.activity = true;
                dirty = true;
            } else {
                let (total, visible) = if viewport.activity {
                    (activities.len(), views::ACTIVITY_ROWS)
                } else {
                    (messages.len(), views::MESSAGE_ROWS)
                };
                let movement = buttons.movement(visible);
                if movement != 0 {
                    viewport.current_mut().move_by(movement, total, visible);
                    dirty = true;
                }
                if buttons.contains(CtrlButtons::TRIANGLE) {
                    viewport.current_mut().follow = true;
                    if viewport.activity {
                        pending_activities = views::refresh_content(
                            &mut activities,
                            views::lines(&detail, true),
                            &mut viewport.activities,
                            visible,
                        );
                    } else {
                        pending_messages = views::refresh_content(
                            &mut messages,
                            views::lines(&detail, false),
                            &mut viewport.messages,
                            visible,
                        );
                    }
                    dirty = true;
                }
            }
        }
        pause();
    }
}
fn psp_main() {
    psp::enable_home_button();
    unsafe {
        sceCtrlSetSamplingCycle(0);
        sceCtrlSetSamplingMode(CtrlMode::Analog);
    }
    let mut frame = Frame::new();
    notice(&mut frame, "T3 PSP", "Starting…", false);
    let mut drafts = Vec::new();
    loop {
        if let Err(error) =
            network::load_config().and_then(|mut config| run(&mut frame, &mut config, &mut drafts))
        {
            let mut previous = input::Input::default();
            dialog(
                &mut frame,
                "T3 PSP: error",
                &format!("{error}\n\n× Try again"),
                &mut previous,
            );
        }
    }
}
