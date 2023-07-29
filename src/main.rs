///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! I don't know why, but i am making Docs for this.
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	panic,
	cell::OnceCell,
	io::Seek,
	path::{ MAIN_SEPARATOR_STR, PathBuf },
	time::{ Duration, Instant },
	env::{ VarError, var, args },
};
use crossterm::terminal::{ enable_raw_mode, disable_raw_mode };
use crossbeam_channel::RecvTimeoutError;
use serde::Deserialize;
use fastrand::Rng;
use load::{
	FILES,
	get_file,
	map_files,
	songs,
	songlists,
};
use state::{ State, Signal };
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Module for interacting with the file-system.
mod load;

/// Runtime state struct declaration and implementations.
// NOTE: state is not a got name, it was a name i cam up with on a whim.
// TODO: Rename to more sensical name.
mod state;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Constant signal rate (tick rate).
///
/// Info:
///
/// This constant
const TICK: Duration = Duration::from_millis(250);

/// Inter-thread communication channel disconnected.
///
/// This is just a default message, that is used when a sender, or receiver, has hung up the between thread connection.
const DISCONNECTED: &'static str = "DISCONNECTED CHANNEL";
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Deserialize)]
/// A playlist with some metadata
struct Playlist {
	name: Option<Box<str>>,
	song: Vec<Song>,
	time: Option<isize>,
}

