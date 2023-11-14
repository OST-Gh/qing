///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use std::{
	panic,
	env::args,
	convert::identity,
	ops::{ Deref, DerefMut },
	io::{
		stdin,
		BufRead,
		IsTerminal,
	},
};
use crossterm::terminal::{
	disable_raw_mode,
	enable_raw_mode,
	is_raw_mode_enabled,
};
use quing::{
	Error,
	VectorError,
	serde::SerDePlaylist,
	playback::{
		Playhandle,
		Playlist,
		ControlFlow,
	},
};
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
create_flags!{
	#[cfg_attr(debug_assertions, derive(Debug))]
	/// A flag bundle.
	///
	/// This structure is used to partially parse the passed in [`program arguments`] for further use.
	///
	/// The position of flags can only directly be after the executable path (e.g.: //usr/local/bin/quing).\
	/// This' made to be that way, due to the fact that the arguments, after the flags, could all be considered file names.\
	/// Flags can be merged, meaning that one does not need to specify multiple separate flags, for example: `quing -h -f`, is instead, `quing -hf`.\
	/// Flag ordering does not matter.
	///
	/// See the associated constants on [`Flags`] for which [`character`] identifies which flag.
	///
	/// [`program arguments`]: std::env::args
	/// [`character`]: char
	[[Flags]]

	/// Don't use this option.
	should_spawn_headless = 'h'

	/// If the program should merge all given [`Playlists`] into one.
	///
	/// [`Playlists`]: crate::songs::Playlist
	should_flatten = 'f'

	/// Wether or not the file-playlist should repeat infinitely
	should_repeat_playlist = 'p'

	/// When present, will indicate that each file in the file-playlist should reoeat infinitely.
	should_repeat_track = 't'

	/// Wether or not the program should output some information.
	should_print_version = 'v'

	[const]
	/// A set made up of each flag identifier.
	INUSE_IDENTIFIERS = [..]
	/// The starting position of the allowed ASCII character range.
	SHIFT = 97 // minimum position in ascii
	/// The length of the set that contain all possible single character flags.
	LENGTH = 26
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[macro_export]
/// A macro for general interaction with Standard-Out.
///
/// This macro is, in a general sense, just a fancier [`println`] macro, which also is more tailored towards [raw-mode].
///
/// [raw-mode]: crossterm::terminal#raw-mode
macro_rules! log {
	($($value: expr),*; $message: literal $($why: ident)+ $(; $($retaliation: tt)+)?) => {
		{
			print!(
				concat!("\rError whilst ", $message, ';')
				$(, $value)*
			);
			$(print!(" '{}'", $why);)+
			print!("\n");
			$($($retaliation)+)?
		}
	};

}

#[macro_export]
macro_rules! count {
	($thing: expr) => { 1 };
	($($thing: expr),* $(,)?) => { 0 $(+ $crate::count!($thing))* };
}

#[macro_export]
/// Macro that creates the [`Flags`] structure.
macro_rules! create_flags {
	(
		$(#[$structure_attribute: meta])* [[$structure: ident]]
		$($(#[$field_attribute: meta])* $field: ident = $flag: literal)+

		[const]
		$(#[$lone_attribute: meta])* $lone: ident = [..]
		$(#[$shift_attribute: meta])* $shift: ident = $by: literal
		$(#[$length_attribute: meta])* $length: ident = $number: literal
	) => {
		$(#[$structure_attribute])*
		pub(crate) struct $structure(u32);

		impl $structure {
			$(#[$lone_attribute])* const $lone: [char; count!($($flag),+)] = [$($flag),+];
			$(#[$shift_attribute])* const $shift: u32 = $by;
			$(#[$length_attribute])* const $length: u32 = $number;
			$(
				#[doc = concat!("Specify using '`-", $flag, "`'.")]
				$(#[$field_attribute])*
				// macro bullshit
				pub(crate) fn $field(&self) -> bool {
					#[cfg(debug_assertions)] if !flag_check(&$flag) { panic!("get a flag  NOT-ALPHA") }
					**self >> Self::from($flag).into_inner() & 1 == 1 // bit hell:)
					// One copy call needed (**)
					// 0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0   0
					//                         z   y   x   w   v   u   t   s   r   q   p   o   n   m   l   k   j   i   h   g   f   e   d   c   b   a
					//                       122 121 120 119 118 117 116 115 114 113 112 111 110 109 108 107 106 105 104 103 102 101 100 099 098 097
					//                       025 024 023 022 021 020 019 018 017 016 015 014 013 012 011 010 009 008 007 006 005 004 003 002 001 000
				}
			)+

		}

	};
}


///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
fn flag_check(symbol: &char) -> bool { symbol.is_ascii_alphabetic() && symbol.is_ascii_lowercase() }

fn run(arguments: impl Iterator<Item = String>, flags: Flags) -> Result<(), Error> {
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
					.unwrap_or(&"NO_DISPLAYABLE_INFORMATION");
				println!("\rAn error occurred whilst attempting to {message}; '{reason}'");
				let _ = disable_raw_mode();
			}
		)
	);

	let mut lists: Vec<SerDePlaylist> = SerDePlaylist::try_from_paths(arguments)?;
	if flags.should_repeat_track() {
		let last = lists
			.last_mut()
			.map_or(Err(VectorError::Empty), Ok)?;
		if flags.should_repeat_playlist() { last.time_set(-1) }
		if flags.should_repeat_track() {
			for track in last
				.song_get_mut()
				.iter_mut()
			{ track.set_time(-1) }
		}
	}
	if flags.should_flatten() { lists = vec![SerDePlaylist::flatten(lists)?]; }
	let streams = lists
		.into_iter()
		.map(Playlist::try_from)
		.collect::<Result<Vec<Playlist>, Error>>()?;

	let mut player = Playhandle::try_from(streams)?;
	match player.all_playlists_play()? {
		ControlFlow::Break => return Ok(()),
		ControlFlow::Skip | ControlFlow::SkipSkip => unimplemented!(), // NOTE(by: @OST-Gh): see playback.rs Playhandle::all_streams_play match
		ControlFlow::Default => { },
	};

	let controls = player
		.io_handle_take()
		.controls_take();
	controls.exit_notify();
	controls.clean_up();
	Ok(())
}

fn main() {
	let mut arguments: Vec<String> = args()
		.skip(1) // skips the executable path (e.g.: //bin/{bin-name})
		.collect();
	let is_terminal = stdin().is_terminal();
	if !is_terminal {
		arguments.reserve(16);
		arguments.extend(
			stdin()
				.lock()
				.lines()
				.map_while(|result|
					result
						.as_ref()
						.map_or(false, |line| !line.is_empty())
						.then(|| result.unwrap())
				)
				.map(String::from)
		)
	};
	if arguments
		.first()
		.is_none()
	{
		println!("No input given.");
		return
	}

	let (flags, files) = Flags::separate_from(arguments);

	if flags.should_print_version() { print!(concat!('\r', env!("CARGO_PKG_NAME"), " on version ", env!("CARGO_PKG_VERSION"), " by ", env!("CARGO_PKG_AUTHORS"), ".\n\0")) }
	if !flags.should_spawn_headless() && is_terminal && !is_raw_mode_enabled().is_ok_and(identity) {
		let _ = enable_raw_mode();
	}

	let result = run(files, flags);

	let _ = disable_raw_mode();
	if let Err(error) = result { println!("{error}") }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl Flags {
	#[inline(always)]
	/// Get the underlying unsigned integer.
	pub(crate) fn into_inner(self) -> u32 { self.0 }

	/// Split the program arguments into files and flags.
	///
	/// # Panics:
	///
	/// - Arguments are empty.
	pub(crate) fn separate_from(iterator: Vec<String>) -> (Self, impl Iterator<Item = String>) {
		let mut flag_count = 0;
		let bits = iterator
			.iter()
			.map_while(|argument|
				{
					let raw = argument
						.strip_prefix('-')?
						.replace(|symbol| !flag_check(&symbol), "");
					flag_count += 1;
					Some(raw)
				}
			)
			.fold(
				Self(0),
				|mut bits, raw|
				{
					for symbol in raw
						.chars()
						.filter(|symbol| Self::INUSE_IDENTIFIERS.contains(symbol))
					{ *bits |= 1 << Self::from(symbol).into_inner() }
					bits
				}
			);
		(
			bits,
			iterator
				.into_iter()
				.skip(flag_count)
		)
	}
}

impl From<char> for Flags {
	fn from(symbol: char) -> Self {
		#[cfg(debug_assertions)] if !flag_check(&symbol) { panic!("get a flag  NOT-ALPHA") }
		Self((symbol as u32 - Self::SHIFT) % Self::LENGTH)
	}
}

impl Deref for Flags {
	type Target = u32;

	#[inline(always)]
	fn deref(&self) -> &u32 { &self.0 }
}

impl DerefMut for Flags {
	#[inline(always)]
	fn deref_mut(&mut self) -> &mut u32 { &mut self.0 }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
