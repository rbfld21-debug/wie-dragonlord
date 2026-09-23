use std::{
    collections::BTreeSet,
    env, fs,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

use test_utils::{TestPlatform, TestPlatformEvent};
use wie_backend::{AudioCommand, AudioEventData, Emulator, Event, KeyCode, Options, extract_zip};
use wie_ktf::KtfEmulator;
use wie_util::{Result, WieError};

fn frame_hash(frame: &[u32]) -> u64 {
    frame.iter().fold(0xcbf29ce484222325, |hash, pixel| {
        (hash ^ u64::from(*pixel)).wrapping_mul(0x100000001b3)
    })
}

fn save_ppm(path: &str, frame: &[u32]) -> Result<()> {
    let (width, height) = match frame.len() {
        38_720 => (176, 220),
        76_800 => (320, 240),
        _ => {
        return Err(WieError::FatalError(format!("unexpected frame size: {}", frame.len())));
        }
    };
    let mut output = format!("P6\n{width} {height}\n255\n").into_bytes();
    for pixel in frame {
        let [_, r, g, b] = pixel.to_be_bytes();
        output.extend_from_slice(&[r, g, b]);
    }
    fs::write(path, output).map_err(|error| WieError::FatalError(error.to_string()))
}

fn tick(emulator: &mut KtfEmulator, count: usize) -> Result<()> {
    for _ in 0..count {
        emulator.tick()?;
    }
    Ok(())
}

fn press(emulator: &mut KtfEmulator, key: KeyCode, held_ticks: usize) -> Result<()> {
    emulator.handle_event(Event::Keydown(key));
    tick(emulator, held_ticks)?;
    emulator.handle_event(Event::Keyup(key));
    tick(emulator, 300)
}

fn main() -> Result<()> {
    let zip_path = env::args().nth(1).ok_or_else(|| WieError::FatalError("usage: dragonlord_verify GAME.zip".into()))?;
    let capture = Arc::new(spin::Mutex::new(Vec::new()));
    let audio_capture = Arc::new(spin::Mutex::new(Vec::new()));
    let exited = Arc::new(AtomicBool::new(false));
    let exited_for_handler = exited.clone();
    let platform = Box::new(TestPlatform::with_screen_and_audio_capture(capture.clone(), audio_capture.clone(), move |event| {
        if matches!(event, TestPlatformEvent::Exit) {
            exited_for_handler.store(true, Ordering::SeqCst);
        }
    }));
    let archive = extract_zip(&fs::read(zip_path).map_err(|error| WieError::FatalError(error.to_string()))?)?;
    let mut emulator = KtfEmulator::from_archive(platform, archive, Options { enable_gdbserver: false, profile: None })?;

    let mut elapsed = 0;
    for checkpoint in [4500, 6000, 8000, 10000, 12000, 15000] {
        tick(&mut emulator, checkpoint - elapsed)?;
        elapsed = checkpoint;
        let frame = capture.lock().clone();
        save_ppm(&format!("dragonlord-{checkpoint}.ppm"), &frame)?;
        println!(
            "checkpoint={checkpoint} hash={:016x} non_white={}",
            frame_hash(&frame),
            frame.iter().filter(|pixel| **pixel != 0xffff_ffff).count()
        );
    }
    let menu = capture.lock().clone();
    save_ppm("dragonlord-menu.ppm", &menu)?;
    press(&mut emulator, KeyCode::DOWN, 2)?;
    let after_down = capture.lock().clone();
    save_ppm("dragonlord-after-down.ppm", &after_down)?;
    press(&mut emulator, KeyCode::OK, 2)?;
    let after_ok = capture.lock().clone();
    save_ppm("dragonlord-after-ok.ppm", &after_ok)?;
    for step in 0..12 {
        tick(&mut emulator, 2000)?;
        press(&mut emulator, KeyCode::OK, 2)?;
        let frame = capture.lock().clone();
        save_ppm(&format!("dragonlord-story-{step:02}.ppm"), &frame)?;
        println!("story_step={step} hash={:016x}", frame_hash(&frame));
    }
    tick(&mut emulator, 5000)?;
    let after_startup = capture.lock().clone();
    save_ppm("dragonlord-after-startup.ppm", &after_startup)?;

    let white = 0xffff_ffff;
    let non_white = menu.iter().filter(|pixel| **pixel != white).count();
    let menu_hash = frame_hash(&menu);
    let down_hash = frame_hash(&after_down);
    let ok_hash = frame_hash(&after_ok);
    println!(
        "menu={menu_hash:016x} down={down_hash:016x} ok={ok_hash:016x} startup={:016x} non_white={non_white} exited={}",
        frame_hash(&after_startup),
        exited.load(Ordering::SeqCst)
    );
    let commands = audio_capture.lock();
    for command in commands.iter() {
        if let AudioCommand::Play { handle, sequence, repeat } = command {
            let mut midi_events = 0usize;
            let mut note_on = 0usize;
            let mut note_off = 0usize;
            let mut program_change = 0usize;
            let mut control_change = 0usize;
            let mut pitch_bend = 0usize;
            let mut sysex = 0usize;
            let mut percussion = 0usize;
            let mut program_values = BTreeSet::new();
            let mut sysex_values = BTreeSet::new();
            let mut wave_events = 0usize;
            let mut wave_samples = 0usize;
            let mut peak = 0i16;
            for event in &sequence.events {
                match &event.data {
                    AudioEventData::Smaf(data) => {
                        if let Ok(directory) = env::var("DRAGONLORD_DUMP_SMAF_DIR") {
                            fs::write(format!("{directory}/audio-{handle}.mmf"), data)
                                .map_err(|error| WieError::FatalError(error.to_string()))?;
                        }
                    }
                    AudioEventData::Midi(data) => {
                        midi_events += 1;
                        let status = data.first().copied().unwrap_or(0);
                        match status & 0xf0 {
                            0x80 => note_off += 1,
                            0x90 => {
                                if data.get(2).copied().unwrap_or(0) == 0 {
                                    note_off += 1;
                                } else {
                                    note_on += 1;
                                    if status & 0x0f == 9 {
                                        percussion += 1;
                                    }
                                }
                            }
                            0xb0 => control_change += 1,
                            0xc0 => {
                                program_change += 1;
                                program_values.insert((status & 0x0f, data.get(1).copied().unwrap_or(0)));
                            }
                            0xe0 => pitch_bend += 1,
                            0xf0 => {
                                sysex += 1;
                                sysex_values.insert(data.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join(" "));
                            }
                            _ => {}
                        }
                    }
                    AudioEventData::Wave { samples, .. } => {
                        wave_events += 1;
                        wave_samples += samples.len();
                        peak = peak.max(samples.iter().map(|sample| sample.saturating_abs()).max().unwrap_or(0));
                    }
                }
            }
            println!(
                "audio handle={handle} repeat={repeat} duration={} midi_events={midi_events} note_on={note_on} note_off={note_off} programs={program_change} controls={control_change} bends={pitch_bend} sysex={sysex} percussion={percussion} wave_events={wave_events} wave_samples={wave_samples} peak={peak}",
                sequence.duration
            );
            println!("audio programs={program_values:?}");
            for value in sysex_values {
                println!("audio sysex={value}");
            }
        } else if let AudioCommand::Stop { handle } = command {
            println!("audio stop handle={handle}");
        }
    }
    drop(commands);
    if exited.load(Ordering::SeqCst) || non_white < 100 || menu_hash == down_hash || down_hash == ok_hash {
        return Err(WieError::FatalError("Dragonlord menu/input verification failed".into()));
    }
    Ok(())
}
