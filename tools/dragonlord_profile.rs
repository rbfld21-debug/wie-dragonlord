use std::{collections::BTreeMap, env, fs, sync::Arc};

use test_utils::TestPlatform;
use wie_backend::{Emulator, Event, KeyCode, Options, ProfileSample, extract_zip};
use wie_ktf::KtfEmulator;
use wie_util::{Result, WieError};

fn tick(emulator: &mut KtfEmulator, count: usize) -> Result<()> {
    for _ in 0..count {
        emulator.tick()?;
    }
    Ok(())
}

fn press(emulator: &mut KtfEmulator, key: KeyCode) -> Result<()> {
    emulator.handle_event(Event::Keydown(key));
    tick(emulator, 3)?;
    emulator.handle_event(Event::Keyup(key));
    tick(emulator, 300)
}

fn main() -> Result<()> {
    let zip_path = env::args().nth(1).ok_or_else(|| WieError::FatalError("usage: dragonlord_profile GAME.zip".into()))?;
    let samples = Arc::new(spin::Mutex::new(BTreeMap::<Vec<u32>, u64>::new()));
    let output = samples.clone();
    let profile = Box::new(move |batch: Vec<ProfileSample>| {
        let mut output = output.lock();
        for sample in batch {
            *output.entry(sample.stack).or_default() += sample.count;
        }
    });
    let archive = extract_zip(&fs::read(zip_path).map_err(|error| WieError::FatalError(error.to_string()))?)?;
    let mut emulator = KtfEmulator::from_archive(
        Box::new(TestPlatform::new()),
        archive,
        Options { enable_gdbserver: false, profile: Some(profile) },
    )?;

    tick(&mut emulator, 18_000)?;
    press(&mut emulator, KeyCode::DOWN)?;
    press(&mut emulator, KeyCode::OK)?;
    for index in 0..12 {
        tick(&mut emulator, 8_000)?;
        press(&mut emulator, if index % 3 == 2 { KeyCode::DOWN } else { KeyCode::OK })?;
    }
    drop(emulator);

    let mut ranked = samples.lock().iter().map(|(stack, count)| (*count, stack.clone())).collect::<Vec<_>>();
    ranked.sort_unstable_by(|left, right| right.cmp(left));
    for (count, stack) in ranked.into_iter().take(100) {
        let frames = stack.iter().map(|pc| format!("{pc:#010x}")).collect::<Vec<_>>().join(" ");
        println!("{count:12} {frames}");
    }
    Ok(())
}
