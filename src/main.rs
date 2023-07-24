///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! I don't know why, but i am making Docs for this.
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	thread::{ JoinHandle, Builder },
	path::{ PathBuf, MAIN_SEPARATOR_STR },
	time::{ Duration, Instant },
	io::{ Seek, BufReader },
	fs::File,
	env::{ var, args, VarError },
	cell::OnceCell,
	panic,
	any::Any,
};
use crossterm::{
	terminal::{ enable_raw_mode, disable_raw_mode },
	event::{
		self,
		Event,
		KeyEvent,
		KeyCode,
	},
};
use crossbeam_channel::{ unbounded, RecvTimeoutError, Sender, Receiver };
use rodio::{ OutputStream, OutputStreamHandle };
use serde::Deserialize;
use fastrand::Rng;
use load::{ songs, songlists };
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Module for interacting with the file-system.
mod load;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Constant signal rate.
const FOURTH_SECOND: Duration = Duration::from_millis(250);
/// Constant used for rewinding.
const SECOND: Duration = Duration::from_secs(1);

/// Inter-thread communication channel disconnected.
const DISCONNECTED: &'static str = "DISCONNECTED CHANNEL";

/// Exit text sequence
const EXIT: fn() = || print!("\r\x1b[0m\0");
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Global audio stream data.
static mut FILES: Vec<BufReader<File>> = Vec::new();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Deserialize)]
/// Playlist
struct Songlist {
	name: Option<Box<str>>,
	song: Vec<Song>,
	time: Option<isize>,
}

#[derive(Deserialize)]
#[derive(Clone)]
/// Track
struct Song {
	name: Option<Box<str>>,
	file: Box<str>,
	time: Option<isize>,
}

