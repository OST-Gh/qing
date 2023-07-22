///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! I don't know why, but i am making Docs for this
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	thread::{ JoinHandle, Builder },
	fs::{ self, File },
	path::{ PathBuf, MAIN_SEPARATOR_STR },
	time::{ Duration, Instant },
	io::{ BufReader, Seek },
	env::{
		var,
		args,
		VarError,
	},
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
use lofty::{ read_from_path, AudioFile };
use crossbeam_channel::{ unbounded, RecvTimeoutError, Sender, Receiver };
use rodio::{ OutputStream, OutputStreamHandle };
use serde::Deserialize;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Constant signal rate.
const FOURTH_SECOND: Duration = Duration::from_millis(250);
/// Constant used for rewinding.
const SECOND: Duration = Duration::from_secs(1);

/// Inter-thread communication channel disconnected.
const DISCONNECTED: &'static str = "DISCONNECTED CHANNEL";
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Global audio stream data.
static mut FILES: Vec<BufReader<File>> = Vec::new();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Deserialize)]
/// Playlist
struct Songlist {
	name: Option<Box<str>>,
	song: Vec<Song>,
        :close

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
macro_rules! log {
	(err$([$($visible: ident)+])?: $message: literal => $($why: ident)+ $(; $($retaliation: tt)+)?) => {
		{
			print!(concat!("\r\x1b[38;2;254;205;33m\x1b[4mAn error occurred whilst attempting to ", $message, ';') $(, $($visible = $visible),+)?);
			$(print!(" '\x1b[1m{}\x1b[22m'", format!("{}", $why).replace('\n', "\r\n"));)+
			println!("\x1b[24m\0");
			$($($retaliation)+)?
		}
	};
	(info$([$($visible: ident)+])?: $message: literal) => { println!(concat!("\r\x1b[38;2;254;205;33m", $message, '\0') $(, $($visible = $visible),+)?) };
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
/// Format a text representation of a path into an absolute path.
fn fmt_path(text: impl AsRef<str>) -> PathBuf {
	fn expand(name: &str) -> Result<String, VarError> {
		let mut buffer = Vec::new();
		for part in var(if name.starts_with('$') { expand(&name[1..])? } else { String::from(name) })?
			.split(MAIN_SEPARATOR_STR)
			.map(|part| if part.starts_with('$') { expand(&part[1..]) } else { Ok(String::from(part)) })
		{ buffer.push(part?) }
		Ok(buffer.join(MAIN_SEPARATOR_STR))
	}
	PathBuf::from(
		text
			.as_ref()
			.split(MAIN_SEPARATOR_STR)
			.filter_map(|part|
				match match part {
					"~" => expand("HOME"),
					_ if part.starts_with('$') => expand(&part[1..]), // add support for multiple $ vars ($$VAR => $VALUE_OF_VAR => VALUE_OF_VALUE_OF_VAR)
					_ => return Some(String::from(part)),
				} {
					Ok(part) => Some(part),
					Err(why) => log!(err: "expand a shell expression to a path" => why; None)
				}
			)
			.collect::<Vec<String>>()
			.join(MAIN_SEPARATOR_STR)
	)
		.canonicalize()
		.unwrap_or_else(|why| log!(err: "canonicalise a path" => why; PathBuf::new()))
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

/// Custom panic handle. (similar to log-err)
fn panic_handle(info: &panic::PanicInfo) {
	let panic = panic_payload(info.payload());
	unsafe {
		let panic = panic
			.splitn(2, "  ")
			.collect::<Vec<&str>>();
		let message = panic.get_unchecked(0);
		let reason = panic
			.get(1)
			.unwrap_or(&"NO_DISPLAYABLE_INFORMATION")
			.replace('\n', "\r\n");
		println!("\r\x1b[38;2;254;205;33m\x1b[4mAn error occurred whilst attempting to {message}; '\x1b[1m{reason}\x1b[22m'\x1b[24m\0")
	};
}

fn next(time: &mut isize, index: &mut usize) {
	if *time == 0 { *index += 1 } else { *time -= 1 }
}

fn main() {
	panic::set_hook(Box::new(panic_handle));

	let mut files = args()
		.skip(1) // skips the executable path (e.g.: //usr/local/bin/{bin-name})
		.peekable();
	if files
		.peek()
		.is_none() // can't run without one playlist
	{ panic!("get the program arguments  no arguments given") }

	if let Err(why) = enable_raw_mode() { log!(err: "enable the raw mode of the current terminal" => why; return) }

	let mut generator = fastrand::Rng::new();
	let init = OnceCell::new(); // expensive operation only executed if no err.
	let mut lists: Vec<(Vec<Song>, Box<str>, isize)> = files
		.filter_map(|path|
			{
				log!(info[path]: "Loading and parsing data from [{path}].");
				let Songlist { song, name, time } = match fs::read_to_string(fmt_path(&path)).map(|contents| toml::from_str(&contents)) {
					Ok(Ok(playlist)) => playlist,
					Ok(Err(why)) => log!(err[path]: "parse the contents of [{path}]" => why; None?),
					Err(why) => log!(err[path]: "load the contents of [{path}]" => why; None?),
				};
				Some((song, name.unwrap_or_default(), time.unwrap_or_default()))
			}
		)
		.collect();
	let length = lists.len();
	let mut list_index = 0;
	'queue: while list_index < length {
		let old_list_index = list_index;
		let (song, name, list_time) = unsafe { lists.get_unchecked_mut(old_list_index) };
		let mut song = song.clone();
		let mut song: Vec<(Box<str>, Duration, isize)> = {

			log!(info[name]: "Shuffling all of the songs in [{name}].");
			generator.shuffle(&mut song);

			log!(info[name]: "Loading all of the audio contents of the songs in [{name}].");
			song
				.into_iter()
				.filter_map(|Song { name, file, time }|
					{
						let name = name.unwrap_or_default();
						let formatted = fmt_path(file);
						match (File::open(&formatted), read_from_path(formatted)) {
							(Ok(contents), Ok(info)) => {
								unsafe { FILES.push(BufReader::new(contents)) }
								return Some(info.properties()).map(|info| (name, info.duration(), time.unwrap_or(0)))
							},
							(Err(why), Ok(_)) => log!(err[name]: "load the audio contents of [{name}]" => why),
							(Ok(_), Err(why)) => log!(err[name]: "load the audio properties of [{name}]" => why),
							(Err(file_why), Err(info_why)) => log!(err[name]: "load the audio contents and properties of [{name}]" => file_why info_why),
						}
						None
					}
				)
				.collect()
		};
		println!("\r\0");

		let State { output, control, signal, .. } = init.get_or_init(State::initialise);


		let length = song.len();
		let mut song_index = 0;

		'list_playback: { // i hate this
			'playlist: while song_index < length && !control.is_finished() {
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
									*list_time = 0;
									break 'playlist
								},
								Ok(Signal::SkipBackPlaylist) => {
									list_index -= (old_list_index > 0 && elapsed <= SECOND) as usize;
									break 'list_playback
								},

								Ok(Signal::SkipNext) => {
									*song_time = 0;
									break
								},
								Ok(Signal::SkipBack) => break 'song_playback song_index -= (old_song_index > 0 && elapsed <= SECOND) as usize,

								Ok(Signal::TogglePlayback) => {
									if paused { playback.play() } else { playback.pause() }
									now.elapsed()
								},

								Err(RecvTimeoutError::Disconnected) => break 'queue, // chain reaction will follow
							};
						}
						next(song_time, &mut song_index)
					},
					Err(why) => log!(err[name]: "playback [{name}] from the default audio output device" => why; break 'queue), // assume error will occur on the other tracks too
				};
				if let Err(why) = unsafe { FILES.get_unchecked_mut(old_song_index) }.rewind() { log!(err[name]: "reset the player position inside of [{name}]" => why) }
			}
			next(list_time, &mut list_index);
		}
		unsafe { FILES.clear() }
		print!("\r\n\n\0");
	}

	init
		.into_inner()
		.map(State::clean_up);
	if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
	print!("\r\x1b[0m\0")
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
	}
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
