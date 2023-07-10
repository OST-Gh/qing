///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
//! NOTE: crt in raw mode behaves strangely or maps <100% keyboard maps to 100% maps, e.g.: backspace in raw-mode = h
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	fs::{ self, File },
	path::{ PathBuf, MAIN_SEPARATOR_STR },
	time::{ Duration, Instant },
	io::{ BufReader, Seek },
	env::{ var, args },
};
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
use lofty::{ read_from_path, AudioFile };
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
const FOURTH_SECOND: Duration = Duration::from_millis(250);
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
static mut FILES: Vec<BufReader<File>> = Vec::new();
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(serde::Deserialize)]
struct Songlist {
	name: Box<str>,
	song: Vec<Song>,
}

#[derive(serde::Deserialize)]
struct Song {
	name: Box<str>,
	file: Box<str>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
macro_rules! log {
	(err$([$($visible: ident)+])?: $message: literal => $($why: ident)+) => {
		{
			print!(concat!("\r\x1b[38;2;254;205;33m\x1b[4mAn error occured whilst attempting to ", $message, ';') $(, $($visible = $visible),+)?);
			$(
				print!(
					" '\x1b[1m{}\x1b[22m'",
					format!("{}", $why).replace('\n', "\n\r")
				);
			)+
			println!("\x1b[24m\0")
		}
	};
	(info$([$($visible: ident)+])?: $message: literal) => { println!(concat!("\r\x1b[38;2;254;205;33m", $message, '\0') $(, $($visible = $visible),+)?) };
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
fn fmt_path(text: impl AsRef<str>) -> PathBuf {
	text
		.as_ref()
		.split(MAIN_SEPARATOR_STR)
		.filter_map(|part|
			{
				if part == "~" { return var("HOME").ok() }
				if part.starts_with('$') { return var(&part[1..]).ok() }
				Some(String::from(part))
			}
		)
		.collect::<Vec<String>>()
		.join(MAIN_SEPARATOR_STR)
		.into()
}

fn main() {
	let mut files = args()
		.skip(1) // skips the executable path (e.g.: //usr/local/bin/{bin-name})
		.peekable();
	if files.peek() == None { return println!("Requires at least one playlist.toml file.") }

	if let Err(why) = enable_raw_mode() { return log!(err: "enable the raw mode of the current terminal" => why) }
	let mut generator = fastrand::Rng::new();

	log!(info: "Determining the output device.");
	let handles = match rodio::OutputStream::try_default() {
		Ok(handles) => handles,
		Err(why) => {
			log!(err: "determine the default audio output device" => why);
			if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
			return
		},
	};

	'queue: for path in files {
		log!(info[path]: "\n\nLoading and parsing data from [{path}].");
		let Songlist { mut song, name } = 'load: {
			match fs::read_to_string(&path).map(|contents| toml::from_str(&contents)) {
				Ok(Ok(playlist)) => break 'load playlist,
				Ok(Err(why)) => log!(err[path]: "parse the contents of [{path}]" => why),
				Err(why) => log!(err[path]: "load the contents of [{path}]" => why),
			}
			continue 'queue
		};

		log!(info[name]: "Shuffling all of the songs in [{name}].");
		let song: Vec<(Box<str>, Duration)> = {
			let length = song.len();
			for _ in 0..length {
				let old = generator.usize(0..length);
				let new = generator.usize(0..length);
				song.swap(old, new);
			}
			log!(info[name]: "Loading all of the audio contents of the songs in [{name}].");
			song
				.into_iter()
				.filter_map(|Song { name, file }|
					{
						let formatted = fmt_path(file);
						match (File::open(&formatted), read_from_path(formatted)) {
							(Ok(contents), Ok(info)) => {
								unsafe { FILES.push(BufReader::new(contents)) }
								return Some(
									(
										name,
										info
											.properties()
											.duration(),
									)
								)
							},
							(Err(why), Ok(_)) => log!(err[name]: "load the audio contents of [{name}]" => why),
							(Ok(_), Err(why)) => log!(err[name]: "load the audio properties of [{name}]" => why),
							(Err(file_why), Err(info_why)) => log!(err[name]: "load the audio contents and properties of [{name}]" => file_why info_why),
						};
						None
					}
				)
				.collect()
		};
		log!(info: "");

		let length = song.len();
		let mut index = 0;

		'playlist: while index < length {
			let (name, duration) = unsafe { song.get_unchecked(index) };
			match handles
				.1
				.play_once(unsafe { FILES.get_unchecked_mut(index) })
			{
				Ok(playback) => {
					log!(info[name]: "Playing back the audio contents of [{name}].");
					let mut elapsed = Duration::ZERO;
					'play: while &elapsed <= duration {
						let now = Instant::now();
						let event = 'read: {
							match event::poll(FOURTH_SECOND) {
								Ok(truth) => if truth { break 'read event::read() },
								Err(why) => log!(err: "poll an event from the current terminal" => why),
							}
							continue 'play
						};
						let time = match event {
							Ok(Event::Key(KeyEvent { code: KeyCode::Char(key), .. })) => {
								match key {
									'q' | 'c' => break 'queue,
									'/' | 'h' => break 'playlist,
									',' | 'j' | '.' | 'l' => if let Err(why) = unsafe { FILES.get_unchecked_mut(index) }.rewind() { log!(err[name]: "reset the player position inside of [{name}]" => why) } else {
										match key {
											',' | 'j' => {
												index -= (index > 0) as usize;
												continue 'playlist
											},
											'.' | 'l' | _ => break 'play
										}
									},
									' ' | 'k' => if playback.is_paused() { playback.play() } else { playback.pause() },
									_ => { },
								}
								now.elapsed()
							},
							Err(why) => {
								log!(err: "read an event from the current terminal" => why);
								break 'queue
							},
							_ => FOURTH_SECOND,
						};
						if !playback.is_paused() { elapsed += time }
					}
				},
				Err(why) => log!(err[name]: "playback [{name}] from the default audio output device" => why),
			}
			index += 1
		}
		unsafe { FILES.clear() }
	}

	if let Err(why) = disable_raw_mode() { log!(err: "disable the raw mode of the current terminal" => why) }
	print!("\r\x1b[0m\0")
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////