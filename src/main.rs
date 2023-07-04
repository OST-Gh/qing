///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! NOTE: crt in raw mode behaves strangely or maps <100% keyboard maps to 100% maps, e.g.: backspace in raw-mode = h
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	fs::{ self, File },
	path::{ PathBuf, MAIN_SEPARATOR_STR },
	time::{ Duration, Instant },
	thread::spawn,
	io::BufReader,
	env::var,
};
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
use chrono::{ Timelike, Utc };
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
	SkipPlaylist,
	SkipNext,
	SkipBack,
	TogglePlayback,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
macro_rules! log {
	() => {
		{
			let now = Utc::now();
			print!(
				"\r\x1b[0m[{:0>2}:{:0>2}:{:0>2}]",
				now.hour(),
				now.minute(),
				now.second(),
			);
		}
	};
	(err$([$($visible: ident)+])?: $message: literal => $($why: ident)+) => {
		{
			log!();
			print!(concat!(" \x1b[38;2;254;205;33m\x1b[4mAn error occured whilst attempting to ", $message, ';') $(, $($visible = $visible),+)?);
			$(print!(" '\x1b[1m{}\x1b[22m'", $why);)+
			println!("\0");
		}
	};
	(info$([$($visible: ident)+])?: $message: literal) => {
		{
			log!();
			println!(concat!(" \x1b[38;2;254;205;33m", $message, '\0') $(, $($visible = $visible),+)?);
		}
	};
	($($_: tt)+) => {
		{
			$(
				stringify!($_);
				log!();
				println!("\0");
			)+
		}
	};
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
	if let Err(why) = enable_raw_mode() { log!(err: "enable the raw mode of the current terminal" => why); return };

	log!(info: "Spinning up the playback control thread.");
	let (sender, receiver) = unbounded();
	let (exit_sender, exit_receiver) = unbounded();
	let playback_control = spawn(
		move || loop {
			match exit_receiver.try_recv() {
				Ok(_) => break,
				Err(TryRecvError::Empty) => {
					let event = match event::poll(Duration::ZERO) {
						Ok(truth) => if truth { event::read() } else { continue },
						Err(why) => {
							log!(err: "poll an event from the current terminal" => why);
							continue
						},
					};
					let send_result = match event {
						Ok(Event::Key(KeyEvent { code: KeyCode::Char('q' | 'c'), .. })) => sender.send(Signal::ManualExit),
						Ok(Event::Key(KeyEvent { code: KeyCode::Char('/' | 'h'), .. })) => sender.send(Signal::SkipPlaylist),
						Ok(Event::Key(KeyEvent { code: KeyCode::Char('.' | 'l'), .. })) => sender.send(Signal::SkipNext),
						Ok(Event::Key(KeyEvent { code: KeyCode::Char(',' | 'j'), .. })) => sender.send(Signal::SkipBack),
						Ok(Event::Key(KeyEvent { code: KeyCode::Char(' ' | 'k'), .. })) => sender.send(Signal::TogglePlayback),
						Err(why) => {
							log!(err: "read an event from the current terminal" => why);
							continue
						},
						_ => continue,
					};
					if let Err(why) = send_result { log!(err: "send a signal to the playback" => why) };
				},
				Err(why) => log!(err: "receive a signal from the main thread" => why),
			};
		}
	);

	log!(info: "Determining the output device.");
	let handles = match OutputStream::try_default() {
		Ok(handles) => handles,
		Err(why) => {
			log!(err: "determine the default audio output device" => why);
			return
		},
	};
	log!(,,);

	'playback: for path in std::env::args().skip(1) {

		log!(info[path]: "Loading and parsing data from [{path}].");
		let Songlist { mut song, name } = 'load: {
			match fs::read_to_string(fmt_path(&path)).map(|contents| toml::from_str(&contents)) {
				Ok(Ok(playlist)) => break 'load playlist,
				Ok(Err(why)) => log!(err[path]: "parse the contents of [{path}]" => why),
				Err(why) => log!(err[path]: "load the contents of [{path}]" => why),
			};
			continue 'playback
		};

		log!(info[name]: "Shuffling all of the songs in [{name}].");
		let (length, song) = {
			let length = song.len();
			let mut new = Vec::with_capacity(length);
			let mut generator = Generator::new();

			while !song.is_empty() { new.push(song.swap_remove(generator.usize(0..song.len()))) }
			(length, new)
		};
		let mut index = 0;

		log!(info[name]: "Playing back all of the songs in [{name}].");
		log!(,);
		'playlist: while index < length {
			let Song { name, file } = song
				.get(index)
				.unwrap() /* unwrap safe */;

			log!(,);
			log!(info[name]: "Loading the audio contents and properties of [{name}].");
			let (contents, mut duration) = 'load: {
				let formatted = fmt_path(file);
				match (File::open(&formatted), read_from_path(formatted)) {
					(Ok(contents), Ok(info)) => break 'load (
						BufReader::new(contents),
						info
							.properties()
							.duration(),
					),
					(Err(why), Ok(_)) => log!(err[name]: "load the audio contents of [{name}]" => why),
					(Ok(_), Err(why)) => log!(err[name]: "load the audio properties of [{name}]" => why),
					(Err(file_why), Err(info_why)) => log!(err[name]: "load the audio contents and properties of [{name}]" => file_why info_why),
				};
				index += 1;
				continue 'playlist
			};

			'controls: {
				match handles
					.1
					.play_once(contents)
				{
					Ok(playback) => {
						log!(info[name]: "Playing back the audio contents of [{name}].");
						let mut measure = Instant::now();
						let mut elapsed = measure.elapsed();
						while elapsed <= duration {
							if !playback.is_paused() { elapsed = measure.elapsed() }
							match receiver.try_recv() {
								Ok(Signal::ManualExit) => break 'playback,
								Ok(Signal::SkipPlaylist) => break 'playlist,
								Ok(Signal::SkipNext) => break,
								Ok(Signal::SkipBack) => {
									if index > 0 { index -= 1 };
									break 'controls
								},
								Ok(Signal::TogglePlayback) if playback.is_paused() => {
									measure = Instant::now();
									playback.play();
								},
								Ok(Signal::TogglePlayback) => {
									duration -= elapsed;
									elapsed = Duration::ZERO;
									playback.pause()
								},
								Err(TryRecvError::Empty) => continue,
								Err(why) => {
									log!(err: "receive a signal from the playback control thread" => why);
									break 'playback
								},
							}
						}
					},
					Err(why) => log!(err[name]: "playback [{name}] from the default audio output device" => why),
				}
				index += 1;
			}
		}
		log!(,,,);
	}

	if let Err(why) = exit_sender.send(0) { log!(err: "send the exit signal to the playback control thread" => why) };
	let _ = playback_control.join();
	if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) };
	print!("\r\x1b[0m\0")
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
