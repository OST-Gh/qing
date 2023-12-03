///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
use serde::Deserialize;
use std::fs::read_to_string;
use super::{
	Error,
	VectorError,
	utilities::fmt_path,
};
use toml::from_str;
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
#[cfg_attr(any(debug_assertions, feature = "traits"), derive(PartialEq, Eq, PartialOrd, Ord), derive(Hash))]
#[derive(Deserialize)]
/// A playlist with some metadata.
pub struct SerDePlaylist {
	pub(crate) song: Vec<SerDeTrack>,
	pub(crate) time: Option<isize>,
}

#[cfg_attr(any(debug_assertions, feature = "debug"), derive(Debug))]
#[cfg_attr(any(debug_assertions, feature = "traits"), derive(PartialEq, Eq, PartialOrd, Ord), derive(Hash))]
#[derive(Deserialize)]
#[derive(Clone)]
/// A song path with additional metadata.
pub struct SerDeTrack {
	pub(crate) file: Box<str>,
	pub(crate) time: Option<isize>,
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
impl SerDePlaylist {
	#[inline(always)] pub fn song_get(&self) -> &Vec<SerDeTrack> { &self.song }
	#[inline(always)] pub fn song_get_mut(&mut self) -> &mut Vec<SerDeTrack> { &mut self.song }
	#[inline(always)] pub fn song_take(self) -> Vec<SerDeTrack> { self.song }
	#[inline(always)] pub fn time_set(&mut self, value: isize) { self.time = Some(value) }
	#[inline(always)] pub fn time_unset(&mut self) { self.time = None }

	/// Filter out [`SerDePlaylist`] [`files`] from audio [`files`].
	///
	/// This function returns a [`Vec`] that contains all successfully parsed playlists.\
	/// The last item of the [`Vec`] is the so called outlier, items of the [`Iterator`] that could not be parsed to a playlist, and so are treated as tracks instead.
	///
	/// [`files`]: std::fs::File
	pub fn try_from_paths(iterator: impl IntoIterator<Item = String>) -> Result<Vec<Self>, Error> {
		let mut rest = Vec::with_capacity(8);
		let mut outliers = SerDePlaylist {
			song: Vec::with_capacity(8),
			time: None,
		};
		for path in iterator {
			match read_to_string(fmt_path(&path)?) { // might not always work (might sometimes be mp3 but still contain fully valid utf-8 'till the end)
				Ok(contents) => rest.push(Self::try_from_contents(contents)?),
				Err(_) => {
					outliers
						.song
						.push(
							SerDeTrack {
								file: path.into_boxed_str(),
								time: None,
							}
						);
				},
			}
		}
		rest.push(outliers);
		Ok(
			rest
				.into_iter()
				.filter(|list| !list.is_empty())
				.collect()
		)
	}

	/// Merge a list of [`SerDePlaylists`] into a single [`SerDePlaylist`].
	///
	/// [`SerDePlaylists`]: SerDePlaylist
	pub fn flatten(lists: Vec<Self>) -> Result<Self, Error> {
		let repeats = lists
			.iter()
			.min_by_key(|Self { time, .. }| time.unwrap_or_default())
			.map_or(Err(VectorError::Empty), Ok)?
			.time
			.unwrap_or_default();
		let tracks: Vec<SerDeTrack> = lists
			.into_iter()
			.flat_map(|list| list.song)
			.collect();
		Ok(
			Self {
				song: tracks,
				time: Some(repeats),
			}
		)
	}

	#[inline(always)]
	/// Find out if a [`SerDePlaylist`] is empty.
	///
	/// This function is equal to a [`Vec.is_empty()`] call.
	///
	/// [`Vec.is_empty()`]: Vec.is_empty
	pub fn is_empty(&self) -> bool {
		self
			.song
			.is_empty()
	}

	#[inline(always)]
	/// Load a [`Playlist`] from a [`Path`] represented as a [`String`].
	///
	/// The string is, before being loaded, passed into the [`fmt_path`] function.
	///
	/// [`Path`]: std::path::Path
	fn try_from_contents(contents: String) -> Result<Self, Error> { from_str(&contents).map_err(Error::from) }
}

impl SerDeTrack {
	#[inline(always)] pub fn set_time(&mut self, value: isize) { self.time = Some(value) }
	#[inline(always)] pub fn unset_time(&mut self) { self.time = None }
}
///////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////////
