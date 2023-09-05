///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! [I hate myself, for making documentation.]
//!
//! ### How Quing works.
//! Quing works around 2 central structures:
//! - A [`Track`]
//! - A [`Playlist`] (grouping of [`Tracks`], with additional data)
//!
//! [`Track`]: songs::Track
//! [`Tracks`]: songs::Track
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	panic,
	cell::OnceCell,
	io::{ Seek, Write, stdout },
	path::{ MAIN_SEPARATOR_STR, PathBuf },
	time::{ Duration, Instant },
	env::{ VarError, var },
};
use crossterm::{
	cursor::Hide,
	execute,
	terminal::{ enable_raw_mode, disable_raw_mode },
	style::{
		SetForegroundColor,
		Color,
	},
};
use crossbeam_channel::RecvTimeoutError;
use in_out::{ Bundle, Signal, Flags };
use echo::{ exit, clear };
use songs::{
	Playlist,
	get_file,
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// A module for handling and interacting with external devices.
mod in_out;

/// A collection of functions that are used repeatedly to display certain sequences.
mod echo;

/// A collection of file related structures, or implementations.
mod songs;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Constant signal [`Duration`] (tick rate). [250 milliseconds]
///
/// Every time related operation is tackted after this constant.\
const TICK: Duration = Duration::from_millis(250);
/// This is a default message that is used when a [`Sender`] or [`Receiver`] has hung up the connection.
///
/// [`Sender`]: crossbeam_channel::Sender
/// [`Receiver`]: crossbeam_channel::Receiver
const DISCONNECTED: &'static str = "DISCONNECTED CHANNEL";
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[macro_export]
/// A macro for general interaction with Standard-Out.
///
/// This macro is, in a general sense, just a fancier [`println`] macro, which also is more tailored towards [raw-mode].
///
/// [raw-mode]: crossterm::terminal#raw-mode
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
/// This recursive function is used for unexpanded shell(zsh based) expressions, on a call site, and songs' file fields.
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
			.enumerate()
			.filter_map(|(index, part)|
				match match part {
					"~" if index == 0 => expand("HOME"),
					_ if part.starts_with('$') => expand(&part[1..]),
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

	let (flags, files) = Flags::new();

	if !flags.is_headless() {
		if let Err(why) = enable_raw_mode() { panic!("enable the raw mode of the current terminal  {why}") }
		if let Err(why) = execute!(out,
			Hide,
			SetForegroundColor(Color::Yellow),
		) { log!(err: "set the terminal style" => why) }
	}

	let mut lists = files
		.filter_map(|path|
			{
				log!(info[path]: "Loading and parsing data from [{path}].");
				Playlist::try_from_path(path)
			}
		)
		.collect();
	print!("\r\n\n\0");

	if flags.should_flatten() {
		lists = vec![Playlist::flatten(lists)];
	}

	let initialisable_bundle = OnceCell::new(); // expensive operation only executed if no err.
	const SECOND: Duration = Duration::from_secs(1);


	let mut volume = 1.;
	let mut volume_before_mute = 1.;

	let lists_length = lists.len();
	let mut lists_index = 0;
	'queue: while lists_index < lists_length {
		let old_lists_index = lists_index;
		let list = unsafe { lists.get_unchecked_mut(old_lists_index) };

		let name = list.get_name();

		log!(info[name]: "Shuffling all of the songs in [{name}].");
		list.shuffle_song();

		log!(info[name]: "Loading all of the audio contents of the songs in [{name}].");
		list.load_song();
		print!("\r\n\0");

		let songs = list.get_song_mut();

		let bundle = initialisable_bundle.get_or_init(|| if flags.is_headless() { Bundle::headless() } else { Bundle::new() });
		let controls = bundle.get_controls();

		'list_playback: { // i hate this
			let songs_length = songs.len();
			let mut songs_index = 0;
			while songs_index < songs_length {
				let old_songs_index = songs_index; // (sort of) proxy to index (used because of rewind code)
				// unless something is very wrong with the index (old), this will not error.
				let data = get_file(old_songs_index);
				let duration = data.get_duration();
				let song = unsafe { songs.get_unchecked_mut(old_songs_index) };
				let name = song.get_name();


				match (bundle.play_file(data), controls) {
					(Ok(playback), control_state) => 'song_playback: {
						log!(info[name]: "Playing back the audio contents of [{name}].");

						let Some(controls) = control_state else {
							song.repeat_or_increment(&mut songs_index); // maybe reduce repeats?
							break 'song_playback playback.sleep_until_end()
						};

						playback.set_volume(volume);

						let mut elapsed = Duration::ZERO;
						while elapsed <= duration {
							let paused = playback.is_paused();

							print!("\r[{}][{volume:.2}]\0",
								{
									let seconds = elapsed.as_secs();
									let minutes = seconds / 60;
									format!("{:0>2}:{:0>2}:{:0>2}", minutes / 60, minutes % 60, seconds % 60)
								}
							);
							if let Err(why) = out.flush() { log!(err: "flush the standard output" => why) }

							let now = Instant::now();
							elapsed += match controls.receive_signal(now) {
								Err(RecvTimeoutError::Timeout) => if paused { continue } else { TICK },

								Ok(Signal::ProgramExit) => {
									clear();
									break 'queue
								},

								Ok(Signal::PlaybackToggle) => {
									if paused { playback.play() } else { playback.pause() }
									now.elapsed()
								},

								Ok(signal @ (Signal::PlaylistNext | Signal::PlaylistBack | Signal::TrackNext | Signal::TrackBack)) => { // group similar things together to perform DRY.
									let is_under_threshold = elapsed <= SECOND;
									match signal {
										Signal::PlaylistNext => break 'list_playback lists_index += 1,
										Signal::PlaylistBack => break 'list_playback lists_index -= (old_lists_index > 0 && is_under_threshold) as usize,
										Signal::TrackNext => break 'song_playback songs_index += 1,
										Signal::TrackBack => break 'song_playback songs_index -= (old_songs_index > 0 && is_under_threshold) as usize,
										_ => unimplemented!(),
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
										_ => unimplemented!(),
									}
									volume = volume.clamp(0., 2.);
									playback.set_volume(volume);
									if paused { continue }
									now.elapsed()
								},

								Err(RecvTimeoutError::Disconnected) => {
									log!(err: "receive a signal from the control thread" => DISCONNECTED);
									log!(info: "Exiting the program."); 
									break 'queue
								}, // chain reaction will follow
							};
						}
						song.repeat_or_increment(&mut songs_index);
						clear()
					},

					(Err(why), _) => log!(err[name]: "playback [{name}] from the default audio output device" => why; break 'queue), // assume error will occur on the other tracks too
				}
				if let Err(why) = get_file(old_songs_index).rewind() { log!(err[name]: "reset the player's position inside of [{name}]" => why) }
			}
		}
		list.repeat_or_increment(&mut lists_index);
	}

	if let Some(controls) = initialisable_bundle
		.into_inner()
		.map(Bundle::take_controls)
		.flatten()
	{
		controls.notify_exit();
		controls.clean_up();
	}
	if !flags.is_headless() {
		if let Err(why) = disable_raw_mode() { panic!("disable the raw mode of the current terminal  {why}") }
	}
	exit()
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
