#![no_std]
#![feature(asm_experimental_arch)]
#![no_main]
extern crate alloc;
mod audio_stream;
mod diagnostics;
mod model;
mod network;
mod stream_queue;
mod ui;
mod views;
use alloc::{format, string::String, vec::Vec};
use model::{Thread, ThreadViews, decode_field, parse_detail, parse_threads};
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
fn pressed(previous: &mut CtrlButtons) -> CtrlButtons {
    let mut pad = SceCtrlData::default();
    unsafe {
        sceCtrlReadBufferPositive(&mut pad, 1);
    }
    let result = pad.buttons & !*previous;
    *previous = pad.buttons;
    result
}
fn connection(online: bool) -> String {
    let battery = unsafe { scePowerGetBatteryLifePercent() };
    let status = if online {
        "Připojeno"
    } else {
        "Bez spojení s bránou"
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
        "Čekej prosím…",
        "HOME Ukončit",
        &connection(online),
    );
    frame.present();
}
fn dialog(frame: &mut Frame, title: &str, message: &str, previous: &mut CtrlButtons) {
    let lines = ui::wrap_text(message, 452);
    let mut offset = 0usize;
    pressed(previous);
    loop {
        frame.clear();
        frame.header(title, "", "T3 PSP");
        for (row, line) in lines.iter().skip(offset).take(9).enumerate() {
            frame.text(12, 62 + row as i32 * 18, line, ui::TEXT);
        }
        frame.scrollbar(offset, lines.len(), 9);
        frame.footer("× Zpět    ↑↓ Posun", "HOME Ukončit");
        frame.present();
        loop {
            let buttons = pressed(previous);
            if buttons.contains(CtrlButtons::CROSS) {
                return;
            }
            if buttons.contains(CtrlButtons::UP) {
                offset = offset.saturating_sub(4);
                break;
            }
            if buttons.contains(CtrlButtons::DOWN) {
                offset = (offset + 4).min(lines.len().saturating_sub(9));
                break;
            }
            pause();
        }
    }
}
fn peek_pressed(previous: &mut CtrlButtons) -> CtrlButtons {
    let mut pad = SceCtrlData::default();
    unsafe {
        sceCtrlPeekBufferPositive(&mut pad, 1);
    }
    let result = pad.buttons & !*previous;
    *previous = pad.buttons;
    result
}

fn cancel_recording(config: &network::Config, id: &str) {
    // Best effort after the uploader has exited. Abandoned sessions expire on the desktop.
    let _ = network::request(config, &format!("/v1/recordings/{id}/cancel"), &[], true);
}

