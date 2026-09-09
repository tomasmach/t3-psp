// Host-side renderer: these are the same pixels, fonts and layouts used by the EBOOT.
extern crate alloc;
#[path = "../src/input.rs"]
mod input;
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
        "THREAD\tpsp\trunning\tPSP voice prompts\tT3 Code\nTHREAD\tapple\twaiting\tSign in with Apple\tMobile app\nTHREAD\treviews\tidle\tReview dashboard\tWeb app\nTHREAD\tfilter\tidle\tFilter open venues\tMobile app\n",
    );
    let detail = model::parse_detail(
        "MSG\tuser\tCheck voice prompt submission.\nMSG\tassistant\tThe recording reached the desktop. I will check the transcript and send it to the correct thread.\nACT\tAudio received from PSP\nACT\tTesting prompt submission\nACT\tReading apps/psp-gateway/src/gateway.ts\n",
    );
    let draft = "Increase the text size in the thread list. Keep the agent status on the right and add a stop action.";
    let connection = "Connected  •  76 %";
    let mut frame = ui::Frame::new();
    views::thread_list(&mut frame, &threads, 0, &[], connection, "");
    save(&frame, "threads");
    views::thread_list(
        &mut frame,
        &threads,
        0,
        &[],
        "Gateway offline  •  76 %",
        "Gateway not responding. SELECT: connections.",
    );
    save(&frame, "threads-offline");
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
        "",
    );
    save(&frame, "messages");
    views::thread_drawer(&mut frame, &threads, 0, &[], "");
    save(&frame, "drawer");
    let saved = state.messages.clone();
    state.activity = !state.activity;
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
        "",
    );
    save(&frame, "activity");
    state.activity = !state.activity;
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
    views::stop_confirmation(&mut frame, &threads[0]);
    save(&frame, "stop-confirmation");
    views::notice(
        &mut frame,
        "PSP voice prompts",
        "Transcribing on desktop…",
        "Please wait…",
        "HOME Exit",
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
        "Gateway offline  •  76 %",
        "Gateway not responding. SELECT: connections.",
        false,
        draft,
    );
    save(&frame, "offline");
    views::thread_list(&mut frame, &[], 0, &[], connection, "");
    save(&frame, "empty");
    views::conversation(
        &mut frame,
        &threads[1],
        &messages,
        &model::Viewport::default(),
        false,
        "",
        connection,
        "",
        false,
        "",
    );
    save(&frame, "waiting");
    let long_detail = model::Detail {
        messages: vec![
            (
                String::from("user"),
                "Check the entire long prompt. Keep every word and preserve its order. ".repeat(15),
            ),
            (
                String::from("assistant"),
                "Response to the long prompt.".into(),
            ),
        ],
        activities: Vec::new(),
    };
    let long_lines = views::lines(&long_detail, false);
    let history = model::Viewport {
        offset: 3,
        follow: false,
    };
    views::conversation(
        &mut frame,
        &threads[0],
        &long_lines,
        &history,
        false,
        "",
        connection,
        "",
        true,
        draft,
    );
    save(&frame, "history");
    let before_drawer = frame.pixels.clone();
    let mut navigation = model::Navigation::default();
    assert!(navigation.open_selected(&threads));
    navigation.back();
    views::thread_drawer(&mut frame, &threads, navigation.selected, &[], "");
    navigation.back();
    views::conversation(
        &mut frame,
        navigation.opened.as_ref().unwrap(),
        &long_lines,
        &history,
        false,
        "",
        connection,
        "",
        true,
        draft,
    );
    assert_eq!(
        frame.pixels, before_drawer,
        "Closing the drawer changed the visible history"
    );
    save(&frame, "history-return");
    let many_threads: Vec<_> = (0..32)
        .map(|index| model::Thread {
            id: format!("thread-{index}"),
            title: format!("{index}: A very long thread title that must fit in the sidebar"),
            project: format!("Project {}", index / 2),
            status: String::from(if index % 3 == 0 { "waiting" } else { "running" }),
        })
        .collect();
    views::thread_drawer(
        &mut frame,
        &many_threads,
        31,
        &[(many_threads[31].id.clone(), draft.into())],
        "",
    );
    save(&frame, "drawer-long");
    views::conversation(
        &mut frame,
        &threads[0],
        &long_lines,
        &history,
        false,
        "",
        connection,
        "",
        true,
        draft,
    );
    views::thread_drawer(
        &mut frame,
        &many_threads,
        30,
        &[(many_threads[30].id.clone(), draft.into())],
        "",
    );
    save(&frame, "drawer-waiting-draft");
    views::composer(&mut frame, &threads[0], "", 0, false, 0, false, connection);
    save(&frame, "draft-empty");
    views::composer(&mut frame, &threads[0], "", 0, true, 0, false, connection);
    save(&frame, "keyboard-empty");
    views::composer(
        &mut frame,
        &threads[0],
        draft,
        0,
        true,
        views::KEYS.len() - 1,
        false,
        connection,
    );
    save(&frame, "keyboard-last");
    // Controller hints must fit at the native font size.
    for text in [
        "↑↓ / Analog Scroll    ←→ Page    △ Latest    ○ Threads",
        "↑↓ / Analog Select    ←→ Page    × Open    ○ Close",
        "↑↓ / Analog Select   ←→ Page   × Open   SELECT Wi-Fi",
        "↑↓←→ Select   × Type   □ Delete   △ Aa",
        "Analog / L/R Scroll text   ○ Review draft",
        "↑↓ / Analog Scroll    ←→ Page    ○ Back (draft kept)",
    ] {
        assert!(ui::small_width(text) <= 460, "Footer too wide: {text}");
    }
}