#[derive(Deserialize)]
#[derive(Clone)]
/// A song path with aditional metadata
struct Song {
	name: Option<Box<str>>,
	file: Box<str>,
	time: Option<isize>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[macro_export]
/// Macro for general interaction with Standard-out.
macro_rules! log {
	(err$([$($visible: ident)+])?: $message: literal => $($why: ident)+ $(; $($retaliation: tt)+)?) => {
		{
			print!(concat!("\r\x1b[4mA non-fatal error occurred whilst attempting to ", $message, ';') $(, $($visible = $visible),+)?);
			$(print!(" '\x1b[1m{}\x1b[22m'", format!("{}", $why).replace('\n', "\r\n"));)+
			println!("\x1b[24m\0");
			$($($retaliation)+)?
		}
	};
	(info$([$($visible: ident)+])?: $message: literal) => { println!(concat!('\r', $message, '\0') $(, $($visible = $visible),+)?) };
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Print the reset ansi sequence.
fn exit_sequence() { print!("\r\x1b[0m\0") }

/// Format a text representation of a path into an absolute path.
///
/// This recursive function is used for unexpanded shell(zsh based) expressions, on the call site, and inside the playlist file key(?) of songs inside of a playlist.
/// It can currently only expand environment variables, which might recurse.
fn fmt_path(path: impl AsRef<str>) -> PathBuf {
	fn expand(name: &str) -> Result<String, VarError> {
		let mut buffer = Vec::new();
		for part in var(if name.starts_with('$') { expand(&name[1..])? } else { String::from(name) })?
			.split(MAIN_SEPARATOR_STR)
			.map(|part| if part.starts_with('$') { expand(&part[1..]) } else { Ok(String::from(part)) })
		{ buffer.push(part?) }
		Ok(buffer.join(MAIN_SEPARATOR_STR))
	}

	let path = path.as_ref();
	PathBuf::from(
		path
			.split(MAIN_SEPARATOR_STR)
			.filter_map(|part|
				match match part {
					"~" => expand("HOME"),
					_ if part.starts_with('$') => expand(&part[1..]), // add support for multiple $ vars ($$VAR => $VALUE_OF_VAR => VALUE_OF_VALUE_OF_VAR)
					_ => return Some(String::from(part)),
				} {
					Ok(part) => Some(part),
					Err(why) => log!(err[part]: "expand the shell expression [{part}] to a path" => why; None)
				}
			)
			.collect::<Vec<String>>()
			.join(MAIN_SEPARATOR_STR)
	)
		.canonicalize()
		.unwrap_or_else(|why| log!(err[path]: "canonicalise the path [{path}]" => why; PathBuf::new()))
}

fn main() {
	panic::set_hook(
		Box::new(|info|
			unsafe {
				let payload = info.payload();
				let panic = payload
					.downcast_ref::<&str>()
					.map(|slice| String::from(*slice))
					.xor(
						payload
							.downcast_ref::<String>()
							.map(String::from)
					)
					.unwrap();
				let panic = panic
					.splitn(2, "  ")
					.collect::<Vec<&str>>();
				let message = panic.get_unchecked(0);
				let reason = panic
					.get(1)
					.unwrap_or(&"NO_DISPLAYABLE_INFORMATION")
					.replace('\n', "\r\n");
				println!("\r\x1b[4mAn error occurred whilst attempting to {message}; '\x1b[1m{reason}\x1b[22m'\x1b[24m\0");
				exit_sequence()
			}
		)
	);

	{
		let default = vec![254, 205, 033];
		let colours = if let Ok(text) = var("COLOUR") {
			let inner_colours: Vec<u8> = text
				.split(|symbol: char| !symbol.is_numeric())
				.filter_map(|text|
					text
						.parse::<u8>()
						.ok()
				)
				.collect();
			if inner_colours.len() < 3 { default } else { inner_colours }
		} else { default };
		print!("\x1b[38;2;{};{};{}m", colours[0], colours[1], colours[2]);
	}

	let mut lists = {
		let mut files = args()
			.skip(1) // skips the executable path (e.g.: //bin/{bin-name})
			.peekable();
		if let None = files.peek() { panic!("get the program arguments  no arguments given") }

		if let Err(why) = enable_raw_mode() { log!(err: "enable the raw mode of the current terminal" => why; return exit_sequence()) }

		songlists(files)
	};
	let init = OnceCell::new(); // expensive operation only executed if no err.
	let mut generator = Rng::new();
	const SECOND: Duration = Duration::from_secs(1);


	let length = lists.len();
	let mut list_index = 0;
	let mut volume_of_previous_track = 1.;
	let mut volume_before_mute = 1.;
	'queue: while list_index < length {
		let old_list_index = list_index;
		let (name, song, list_time) = unsafe { lists.get_unchecked_mut(old_list_index) };

		log!(info[name]: "Shuffling all of the songs in [{name}].");
		let length = song.len();
		for value in 0..length {
			let index = value % length;
			song.swap(index, generator.usize(0..=index));
			song.swap(index, generator.usize(index..length));
			// a b c; b inclusive in both random ranges
			// b a c
			// b c a
		}

		let mut song = songs(&name, &song);
		let state = init.get_or_init(State::initialise);


		'list_playback: { // i hate this
			let length = song.len();
			let mut song_index = 0;
			while song_index < length && state.is_alive() {
				let old_song_index = song_index; // (sort of) proxy to index (used because of rewind code)
				// unless something is very wrong with the index (old), this will not error.
				let (name, duration, song_time) = unsafe { song.get_unchecked_mut(old_song_index) };
				match state.play_file(get_file(old_song_index)) {
					Ok(playback) => 'song_playback: {
						log!(info[name]: "Playing back the audio contents of [{name}].");

						playback.set_volume(volume_of_previous_track);

						let mut elapsed = Duration::ZERO;
						while &elapsed <= duration {
							let now = Instant::now();
							let paused = playback.is_paused();
							elapsed += match state.receive_signal(now) {
								Err(RecvTimeoutError::Timeout) => if paused { continue } else { TICK },

								Ok(Signal::ProgramExit) => break 'queue,

								Ok(signal @ (Signal::PlaylistNext | Signal::PlaylistBack)) => break 'list_playback match signal {
									Signal::PlaylistNext => list_index += 1,
									Signal::PlaylistBack => list_index -= (old_list_index > 0 && elapsed <= SECOND) as usize,
									_ => unimplemented!()
								},

								Ok(signal @ (Signal::SongNext | Signal::SongBack)) => break 'song_playback match signal {
									Signal::SongNext => song_index += 1,
									Signal::SongBack => song_index -= (old_song_index > 0 && elapsed <= SECOND) as usize,
									_ => unimplemented!()
								},

								Ok(Signal::PlaybackToggle) => {
									if paused { playback.play() } else { playback.pause() }
									now.elapsed()
								},

								Ok(signal @ (Signal::VolumeIncrease | Signal::VolumeDecrease | Signal::VolumeToggle)) => {
									let mut volume = playback.volume();
									match signal {
										Signal::VolumeToggle => if volume <= 0. { volume = volume_before_mute } else {
											volume_before_mute = volume;
											volume = 0.
										},
										Signal::VolumeIncrease => volume += 0.25,
										Signal::VolumeDecrease => volume -= 0.25,
										_ => unimplemented!()
									}
									volume = volume.clamp(0., 3.);
									volume_of_previous_track = volume;
									playback.set_volume(volume);
									now.elapsed()
								},

								Err(RecvTimeoutError::Disconnected) => break 'queue, // chain reaction will follow
							};
						}
						if *song_time == 0 { song_index += 1 } else {
							log!(info[name]: "Repeating the song [{name}]");
							*song_time -= 1
						}
					},
					Err(why) => log!(err[name]: "playback [{name}] from the default audio output device" => why; break 'queue), // assume error will occur on the other tracks too
				};
				if let Err(why) = unsafe { FILES.get_unchecked_mut(old_song_index) }.rewind() { log!(err[name]: "reset the player position inside of [{name}]" => why) }
			}
			if *list_time == 0 { list_index += 1 } else {
				log!(info[name]: "Reloading the playlist [{name}]");
				*list_time -= 1
			}
		}
		map_files(Vec::clear);
		print!("\r\n\n\0");
	}

	if let Some(inner) = init.into_inner() {
		inner.notify_exit();
		inner.clean_up();
		if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
		exit_sequence()
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
