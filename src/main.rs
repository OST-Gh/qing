///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	fs::{ self, File },
	time::{ Duration, Instant },
	thread::spawn,
};
use nitrogen::{ fmt_path, traits::* };
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
pub const ENBOLD: &str = Formatting::Bold.enable();
pub const DISBOLD: &str = Formatting::Bold.disable();
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

struct Stream {
	name: Box<str>,
	file: File,
	time: Duration,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
enum Signal {
	ManualExit, // signal sent by pb_ctl to main for indication of the user manually exiting
	SkipPlayback,
	TogglePlayback,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
fn main() {
	let handle = custom![
		'\r',
		Time::from(' '),
		Colour::from(Empty)
			.colour(colours::QING)
			.terminated(false),
	]
		.pipe(Handle::from);
	if let Err(why) = enable_raw_mode() { handle.print(format!("{LINE}An error occured whilst attempting to enable the raw mode of the current terminal; '{ENBOLD}{why}{DISBOLD}'")) };

	handle.print(format!("Spinning up the playback control thread."));
	let (sender, receiver) = unbounded();
	let (exit_sender, exit_receiver) = unbounded();
	let playback_control = spawn(
		move || {
			let handle = custom![
				'\r',
				Time::from(' '),
				Colour::from(Empty)
					.colour(colours::QING)
					.terminated(false),
			]
				.pipe(Handle::from);
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
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('.' | 'l'), .. })) => sender.send(Signal::SkipPlayback),
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

	let mut generator = Generator::new();

	handle.print("Determining the output device.");
	let handles = match OutputStream::try_default() {
		Ok(handles) => handles,
		Err(why) => {
			handle.print(format!("{LINE}A fatal error occured whilst attempting to determine the default audio output device; '{ENBOLD}{why}{DISBOLD}'"));
			return
		},
	};
	handle.print('\0');

	for path in std::env::args().skip(1) {

		handle.print(format!("Loading and parsing data from [{path}]."));
		let Songlist { song, name } = match fs::read_to_string(fmt_path(&path)).map(|contents| toml::from_str(&contents)) {
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

		handle.print(format!("Loading all of the songs in [{name}]."));
		let mut files: Vec<Stream> = song
			.into_iter()
			.filter_map(|Song { name, file }|
				{
					handle.print(format!("Loading the audio contents of [{name}]."));
					let formatted = fmt_path(file);
					let contents = match File::open(&formatted) {
						Ok(contents) => contents,
						Err(why) => {
							handle.print(format!("{LINE}An error occured whilst attempting to load the audio contents of [{name}]; '{ENBOLD}{why}{DISBOLD}'"));
							None?
						},
					};
					let duration = match read_from_path(formatted) {
						Ok(tagged) => tagged
							.properties()
							.duration(),
						Err(why) => {
							handle.print(format!("{LINE}An error occured whilst attempting to aquire the audio properties of [{name}]; '{ENBOLD}{why}{DISBOLD}'"));
							None?
						},
					};
					Some(Stream { name, file: contents, time: duration })
				}
			)
			.collect();
		handle.print('\0');

		handle.print(format!("Playing back all of the songs in [{name}]."));
		'playback: while !files.is_empty() {
			let Stream { name, file, mut time } = files.remove(generator.usize(0..files.len()));
			match handles
				.1
				.play_once(file)
			{
				Ok(playback) => {
					handle.print(format!("Playing back the audio contents of [{name}]."));
					let mut measure = Instant::now();
					let mut elapsed = measure.elapsed();
					while elapsed <= time {
						if !playback.is_paused() { elapsed = measure.elapsed() }
						match receiver.try_recv() {
							Ok(Signal::ManualExit) => {
								drop(playback);
								break 'playback
							},
							Ok(Signal::TogglePlayback) => if playback.is_paused() {
								measure = Instant::now();
								playback.play();
							} else {
								time -= elapsed;
								elapsed = Duration::ZERO;
								playback.pause()
							},
							Ok(Signal::SkipPlayback) => {
								playback.stop();
								break
							},
							Err(TryRecvError::Empty) => continue,
							Err(why) => {
								handle.print(format!("{LINE}A fatal error occured whilst attempting to receive a signal from the playback control thread; '{ENBOLD}{why}{DISBOLD}'"));
								break 'playback
							},
						}
					}
				},
				Err(why) => {
					handle.print(format!("{LINE}An error occured whilst attempting to playback [{name}] from the default audio output device; '{ENBOLD}{why}{DISBOLD}'"));
					continue
				},
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
