// Host-side renderer: these are the same pixels, fonts and layouts used by the EBOOT.
extern crate alloc;
#[path = "../src/model.rs"]
mod model;
#[path = "../src/ui.rs"]
mod ui;
#[path = "../src/views.rs"]
mod views;
use std::{fs::File, io::Write};

fn save(frame: &ui::Frame, name: &str) {
    let path = format!("/tmp/t3-psp-gui-{name}.ppm");
    let mut output = File::create(&path).unwrap();
    write!(output, "P6\n480 272\n255\n").unwrap();
    let rgb: Vec<u8> = frame
        .pixels
        .iter()
        .flat_map(|c| [*c as u8, (*c >> 8) as u8, (*c >> 16) as u8])
        .collect();
    output.write_all(&rgb).unwrap();
    println!("{path}");
}

fn main() {
    let threads = model::parse_threads(
        "THREAD\tpsp\trunning\tHlasové ovládání PSP\tT3 Code\nTHREAD\tapple\twaiting\tPřihlášení přes Apple\tMobilní aplikace\nTHREAD\treviews\tidle\tPřehled recenzí\tWebová aplikace\nTHREAD\tfilter\tidle\tFiltr otevřených podniků\tMobilní aplikace\n",
    );
    let detail = model::parse_detail(
        "MSG\tuser\tZkontroluj odesílání hlasových promptů.\nMSG\tassistant\tNahrávka už dorazila na počítač. Teď ověřím přepis a odeslání do správného threadu.\nACT\tAudio přijato z PSP\nACT\tSpouštím test odeslání promptu\nACT\tČtu soubor apps/psp-gateway/src/gateway.ts\n",
    );
    let draft =
        "Zvětši písmo v seznamu threadů. Stav agenta nech vpravo a přidej možnost přerušit běh.";
    let connection = "Připojeno  •  76 %";
    let mut frame = ui::Frame::new();
    views::thread_list(&mut frame, &threads, 0, &[], connection, "");
    save(&frame, "threads");
    let messages = views::lines(&detail, false);
    let activities = views::lines(&detail, true);
    let mut state = model::ThreadViews::default();
    state.messages.update(messages.len(), views::MESSAGE_ROWS);
    views::conversation(
        &mut frame,
        &threads[0],
        &messages,
        state.current(),
        false,
        &detail.activities[1],
        connection,
        "",
        false,
    );
    save(&frame, "messages");
    let saved = state.messages.clone();
    state.toggle();
    state
        .current_mut()
        .update(activities.len(), views::ACTIVITY_ROWS);
    views::conversation(
        &mut frame,
        &threads[0],
        &activities,
        state.current(),
        true,
        "",
        connection,
        "",
        false,
    );
    save(&frame, "activity");
    state.toggle();
    assert_eq!(*state.current(), saved);
    views::composer(
        &mut frame,
        &threads[0],
        draft,
        0,
        false,
        0,
        false,
        connection,
    );
    save(&frame, "draft");
    views::composer(
        &mut frame,
        &threads[0],
        draft,
        0,
        true,
        12,
        false,
        connection,
    );
    save(&frame, "keyboard");
    views::recording(
        &mut frame,
        &threads[0],
        65 * 11025,
        63 * 11025,
        false,
        false,
        connection,
    );
    save(&frame, "recording");
    views::notice(
        &mut frame,
        "Hlasové ovládání PSP",
        "Přepisuji na počítači…",
        "Čekej prosím…",
        "HOME Ukončit",
        connection,
    );
    save(&frame, "transcribing");
    views::conversation(
        &mut frame,
        &threads[0],
        &messages,
        state.current(),
        false,
        "",
        "Bez spojení s bránou  •  76 %",
        "Brána neodpovídá. SELECT: připojení.",
        false,
    );
    save(&frame, "offline");
    // Important controls must fit without clipping even with Czech diacritics.
    for text in [
        "L Aktivita    □ Hlas    × Prompt    ○ Zpět",
        "↑↓ Posun    R Nejnovější    △ + START Přerušit",
        "START Odeslat    × Upravit    □ Přidat hlas",
        "Nahrávání skončí až dalším stiskem □.",
    ] {
        assert!(ui::small_width(text) <= 460, "Footer too wide: {text}");
    }
}
