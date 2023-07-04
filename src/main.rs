///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	fs::{ self, File },
	path::{ PathBuf, MAIN_SEPARATOR_STR },
	time::{ Duration, Instant },
	thread::spawn,
	io::BufReader,
	env::var,
};
use oxygen::*;
use serde::Deserialize;
use rodio::OutputStream;
use crossterm::{
	terminal::{
		enable_raw_mode,
		disable_raw_mode,
	},
	event::{
		self,
		Event,
		KeyEvent,
		KeyCode,
	},
};
use crossbeam_channel::{ unbounded, TryRecvError };
use fastrand::Rng as Generator;
use lofty::{ read_from_path, AudioFile };
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
pub const LINE: &str = Formatting::UnderLined.enable();
pub const ENBOLD: &str = Formatting::Bold.enable(); pub const DISBOLD: &str = Formatting::Bold.disable();

pub const HANDLE: fn() -> Handle<Custom> = || Handle::from(
	custom![
		'\r',
		Time::from(' '),
		Colour::from(Empty)
			.colour(colours::QING)
			.terminated(false),
	]
);
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Deserialize)]
struct Songlist {
	name: Box<str>,
	song: Vec<Song>,
}

#[derive(Deserialize)]
struct Song {
	name: Box<str>,
	file: Box<str>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
enum Signal {
	ManualExit, // signal sent by pb_ctl to main for indication of the user manually exiting
	SkipNext,
	SkipBack,
	TogglePlayback,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
fn fmt_path(text: impl AsRef<str>) -> PathBuf {
	text
		.as_ref()
		.split(MAIN_SEPARATOR_STR)
		.filter_map(|part|
			{
				if part == "~" { return var("HOME").ok() };
				if part.starts_with('$') { return var(&part[1..]).ok() };
				Some(String::from(part))
			}
		)
		.collect::<Vec<String>>()
		.join(MAIN_SEPARATOR_STR)
		.into()
}

fn main() {
	let handle = HANDLE();
	if let Err(why) = enable_raw_mode() { handle.print(format!("{LINE}An error occured whilst attempting to enable the raw mode of the current terminal; '{ENBOLD}{why}{DISBOLD}'")) };

	handle.print(format!("Spinning up the playback control thread."));
	let (sender, receiver) = unbounded();
	let (exit_sender, exit_receiver) = unbounded();
	let playback_control = spawn(
		move || {
			let handle = HANDLE();
			loop {
				match exit_receiver.try_recv() {
					Ok(_) => break,
					Err(TryRecvError::Empty) => {
						let event = match event::poll(Duration::ZERO) {
							Ok(truth) => if truth { event::read() } else { continue },
							Err(why) => {
								handle.print(format!("{LINE}An error occured whilst attempting to poll an event from the current terminal; '{ENBOLD}{why}{DISBOLD}'"));
								continue
							},
						};
						let send_result = match event {
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('q' | 'c'), .. })) => sender.send(Signal::ManualExit),
							Ok(Event::Key(KeyEvent { code: KeyCode::Char(' ' | 'k'), .. })) => sender.send(Signal::TogglePlayback),
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('.' | 'l'), .. })) => sender.send(Signal::SkipNext),
							Ok(Event::Key(KeyEvent { code: KeyCode::Char(',' | 'j'), .. })) => sender.send(Signal::SkipBack),
							Err(why) => {
								handle.print(format!("{LINE}An error occured whilst attempting to read an event from the current terminal; '{ENBOLD}{why}{DISBOLD}'"));
								continue
							},
							_ => continue,
						};
						if let Err(why) = send_result { handle.print(format!("{LINE}An error occured whilst attempting to send a signal to the playback; '{ENBOLD}{why}{DISBOLD}'")) };
					},
					Err(why) => handle.print(format!("{LINE}An error occured whilst attempting to receive a signal from the main thread; '{ENBOLD}{why}{DISBOLD}'")),
				};
			}
		}
	);

	handle.print("Determining the output device.");
	let handles = match OutputStream::try_default() {
		Ok(handles) => handles,
		Err(why) => {
			handle.print(format!("{LINE}A fatal error occured whilst attempting to determine the default audio output device; '{ENBOLD}{why}{DISBOLD}'"));
			return
		},
	};
	handle.print('\0');
	handle.print('\0');

	for path in std::env::args().skip(1) {

		handle.print(format!("Loading and parsing data from [{path}]."));
		let Songlist { mut song, name } = match fs::read_to_string(fmt_path(&path)).map(|contents| toml::from_str(&contents)) {
			Ok(Ok(playlist)) => playlist,
			Ok(Err(why)) => {
				handle.print(format!("{LINE}An error occured whilst attempting to parse the contents of [{path}]; '{ENBOLD}{why}{DISBOLD}'"));
				continue
			},
			Err(why) => {
				handle.print(format!("{LINE}An error occured whilst attempting to load the contents of [{path}]; '{ENBOLD}{why}{DISBOLD}'"));
				continue
			},
		};

		handle.print(format!("Shuffling all of the songs in [{name}]."));
		let (length, song) = {
			let length = song.len();
			let mut new = Vec::with_capacity(length);
			let mut generator = Generator::new();

			while !song.is_empty() { new.push(song.remove(generator.usize(0..song.len()))) }
			(length, new)
		};
		let mut index = 0;

		handle.print(format!("Playing back all of the songs in [{name}]."));
		handle.print('\0');
		'playback: while index < length {
			let Song { name, file } = song
				.get(index)
				.unwrap() /* unwrap safe */;

			handle.print('\0');
			handle.print(format!("Loading the audio contents and properties of [{name}]."));
			let (contents, mut duration) = {
				let formatted = fmt_path(file);
				match (File::open(&formatted), read_from_path(formatted)) {
					(Ok(contents), Ok(info)) => (
						BufReader::new(contents),
						info
							.properties()
							.duration(),
					),
					(file, info) => {
						handle.print(
							format!(
								"{LINE}An error occured whilst attempting to load the audio contents and properties of [{name}];{}{}",
								if let Err(why) = file { format!(" '{ENBOLD}{why}{DISBOLD}'") } else { String::new() },
								if let Err(why) = info { format!(" '{ENBOLD}{why}{DISBOLD}'") } else { String::new() },
							)
						);
						index += 1;
						continue
					},
				}
			};

			'controls: {
				match handles
					.1
					.play_once(contents)
				{
					Ok(playback) => {
						handle.print(format!("Playing back the audio contents of [{name}]."));
						let mut measure = Instant::now();
						let mut elapsed = measure.elapsed();
						while elapsed <= duration {
							if !playback.is_paused() { elapsed = measure.elapsed() }
							match receiver.try_recv() {
								Ok(Signal::ManualExit) => break 'playback,
								Ok(Signal::TogglePlayback) => if playback.is_paused() {
									measure = Instant::now();
									playback.play();
								} else {
									duration -= elapsed;
									elapsed = Duration::ZERO;
									playback.pause()
								},
								Ok(Signal::SkipNext) => break,
								Ok(Signal::SkipBack) => {
									if index > 0 { index -= 1 };
									break 'controls
								}
								Err(TryRecvError::Empty) => continue,
								Err(why) => {
									handle.print(format!("{LINE}A fatal error occured whilst attempting to receive a signal from the playback control thread; '{ENBOLD}{why}{DISBOLD}'"));
									break 'playback
								},
							}
						}
					},
					Err(why) => handle.print(format!("{LINE}An error occured whilst attempting to playback [{name}] from the default audio output device; '{ENBOLD}{why}{DISBOLD}'")),
				}
				index += 1;
			}
		}
		handle.print('\0');
	}

	if let Err(why) = exit_sender.send(0) { handle.print(format!("{LINE}An error occured whilst attempting to send the exit signal to the playback control thread; '{ENBOLD}{why}{DISBOLD}'")) };
	let _ = playback_control.join();
	if let Err(why) = disable_raw_mode() { handle.print(format!("{LINE}An error occured whilst attempting to disable the raw mode of the current terminal; '{ENBOLD}{why}{DISBOLD}'")) };
	handle.print('\0');
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