fn recording(
    frame: &mut Frame,
    config: &network::Config,
    thread: &Thread,
    previous: &mut CtrlButtons,
    online: &mut bool,
) -> Result<Option<String>, String> {
    if network::disconnected()? {
        *online = false;
        notice(frame, "Wi-Fi", "Obnovuji připojení…", false);
        network::connect(config)?;
    }
    notice(frame, &thread.title, "Připravuji nahrávání…", *online);
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
        .ok_or_else(|| String::from("Brána nevrátila platné ID nahrávky."))?;
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
    peek_pressed(previous);
    loop {
        if let Err(error) = session.tick() {
            if failure.is_none() {
                failure = Some(error);
            }
            session.cancel();
        }
        let buttons = peek_pressed(previous);
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
                    "Nahrávání selhalo",
                    error,
                    "HOME Ukončit",
                    "Čekám na uvolnění mikrofonu a spojení.",
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
        notice(frame, &thread.title, "Ruším nahrávku na počítači…", *online);
        cancel_recording(config, id);
        return match failure {
            Some(error) => Err(error),
            None => Ok(None),
        };
    }
    notice(frame, &thread.title, "Přepisuji na počítači…", *online);
    let finish = network::request(config, &format!("/v1/recordings/{id}/finish"), &[], true);
    *online = finish.is_ok();
    if let Err(error) = finish {
        cancel_recording(config, id);
        return Err(error);
    }
    let started = now();
    let mut poll_at = 0;
    let mut draw_at = 0;
    peek_pressed(previous);
    loop {
        if peek_pressed(previous).contains(CtrlButtons::CIRCLE) {
            notice(frame, &thread.title, "Ruším přepis…", *online);
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
                        .ok_or_else(|| String::from("Brána nevrátila přepis."));
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
                    "Přepisuji na počítači…\n{}",
                    views::duration(((now() - started) / 1_000_000) as u64)
                ),
                "○ Zrušit přepis",
                "Prompt zatím nebyl odeslaný.",
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
    previous: &mut CtrlButtons,
    mut voice: bool,
    mut online: bool,
) {
    let mut key = 0usize;
    let mut scroll = 0usize;
    let mut edit = false;
    let mut uppercase = false;
    let mut dirty = true;
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
                    "Nahrávka se nepodařila",
                    &format!("{error}\nKoncept zůstal zachovaný."),
                    previous,
                );
            }
            scroll = ui::wrap_text(draft, 452)
                .len()
                .saturating_sub(views::DRAFT_ROWS);
            dirty = true;
            // Sending after recording/network waits needs a fresh START.
            pressed(previous);
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
        let buttons = pressed(previous);
        if buttons.contains(CtrlButtons::CIRCLE) {
            if !edit {
                return;
            }
            edit = false;
            scroll = scroll.min(
                ui::wrap_text(draft, 452)
                    .len()
                    .saturating_sub(views::DRAFT_ROWS),
            );
            dirty = true;
        } else if edit {
            let count = views::KEYS.len();
            if buttons.contains(CtrlButtons::LEFT) {
                key = (key + count - 1) % count;
                dirty = true;
            }
            if buttons.contains(CtrlButtons::RIGHT) {
                key = (key + 1) % count;
                dirty = true;
            }
            if buttons.contains(CtrlButtons::UP) {
                key = (key + count - 7) % count;
                dirty = true;
            }
            if buttons.contains(CtrlButtons::DOWN) {
                key = (key + 7) % count;
                dirty = true;
            }
            if buttons.contains(CtrlButtons::SELECT) {
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
                scroll = ui::wrap_text(draft, 452).len().saturating_sub(2);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::TRIANGLE) {
                draft.pop();
                scroll = ui::wrap_text(draft, 452).len().saturating_sub(2);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::LTRIGGER) {
                scroll = scroll.saturating_sub(2);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::RTRIGGER) {
                scroll = (scroll + 2).min(ui::wrap_text(draft, 452).len().saturating_sub(2));
                dirty = true;
            }
        } else if buttons.contains(CtrlButtons::SQUARE) {
            voice = true;
        } else if buttons.contains(CtrlButtons::CROSS) {
            edit = true;
            scroll = ui::wrap_text(draft, 452).len().saturating_sub(2);
            dirty = true;
        } else if buttons.contains(CtrlButtons::START) && !draft.trim().is_empty() {
            notice(frame, &thread.title, "Odesílám prompt…", online);
            match network::request(
                config,
                &format!("/v1/threads/{}/prompt", thread.id),
                draft.as_bytes(),
                true,
            ) {
                Ok(_) => {
                    draft.clear();
                    return;
                }
                Err(error) => {
                    online = false;
                    dialog(
                        frame,
                        "Odeslání není potvrzené",
                        &format!(
                            "{error}\nPřed opakováním zkontroluj thread. Prompt mohl dorazit. Koncept zůstal zachovaný."
                        ),
                        previous,
                    );
                    dirty = true;
                }
            }
        } else {
            if buttons.contains(CtrlButtons::UP) {
                scroll = scroll.saturating_sub(4);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::DOWN) {
                scroll = (scroll + 4).min(
                    ui::wrap_text(draft, 452)
                        .len()
                        .saturating_sub(views::DRAFT_ROWS),
                );
                dirty = true;
            }
        }
        pause();
    }
}
fn select_profile(frame: &mut Frame, config: &mut network::Config) -> Result<(), String> {
    let profiles = network::profiles();
    if profiles.is_empty() {
        return Err(String::from(
            "V PSP nejsou uložené Wi-Fi sítě. Nejdřív vytvoř připojení v nastavení PSP.",
        ));
    }
    let mut selected = profiles
        .iter()
        .position(|p| p.id == config.profile)
        .unwrap_or(0);
    let mut previous = CtrlButtons::empty();
    pressed(&mut previous);
    let mut dirty = true;
    loop {
        if dirty {
            frame.clear();
            frame.header(
                "Vyber Wi-Fi",
                &format!("{} / {}", selected + 1, profiles.len()),
                "Nepřipojeno",
            );
            for (row, profile) in profiles.iter().skip(selected / 4 * 4).take(4).enumerate() {
                let y = 56 + row as i32 * 43;
                if selected % 4 == row {
                    frame.rect(0, y, 480, 43, ui::PANEL);
                    frame.rect(0, y, 3, 43, ui::ACCENT);
                }
                frame.text(11, y + 3, &ui::ellipsis(&profile.name, 450), ui::TEXT);
                frame.small(11, y + 23, &ui::ellipsis(&profile.ssid, 450), ui::MUTED);
            }
            frame.scrollbar(selected / 4 * 4, profiles.len(), 4);
            frame.footer("↑↓ Vybrat    × Připojit", "HOME Ukončit");
            frame.present();
            dirty = false;
        }
        let buttons = pressed(&mut previous);
        if buttons.contains(CtrlButtons::UP) {
            selected = selected.saturating_sub(1);
            dirty = true;
        }
        if buttons.contains(CtrlButtons::DOWN) {
            selected = (selected + 1).min(profiles.len() - 1);
            dirty = true;
        }
        if buttons.contains(CtrlButtons::CROSS) {
            config.profile = profiles[selected].id;
            notice(
                frame,
                "Wi-Fi",
                &format!("Připojuji k {}…", profiles[selected].ssid),
                false,
            );
            return Ok(());
        }
        pause();
    }
}
fn run(
    frame: &mut Frame,
    config: &mut network::Config,
    drafts: &mut Vec<(String, String)>,
) -> Result<(), String> {
    notice(frame, "Wi-Fi", "Načítám síť…", false);
    network::init(config)?;
    select_profile(frame, config)?;
    network::connect(config)?;
    let mut threads: Vec<Thread> = Vec::new();
    let mut selected = 0usize;
    let mut opened: Option<Thread> = None;
    let mut detail = model::Detail::default();
    let mut messages = Vec::new();
    let mut activities = Vec::new();
    let mut viewport = ThreadViews::default();
    let mut pending_messages = false;
    let mut pending_activities = false;
    let mut error = String::new();
    let mut previous = CtrlButtons::empty();
    pressed(&mut previous);
    let mut refresh_at = 0;
    let mut reconnect_at = 0;
    let mut dirty = true;
    loop {
        if now() >= refresh_at {
            let path = opened
                .as_ref()
                .map(|t| format!("/v1/threads/{}?projectNames=1", t.id))
                .unwrap_or_else(|| String::from("/v1/threads?projectNames=1"));
            // Only reconnect outside recording/upload. Never replay a prompt POST.
            let response = match network::disconnected() {
                Ok(true) if now() >= reconnect_at => {
                    notice(frame, "Wi-Fi", "Spojení vypadlo. Připojuji znovu…", false);
                    let result = network::connect(config);
                    reconnect_at = now() + 30_000_000;
                    dirty = true;
                    result.and_then(|()| network::request(config, &path, &[], false))
                }
                Ok(true) => Err(String::from("Wi-Fi je odpojená. SELECT: připojit znovu.")),
                Ok(false) => network::request(config, &path, &[], false),
                Err(error) => Err(error),
            };
            match response {
                Ok(response) => {
                    dirty |= !error.is_empty();
                    error.clear();
                    if let Some(thread) = &mut opened {
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
                    } else {
                        let old_id = threads.get(selected).map(|thread| thread.id.clone());
                        let next = parse_threads(&response);
                        dirty |= next != threads;
                        threads = next;
                        selected = old_id
                            .and_then(|id| threads.iter().position(|t| t.id == id))
                            .unwrap_or(0);
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
            if let Some(thread) = &opened {
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
                );
            } else {
                views::thread_list(
                    frame,
                    &threads,
                    selected,
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
            match select_profile(frame, config).and_then(|()| network::connect(config)) {
                Ok(()) => {
                    error.clear();
                    refresh_at = 0;
                }
                Err(message) => {
                    dialog(frame, "Připojení se nepodařilo", &message, &mut previous);
                    error = message;
                }
            }
            pressed(&mut previous);
            dirty = true;
            continue;
        }
        if let Some(thread) = opened.clone() {
            if buttons.contains(CtrlButtons::CIRCLE) {
                opened = None;
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
                && previous.contains(CtrlButtons::TRIANGLE)
            {
                notice(frame, &thread.title, "Žádám o přerušení…", error.is_empty());
                match network::request(
                    config,
                    &format!("/v1/threads/{}/interrupt", thread.id),
                    &[],
                    true,
                ) {
                    Ok(_) => dialog(
                        frame,
                        "Přerušení",
                        "Požadavek byl přijat. Stav běhu se aktualizuje po potvrzení agentem.",
                        &mut previous,
                    ),
                    Err(message) => {
                        dialog(frame, "Přerušení se nepodařilo", &message, &mut previous)
                    }
                }
                refresh_at = 0;
                dirty = true;
            } else if buttons.contains(CtrlButtons::LTRIGGER) {
                viewport.toggle();
                dirty = true;
            } else {
                let (total, visible) = if viewport.activity {
                    (activities.len(), views::ACTIVITY_ROWS)
                } else {
                    (messages.len(), views::MESSAGE_ROWS)
                };
                if buttons.contains(CtrlButtons::UP) {
                    viewport.current_mut().move_by(-3, total, visible);
                    dirty = true;
                }
                if buttons.contains(CtrlButtons::DOWN) {
                    viewport.current_mut().move_by(3, total, visible);
                    dirty = true;
                }
                if buttons.contains(CtrlButtons::RTRIGGER) {
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
        } else {
            if buttons.contains(CtrlButtons::UP) {
                selected = selected.saturating_sub(1);
                dirty = true;
            }
            if buttons.contains(CtrlButtons::DOWN) {
                selected = (selected + 1).min(threads.len().saturating_sub(1));
                dirty = true;
            }
            if buttons.contains(CtrlButtons::CROSS) {
                opened = threads.get(selected).cloned();
                detail = model::Detail::default();
                messages.clear();
                activities.clear();
                viewport = ThreadViews::default();
                pending_messages = false;
                pending_activities = false;
                refresh_at = 0;
                dirty = true;
            }
        }
        pause();
    }
}
fn psp_main() {
    psp::enable_home_button();
    unsafe {
        sceCtrlSetSamplingCycle(0);
        sceCtrlSetSamplingMode(CtrlMode::Digital);
    }
    let mut frame = Frame::new();
    notice(&mut frame, "T3 PSP", "Spouštím…", false);
    let mut drafts = Vec::new();
    loop {
        if let Err(error) =
            network::load_config().and_then(|mut config| run(&mut frame, &mut config, &mut drafts))
        {
            let mut previous = CtrlButtons::empty();
            dialog(
                &mut frame,
                "T3 PSP: chyba",
                &format!("{error}\n\n× Zkusit znovu"),
                &mut previous,
            );
        }
    }
}
