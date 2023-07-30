///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! I don't know why, but i am making Docs for this.
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	panic,
	cell::OnceCell,
	io::{ Seek, Write, stdout },
	path::{ MAIN_SEPARATOR_STR, PathBuf },
	time::{ Duration, Instant },
	env::{ VarError, var, args },
};
use crossterm::{
	execute,
	cursor::Hide,
	terminal::{
		enable_raw_mode,
		disable_raw_mode
	},
	style::{
		SetForegroundColor as SetForegroundColour,
		Color as Colour,
	},
};
use crossbeam_channel::RecvTimeoutError;
use serde::Deserialize;
use fastrand::Rng;
use load::{
	FILES,
	get_file,
	map_files,
	tracks,
	playlists,
};
use state::{ State, Signal };
use echo::{ exit, clear };
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Module for interacting with the file-system.
mod load;

/// Runtime state structure declaration and implementations.
// NOTE: state is not a got name, it was a name i cam up with on a whim.
// TODO: Rename to more sensual name.
mod state;

/// A collection of functions that are used repeatedly to display certain sequences.
mod echo;
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
	song: Vec<Track>,
	time: Option<isize>,
}

#[derive(Deserialize)]
#[derive(Clone)]
/// A song path with aditional metadata
struct Track {
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
			print!(concat!("\rA non-fatal error occurred whilst attempting to ", $message, ';') $(, $($visible = $visible),+)?);
			$(print!(" '{}'", format!("{}", $why).replace('\n', "\r\n"));)+
			print!("\0\n");
			$($($retaliation)+)?
		}
	};
	(info$([$($visible: ident)+])?: $message: literal) => { print!(concat!('\r', $message, "\0\n") $(, $($visible = $visible),+)?) };
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Format a text representation of a path into an absolute path.
///
/// This recursive function is used for unexpanded shell(zsh based) expressions, on the call site, and inside the playlist file key(?) of songs inside of a playlist.
/// It can currently only expand environment variables, which might recurs.
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
				print!("\rAn error occurred whilst attempting to {message}; '{reason}'\0\n");
				exit();

			}
		)
	);

	let mut out = stdout();
	if let Err(why) = execute!(out,
		Hide,
		SetForegroundColour(Colour::Yellow),
	) { log!(err: "set the terminal style" => why) }


	let mut lists = {
		let mut files = args()
			.skip(1) // skips the executable path (e.g.: //bin/{bin-name})
			.peekable();
		if let None = files.peek() { panic!("get the program arguments  no arguments given") }

		if let Err(why) = enable_raw_mode() { log!(err: "enable the raw mode of the current terminal" => why; return exit()) }

		playlists(files)
	};
	let init = OnceCell::new(); // expensive operation only executed if no err.
	let mut generator = Rng::new();
	const SECOND: Duration = Duration::from_secs(1);


	let mut volume = 1.;
	let mut volume_before_mute = 1.;

	let lists_length = lists.len();
	let mut lists_index = 0;
	'queue: while lists_index < lists_length {
		let old_lists_index = lists_index;
		let (name, songs, list_repeats) = unsafe { lists.get_unchecked_mut(old_lists_index) };
		if *list_repeats == 0 { lists_index += 1 } else { *list_repeats -= 1 }


		log!(info[name]: "Shuffling all of the songs in [{name}].");
		let length = songs.len();
		for value in 0..length {
			let index = value % length;
			songs.swap(index, generator.usize(0..=index));
			songs.swap(index, generator.usize(index..length));
			// a b c; b inclusive in both random ranges
			// b a c
			// b c a
		}

		let mut songs = tracks(&name, &songs);
		let state = init.get_or_init(State::initialise);

		'list_playback: { // i hate this
			let songs_length = songs.len();
			let mut songs_index = 0;
			while songs_index < songs_length {
				let old_songs_index = songs_index; // (sort of) proxy to index (used because of rewind code)
				// unless something is very wrong with the index (old), this will not error.
				let (name, duration, song_repeats) = unsafe { songs.get_unchecked_mut(old_songs_index) };
				if *song_repeats == 0 { songs_index += 1 } else { *song_repeats -= 1 }


				match state.play_file(get_file(old_songs_index)) {
					Ok(playback) => 'song_playback: {
						log!(info[name]: "Playing back the audio contents of [{name}].");
						if state.is_headless() { break 'song_playback playback.sleep_until_end() }

						playback.set_volume(volume);

						let mut elapsed = Duration::ZERO;
						while &elapsed <= duration {
							let paused = playback.is_paused();

							print!("\r[{}][{volume:.3}]\0",
								{
									let seconds = elapsed.as_secs();
									let minutes = seconds / 60;
									format!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
								}
							);
							if let Err(why) = stdout().flush() { log!(err: "flush the standard output" => why) }

							let now = Instant::now();
							elapsed += match state.receive_signal(now) {
								Err(RecvTimeoutError::Timeout) => if paused { continue } else { TICK },

								Ok(Signal::ProgramExit) => {
									clear();
									break 'queue
								},

								Ok(Signal::PlaybackToggle) => {
									if paused { playback.play() } else { playback.pause() }
									now.elapsed()
								},

								Ok(signal @ (Signal::PlaylistNext | Signal::PlaylistBack | Signal::TrackNext | Signal::TrackBack)) => {
									let is_under_threshold = elapsed <= SECOND;
									match signal {
										Signal::PlaylistNext => break 'list_playback lists_index += 1,
										Signal::PlaylistBack => break 'list_playback lists_index -= (old_lists_index > 0 && is_under_threshold) as usize,
										Signal::TrackNext => break 'song_playback songs_index += 1,
										Signal::TrackBack => break 'song_playback songs_index -= (old_songs_index > 0 && is_under_threshold) as usize,
										_ => unimplemented!()
									}
								},

								Ok(signal @ (Signal::VolumeIncrease | Signal::VolumeDecrease | Signal::VolumeToggle)) => {
									match signal {
										Signal::VolumeToggle => if volume <= 0. { volume = volume_before_mute } else {
											volume_before_mute = volume;
											volume = 0.
										},
										Signal::VolumeIncrease => volume += 0.05,
										Signal::VolumeDecrease => volume -= 0.05,
										_ => unimplemented!()
									}
									volume = volume.clamp(0., 2.);
									playback.set_volume(volume);
									now.elapsed()
								},

								Err(RecvTimeoutError::Disconnected) => break 'queue, // chain reaction will follow
							};
						}
					},
					Err(why) => log!(err[name]: "playback [{name}] from the default audio output device" => why; break 'queue), // assume error will occur on the other tracks too
				};
				if let Err(why) = unsafe { FILES.get_unchecked_mut(old_songs_index) }.rewind() { log!(err[name]: "reset the player position inside of [{name}]" => why) }
			}
		}
		map_files(Vec::clear);
		clear()
	}

	if let Some(inner) = init.into_inner() {
		inner.notify_exit();
		inner.clean_up();
	}
	if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
	exit()
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