/// Bundled lazily initialised state.
struct State {
	output: (OutputStream, OutputStreamHandle),
	control: JoinHandle<()>,
	exit: Sender<u8>,
	signal: Receiver<Signal>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// High level control signal representation
enum Signal {
	ManualExit,
	SkipNextPlaylist,
	SkipBackPlaylist,
	SkipNext,
	SkipBack,
	TogglePlayback,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Macro for general interaction with Standard-out.
#[macro_export]
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
/// Format a text representation of a path into an absolute path.
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

/// Extract the panic payload out of thread err or panics.
fn panic_payload(payload: &(dyn Any + Send)) -> String {
	payload
		.downcast_ref::<&str>()
		.map(|slice| String::from(*slice))
		.xor(
			payload
				.downcast_ref::<String>()
				.map(String::from)
		)
		.unwrap()
}

fn main() {
	panic::set_hook(
		Box::new(|info|
			unsafe {
				let panic = panic_payload(info.payload());
				let panic = panic
					.splitn(2, "  ")
					.collect::<Vec<&str>>();
				let message = panic.get_unchecked(0);
				let reason = panic
					.get(1)
					.unwrap_or(&"NO_DISPLAYABLE_INFORMATION")
					.replace('\n', "\r\n");
				println!("\r\x1b[4mAn error occurred whilst attempting to {message}; '\x1b[1m{reason}\x1b[22m'\x1b[24m\0");
				EXIT()
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

		if let Err(why) = enable_raw_mode() { log!(err: "enable the raw mode of the current terminal" => why; return EXIT()) }

		songlists(files)
	};
	let init = OnceCell::new(); // expensive operation only executed if no err.
	let mut generator = Rng::new();

	let length = lists.len();
	let mut list_index = 0;
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


		let State { output, control, signal, .. } = init.get_or_init(State::initialise);


		let length = song.len();
		let mut song_index = 0;

		'list_playback: { // i hate this
			while song_index < length && !control.is_finished() {
				let old_song_index = song_index; // (sort of) proxy to index (used because of rewind code)
				// unless something is very wrong with the index (old), this will not error.
				let (name, duration, song_time) = unsafe { song.get_unchecked_mut(old_song_index) };
				match output
					.1
					.play_once(unsafe { FILES.get_unchecked_mut(old_song_index) })
				{
					Ok(playback) => 'song_playback: {
						log!(info[name]: "Playing back the audio contents of [{name}].");
						let mut elapsed = Duration::ZERO;
						while &elapsed <= duration {
							let now = Instant::now();
							let paused = playback.is_paused();
							elapsed += match signal.recv_deadline(now + FOURTH_SECOND) {
								Err(RecvTimeoutError::Timeout) => if paused { continue } else { FOURTH_SECOND },

								Ok(Signal::ManualExit) => break 'queue,

								Ok(Signal::SkipNextPlaylist) => {
									list_index += 1;
									break 'list_playback
								},
								Ok(Signal::SkipNext) => {
									song_index += 1;
									break 'song_playback
								},
								Ok(Signal::SkipBackPlaylist) => break 'list_playback list_index -= (old_list_index > 0 && elapsed <= SECOND) as usize,
								Ok(Signal::SkipBack) => break 'song_playback song_index -= (old_song_index > 0 && elapsed <= SECOND) as usize,

								Ok(Signal::TogglePlayback) => {
									if paused { playback.play() } else { playback.pause() }
									now.elapsed()
								},

								Err(RecvTimeoutError::Disconnected) => break 'queue, // chain reaction will follow
							};
						}
						if *song_time == 0 { song_index += 1 } else { *song_time -= 1 }
					},
					Err(why) => log!(err[name]: "playback [{name}] from the default audio output device" => why; break 'queue), // assume error will occur on the other tracks too
				};
				if let Err(why) = unsafe { FILES.get_unchecked_mut(old_song_index) }.rewind() { log!(err[name]: "reset the player position inside of [{name}]" => why) }
			}
			if *list_time == 0 { list_index += 1 } else { *list_time -= 1 }
		}
		unsafe { FILES.clear() }
		print!("\r\n\n\0");
	}

	init
		.into_inner()
		.map(State::clean_up);
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl State {
	/// Initialize state.
	fn initialise() -> Self {
		log!(info: "Determining the output device.");
		let output = match rodio::OutputStream::try_default() {
			Ok(handles) => handles,
			Err(why) => {
				if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
				panic!("determine the default audio output device  {why}")
			},
		};


		log!(info: "Spinning up the playback control thread.");
		let (sender, signal) = unbounded();
		let (exit, exit_receiver) = unbounded();
		let control = {
			match Builder::new()
				.name(String::from("Playback-Control"))
				.spawn(move ||
					while let Err(RecvTimeoutError::Timeout) = exit_receiver.recv_timeout(FOURTH_SECOND) {
						let signal = match match event::poll(FOURTH_SECOND) {
							Ok(truth) => if truth { event::read() } else { continue },
							Err(why) => panic!("poll an event from the current terminal  {why}"),
						} {
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('c' | 'C'), .. })) => {
								if let Err(why) = sender.send(Signal::ManualExit) { log!(err: "send a signal to the playback" => why) }
								return
							},
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('n' | 'N'), .. })) => Signal::SkipNextPlaylist,
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('b' | 'B'), .. })) => Signal::SkipBackPlaylist,
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('l' | 'L'), .. })) => Signal::SkipNext,
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('j' | 'J'), .. })) => Signal::SkipBack,
							Ok(Event::Key(KeyEvent { code: KeyCode::Char('k' | 'K'), .. })) => Signal::TogglePlayback,
							Err(why) => panic!("read an event from the current terminal  {why}"),
							_ => continue,
						};
						if let Err(_) = sender.send(signal) { panic!("send a signal to the playback  {DISCONNECTED}") }
					}
				)
			{
				Ok(thread) => thread,
				Err(why) => panic!("create the playback control thread  {why}"),
			}
		};

		Self {
			output,
			control,
			exit,
			signal,
		}
	}

	/// Clean-up the state.
	fn clean_up(self) {
		let Self { control, exit, .. } = self;
		if !control.is_finished() {
			let _ = exit.send(0); // error might occur between check, manual shutdown on control side, and exit signal sending
			// not really an error.
		} // assume that there's no error here. If there is, then the thread either finished(/panicked) 
		if let Err(why) = control.join() {
			let why = panic_payload(&why);
			log!(err: "clean up the playback control thread" => why)
		}
		if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
		EXIT()
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
