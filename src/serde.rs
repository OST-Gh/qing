///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use super::{utilities::fmt_path, Error, VectorError};
use serde::Deserialize;
use std::{fs::read_to_string, num::NonZero};
use toml::from_str;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
#[cfg_attr(
	any(debug_assertions, feature = "traits"),
	derive(PartialEq, Eq, PartialOrd, Ord),
	derive(Hash)
)]
#[derive(Deserialize)]
/// A playlist with some metadata.
pub struct SerDePlaylist {
	pub(crate) song: Vec<SerDeTrack>,
	pub(crate) time: Option<isize>,
	pub(crate) vary: Option<bool>,
}

#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
#[cfg_attr(
	any(debug_assertions, feature = "traits"),
	derive(PartialEq, Eq, PartialOrd, Ord),
	derive(Hash)
)]
#[derive(Deserialize)]
#[derive(Clone)]
/// A song path with additional metadata.
pub struct SerDeTrack {
	pub(crate) file: Box<str>,
	pub(crate) time: Option<isize>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl SerDePlaylist {
	#[inline(always)]
	/// Destructure `self` and get a reference to the contained tracks.
	pub fn song_get(&self) -> &Vec<SerDeTrack> {
		&self.song
	}
	#[inline(always)]
	/// Destructure `self` and get the mutable reference to the contained tracks.
	pub fn song_get_mut(&mut self) -> &mut Vec<SerDeTrack> {
		&mut self.song
	}
	#[inline(always)]
	/// Destructure `self` and take the contained tracks.
	pub fn song_take(self) -> Vec<SerDeTrack> {
		self.song
	}
	#[inline(always)]
	/// Primitive for setting repeats equal to some non-zero value.
	pub fn time_set(&mut self, value: isize) {
		self.time = NonZero::<isize>::new(value).map(NonZero::get)
	}
	#[inline(always)]
	/// Primitive for setting repeats equal to null.
	pub fn time_unset(&mut self) {
		self.time_set(0)
	}

	#[inline(always)]
	/// Primitive for setting shuffling equal to some state.
	pub fn vary_set(&mut self, state: bool) {
		self.vary = state.then_some(true);
	}

	#[inline(always)]
	/// Primitive for setting shuffling equal to null.
	pub fn vary_unset(&mut self) {
		self.vary_set(false)
	}

	#[inline]
	/// Filter out [`SerDePlaylist`] [`files`] from audio [`files`].
	///
	/// This function returns a [`Vec`] that contains all successfully parsed playlists.\
	/// The last item of the [`Vec`] is the so called outlier, items of the [`Iterator`] that could not be parsed to a playlist, and so are treated as tracks instead.
	///
	/// [`files`]: std::fs::File
	pub fn try_from_paths(
		iterator: impl IntoIterator<Item = String>,
	) -> Result<Vec<Self>, Error> {
		let mut rest = Vec::with_capacity(8);
		let mut outliers = SerDePlaylist {
			song: Vec::with_capacity(8),
			time: None,
			vary: None,
		};
		for path in iterator {
			match read_to_string(fmt_path(&path)?) {
				// might not always work (might sometimes be mp3 but still contain fully valid utf-8 'till the end)
				Ok(contents) => rest.push(Self::try_from_contents(contents)?),
				Err(_) => outliers
					.song
					.push(SerDeTrack {
						file: path.into_boxed_str(),
						time: None,
					}),
			}
		}
		rest.push(outliers);
		Ok(rest.into_iter()
			.filter(|list| !list.is_empty())
			.collect())
	}

	#[inline]
	/// Merge a list of [`SerDePlaylists`] into a single [`SerDePlaylist`].
	///
	/// [`SerDePlaylists`]: SerDePlaylist
	pub fn flatten(lists: Vec<Self>) -> Result<Self, Error> {
		let repeats = lists
			.iter()
			.min_by_key(|Self { time, .. }| time.unwrap_or_default())
			.ok_or(VectorError::Empty)?
			.time
			.unwrap_or_default();
		let shuffle = lists
			.iter()
			.find_map(|Self { vary, .. }| match vary {
				Some(false) | None => Some(false),
				Some(true) => None,
			})
			.ok_or(VectorError::Empty)?;
		let tracks: Vec<SerDeTrack> = lists
			.into_iter()
			.flat_map(|list| list.song)
			.collect();
		Ok(Self {
			vary: Some(shuffle),
			song: tracks,
			time: Some(repeats),
		})
	}

	#[inline(always)]
	/// Find out if a [`SerDePlaylist`] is empty.
	///
	/// This function is equal to a [`Vec.is_empty()`] call.
	///
	/// [`Vec.is_empty()`]: Vec.is_empty
	pub fn is_empty(&self) -> bool {
		self.song
			.is_empty()
	}

	#[inline(always)]
	/// Load a [`Playlist`] from a [`Path`] represented as a [`String`].
	///
	/// The string is, before being loaded, passed into the [`fmt_path`] function.
	///
	/// [`Path`]: std::path::Path
	fn try_from_contents(contents: String) -> Result<Self, Error> {
		from_str(&contents).map_err(Error::from)
	}
}

impl SerDeTrack {
	#[inline(always)]
	/// Set the amount that the track should be repeated.
	pub fn set_time(&mut self, value: isize) {
		self.time = Some(value)
	}
	#[inline(always)]
	/// Set the repeat-amount to default.
	pub fn unset_time(&mut self) {
		self.time = None
	}
}
